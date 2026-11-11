//! Clojure front end: turns the sources of a `deps.edn` project into a graph.

mod declare;
mod deps;
mod ns;
mod syntax;

pub use cg_core::SourceFile;
use cg_core::{Edge, EdgeKind, Graph, Node, NodeKind, PackageNode, Span};
use deps::SourceDirs;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use tree_sitter::Parser;

/// What the indexer could not read or place.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Report {
    /// Source files below a `deps.edn` directory but outside all its source directories
    /// (`build.clj` scripts, `dev` and `test` directories that no alias names).
    pub files_outside_source_dirs: usize,
    /// Source files with no `deps.edn` above them.
    pub files_without_deps_edn: usize,
    /// Source files that are read and hold no `ns` form: no nodes.
    pub files_without_ns: usize,
    /// Files that name a namespace that an earlier file (in path order) names too: they
    /// give no nodes. Sorted.
    pub duplicate_namespaces: Vec<String>,
    /// The top-level forms that give no node, counted by their head symbol.
    pub forms_without_node: BTreeMap<String, usize>,
}

/// Whether the indexer wants the file at `path` (relative to the root, `/` as separator):
/// a `.clj` or `.cljc` file or a `deps.edn`, not below a directory that
/// `cg_core::holds_no_sources`.
pub fn is_input(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    let source = [".clj", ".cljc"]
        .iter()
        .any(|ext| name.len() > ext.len() && name.ends_with(ext))
        // The Leiningen project file is Clojure code, but it names no namespace.
        && name != "project.clj";
    (name == "deps.edn" || source) && cg_core::outside_excluded_directories(path)
}

/// The `deps.edn` modules of a source set: what is needed to tell whether a path is test
/// code without reading the sources (the changed files of an overlay).
#[derive(Debug, Clone, Default)]
pub struct Layout {
    modules: Vec<(String, SourceDirs)>,
}

impl Layout {
    /// Reads the `deps.edn` files among `files`; other files are ignored.
    pub fn new(files: &[SourceFile]) -> Layout {
        let mut parser = clojure_parser();
        let modules = files
            .iter()
            .filter(|f| is_input(&f.path))
            .filter_map(|f| {
                let dir = match f.path.rsplit_once('/') {
                    Some((dir, "deps.edn")) => dir,
                    None if f.path == "deps.edn" => "",
                    _ => return None,
                };
                Some((dir.to_string(), SourceDirs::parse(&mut parser, &f.text)))
            })
            .collect();
        Layout { modules }
    }

    /// Whether the Clojure file at `path` is test code by ADR 23: the longest source
    /// directory of the nearest `deps.edn` above it comes from an alias. `None` for a
    /// path that is no `.clj` or `.cljc` file, has no `deps.edn` above it, or lies in no
    /// source directory of that module.
    pub fn test_code(&self, path: &str) -> Option<bool> {
        if !(path.ends_with(".clj") || path.ends_with(".cljc")) {
            return None;
        }
        let (dir, dirs) = self
            .modules
            .iter()
            .filter(|(dir, _)| in_directory(dir, path))
            .max_by_key(|(dir, _)| dir.len())?;
        dirs.test_code(relative_to(dir, path))
    }
}

fn clojure_parser() -> Parser {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_clojure_orchard::LANGUAGE.into())
        .expect("the Clojure grammar matches the tree-sitter version");
    parser
}

/// Whether `path` is below the directory `dir` (empty: the root).
fn in_directory(dir: &str, path: &str) -> bool {
    dir.is_empty() || path.strip_prefix(dir).is_some_and(|r| r.starts_with('/'))
}

fn relative_to<'a>(dir: &str, path: &'a str) -> &'a str {
    path.strip_prefix(dir)
        .map_or(path, |rest| rest.trim_start_matches('/'))
}

/// Indexes sources that are given as data. The result does not depend on the order of
/// `files`. Files that `is_input` rejects are ignored.
pub fn index_sources(root_name: &str, files: &[SourceFile]) -> (Graph, Report) {
    let mut files: Vec<&SourceFile> = files.iter().filter(|f| is_input(&f.path)).collect();
    // The order of the components, as `Path` compares, not of the bytes.
    files.sort_by(|a, b| {
        a.path
            .split('/')
            .cmp(b.path.split('/'))
            .then_with(|| a.text.cmp(&b.text))
    });
    files.dedup_by(|a, b| a.path == b.path);

    let mut parser = clojure_parser();
    let mut graph = Graph::default();
    let mut module_dirs: Vec<&str> = Vec::new();
    let mut source_dirs: HashMap<&str, SourceDirs> = HashMap::new();
    for file in &files {
        let dir = match file.path.rsplit_once('/') {
            Some((dir, "deps.edn")) => dir,
            None if file.path == "deps.edn" => "",
            _ => continue,
        };
        module_dirs.push(dir);
        source_dirs.insert(dir, SourceDirs::parse(&mut parser, &file.text));
        graph.nodes.push(Node {
            id: format!("mod:{dir}"),
            parent: None,
            kind: NodeKind::Module,
            name: dir
                .rsplit('/')
                .next()
                .filter(|n| !n.is_empty())
                .unwrap_or(root_name)
                .to_string(),
            file: None,
            span: None,
            test: false,
            hash: None,
            change: None,
        });
    }

    let mut report = Report::default();
    let mut packages: BTreeMap<(Option<&str>, String), bool> = BTreeMap::new();
    let mut declared: Vec<Node> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut namespaces: BTreeSet<String> = BTreeSet::new();
    let mut requires: Vec<(String, Vec<String>)> = Vec::new();
    let mut package_ids: BTreeMap<String, String> = BTreeMap::new();
    for file in files.iter().filter(|f| !f.path.ends_with("deps.edn")) {
        let Some(module) = module_of(&module_dirs, &file.path) else {
            report.files_without_deps_edn += 1;
            continue;
        };
        let Some(is_test) = source_dirs[module].test_code(relative_to(module, &file.path)) else {
            report.files_outside_source_dirs += 1;
            continue;
        };
        let Some(tree) = parser.parse(&file.text, None) else {
            continue;
        };
        let source = file.text.as_str();
        let forms = syntax::flat_items(tree.root_node());
        let Some((ns_form, namespace)) = ns::namespace(&forms, source) else {
            report.files_without_ns += 1;
            continue;
        };
        if !namespaces.insert(namespace.clone()) {
            report.duplicate_namespaces.push(file.path.clone());
            continue;
        }
        packages.insert((Some(module), namespace.clone()), is_test);
        package_ids.insert(namespace.clone(), package_id(module, &namespace));
        requires.push((namespace.clone(), ns::requires(ns_form, source)));
        let rest: Vec<_> = forms.iter().copied().filter(|f| *f != ns_form).collect();
        let decls =
            declare::declarations(&rest, source, &namespace, &mut report.forms_without_node);
        // The methods of a type that was declared before are skipped with it.
        let mut skipped: BTreeSet<String> = BTreeSet::new();
        for decl in decls {
            if decl.parent.as_ref().is_some_and(|p| skipped.contains(p)) {
                continue;
            }
            if !seen.insert(decl.id.clone()) {
                skipped.insert(decl.id);
                continue;
            }
            declared.push(Node {
                hash: Some(own_hash(&decl, source)),
                id: decl.id,
                parent: Some(
                    decl.parent
                        .unwrap_or_else(|| package_id(module, &namespace)),
                ),
                kind: decl.kind,
                name: decl.name,
                file: Some(file.path.clone()),
                span: Some(span_of(decl.node)),
                test: is_test,
                change: None,
            });
        }
    }
    for (module, nodes) in cg_core::package_nodes(&packages) {
        let module = module.expect("every namespace has a module");
        for node in &nodes {
            // A namespace of test code is test code, so that hiding the tests also
            // hides the namespaces that hold no node (a file of `deftest` only). Unless
            // a namespace below it is main code: it would be hidden with its parent.
            let is_test = |n: &PackageNode| {
                packages
                    .get(&(Some(module), n.name.clone()))
                    .copied()
                    .unwrap_or(n.test)
            };
            let test = is_test(node)
                && nodes
                    .iter()
                    .filter(|n| is_below(&n.name, &node.name))
                    .all(is_test);
            graph.nodes.push(Node {
                id: package_id(module, &node.name),
                parent: Some(match &node.parent {
                    Some(parent) => package_id(module, parent),
                    None => format!("mod:{module}"),
                }),
                kind: NodeKind::Package,
                name: node.display.clone(),
                file: None,
                span: None,
                test,
                hash: None,
                change: None,
            });
        }
    }
    graph.nodes.append(&mut declared);
    graph.edges = imports(&graph, &package_ids, &requires);
    (graph, report)
}

/// Whether `name` is a namespace below `prefix`, on `.` boundaries.
fn is_below(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix)
        .is_some_and(|rest| rest.starts_with('.'))
}

fn package_id(module: &str, namespace: &str) -> String {
    format!("pkg:{module}:{namespace}")
}

/// The directory of the nearest `deps.edn` above the file.
fn module_of<'a>(module_dirs: &[&'a str], path: &str) -> Option<&'a str> {
    module_dirs
        .iter()
        .copied()
        .filter(|dir| in_directory(dir, path))
        .max_by_key(|dir| dir.len())
}

/// The hash of the own source text of a declaration: the text without the members that
/// are nodes themselves.
fn own_hash(decl: &declare::Decl, source: &str) -> String {
    let mut own = String::new();
    let mut position = decl.node.start_byte();
    for member in &decl.members {
        own.push_str(&source[position..member.start_byte()]);
        position = member.end_byte();
    }
    own.push_str(&source[position..decl.node.end_byte()]);
    cg_core::text_hash(&own)
}

/// The lines of a form: 1-based, inclusive.
fn span_of(node: tree_sitter::Node) -> Span {
    Span {
        start_line: node.start_position().row as u32 + 1,
        end_line: node.end_position().row as u32 + 1,
    }
}

/// The `imports` edges from each namespace to the namespaces of the repository that it
/// requires, but none between a node and its ancestor or descendant in the node tree.
fn imports(
    graph: &Graph,
    package_ids: &BTreeMap<String, String>,
    requires: &[(String, Vec<String>)],
) -> Vec<Edge> {
    let parents: HashMap<&str, Option<&str>> = graph
        .nodes
        .iter()
        .map(|n| (n.id.as_str(), n.parent.as_deref()))
        .collect();
    let is_ancestor = |ancestor: &str, node: &str| {
        let mut current = parents.get(node).copied().flatten();
        while let Some(id) = current {
            if id == ancestor {
                return true;
            }
            current = parents.get(id).copied().flatten();
        }
        false
    };
    let mut edges = BTreeSet::new();
    for (from, required) in requires {
        let from = &package_ids[from];
        for to in required.iter().filter_map(|name| package_ids.get(name)) {
            if from != to && !is_ancestor(from, to) && !is_ancestor(to, from) {
                edges.insert(Edge {
                    from: from.clone(),
                    to: to.clone(),
                    kind: EdgeKind::Imports,
                    change: None,
                });
            }
        }
    }
    edges.into_iter().collect()
}
