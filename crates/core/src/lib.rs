//! Language-neutral model of a code base: a tree of containers with edges
//! between its nodes. No I/O, so the crate builds for native and wasm32.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Module,
    Package,
    Type,
    Function,
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
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub kind: EdgeKind,
    /// What a commit did to the edge. Only set in a diff graph.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change: Option<Change>,
}

/// The two revisions that a diff graph compares, as the user named them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffInfo {
    pub base: String,
    pub head: String,
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
        self.edges
            .iter()
            .any(|e| e.from == from && e.to == to && e.kind == kind)
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
    let new: std::collections::HashSet<&str> = after.nodes.iter().map(|n| n.id.as_str()).collect();
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
        .filter(|n| !new.contains(n.id.as_str()))
        .map(|node| Node {
            change: Some(Change::Removed),
            ..node.clone()
        });
    // Identity of an edge is (from, to, kind).
    let key = |e: &Edge| (e.from.clone(), e.to.clone(), e.kind);
    let mut edges: std::collections::BTreeMap<_, Edge> = before
        .edges
        .iter()
        .map(|e| {
            let change = Some(Change::Removed);
            (
                key(e),
                Edge {
                    change,
                    ..e.clone()
                },
            )
        })
        .collect();
    for e in &after.edges {
        let change = match edges.get(&key(e)) {
            Some(_) => None,
            None => Some(Change::Added),
        };
        edges.insert(
            key(e),
            Edge {
                change,
                ..e.clone()
            },
        );
    }
    Graph {
        nodes: kept.chain(removed).collect(),
        edges: edges.into_values().collect(),
        diff: None,
    }
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

impl Graph {
    /// Cuts the container tree. A node is visible if each of its ancestors is
    /// expanded. Each edge moves to the nearest visible ancestors of its ends,
    /// and an edge with both ends below the same visible node is not shown.
    pub fn cut(&self, expanded: &std::collections::HashSet<String>) -> View {
        use std::collections::{BTreeMap, HashMap};

        let parents: HashMap<&str, Option<&str>> = self
            .nodes
            .iter()
            .map(|n| (n.id.as_str(), n.parent.as_deref()))
            .collect();
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

        let nodes: Vec<String> = self
            .nodes
            .iter()
            .filter(|n| anchor(&n.id) == Some(n.id.as_str()))
            .map(|n| n.id.clone())
            .collect();
        // Per pair: the counts by kind, and how many edges are added and removed.
        type Aggregate = (BTreeMap<EdgeKind, usize>, usize, usize);
        let mut aggregated: BTreeMap<(&str, &str), Aggregate> = BTreeMap::new();
        for edge in &self.edges {
            if let (Some(from), Some(to)) = (anchor(&edge.from), anchor(&edge.to))
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
                from: from.to_string(),
                to: to.to_string(),
                count: kinds.values().sum(),
                kinds,
                added,
                removed,
            })
            .collect();
        let visible: std::collections::HashSet<&str> = nodes.iter().map(String::as_str).collect();
        let with_children: std::collections::HashSet<&str> = self
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
        }
    }
}
