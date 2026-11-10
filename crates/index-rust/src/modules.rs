//! The tree of `mod`s of the crate targets. A mod is a node, a file is an attribute:
//! the tree is found by following `mod x;` from the root of a target.

use crate::syntax::{attribute_items, attribute_path, attributes, is_test_item, named_children};
use std::collections::{BTreeSet, HashMap, HashSet};
use tree_sitter::{Node as Ts, Tree};

/// A parsed `.rs` file.
pub struct Parsed<'a> {
    pub source: &'a str,
    pub tree: Tree,
}

/// One mod of one target. The root of a target is a mod too.
pub struct ModInfo<'t> {
    /// Index of the package in the list of packages.
    pub package: usize,
    /// The name of the mod, or the name of the target for a root.
    pub name: String,
    /// The mod names from the root of the target down to this mod, root included if it
    /// has a node of its own. The root with no node has an empty path.
    pub path: Vec<String>,
    pub parent: Option<usize>,
    /// The mods whose `mod name;` or `mod name { }` declares this one, with the name used
    /// there: the parent, and for a mod that several test targets share, each target.
    pub declared_in: Vec<(usize, String)>,
    /// The file that holds the declarations of the mod.
    pub file: &'t str,
    pub source: &'t str,
    /// The items of the mod, in the order of the source.
    pub items: Vec<Ts<'t>>,
    /// The root of a target that stands for the whole package (`src/lib.rs`, or
    /// `src/main.rs` without a lib) has no node: its items belong to the module.
    pub has_node: bool,
    /// The mod is test code, see `Root::test`.
    pub test: bool,
}

impl ModInfo<'_> {
    /// The mod whose scope `super` and `crate` start from: the parent, else the first mod
    /// that declares this one.
    pub fn outer(&self) -> Option<usize> {
        self.parent.or(self.declared_in.first().map(|d| d.0))
    }
}

/// The mods that follow from the root files, and the files that no `mod` reached.
pub struct Mods<'t> {
    pub mods: Vec<ModInfo<'t>>,
    /// The files that some `mod` declaration reached, or that are a root.
    pub reached: BTreeSet<&'t str>,
    /// Where a `#[path]` attribute sits (`file:line`). Not followed.
    pub path_attributes: Vec<String>,
}

/// A root of a target: the file and the mod path of its root.
pub struct Root<'t> {
    pub package: usize,
    pub file: &'t str,
    /// Empty if the root has no node.
    pub path: Vec<String>,
    /// The target is test code: `tests/`, `benches/`, `examples/`.
    pub test: bool,
}

/// `package_dirs[p]` is the directory of package `p`.
pub fn build<'t>(
    parsed: &'t HashMap<&'t str, Parsed<'t>>,
    roots: &[Root<'t>],
    package_dirs: &[&str],
) -> Mods<'t> {
    let mut builder = Builder {
        parsed,
        mods: Vec::new(),
        reached: BTreeSet::new(),
        path_attributes: Vec::new(),
    };
    for root in roots {
        let Some(&file) = parsed.get_key_value(root.file).map(|(k, _)| k) else {
            continue;
        };
        let dir = file.rsplit_once('/').map_or("", |(dir, _)| dir).to_string();
        let place = Place {
            package: root.package,
            parent: None,
            name: root.path.last().cloned().unwrap_or_default(),
            path: root.path.clone(),
            test: root.test,
        };
        builder.add_file(place, file, &dir);
    }
    let mut path_attributes = Vec::new();
    for place in builder.path_attributes {
        if !path_attributes.contains(&place) {
            path_attributes.push(place);
        }
    }
    Mods {
        mods: share(builder.mods, package_dirs),
        reached: builder.reached,
        path_attributes,
    }
}

/// Cargo compiles a file that several test targets declare as a mod once per target, but
/// it is one file: it becomes one mod below the module, named by its path in the package
/// (`tests/common`), and each target that declares it finds it there. A mod below a
/// target of `src/` is never shared. The `crate` and `super` paths of a shared mod are
/// read from the first target that declares it.
fn share<'t>(mods: Vec<ModInfo<'t>>, package_dirs: &[&str]) -> Vec<ModInfo<'t>> {
    let root_of = |mut index: usize| {
        while let Some(parent) = mods[index].parent {
            index = parent;
        }
        index
    };
    // A file that is a target of its own is the root of that target, never a shared mod.
    let root_files: HashSet<&str> = mods
        .iter()
        .filter(|m| m.parent.is_none())
        .map(|m| m.file)
        .collect();
    let mut groups: HashMap<(usize, &str), Vec<usize>> = HashMap::new();
    for (index, m) in mods.iter().enumerate() {
        if let Some(parent) = m.parent
            && mods[parent].file != m.file
            && !root_files.contains(m.file)
            && mods[root_of(index)].test
        {
            groups.entry((m.package, m.file)).or_default().push(index);
        }
    }
    let mut shared = vec![false; mods.len()];
    let mut duplicate = vec![false; mods.len()];
    let mut copies: HashMap<usize, Vec<usize>> = HashMap::new();
    for members in groups.values() {
        let roots: BTreeSet<usize> = members.iter().map(|m| root_of(*m)).collect();
        if roots.len() > 1 {
            shared[members[0]] = true;
            copies.insert(members[0], members[1..].to_vec());
            for copy in &members[1..] {
                duplicate[*copy] = true;
            }
        }
    }
    // A parent comes before its children, so one run in order sees the parent first.
    let mut dropped = vec![false; mods.len()];
    let mut inside = vec![false; mods.len()];
    for (index, m) in mods.iter().enumerate() {
        if let Some(parent) = m.parent {
            dropped[index] = duplicate[index] || dropped[parent];
            inside[index] = shared[index] || inside[parent];
        }
    }
    let new_index: Vec<Option<usize>> = {
        let mut next = 0;
        dropped
            .iter()
            .map(|gone| {
                (!gone).then(|| {
                    next += 1;
                    next - 1
                })
            })
            .collect()
    };
    let mut declared: Vec<Vec<(usize, String)>> = mods
        .iter()
        .map(|m| {
            m.declared_in
                .iter()
                .filter(|(p, _)| !dropped[*p])
                .cloned()
                .collect()
        })
        .collect();
    for (canonical, copies) in &copies {
        for copy in copies {
            let more = declared[*copy].clone();
            declared[*canonical].extend(more);
        }
    }
    let mut kept: Vec<ModInfo<'t>> = Vec::new();
    for (index, mut m) in mods.into_iter().enumerate() {
        if dropped[index] {
            continue;
        }
        let below_shared = m.parent.is_some_and(|p| inside[p]);
        if shared[index] && !below_shared {
            m.path = vec![file_path(&m, package_dirs[m.package])];
            m.name = m.path[0].clone();
            m.parent = None;
        } else if inside[index] {
            let outer = &kept[new_index[m.parent.unwrap_or(0)].unwrap_or(0)];
            m.path = outer.path.iter().cloned().chain([m.name.clone()]).collect();
        }
        m.parent = m.parent.and_then(|p| new_index[p]);
        m.declared_in = {
            let mut list: Vec<(usize, String)> = Vec::new();
            for (p, name) in declared[index].drain(..) {
                let entry = (new_index[p].unwrap_or(0), name);
                if !list.contains(&entry) {
                    list.push(entry);
                }
            }
            list
        };
        kept.push(m);
    }
    kept
}

/// The path of the file of a mod in its package, without `.rs` and without `/mod`:
/// `tests/common` for `tests/common/mod.rs` and for `tests/common.rs`.
fn file_path(m: &ModInfo, package_dir: &str) -> String {
    let path = m
        .file
        .strip_prefix(package_dir)
        .map_or(m.file, |rest| rest.trim_start_matches('/'));
    let path = path.strip_suffix(".rs").unwrap_or(path);
    path.strip_suffix("/mod").unwrap_or(path).to_string()
}

/// Where a mod sits and what it is called.
struct Place {
    package: usize,
    parent: Option<usize>,
    name: String,
    path: Vec<String>,
    test: bool,
}

struct Builder<'t> {
    parsed: &'t HashMap<&'t str, Parsed<'t>>,
    mods: Vec<ModInfo<'t>>,
    reached: BTreeSet<&'t str>,
    path_attributes: Vec<String>,
}

impl<'t> Builder<'t> {
    /// Adds the mod that a file is the whole body of. `dir` is where the files of its
    /// `mod x;` declarations are looked up.
    fn add_file(&mut self, place: Place, file: &'t str, dir: &str) -> usize {
        let parsed = &self.parsed[file];
        self.reached.insert(file);
        let items = named_children(parsed.tree.root_node());
        self.add(place, (file, parsed.source), items, dir)
    }

    fn add(
        &mut self,
        place: Place,
        (file, source): (&'t str, &'t str),
        items: Vec<Ts<'t>>,
        dir: &str,
    ) -> usize {
        let id = self.mods.len();
        self.mods.push(ModInfo {
            declared_in: place
                .parent
                .map(|parent| (parent, place.name.clone()))
                .into_iter()
                .collect(),
            package: place.package,
            name: place.name,
            has_node: !place.path.is_empty(),
            test: place.test,
            path: place.path.clone(),
            parent: place.parent,
            file,
            source,
            items: items.clone(),
        });
        for item in items.into_iter().filter(|i| i.kind() == "mod_item") {
            let Some(name) = item.child_by_field_name("name") else {
                continue;
            };
            let name = source[name.byte_range()].to_string();
            let mut child_path = place.path.clone();
            child_path.push(name.clone());
            let child_dir = crate::join(dir, &name);
            let child = Place {
                package: place.package,
                parent: Some(id),
                name: name.clone(),
                path: child_path,
                test: place.test || is_test_item(item, source),
            };
            if let Some(attribute) = attribute_items(item)
                .into_iter()
                .find(|a| attribute_path(*a, source) == "path")
            {
                let line = attribute.start_position().row + 1;
                self.path_attributes.push(format!("{file}:{line}"));
            }
            match item.child_by_field_name("body") {
                Some(body) => {
                    self.add(child, (file, source), named_children(body), &child_dir);
                }
                None if has_path_attribute(item, source) => {}
                None => {
                    let found = [
                        crate::join(dir, &format!("{name}.rs")),
                        crate::join(dir, &format!("{name}/mod.rs")),
                    ]
                    .into_iter()
                    .find_map(|candidate| {
                        self.parsed
                            .get_key_value(candidate.as_str())
                            .map(|(k, _)| *k)
                    });
                    if let Some(file) = found {
                        self.add_file(child, file, &child_dir);
                    }
                }
            }
        }
        id
    }
}

fn has_path_attribute(item: Ts, source: &str) -> bool {
    attributes(item, source)
        .iter()
        .any(|(path, _)| *path == "path")
}
