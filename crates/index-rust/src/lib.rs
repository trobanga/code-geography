//! Rust front end: turns the sources of a Cargo project into a graph.

pub use cg_core::SourceFile;
use cg_core::{Graph, Node, NodeKind};
use serde::Serialize;
mod calls;
mod declare;
mod edges;
mod model;
mod modules;
mod syntax;
mod types;
mod world;

use modules::{Parsed, Root};
use std::collections::{BTreeSet, HashMap, HashSet};
use tree_sitter::Parser;

/// How the paths of the sources were resolved. One count per written path: a type name,
/// a path in an expression or pattern, each path of a `use`. Names that a generic
/// parameter or a local item hides, and primitive types, are not counted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct PathReport {
    /// The path reaches an item of the repository.
    pub resolved: usize,
    /// The path leaves the repository: `std`, the prelude, a dependency.
    pub external: usize,
    /// The path should name something in the repository and cannot be followed.
    pub unresolved: usize,
}

impl PathReport {
    pub(crate) fn count(&mut self, resolution: world::Res) {
        match resolution {
            world::Res::Item(..) => self.resolved += 1,
            world::Res::External => self.external += 1,
            world::Res::Unresolved => self.unresolved += 1,
            world::Res::Hidden => {}
        }
    }

    pub(crate) fn add(&mut self, other: PathReport) {
        self.resolved += other.resolved;
        self.external += other.external;
        self.unresolved += other.unresolved;
    }
}

/// What the indexer could not follow or resolve.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Report {
    /// `.rs` files that no `mod` declaration reaches: they are not compiled, so they
    /// give no nodes. Sorted.
    pub unreached_files: Vec<String>,
    /// The places of `#[path = "..."]` attributes (`file:line`). The indexer does not
    /// follow them, so the file behind such a `mod` is missing.
    pub path_attributes: Vec<String>,
    pub paths: PathReport,
    /// How the call sites were resolved.
    pub calls: cg_core::CallReport,
    /// Of the calls to a function: those where the function is named, not called
    /// (`.map(parse)`).
    pub function_values: usize,
    /// Names followed by `(` in the arguments of macros. The macro decides what they
    /// mean, so they give no edge and are not in `calls`: the size of the gap.
    pub macro_call_sites: usize,
}

/// Whether the indexer wants the file at `path` (relative to the root, `/` as separator):
/// a `.rs` file or a `Cargo.toml`, not below a directory that `cg_core::holds_no_sources`.
pub fn is_input(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    let wanted = name == "Cargo.toml" || (name.len() > ".rs".len() && name.ends_with(".rs"));
    wanted && cg_core::outside_excluded_directories(path)
}

/// A Cargo package: a `Cargo.toml` with a `[package]` table.
struct Package {
    /// Directory relative to the root, empty for the root.
    dir: String,
    /// The package name as written.
    name: String,
    crate_name: String,
    /// The names that the manifest lets code use as crates, with `_` for `-`.
    dependencies: HashSet<String>,
}

/// Indexes sources that are given as data. The result does not depend on the order of
/// `files`. Files that `is_input` rejects are ignored.
pub fn index_sources(_root_name: &str, files: &[SourceFile]) -> (Graph, Report) {
    let mut files: Vec<&SourceFile> = files.iter().filter(|f| is_input(&f.path)).collect();
    // The order of the components, as `Path` compares, not of the bytes.
    files.sort_by(|a, b| {
        a.path
            .split('/')
            .cmp(b.path.split('/'))
            .then_with(|| a.text.cmp(&b.text))
    });
    files.dedup_by(|a, b| a.path == b.path);
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .expect("the Rust grammar matches the tree-sitter version");
    let parsed: HashMap<&str, Parsed> = files
        .iter()
        .filter(|f| f.path.ends_with(".rs"))
        .filter_map(|f| {
            let tree = parser.parse(&f.text, None)?;
            Some((
                f.path.as_str(),
                Parsed {
                    source: f.text.as_str(),
                    tree,
                },
            ))
        })
        .collect();

    let packages = packages(&files);
    let rs_files: BTreeSet<&str> = parsed.keys().copied().collect();
    let roots: Vec<Root> = packages
        .iter()
        .enumerate()
        .flat_map(|(package, p)| targets(package, &p.dir, &rs_files))
        .collect();
    let package_dirs: Vec<&str> = packages.iter().map(|p| p.dir.as_str()).collect();
    let tree = modules::build(&parsed, &roots, &package_dirs);
    let mods = tree.mods;
    let mut report = Report {
        unreached_files: files
            .iter()
            .map(|f| f.path.as_str())
            .filter(|p| p.ends_with(".rs") && *p != "build.rs" && !p.ends_with("/build.rs"))
            .filter(|p| !tree.reached.contains(p))
            .map(str::to_string)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
        path_attributes: tree.path_attributes,
        paths: PathReport::default(),
        calls: cg_core::CallReport::default(),
        function_values: 0,
        macro_call_sites: 0,
    };

    let mut graph = Graph::default();
    for package in &packages {
        graph.nodes.push(Node {
            id: format!("mod:{}", package.dir),
            parent: None,
            kind: NodeKind::Module,
            name: package.name.clone(),
            file: None,
            span: None,
            test: false,
            hash: None,
            change: None,
        });
    }
    let package_names: Vec<(&str, &str)> = packages
        .iter()
        .map(|p| (p.dir.as_str(), p.crate_name.as_str()))
        .collect();
    let mut decls = declare::declare(&package_names, &mods);
    declare::dedup(&mut decls);
    let dependencies: Vec<&HashSet<String>> = packages.iter().map(|p| &p.dependencies).collect();
    let crate_names: Vec<&str> = package_names.iter().map(|(_, name)| *name).collect();
    let impl_decls = declare::declare_impls(
        &world::World::new(&crate_names, &dependencies, &mods, &decls),
        &package_names,
        &mods,
    );
    decls.extend(impl_decls);
    declare::dedup(&mut decls);
    let mut paths = PathReport::default();
    let edges = {
        let world = world::World::new(&crate_names, &dependencies, &mods, &decls);
        let mut edges = edges::compute(&world, &package_names, &mods, &decls, &mut paths);
        let (calls, stats) = calls::compute(&world, &package_names, &mods, &decls, &mut edges);
        report.calls = calls;
        report.function_values = stats.function_values;
        report.macro_call_sites = stats.macro_calls;
        edges::drop_edges_to_ancestors(&mut edges, &decls, &mods, &package_names);
        edges
    };
    report.paths = paths;
    for m in mods.iter().filter(|m| m.has_node) {
        let (dir, crate_name) = package_names[m.package];
        graph.nodes.push(Node {
            id: declare::container_id(dir, crate_name, m),
            parent: Some(match m.parent {
                Some(parent) => declare::container_id(dir, crate_name, &mods[parent]),
                None => format!("mod:{dir}"),
            }),
            kind: NodeKind::Package,
            name: m.name.clone(),
            file: None,
            span: None,
            test: m.test,
            hash: None,
            change: None,
        });
    }
    // The types before the functions, so that a parent comes before its children.
    for kind in [NodeKind::Type, NodeKind::Function] {
        for decl in decls.iter().filter(|d| d.kind == kind) {
            graph.nodes.push(Node {
                id: decl.id.clone(),
                parent: Some(decl.parent.clone()),
                kind: decl.kind,
                name: decl.name.clone(),
                file: Some(mods[decl.module].file.to_string()),
                span: Some(decl.span),
                test: decl.test,
                hash: Some(decl.hash.clone()),
                change: None,
            });
        }
    }
    graph.edges = edges.into_iter().collect();
    (graph, report)
}

/// The roots of the targets of a package: the lib, or else the main, is the root of the
/// package itself; the other targets are packages of their own below the module.
fn targets<'t>(package: usize, dir: &str, files: &BTreeSet<&'t str>) -> Vec<Root<'t>> {
    let find = |path: String| files.get(path.as_str()).copied();
    let lib = find(join(dir, "src/lib.rs"));
    let main = find(join(dir, "src/main.rs"));
    let mut roots = Vec::new();
    let mut push = |file: Option<&'t str>, path: Option<String>, test: bool| {
        if let Some(file) = file {
            roots.push(Root {
                package,
                file,
                path: path.into_iter().collect(),
                test,
            });
        }
    };
    push(lib, None, false);
    push(
        main,
        if lib.is_some() {
            Some("main".into())
        } else {
            None
        },
        false,
    );
    for (folder, test) in [
        ("src/bin", false),
        ("tests", true),
        ("benches", true),
        ("examples", true),
    ] {
        let prefix = join(dir, &format!("{folder}/"));
        let mut names = BTreeSet::new();
        for file in files.iter().filter_map(|f| f.strip_prefix(prefix.as_str())) {
            match file.split('/').collect::<Vec<_>>()[..] {
                [name] => names.extend(name.strip_suffix(".rs")),
                [name, "main.rs"] => {
                    names.insert(name);
                }
                _ => {}
            }
        }
        let kind = folder.strip_prefix("src/").unwrap_or(folder);
        for name in names {
            let file = find(format!("{prefix}{name}.rs"))
                .or_else(|| find(format!("{prefix}{name}/main.rs")));
            push(file, Some(format!("{kind}/{name}")), test);
        }
    }
    roots
}

/// The packages among `files`.
fn packages(files: &[&SourceFile]) -> Vec<Package> {
    files
        .iter()
        .filter_map(|file| {
            let dir = match file.path.rsplit_once('/') {
                Some((dir, "Cargo.toml")) => dir,
                None if file.path == "Cargo.toml" => "",
                _ => return None,
            };
            let name = package_name(&file.text)?;
            Some(Package {
                dir: dir.to_string(),
                crate_name: name.replace('-', "_"),
                dependencies: dependency_names(&file.text),
                name,
            })
        })
        .collect()
}

/// The names of the dependencies in the `[dependencies]`, `[dev-dependencies]` and
/// `[build-dependencies]` tables (also below `[target.*]`) of a `Cargo.toml`, with `_`
/// for `-`. A name is the key as written: a renamed dependency has its new name.
fn dependency_names(manifest: &str) -> HashSet<String> {
    let mut names = HashSet::new();
    let mut in_table = false;
    for line in manifest.lines() {
        let line = line.trim();
        if let Some(header) = line.strip_prefix('[') {
            let header = header
                .trim_start_matches('[')
                .split(']')
                .next()
                .unwrap_or("");
            let parts: Vec<&str> = header
                .split('.')
                .map(|p| p.trim().trim_matches(['"', '\'']))
                .collect();
            let is_dependencies = |part: &str| {
                matches!(
                    part,
                    "dependencies" | "dev-dependencies" | "build-dependencies"
                )
            };
            match parts.iter().position(|p| is_dependencies(p)) {
                // `[dependencies.name]`
                Some(at) if at + 1 < parts.len() => {
                    names.insert(parts[at + 1].replace('-', "_"));
                    in_table = false;
                }
                Some(_) => in_table = true,
                None => in_table = false,
            }
        } else if in_table && let Some((key, _)) = line.split_once('=') {
            let key = key
                .trim()
                .split('.')
                .next()
                .unwrap_or("")
                .trim_matches(['"', '\'']);
            if !key.is_empty() && !key.starts_with('#') {
                names.insert(key.replace('-', "_"));
            }
        }
    }
    names
}

/// The `name` in the `[package]` table of a `Cargo.toml`.
fn package_name(manifest: &str) -> Option<String> {
    let mut in_package = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_package = line.trim_end_matches(|c: char| c.is_whitespace()) == "[package]";
        } else if in_package
            && let Some(value) = line
                .strip_prefix("name")
                .and_then(|rest| rest.trim_start().strip_prefix('='))
        {
            let value = value.trim();
            let quote = value.chars().next().filter(|c| matches!(c, '"' | '\''))?;
            return value[1..].split(quote).next().map(str::to_string);
        }
    }
    None
}

fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}
