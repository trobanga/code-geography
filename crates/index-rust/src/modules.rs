//! The tree of `mod`s of the crate targets. A mod is a node, a file is an attribute:
//! the tree is found by following `mod x;` from the root of a target.

use crate::syntax::{attribute_items, attribute_path, attributes, is_test_item, named_children};
use std::collections::{BTreeSet, HashMap};
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

pub fn build<'t>(parsed: &'t HashMap<&'t str, Parsed<'t>>, roots: &[Root<'t>]) -> Mods<'t> {
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
    Mods {
        mods: builder.mods,
        reached: builder.reached,
        path_attributes: builder.path_attributes,
    }
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
