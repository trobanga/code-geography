//! All front ends together: one rule for the files that any of them wants, one call that
//! indexes them all. A repository can hold Java and Rust; the graphs are joined.

pub use cg_core::SourceFile;
use cg_core::{Graph, Node};
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
