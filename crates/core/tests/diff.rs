use cg_core::{
    Change, ChangeCount, DiffInfo, Edge, EdgeKind, FileSites, Graph, Node, NodeKind, diff,
};
use std::collections::{BTreeMap, HashSet};

fn node(id: &str, parent: Option<&str>, kind: NodeKind) -> Node {
    Node {
        id: id.into(),
        parent: parent.map(Into::into),
        kind,
        name: id.into(),
        file: None,
        span: None,
        test: false,
        hash: None,
        change: None,
    }
}

fn edge(from: &str, to: &str, kind: EdgeKind) -> Edge {
    Edge {
        from: from.into(),
        to: to.into(),
        kind,
        change: None,
        sites: Vec::new(),
    }
}

fn with_sites(from: &str, to: &str, kind: EdgeKind, file: Option<&str>, lines: &[u32]) -> Edge {
    Edge {
        sites: vec![FileSites {
            file: file.map(Into::into),
            lines: lines.to_vec(),
        }],
        ..edge(from, to, kind)
    }
}

fn node_in(id: &str, file: &str) -> Node {
    Node {
        file: Some(file.into()),
        ..node(id, None, NodeKind::Function)
    }
}

#[test]
fn an_edge_that_only_changed_its_sites_carries_no_mark_and_the_sites_of_after() {
    let before = Graph {
        nodes: vec![
            node("a", None, NodeKind::Function),
            node("b", None, NodeKind::Function),
        ],
        edges: vec![with_sites("a", "b", EdgeKind::Calls, None, &[3])],
        ..Graph::default()
    };
    let after = Graph {
        edges: vec![with_sites("a", "b", EdgeKind::Calls, None, &[8, 9])],
        ..before.clone()
    };

    let merged = diff(&before, &after);

    assert_eq!(
        merged.edges,
        vec![with_sites("a", "b", EdgeKind::Calls, None, &[8, 9])]
    );
}

#[test]
fn a_removed_edge_keeps_the_sites_of_before_and_an_added_edge_those_of_after() {
    let nodes = vec![
        node("a", None, NodeKind::Function),
        node("b", None, NodeKind::Function),
        node("c", None, NodeKind::Function),
    ];
    let before = Graph {
        nodes: nodes.clone(),
        edges: vec![with_sites("a", "b", EdgeKind::Calls, None, &[3])],
        ..Graph::default()
    };
    let after = Graph {
        nodes,
        edges: vec![with_sites("a", "c", EdgeKind::Calls, None, &[5])],
        ..Graph::default()
    };

    let merged = diff(&before, &after);

    assert_eq!(
        merged.edges,
        vec![
            Edge {
                change: Some(Change::Removed),
                ..with_sites("a", "b", EdgeKind::Calls, None, &[3])
            },
            Edge {
                change: Some(Change::Added),
                ..with_sites("a", "c", EdgeKind::Calls, None, &[5])
            },
        ]
    );
}

#[test]
fn a_removed_edge_names_the_file_of_before_when_its_from_node_moved() {
    let before = Graph {
        nodes: vec![
            node_in("moved", "old.rs"),
            node_in("same", "s.rs"),
            node_in("gone", "g.rs"),
            node_in("t", "t.rs"),
        ],
        edges: vec![
            with_sites("moved", "t", EdgeKind::Calls, None, &[1]),
            with_sites("same", "t", EdgeKind::Calls, None, &[2]),
            with_sites("gone", "t", EdgeKind::Calls, None, &[3]),
            with_sites("moved", "t", EdgeKind::TypeRef, Some("x.rs"), &[4]),
        ],
        ..Graph::default()
    };
    let after = Graph {
        nodes: vec![
            node_in("moved", "new.rs"),
            node_in("same", "s.rs"),
            node_in("t", "t.rs"),
        ],
        edges: Vec::new(),
        ..Graph::default()
    };

    let merged = diff(&before, &after);

    let sites_of = |from: &str, kind: EdgeKind| {
        merged
            .edges
            .iter()
            .find(|e| e.from == from && e.kind == kind)
            .unwrap()
            .sites
            .clone()
    };
    let one = |file: Option<&str>, lines: &[u32]| {
        vec![FileSites {
            file: file.map(Into::into),
            lines: lines.to_vec(),
        }]
    };
    assert_eq!(
        sites_of("moved", EdgeKind::Calls),
        one(Some("old.rs"), &[1])
    );
    assert_eq!(sites_of("same", EdgeKind::Calls), one(None, &[2]));
    assert_eq!(sites_of("gone", EdgeKind::Calls), one(None, &[3]));
    assert_eq!(
        sites_of("moved", EdgeKind::TypeRef),
        one(Some("x.rs"), &[4])
    );
}

#[test]
fn the_entries_of_a_removed_edge_stay_in_the_order_of_their_paths_when_one_gets_its_file() {
    let entry = |file: Option<&str>, line: u32| FileSites {
        file: file.map(Into::into),
        lines: vec![line],
    };
    let before = Graph {
        nodes: vec![node_in("moved", "old.rs"), node_in("t", "t.rs")],
        edges: vec![Edge {
            sites: vec![entry(None, 1), entry(Some("a.rs"), 7)],
            ..with_sites("moved", "t", EdgeKind::Calls, None, &[])
        }],
        ..Graph::default()
    };
    let after = Graph {
        nodes: vec![node_in("moved", "new.rs"), node_in("t", "t.rs")],
        edges: Vec::new(),
        ..Graph::default()
    };

    let merged = diff(&before, &after);

    assert_eq!(
        merged.edges[0].sites,
        [entry(Some("a.rs"), 7), entry(Some("old.rs"), 1)]
    );
}

#[test]
fn marks_and_diff_info_are_in_the_json_only_when_set() {
    let marked = Node {
        change: Some(Change::Modified),
        ..node("t", None, NodeKind::Type)
    };
    let json = serde_json::to_string(&marked).unwrap();
    assert!(json.contains(r#""change":"modified""#), "{json}");
    assert_eq!(serde_json::from_str::<Node>(&json).unwrap(), marked);
    let plain = serde_json::to_string(&node("t", None, NodeKind::Type)).unwrap();
    assert!(!plain.contains("change"), "{plain}");

    let added = Edge {
        change: Some(Change::Added),
        ..edge("a", "b", EdgeKind::Extends)
    };
    let json = serde_json::to_string(&added).unwrap();
    assert!(json.contains(r#""change":"added""#), "{json}");
    assert_eq!(serde_json::from_str::<Edge>(&json).unwrap(), added);
    let plain = serde_json::to_string(&edge("a", "b", EdgeKind::Extends)).unwrap();
    assert!(!plain.contains("change"), "{plain}");

    let graph = Graph {
        diff: Some(DiffInfo {
            base: "v1".into(),
            head: "v2".into(),
            files: vec![],
        }),
        ..Graph::default()
    };
    let json = serde_json::to_string(&graph).unwrap();
    assert!(
        json.contains(r#""diff":{"base":"v1","head":"v2"}"#),
        "{json}"
    );
    assert_eq!(serde_json::from_str::<Graph>(&json).unwrap(), graph);
    let plain = serde_json::to_string(&Graph::default()).unwrap();
    assert_eq!(plain, r#"{"nodes":[],"edges":[]}"#);
    assert_eq!(
        serde_json::from_str::<Graph>(&plain).unwrap(),
        Graph::default()
    );
}

fn hashed(id: &str, parent: &str, kind: NodeKind, hash: &str) -> Node {
    Node {
        hash: Some(hash.into()),
        ..node(id, Some(parent), kind)
    }
}

fn changes(graph: &Graph) -> Vec<(&str, Option<Change>)> {
    graph
        .nodes
        .iter()
        .map(|n| (n.id.as_str(), n.change))
        .collect()
}

#[test]
fn diff_marks_nodes_added_modified_and_removed_and_orders_them_after_then_removed() {
    let before = Graph {
        nodes: vec![
            node("p", None, NodeKind::Package),
            hashed("Gone", "p", NodeKind::Type, "1"),
            hashed("Same", "p", NodeKind::Type, "2"),
            hashed("Changed", "p", NodeKind::Type, "3"),
            hashed("Gone2", "p", NodeKind::Type, "4"),
        ],
        ..Graph::default()
    };
    let after = Graph {
        nodes: vec![
            node("p", None, NodeKind::Package),
            hashed("New", "p", NodeKind::Type, "9"),
            hashed("Changed", "p", NodeKind::Type, "33"),
            hashed("Same", "p", NodeKind::Type, "2"),
        ],
        ..Graph::default()
    };

    let union = diff(&before, &after);

    assert_eq!(
        changes(&union),
        [
            ("p", None),
            ("New", Some(Change::Added)),
            ("Changed", Some(Change::Modified)),
            ("Same", None),
            ("Gone", Some(Change::Removed)),
            ("Gone2", Some(Change::Removed)),
        ]
    );
    // A removed node keeps its fields from `before`.
    assert_eq!(union.node("Gone").unwrap().hash.as_deref(), Some("1"));
    assert_eq!(union.diff, None);
}

#[test]
fn diff_marks_edges_by_identity_from_to_kind_and_sorts_them() {
    let before = Graph {
        edges: vec![
            edge("a", "b", EdgeKind::Extends),
            edge("a", "c", EdgeKind::TypeRef),
            edge("x", "y", EdgeKind::Imports),
        ],
        ..Graph::default()
    };
    let after = Graph {
        edges: vec![
            edge("x", "y", EdgeKind::Imports),
            edge("a", "b", EdgeKind::TypeRef),
            edge("a", "b", EdgeKind::Extends),
        ],
        ..Graph::default()
    };

    let union = diff(&before, &after);

    let edges: Vec<_> = union
        .edges
        .iter()
        .map(|e| (e.from.as_str(), e.to.as_str(), e.kind, e.change))
        .collect();
    assert_eq!(
        edges,
        [
            ("a", "b", EdgeKind::Extends, None),
            ("a", "b", EdgeKind::TypeRef, Some(Change::Added)),
            ("a", "c", EdgeKind::TypeRef, Some(Change::Removed)),
            ("x", "y", EdgeKind::Imports, None),
        ]
    );
}

#[test]
fn diff_of_a_graph_with_itself_is_that_graph_without_marks() {
    let graph = Graph {
        nodes: vec![
            node("p", None, NodeKind::Package),
            hashed("A", "p", NodeKind::Type, "1"),
            hashed("B", "p", NodeKind::Type, "2"),
        ],
        edges: vec![
            edge("A", "B", EdgeKind::Extends),
            edge("A", "B", EdgeKind::TypeRef),
        ],
        diff: None,
    };

    assert_eq!(diff(&graph, &graph), graph);
}

fn marked_node(id: &str, parent: Option<&str>, kind: NodeKind, change: Change) -> Node {
    Node {
        change: Some(change),
        ..node(id, parent, kind)
    }
}

/// Module `A` > package `a` > `T1` (added), `T2` (modified) with function `f` (removed);
/// module `B` > package `b` > `U`, unchanged.
fn marked_graph() -> Graph {
    Graph {
        nodes: vec![
            node("A", None, NodeKind::Module),
            node("a", Some("A"), NodeKind::Package),
            marked_node("T1", Some("a"), NodeKind::Type, Change::Added),
            marked_node("T2", Some("a"), NodeKind::Type, Change::Modified),
            marked_node("f", Some("T2"), NodeKind::Function, Change::Removed),
            node("B", None, NodeKind::Module),
            node("b", Some("B"), NodeKind::Package),
            node("U", Some("b"), NodeKind::Type),
        ],
        edges: vec![
            Edge {
                change: Some(Change::Added),
                ..edge("T1", "U", EdgeKind::Extends)
            },
            Edge {
                change: Some(Change::Removed),
                ..edge("T2", "U", EdgeKind::TypeRef)
            },
            edge("T1", "T2", EdgeKind::TypeRef),
        ],
        diff: None,
    }
}

fn count(added: usize, removed: usize, modified: usize) -> ChangeCount {
    ChangeCount {
        added,
        removed,
        modified,
    }
}

#[test]
fn the_cut_counts_marked_nodes_in_the_subtree_of_each_visible_node() {
    let graph = marked_graph();

    let collapsed = graph.cut(&HashSet::new());
    assert_eq!(
        collapsed.changes,
        BTreeMap::from([("A".to_string(), count(1, 1, 1))])
    );

    let expanded = graph.cut(&["A", "a"].iter().map(|s| s.to_string()).collect());
    assert_eq!(
        expanded.changes,
        BTreeMap::from([
            ("A".to_string(), count(1, 1, 1)),
            ("a".to_string(), count(1, 1, 1)),
            ("T1".to_string(), count(1, 0, 0)),
            ("T2".to_string(), count(0, 1, 1)),
        ])
    );
}

#[test]
fn a_view_edge_counts_how_many_of_its_edges_are_added_and_removed() {
    let graph = marked_graph();

    let collapsed = graph.cut(&HashSet::new());
    assert_eq!(collapsed.edges.len(), 1);
    let edge = &collapsed.edges[0];
    assert_eq!((edge.count, edge.added, edge.removed), (2, 1, 1));

    let expanded = graph.cut(&["A", "a"].iter().map(|s| s.to_string()).collect());
    let counts: Vec<_> = expanded
        .edges
        .iter()
        .map(|e| (e.from.as_str(), e.to.as_str(), e.count, e.added, e.removed))
        .collect();
    assert_eq!(
        counts,
        [
            ("T1", "B", 1, 1, 0),
            ("T1", "T2", 1, 0, 0),
            ("T2", "B", 1, 0, 1),
        ]
    );

    // The new fields are always in the JSON, also with the value 0.
    let json = serde_json::to_string(&expanded.edges[1]).unwrap();
    assert!(json.contains(r#""added":0,"removed":0"#), "{json}");
    let unmarked = one_unmarked().cut(&HashSet::new());
    assert!(
        serde_json::to_string(&unmarked)
            .unwrap()
            .contains(r#""changes":{}"#)
    );
}

fn one_unmarked() -> Graph {
    Graph {
        nodes: vec![node("A", None, NodeKind::Module)],
        ..Graph::default()
    }
}

#[test]
fn a_package_without_a_hash_is_never_modified_but_added_or_removed_with_its_id() {
    let package = |id: &str, parent: &str| node(id, Some(parent), NodeKind::Package);
    let before = Graph {
        nodes: vec![
            node("m", None, NodeKind::Module),
            package("p.q", "m"),
            package("p.q.x", "p.q"),
            hashed("T", "p.q.x", NodeKind::Type, "1"),
        ],
        ..Graph::default()
    };
    // `p.q` goes, `p.r` comes, `p.q.x` moves to the module and keeps its id.
    let after = Graph {
        nodes: vec![
            node("m", None, NodeKind::Module),
            package("p.r", "m"),
            package("p.q.x", "m"),
            hashed("T", "p.q.x", NodeKind::Type, "1"),
        ],
        ..Graph::default()
    };

    let union = diff(&before, &after);

    assert_eq!(
        changes(&union),
        [
            ("m", None),
            ("p.r", Some(Change::Added)),
            ("p.q.x", None),
            ("T", None),
            ("p.q", Some(Change::Removed)),
        ]
    );
}

#[test]
fn diff_marks_a_value_by_its_hash_and_a_value_ref_edge_by_presence() {
    let before = Graph {
        nodes: vec![
            node("p", None, NodeKind::Package),
            hashed("v", "p", NodeKind::Value, "1"),
            hashed("w", "p", NodeKind::Value, "2"),
        ],
        edges: vec![edge("v", "w", EdgeKind::ValueRef)],
        ..Graph::default()
    };
    let after = Graph {
        nodes: vec![
            node("p", None, NodeKind::Package),
            hashed("v", "p", NodeKind::Value, "9"),
            hashed("w", "p", NodeKind::Value, "2"),
        ],
        edges: vec![],
        ..Graph::default()
    };

    let marked = diff(&before, &after);

    assert_eq!(
        changes(&marked),
        vec![("p", None), ("v", Some(Change::Modified)), ("w", None)]
    );
    assert_eq!(
        marked.edges,
        vec![Edge {
            change: Some(Change::Removed),
            ..edge("v", "w", EdgeKind::ValueRef)
        }]
    );
}
