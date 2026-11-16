//! Language-neutral model of a code base: a tree of containers with edges
//! between its nodes. No I/O, so the crate builds for native and wasm32.

mod edges;
mod input;
mod packages;
mod report;

pub use edges::EdgeSet;
pub use input::{SourceFile, holds_no_sources, outside_excluded_directories, text_hash};
pub use packages::{ClassTable, PackageHolds, PackageNode, marked_package_nodes, package_nodes};
pub use report::CallReport;

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Module,
    Package,
    Type,
    Function,
    /// A top-level value that is not a function (a Clojure `def`). A leaf, a sibling of
    /// functions.
    Value,
}

/// Lines of a declaration in its file: 1-based, inclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub start_line: u32,
    pub end_line: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub parent: Option<String>,
    pub kind: NodeKind,
    pub name: String,
    /// Path relative to the indexed root. Set on types and functions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// Where the declaration sits in `file`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<Span>,
    /// The declaration is in test sources, for example below `src/test/`. Only
    /// meaningful on types and functions.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub test: bool,
    /// 64-bit FNV-1a hash (16 hex digits) of the node's own source text, without
    /// the text of nested declarations that are nodes themselves. Set on types
    /// and functions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    /// What a commit did to the node. Only set in a diff graph.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change: Option<Change>,
}

/// What a commit did to a node or an edge. On edges only `Added` and `Removed` occur.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Change {
    Added,
    Removed,
    Modified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    Extends,
    Implements,
    Imports,
    TypeRef,
    Calls,
    /// A function or value names a value node (parallel to `TypeRef` for types).
    ValueRef,
}

/// The lines of one file where the source makes an edge (ADR 0032): 1-based.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileSites {
    /// Path as `Node::file`. `None`: the file of the `from` node of the edge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// Ascending, each line once.
    pub lines: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub kind: EdgeKind,
    /// What a commit did to the edge. Only set in a diff graph.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change: Option<Change>,
    /// Where the source makes the edge: one entry per file, the entry without `file` first,
    /// then by path. Not part of the identity of the edge, which is `(from, to, kind)`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sites: Vec<FileSites>,
}

/// The two revisions that a diff graph compares, as the user named them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffInfo {
    pub base: String,
    pub head: String,
    /// Each file that the commit changed, whether or not it holds declarations.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<FileChange>,
}

/// A file that a commit changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChange {
    /// Relative to the repository root, with `/` as separator.
    pub path: String,
    pub change: Change,
    /// Id of the module node whose directory is the longest prefix of the path.
    pub module: Option<String>,
    /// The file is in test sources, see `in_test_sources`.
    pub test: bool,
}

/// Whether a directory `src` of a `/`-separated file path is followed by a directory
/// other than `main`: `src/test`, `src/e2e`, `src/it`, ...
pub fn in_test_sources(file: &str) -> bool {
    let dirs: Vec<&str> = file.split('/').collect();
    let dirs = &dirs[..dirs.len() - 1];
    dirs.windows(2).any(|w| w[0] == "src" && w[1] != "main")
}

/// The changed files of a commit, in the given order, each with the module of
/// `graph` that contains it. Module ids are `mod:<directory>`; the directory of the
/// repository root is empty.
pub fn file_changes(graph: &Graph, changed: &[(String, Change)]) -> Vec<FileChange> {
    let modules: Vec<(&str, &str)> = graph
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Module)
        .filter_map(|n| Some((n.id.as_str(), n.id.strip_prefix("mod:")?)))
        .collect();
    changed
        .iter()
        .map(|(path, change)| {
            let module = modules
                .iter()
                .filter(|(_, dir)| {
                    dir.is_empty()
                        || path
                            .strip_prefix(dir)
                            .is_some_and(|rest| rest.starts_with('/'))
                })
                .max_by_key(|(_, dir)| dir.len())
                .map(|(id, _)| id.to_string());
            FileChange {
                path: path.clone(),
                change: *change,
                module,
                test: in_test_sources(path),
            }
        })
        .collect()
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    /// Set on a diff graph: the revisions that were compared.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff: Option<DiffInfo>,
}

impl Graph {
    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn has_edge(&self, from: &str, to: &str, kind: EdgeKind) -> bool {
        self.edge(from, to, kind).is_some()
    }

    /// The edge with the identity `(from, to, kind)`.
    pub fn edge(&self, from: &str, to: &str, kind: EdgeKind) -> Option<&Edge> {
        self.edges
            .iter()
            .find(|e| e.from == from && e.to == to && e.kind == kind)
    }

    /// The edges that a line from the box `from` to the box `to` stands for: each edge that
    /// starts at `from` or below it and ends at `to` or below it, in the order of the graph.
    /// Empty when both are the same node, when one contains the other (the cut draws no line
    /// between such boxes), or when the graph has no such node.
    pub fn edges_between(&self, from: &str, to: &str) -> Vec<&Edge> {
        let parents: HashMap<&str, Option<&str>> = self
            .nodes
            .iter()
            .map(|n| (n.id.as_str(), n.parent.as_deref()))
            .collect();
        let at_or_below = |id: &str, top: &str| {
            std::iter::successors(Some(id), |id| parents.get(id).copied().flatten())
                .any(|ancestor| ancestor == top)
        };
        if !parents.contains_key(from)
            || !parents.contains_key(to)
            || at_or_below(from, to)
            || at_or_below(to, from)
        {
            return Vec::new();
        }
        self.edges
            .iter()
            .filter(|e| at_or_below(&e.from, from) && at_or_below(&e.to, to))
            .collect()
    }

    /// The graph without the `Imports` edges that say nothing new: an import from
    /// `F` to `X` is dropped when `F` or a descendant of `F` has an edge of another
    /// kind to `X` or to a descendant of `X`.
    pub fn without_redundant_imports(&self) -> Graph {
        use std::collections::{HashMap, HashSet};

        let parents: HashMap<&str, Option<&str>> = self
            .nodes
            .iter()
            .map(|n| (n.id.as_str(), n.parent.as_deref()))
            .collect();
        let ancestors_or_self = |id: &'_ str| {
            let parents = &parents;
            std::iter::successors(parents.get_key_value(id).map(|(id, _)| *id), move |id| {
                parents.get(id).copied().flatten()
            })
        };
        // Each (user, used) pair that a use covers, for all ancestors of both ends.
        let mut uses: HashSet<(&str, &str)> = HashSet::new();
        for edge in self.edges.iter().filter(|e| e.kind != EdgeKind::Imports) {
            for from in ancestors_or_self(&edge.from) {
                for to in ancestors_or_self(&edge.to) {
                    uses.insert((from, to));
                }
            }
        }
        let edges = self
            .edges
            .iter()
            .filter(|e| {
                e.kind != EdgeKind::Imports || !uses.contains(&(e.from.as_str(), e.to.as_str()))
            })
            .cloned()
            .collect();
        Graph {
            nodes: self.nodes.clone(),
            edges,
            diff: self.diff.clone(),
        }
    }

    /// The graph without the nodes that have `test == true`, without their
    /// descendants, and without the edges that end at a removed node. A container
    /// that loses all its children this way is removed too.
    pub fn without_tests(&self) -> Graph {
        use std::collections::{HashMap, HashSet};

        let parents: HashMap<&str, Option<&str>> = self
            .nodes
            .iter()
            .map(|n| (n.id.as_str(), n.parent.as_deref()))
            .collect();
        let is_test: HashMap<&str, bool> =
            self.nodes.iter().map(|n| (n.id.as_str(), n.test)).collect();
        // A node goes if it or one of its ancestors is a test node.
        let mut removed: HashSet<&str> = self
            .nodes
            .iter()
            .map(|n| n.id.as_str())
            .filter(|id| {
                let mut current = Some(*id);
                while let Some(node) = current {
                    if is_test.get(node) == Some(&true) {
                        return true;
                    }
                    current = parents.get(node).copied().flatten();
                }
                false
            })
            .collect();
        // A container that lost children and has none left goes too, up the tree.
        loop {
            let mut lost: HashSet<&str> = HashSet::new();
            let mut kept: HashSet<&str> = HashSet::new();
            for node in &self.nodes {
                if let Some(parent) = node.parent.as_deref() {
                    let side = if removed.contains(node.id.as_str()) {
                        &mut lost
                    } else {
                        &mut kept
                    };
                    side.insert(parent);
                }
            }
            let emptied: Vec<&str> = lost
                .into_iter()
                .filter(|id| !kept.contains(id) && !removed.contains(id))
                .collect();
            if emptied.is_empty() {
                break;
            }
            removed.extend(emptied);
        }

        Graph {
            nodes: self
                .nodes
                .iter()
                .filter(|n| !removed.contains(n.id.as_str()))
                .cloned()
                .collect(),
            edges: self
                .edges
                .iter()
                .filter(|e| !removed.contains(e.from.as_str()) && !removed.contains(e.to.as_str()))
                .cloned()
                .collect(),
            diff: self.diff.clone(),
        }
    }
}

/// The union of two graphs of the same code base, with each difference marked.
///
/// Nodes come in the order of `after`, then the nodes only in `before` as `Removed`.
/// A node is `Modified` when both sides have a hash and the hashes differ.
pub fn diff(before: &Graph, after: &Graph) -> Graph {
    use std::collections::HashMap;

    let old: HashMap<&str, &Node> = before.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let new: HashMap<&str, &Node> = after.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let kept = after.nodes.iter().map(|node| {
        let change = match old.get(node.id.as_str()) {
            None => Some(Change::Added),
            Some(previous) => match (&previous.hash, &node.hash) {
                (Some(a), Some(b)) if a != b => Some(Change::Modified),
                _ => None,
            },
        };
        Node {
            change,
            ..node.clone()
        }
    });
    let removed = before
        .nodes
        .iter()
        .filter(|n| !new.contains_key(n.id.as_str()))
        .map(|node| Node {
            change: Some(Change::Removed),
            ..node.clone()
        });
    // Identity of an edge is (from, to, kind).
    fn key(e: &Edge) -> (&str, &str, EdgeKind) {
        (e.from.as_str(), e.to.as_str(), e.kind)
    }
    let had: HashSet<_> = before.edges.iter().map(key).collect();
    // An edge of `after` has the sites of `after`, whether `before` has the edge or not.
    let mut edges: BTreeMap<_, Edge> = after
        .edges
        .iter()
        .map(|e| {
            let change = (!had.contains(&key(e))).then_some(Change::Added);
            (
                key(e),
                Edge {
                    change,
                    ..e.clone()
                },
            )
        })
        .collect();
    for e in &before.edges {
        edges
            .entry(key(e))
            .or_insert_with(|| removed_edge(e, &old, &new));
    }
    Graph {
        nodes: kept.chain(removed).collect(),
        edges: edges.into_values().collect(),
        diff: None,
    }
}

/// An edge that only the graph `before` has, as the diff graph holds it. A node that both
/// graphs have is the node of `after` there: when its file differs, the sites that leave
/// the file out must name the file of `before`.
fn removed_edge(edge: &Edge, before: &HashMap<&str, &Node>, after: &HashMap<&str, &Node>) -> Edge {
    let mut removed = Edge {
        change: Some(Change::Removed),
        ..edge.clone()
    };
    let moved_from = before
        .get(edge.from.as_str())
        .and_then(|n| n.file.as_ref())
        .filter(|file| {
            after
                .get(edge.from.as_str())
                .is_some_and(|n| n.file.as_ref() != Some(*file))
        });
    if let Some(file) = moved_from {
        for sites in removed.sites.iter_mut().filter(|s| s.file.is_none()) {
            sites.file = Some(file.clone());
        }
        removed.sites.sort_by(|a, b| a.file.cmp(&b.file));
    }
    removed
}

/// What the viewer shows for one state of expanded containers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct View {
    /// Ids of the visible nodes, in the order of the graph.
    pub nodes: Vec<String>,
    pub edges: Vec<ViewEdge>,
    /// Ids of the visible nodes that have at least one child in the graph, in
    /// the order of the graph.
    pub expandable: Vec<String>,
    /// For each visible node: how many nodes in its subtree, itself included,
    /// carry each mark. Nodes without marks have no entry.
    pub changes: std::collections::BTreeMap<String, ChangeCount>,
    /// For each container that folds children into a placeholder `rest:<id>`: how
    /// many direct children are folded. Only set by `Graph::cut_focused`.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub rest: std::collections::BTreeMap<String, usize>,
}

/// The number of marked nodes of each kind of change.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeCount {
    pub added: usize,
    pub removed: usize,
    pub modified: usize,
}

/// The aggregate of all graph edges between two visible nodes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewEdge {
    pub from: String,
    pub to: String,
    pub count: usize,
    /// How many of the `count` edges have each kind. The values sum to `count`.
    pub kinds: std::collections::BTreeMap<EdgeKind, usize>,
    /// How many of the `count` edges are marked `Added`.
    pub added: usize,
    /// How many of the `count` edges are marked `Removed`.
    pub removed: usize,
}

/// What a focused cut keeps in view. See `Graph::cut_focused`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Focus {
    /// Explicit nodes of interest. Each of their descendants is kept too.
    pub nodes: std::collections::HashSet<String>,
    /// Also of interest: each node with a change mark and both ends of each edge
    /// with a change mark.
    pub changes: bool,
    /// Also of interest: the other end of each edge that has one end at a node of
    /// interest or at a descendant of one. One step only.
    pub neighbors: bool,
    /// Containers that show all their children in spite of the focus.
    pub unfolded: std::collections::HashSet<String>,
    /// Kind fold (ADR 0019): an expanded container with a package child and a child
    /// of another kind, and no child that the interest keeps, shows its packages and
    /// folds the other children into `rest:<id>`.
    pub fold_mixed: bool,
}

impl Graph {
    /// Like `cut`, but an expanded container with children of interest shows only
    /// those and one placeholder `rest:<id>` for the others.
    pub fn cut_focused(&self, expanded: &HashSet<String>, focus: &Focus) -> View {
        let parents: HashMap<&str, Option<&str>> = self
            .nodes
            .iter()
            .map(|n| (n.id.as_str(), n.parent.as_deref()))
            .collect();
        let explicit: HashSet<&str> = focus
            .nodes
            .iter()
            .map(String::as_str)
            .filter(|id| parents.contains_key(id))
            .collect();
        let mut interest = explicit.clone();
        if focus.changes {
            interest.extend(
                self.nodes
                    .iter()
                    .filter(|n| n.change.is_some())
                    .map(|n| n.id.as_str()),
            );
            for edge in self.edges.iter().filter(|e| e.change.is_some()) {
                interest.extend(
                    [edge.from.as_str(), edge.to.as_str()]
                        .into_iter()
                        .filter(|id| parents.contains_key(id)),
                );
            }
        }
        if focus.neighbors {
            // The other end of each edge with one end at or below a node of interest.
            let inside = |id: &str| {
                std::iter::successors(Some(id), |id| parents.get(id).copied().flatten())
                    .any(|a| interest.contains(a))
            };
            let mut neighbors: Vec<&str> = Vec::new();
            for edge in &self.edges {
                let (from, to) = (edge.from.as_str(), edge.to.as_str());
                if !parents.contains_key(from) || !parents.contains_key(to) {
                    continue;
                }
                if inside(from) {
                    neighbors.push(to);
                }
                if inside(to) {
                    neighbors.push(from);
                }
            }
            interest.extend(neighbors);
        }
        // Nodes whose subtree, itself included, holds a node of interest.
        let mut kept: HashSet<&str> = HashSet::new();
        for id in &interest {
            let mut current = Some(*id);
            while let Some(id) = current {
                if !kept.insert(id) {
                    break;
                }
                current = parents.get(id).copied().flatten();
            }
        }
        let interest_kept = kept.clone();
        // Each descendant of an explicit node is kept as well.
        kept.extend(self.nodes.iter().map(|n| n.id.as_str()).filter(|id| {
            std::iter::successors(Some(*id), |id| parents.get(id).copied().flatten())
                .any(|a| explicit.contains(a))
        }));
        if focus.fold_mixed {
            // Kind fold: a container with packages and other children, and no child
            // that the interest keeps, keeps its packages and folds the rest.
            let kinds: HashMap<&str, NodeKind> =
                self.nodes.iter().map(|n| (n.id.as_str(), n.kind)).collect();
            let mut children: HashMap<&str, Vec<&str>> = HashMap::new();
            for node in &self.nodes {
                if let Some(parent) = node.parent.as_deref() {
                    children.entry(parent).or_default().push(node.id.as_str());
                }
            }
            for (container, children) in &children {
                let is_package = |id: &&str| kinds.get(id) == Some(&NodeKind::Package);
                if children.iter().any(is_package)
                    && !children.iter().all(is_package)
                    && !focus.unfolded.contains(*container)
                    && !children.iter().any(|c| interest_kept.contains(c))
                {
                    for child in children {
                        if is_package(child) {
                            kept.insert(child);
                        } else {
                            kept.remove(child);
                        }
                    }
                }
            }
        }
        let kept_parents: HashSet<&str> = kept
            .iter()
            .filter_map(|id| parents.get(id).copied().flatten())
            .collect();
        // Containers that fold: they have a kept child and are not unfolded.
        let folds = |id: &str| kept_parents.contains(id) && !focus.unfolded.contains(id);

        // Nearest visible ancestor-or-self.
        let anchor = |id: &str| -> Option<&str> {
            let mut path = vec![*parents.get_key_value(id)?.0];
            while let Some(Some(parent)) = parents.get(path[path.len() - 1]) {
                path.push(parent);
            }
            // `path` goes from the node up to its root; walk down while expanded.
            let mut visible = path.pop()?;
            while let Some(child) = path.pop() {
                if !expanded.contains(visible) {
                    break;
                }
                visible = child;
            }
            Some(visible)
        };
        // The container whose placeholder stands for `id`, if `id` is folded.
        let folded_into = |id: &str| -> Option<&str> {
            let mut path = vec![*parents.get_key_value(id)?.0];
            while let Some(Some(parent)) = parents.get(path[path.len() - 1]) {
                path.push(parent);
            }
            let mut visible = path.pop()?;
            while let Some(child) = path.pop() {
                if !expanded.contains(visible) {
                    return None;
                }
                if folds(visible) && !kept.contains(child) {
                    return Some(visible);
                }
                visible = child;
            }
            None
        };

        let nodes: Vec<String> = self
            .nodes
            .iter()
            .filter(|n| anchor(&n.id) == Some(n.id.as_str()) && folded_into(&n.id).is_none())
            .map(|n| n.id.clone())
            .collect();
        // How many direct children each folding container folds.
        let mut rest: BTreeMap<String, usize> = BTreeMap::new();
        let visible: HashSet<&str> = nodes.iter().map(String::as_str).collect();
        for node in &self.nodes {
            if let Some(parent) = node.parent.as_deref()
                && visible.contains(parent)
                && expanded.contains(parent)
                && folds(parent)
                && !kept.contains(node.id.as_str())
            {
                *rest.entry(parent.to_string()).or_default() += 1;
            }
        }
        let nodes = place_rests(nodes, &rest, &parents, &kept);
        let visible: HashSet<&str> = nodes.iter().map(String::as_str).collect();
        // Per pair: the counts by kind, and how many edges are added and removed.
        type Aggregate = (BTreeMap<EdgeKind, usize>, usize, usize);
        let mut aggregated: BTreeMap<(String, String), Aggregate> = BTreeMap::new();
        // The visible node, or the placeholder, that stands for `id`.
        let end = |id: &str| -> Option<String> {
            match folded_into(id) {
                Some(container) => Some(format!("rest:{container}")),
                None => anchor(id).map(str::to_string),
            }
        };
        for edge in &self.edges {
            if let (Some(from), Some(to)) = (end(&edge.from), end(&edge.to))
                && from != to
            {
                let (kinds, added, removed) = aggregated.entry((from, to)).or_default();
                *kinds.entry(edge.kind).or_default() += 1;
                match edge.change {
                    Some(Change::Added) => *added += 1,
                    Some(Change::Removed) => *removed += 1,
                    _ => {}
                }
            }
        }
        let edges = aggregated
            .into_iter()
            .map(|((from, to), (kinds, added, removed))| ViewEdge {
                from,
                to,
                count: kinds.values().sum(),
                kinds,
                added,
                removed,
            })
            .collect();
        let with_children: HashSet<&str> = self
            .nodes
            .iter()
            .filter_map(|n| n.parent.as_deref())
            .collect();
        let expandable = self
            .nodes
            .iter()
            .filter(|n| with_children.contains(n.id.as_str()))
            .filter(|n| visible.contains(n.id.as_str()))
            .map(|n| n.id.clone())
            .collect();
        let mut changes: BTreeMap<String, ChangeCount> = BTreeMap::new();
        for node in self.nodes.iter().filter(|n| n.change.is_some()) {
            let mut current = Some(node.id.as_str());
            while let Some(id) = current {
                if visible.contains(id) {
                    let count = changes.entry(id.to_string()).or_default();
                    match node.change {
                        Some(Change::Added) => count.added += 1,
                        Some(Change::Removed) => count.removed += 1,
                        Some(Change::Modified) => count.modified += 1,
                        None => {}
                    }
                }
                current = parents.get(id).copied().flatten();
            }
        }
        View {
            nodes,
            edges,
            expandable,
            changes,
            rest,
        }
    }

    /// Cuts the container tree. A node is visible if each of its ancestors is
    /// expanded. Each edge moves to the nearest visible ancestors of its ends,
    /// and an edge with both ends below the same visible node is not shown.
    pub fn cut(&self, expanded: &HashSet<String>) -> View {
        self.cut_focused(expanded, &Focus::default())
    }
}

/// Inserts the placeholder `rest:<container>` of each container in `rest` after the
/// last visible node of the container's kept children.
fn place_rests(
    nodes: Vec<String>,
    rest: &BTreeMap<String, usize>,
    parents: &HashMap<&str, Option<&str>>,
    kept: &HashSet<&str>,
) -> Vec<String> {
    if rest.is_empty() {
        return nodes;
    }
    let depth =
        |id: &str| std::iter::successors(Some(id), |id| parents.get(id).copied().flatten()).count();
    // Per node of the view: the placeholders that go right after it.
    let mut after: HashMap<usize, Vec<&str>> = HashMap::new();
    for container in rest.keys() {
        let last = nodes.iter().rposition(|id| {
            std::iter::successors(Some(id.as_str()), |id| parents.get(id).copied().flatten())
                .any(|a| parents.get(a).copied().flatten() == Some(container) && kept.contains(a))
        });
        // A container with folded children has a kept child, and it is visible.
        if let Some(last) = last {
            after.entry(last).or_default().push(container);
        }
    }
    let mut placed = Vec::with_capacity(nodes.len() + rest.len());
    for (i, id) in nodes.into_iter().enumerate() {
        placed.push(id);
        if let Some(mut containers) = after.remove(&i) {
            // The deeper container sits inside the kept subtree of the shallower one.
            containers.sort_by_key(|c| std::cmp::Reverse(depth(c)));
            placed.extend(containers.into_iter().map(|c| format!("rest:{c}")));
        }
    }
    placed
}
