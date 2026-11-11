//! All front ends together: one rule for the files that any of them wants, one call that
//! indexes them all. A repository can hold Java and Rust; the graphs are joined.

pub use cg_core::SourceFile;
use cg_core::{Change, DiffInfo, Graph, Node};
use cg_index_java::CallReport;
use serde::Serialize;
use std::collections::{BTreeSet, HashSet};
use std::io;
use std::path::Path;

/// What the front ends could not resolve, one part per language.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Report {
    /// How the call sites of all front ends were resolved: the sum of the Java part and
    /// `rust.calls`. The viewer reads this.
    pub calls: CallReport,
    pub rust: cg_index_rust::Report,
}

/// Whether some front end wants the file at `path` (relative to the root, `/` as
/// separator).
pub fn is_input(path: &str) -> bool {
    cg_index_java::is_input(path) || cg_index_rust::is_input(path)
}

/// Indexes sources that are given as data with each front end and joins the graphs.
/// The result does not depend on the order of `files`. A module that two front ends
/// declare for the same directory (`mod:<dir>`) is one node, the first one.
pub fn index_sources(root_name: &str, files: &[SourceFile]) -> (Graph, Report) {
    let (java, calls) = cg_index_java::index_sources(root_name, files);
    let (rust, rust_report) = cg_index_rust::index_sources(root_name, files);
    let mut seen: HashSet<String> = HashSet::new();
    let nodes: Vec<Node> = java
        .nodes
        .into_iter()
        .chain(rust.nodes)
        .filter(|n| seen.insert(n.id.clone()))
        .collect();
    let edges: BTreeSet<_> = java.edges.into_iter().chain(rust.edges).collect();
    let graph = Graph {
        nodes,
        edges: edges.into_iter().collect(),
        diff: None,
    };
    let report = Report {
        calls: calls.calls + rust_report.calls,
        rust: rust_report,
    };
    (graph, report)
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

/// The overlay of two source sets: both are indexed with `index_sources`, joined with
/// `cg_core::diff`, and `spec` fills `DiffInfo`. The report is the one of the head index;
/// the base index only provides the removed nodes and edges.
pub fn diff_sources(
    root_name: &str,
    base: &[SourceFile],
    head: &[SourceFile],
    spec: DiffSpec,
) -> (Graph, Report) {
    let (before, _) = index_sources(root_name, base);
    let (after, report) = index_sources(root_name, head);
    let mut graph = cg_core::diff(&before, &after);
    graph.diff = Some(DiffInfo {
        base: spec.base,
        head: spec.head,
        files: cg_core::file_changes(&graph, &spec.changed),
    });
    (graph, report)
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
