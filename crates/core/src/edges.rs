use crate::{Edge, EdgeKind, FileSites, Node};
use std::collections::{BTreeMap, BTreeSet, HashMap};

type Key = (String, String, EdgeKind);

/// The edges of a graph under construction: one edge per `(from, to, kind)`. A further
/// reference of the same edge adds its site to that edge.
#[derive(Debug, Default)]
pub struct EdgeSet {
    edges: BTreeMap<Key, BTreeMap<Option<String>, BTreeSet<u32>>>,
}

impl EdgeSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// A reference from `from` to `to` that stands on `line` (1-based) of `file`.
    pub fn add(&mut self, from: &str, to: &str, kind: EdgeKind, file: &str, line: u32) {
        self.edges
            .entry((from.to_string(), to.to_string(), kind))
            .or_default()
            .entry(Some(file.to_string()))
            .or_default()
            .insert(line);
    }

    /// An edge with the sites it has (possibly none); they join those of the same edge:
    /// entries of the same `file` (also both `None`) are united. The `change` of `edge` is
    /// ignored.
    pub fn insert(&mut self, edge: Edge) {
        let by_file = self
            .edges
            .entry((edge.from, edge.to, edge.kind))
            .or_default();
        for sites in edge.sites {
            by_file.entry(sites.file).or_default().extend(sites.lines);
        }
    }

    /// Keeps the edges for which `keep(from, to)` holds.
    pub fn retain(&mut self, mut keep: impl FnMut(&str, &str) -> bool) {
        self.edges.retain(|(from, to, _), _| keep(from, to));
    }

    /// The edges in the order of `(from, to, kind)` (kind in the order of the enum), without
    /// marks. A site file that is the `file` of the `from` node in `nodes` is left out
    /// (`FileSites::file == None`) and united with an entry that already has no file.
    pub fn into_edges(self, nodes: &[Node]) -> Vec<Edge> {
        let files: HashMap<&str, &str> = nodes
            .iter()
            .filter_map(|n| Some((n.id.as_str(), n.file.as_deref()?)))
            .collect();
        self.edges
            .into_iter()
            .map(|((from, to, kind), by_file)| {
                let own = files.get(from.as_str()).copied();
                let mut merged: BTreeMap<Option<String>, BTreeSet<u32>> = BTreeMap::new();
                for (file, lines) in by_file {
                    let file = file.filter(|f| Some(f.as_str()) != own);
                    merged.entry(file).or_default().extend(lines);
                }
                Edge {
                    from,
                    to,
                    kind,
                    change: None,
                    sites: merged
                        .into_iter()
                        .map(|(file, lines)| FileSites {
                            file,
                            lines: lines.into_iter().collect(),
                        })
                        .collect(),
                }
            })
            .collect()
    }
}

impl Extend<Edge> for EdgeSet {
    fn extend<I: IntoIterator<Item = Edge>>(&mut self, edges: I) {
        for edge in edges {
            self.insert(edge);
        }
    }
}

impl FromIterator<Edge> for EdgeSet {
    fn from_iter<I: IntoIterator<Item = Edge>>(edges: I) -> Self {
        let mut set = EdgeSet::new();
        set.extend(edges);
        set
    }
}
