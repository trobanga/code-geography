//! All front ends together: one rule for the files that any of them wants, one call that
//! indexes them all. A repository can hold Java, Rust, Clojure, Elixir and
//! TypeScript; the graphs are joined.

pub use cg_core::SourceFile;
use cg_core::{Change, DiffInfo, EdgeSet, Graph, Node, NodeKind, PackageHolds};
use cg_index_java::CallReport;
use serde::Serialize;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::io;
use std::path::Path;

/// What the front ends could not resolve, one part per language.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Report {
    /// How the call sites of all front ends were resolved: the sum of the Java part and
    /// `rust.calls`, `clojure.calls`, `elixir.calls` and `typescript.calls`. The viewer reads
    /// this.
    pub calls: CallReport,
    pub rust: cg_index_rust::Report,
    pub clojure: cg_index_clojure::Report,
    /// What the Elixir front end could not read or place, and its call counts.
    pub elixir: cg_index_elixir::Report,
    /// What the TypeScript front end could not read or resolve, and its call counts.
    pub typescript: cg_index_typescript::Report,
}

/// Whether some front end wants the file at `path` (relative to the root, `/` as
/// separator).
pub fn is_input(path: &str) -> bool {
    cg_index_java::is_input(path)
        || cg_index_rust::is_input(path)
        || cg_index_clojure::is_input(path)
        || cg_index_elixir::is_input(path)
        || cg_index_typescript::is_input(path)
}

/// Indexes sources that are given as data with each front end and joins the graphs.
/// The result does not depend on the order of `files`. A module that two front ends
/// declare for the same directory (`mod:<dir>`) is one node, the first one.
pub fn index_sources(root_name: &str, files: &[SourceFile]) -> (Graph, Report) {
    let layout = cg_index_clojure::Layout::new(files);
    let test_code = |path: &str| layout.module_test_code(path).unwrap_or(false);
    let modules = cg_index_java::Modules {
        dirs: layout.module_dirs().map(str::to_string).collect(),
        test_code: &test_code,
    };
    let java = cg_index_java::index_sources_in(root_name, files, &modules);
    let (rust, rust_report) = cg_index_rust::index_sources(root_name, files);
    let clojure = cg_index_clojure::index_sources_with(root_name, files, &java.classes);
    let (elixir, elixir_report) = cg_index_elixir::index_sources(root_name, files);
    let (typescript, typescript_report) = cg_index_typescript::index_sources(root_name, files);
    let (mut java_graph, mut clojure_graph) = (java.graph, clojure.graph);
    nest_shared_packages(
        &mut java_graph,
        &mut clojure_graph,
        &java.packages,
        &clojure.packages,
    );
    let (java, calls, clojure, clojure_report) =
        (java_graph, java.report, clojure_graph, clojure.report);
    let mut seen: HashSet<String> = HashSet::new();
    let nodes: Vec<Node> = java
        .nodes
        .into_iter()
        .chain(rust.nodes)
        .chain(clojure.nodes)
        .chain(elixir.nodes)
        .chain(typescript.nodes)
        .filter(|n| seen.insert(n.id.clone()))
        .collect();
    // The Clojure front end does not know where the Java types are in the tree: an edge from
    // a namespace to a Java type inside it (same package) is for this place to drop.
    let parents: HashMap<&str, Option<&str>> = nodes
        .iter()
        .map(|n| (n.id.as_str(), n.parent.as_deref()))
        .collect();
    let inside = |outer: &str, inner: &str| {
        let mut current = parents.get(inner).copied().flatten();
        while let Some(id) = current {
            if id == outer {
                return true;
            }
            current = parents.get(id).copied().flatten();
        }
        false
    };
    let clojure_edges = clojure
        .edges
        .into_iter()
        .filter(|e| !inside(&e.from, &e.to) && !inside(&e.to, &e.from));
    // The same edge of two front ends is one edge with the sites of both.
    let edges: EdgeSet = java
        .edges
        .into_iter()
        .chain(rust.edges)
        .chain(clojure_edges)
        .chain(elixir.edges)
        .chain(typescript.edges)
        .collect();
    let graph = Graph {
        edges: edges.into_edges(&nodes),
        nodes,
        diff: None,
    };
    let report = Report {
        calls: calls.calls
            + rust_report.calls
            + clojure_report.calls
            + elixir_report.calls
            + typescript_report.calls,
        rust: rust_report,
        clojure: clojure_report,
        elixir: elixir_report,
        typescript: typescript_report,
    };
    (graph, report)
}

/// The module that a package node is in: the directory of the `mod:` node at the top of its
/// parent chain.
fn module_dir<'g>(nodes: &HashMap<&'g str, &'g Node>, node: &'g Node) -> Option<&'g str> {
    let mut current = node;
    while let Some(parent) = current.parent.as_deref() {
        current = nodes.get(parent)?;
    }
    current.id.strip_prefix("mod:")
}

/// The packages of a module that holds Java types and has a `deps.edn` nest as one set
/// (ADR 18): the package nodes of both front ends are replaced by those of the union of
/// their package names. The ids stay the same, so the types and functions below keep their
/// parents; a prefix node, the parent and the display name can differ from what each front
/// end made alone.
fn nest_shared_packages(
    java: &mut Graph,
    clojure: &mut Graph,
    java_holds: &PackageHolds,
    clojure_holds: &PackageHolds,
) {
    let modules: BTreeSet<&str> = java_holds.keys().map(|(m, _)| m.as_str()).collect();
    if modules.is_empty() {
        return;
    }
    let mut union = PackageHolds::new();
    for (key, test) in java_holds.iter().chain(clojure_holds) {
        if modules.contains(key.0.as_str()) {
            *union.entry(key.clone()).or_insert(true) &= *test;
        }
    }
    for graph in [&mut *java, &mut *clojure] {
        let index: HashMap<&str, &Node> = graph.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
        let old: HashSet<String> = graph
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Package)
            .filter(|n| module_dir(&index, n).is_some_and(|m| modules.contains(m)))
            .map(|n| n.id.clone())
            .collect();
        graph.nodes.retain(|n| !old.contains(&n.id));
    }
    for (module, nodes) in cg_core::marked_package_nodes(&union) {
        let id = |name: &str| format!("pkg:{module}:{name}");
        for node in nodes {
            java.nodes.push(Node {
                id: id(&node.name),
                parent: Some(match &node.parent {
                    Some(parent) => id(parent),
                    None => format!("mod:{module}"),
                }),
                kind: NodeKind::Package,
                name: node.display,
                file: None,
                span: None,
                test: node.test,
                hash: None,
                change: None,
            });
        }
    }
}

/// What names a commit overlay besides the two source sets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffSpec {
    /// Name of the base revision, as `DiffInfo.base`.
    pub base: String,
    /// Name of the head revision, as `DiffInfo.head`.
    pub head: String,
    /// Every file that differs between the revisions, also those that no front end
    /// reads, in the order that `DiffInfo.files` gets.
    pub changed: Vec<(String, Change)>,
}

/// Whether two source sets hold the same paths with the same texts, in any order.
fn same_sources(a: &[SourceFile], b: &[SourceFile]) -> bool {
    fn set(s: &[SourceFile]) -> std::collections::BTreeMap<&str, &str> {
        s.iter()
            .map(|f| (f.path.as_str(), f.text.as_str()))
            .collect()
    }
    a.len() == b.len() && set(a) == set(b)
}

/// The overlay of two source sets: both are indexed with `index_sources`, joined with
/// `cg_core::diff`, and `spec` fills `DiffInfo`. The report is the one of the head index;
/// the base index only provides the removed nodes and edges. Equal source sets (a clean
/// working tree, ADR 0039) are indexed once: the index does not depend on the order of the
/// files, so the base index would be the head index.
pub fn diff_sources(
    root_name: &str,
    base: &[SourceFile],
    head: &[SourceFile],
    spec: DiffSpec,
) -> (Graph, Report) {
    let (after, report) = index_sources(root_name, head);
    let mut graph = if same_sources(base, head) {
        cg_core::diff(&after, &after)
    } else {
        cg_core::diff(&index_sources(root_name, base).0, &after)
    };
    let mut files = cg_core::file_changes(&graph, &spec.changed);
    // The rule of ADR 8 calls `src/blaze/db/api.clj` test code. A file of a `deps.edn`
    // module follows the source directories of its `deps.edn` (ADR 23); a removed file's
    // module may exist only in the base.
    let (base_rule, head_rule) = (TestRule::new(base), TestRule::new(head));
    for file in &mut files {
        if let Some(test) = head_rule
            .test_code(&file.path)
            .or_else(|| base_rule.test_code(&file.path))
        {
            file.test = test;
        }
    }
    graph.diff = Some(DiffInfo {
        base: spec.base,
        head: spec.head,
        files,
    });
    (graph, report)
}

/// Which files are test code by the source directories of a `deps.edn` instead of the rule
/// of ADR 8: Clojure files and Java files in a module that has no `pom.xml`, and the
/// Elixir files of a Mix project (the directory `test`, ADR 35); TypeScript files are test
/// code by their name (`*.test.ts`, `*.spec.ts`, `__tests__`, `__mocks__`), not by the path.
struct TestRule {
    layout: cg_index_clojure::Layout,
    elixir: cg_index_elixir::Layout,
    pom_dirs: Vec<String>,
}

impl TestRule {
    fn new(files: &[SourceFile]) -> TestRule {
        let pom_dirs = files
            .iter()
            .filter_map(|f| match f.path.rsplit_once('/') {
                _ if f.path == "pom.xml" => Some(String::new()),
                Some((dir, "pom.xml")) => Some(dir.to_string()),
                _ => None,
            })
            .collect();
        TestRule {
            layout: cg_index_clojure::Layout::new(files),
            elixir: cg_index_elixir::Layout::new(files),
            pom_dirs,
        }
    }

    /// `None`: the rule of ADR 8 holds.
    fn test_code(&self, path: &str) -> Option<bool> {
        if path.ends_with(".ts") || path.ends_with(".tsx") {
            return Some(cg_index_typescript::is_test_file(path));
        }
        if !path.ends_with(".java") {
            return self
                .layout
                .test_code(path)
                .or_else(|| self.elixir.test_code(path));
        }
        let below = |dir: &&str| {
            dir.is_empty() || path.strip_prefix(dir).is_some_and(|r| r.starts_with('/'))
        };
        let nearest = |dirs: &mut dyn Iterator<Item = &str>| dirs.filter(below).map(str::len).max();
        let pom = nearest(&mut self.pom_dirs.iter().map(String::as_str));
        let deps = nearest(&mut self.layout.module_dirs());
        // The nearest module wins, a directory with both is a Maven module.
        match (pom, deps) {
            (Some(pom), Some(deps)) if deps <= pom => None,
            (None, Some(_)) | (Some(_), Some(_)) => self.layout.module_test_code(path),
            _ => None,
        }
    }
}

pub fn index(root: &Path) -> io::Result<Graph> {
    index_with_report(root).map(|(graph, _)| graph)
}

/// Walks `root` with `cg_walk::read_sources`, keeps the files that `is_input` accepts and
/// indexes them with `index_sources`.
pub fn index_with_report(root: &Path) -> io::Result<(Graph, Report)> {
    let files = cg_walk::read_sources(root, is_input)?;
    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(index_sources(&name, &files))
}
