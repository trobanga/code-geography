//! All front ends together: one rule for the files that any of them wants, one call that
//! indexes them all. A repository can hold Java and Rust; the graphs are joined.

pub use cg_core::SourceFile;
use cg_core::{Graph, Node};
use cg_index_java::CallReport;
use serde::Serialize;
use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// What the front ends could not resolve, one part per language.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Report {
    /// How the call sites of the Java sources were resolved.
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
        calls: calls.calls,
        rust: rust_report,
    };
    (graph, report)
}

pub fn index(root: &Path) -> io::Result<Graph> {
    index_with_report(root).map(|(graph, _)| graph)
}

/// Walks `root`, reads the files that `is_input` accepts and indexes them with
/// `index_sources`.
pub fn index_with_report(root: &Path) -> io::Result<(Graph, Report)> {
    let mut files = Vec::new();
    for path in input_files(root)? {
        let relative = slash_path(path.strip_prefix(root).unwrap_or(&path));
        // Only the place of a `pom.xml` counts.
        let text = if relative.ends_with("pom.xml") {
            String::new()
        } else {
            String::from_utf8_lossy(&fs::read(&path)?).into_owned()
        };
        files.push(SourceFile {
            path: relative,
            text,
        });
    }
    Ok(index_sources(&file_name(root), &files))
}

/// The files below `root` for which `is_input` holds, without the directories that
/// `cg_core::holds_no_sources` rejects.
fn input_files(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            if path.is_dir() {
                if !cg_core::holds_no_sources(&file_name(&path)) {
                    pending.push(path);
                }
            } else if is_input(&slash_path(path.strip_prefix(root).unwrap_or(&path))) {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

/// The path with `/` as separator, whatever the platform.
fn slash_path(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}
