use cg_core::{Edge, EdgeKind, Graph, Node, NodeKind, ViewEdge};
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

fn view_edge(from: &str, to: &str, kinds: &[(EdgeKind, usize)]) -> ViewEdge {
    ViewEdge {
        from: from.into(),
        to: to.into(),
        count: kinds.iter().map(|(_, n)| n).sum(),
        kinds: kinds.iter().copied().collect(),
        added: 0,
        removed: 0,
    }
}

/// Two modules. `A` has the types `T1` and `T2` in package `a`, `B` has `U` in package `b`.
fn two_modules() -> Graph {
    Graph {
        nodes: vec![
            node("A", None, NodeKind::Module),
            node("a", Some("A"), NodeKind::Package),
            node("T1", Some("a"), NodeKind::Type),
            node("T2", Some("a"), NodeKind::Type),
            node("B", None, NodeKind::Module),
            node("b", Some("B"), NodeKind::Package),
            node("U", Some("b"), NodeKind::Type),
        ],
        edges: vec![
            edge("T1", "U", EdgeKind::Extends),
            edge("T2", "U", EdgeKind::TypeRef),
            edge("T1", "T2", EdgeKind::TypeRef),
        ],
        ..Graph::default()
    }
}

fn expanded(ids: &[&str]) -> HashSet<String> {
    ids.iter().map(|id| id.to_string()).collect()
}

#[test]
fn collapsed_modules_show_one_edge_with_the_count_of_the_leaf_edges() {
    let view = two_modules().cut(&expanded(&[]));

    assert_eq!(view.nodes, vec!["A", "B"]);
    assert_eq!(
        view.edges,
        vec![view_edge(
            "A",
            "B",
            &[(EdgeKind::Extends, 1), (EdgeKind::TypeRef, 1)]
        )]
    );
}

#[test]
fn an_expanded_container_shows_its_children_and_their_edges() {
    let view = two_modules().cut(&expanded(&["A", "a"]));

    assert_eq!(view.nodes, vec!["A", "a", "T1", "T2", "B"]);
    assert_eq!(
        view.edges,
        vec![
            view_edge("T1", "B", &[(EdgeKind::Extends, 1)]),
            view_edge("T1", "T2", &[(EdgeKind::TypeRef, 1)]),
            view_edge("T2", "B", &[(EdgeKind::TypeRef, 1)]),
        ]
    );
}

#[test]
fn an_expanded_container_below_a_collapsed_one_stays_hidden() {
    let view = two_modules().cut(&expanded(&["a"]));

    assert_eq!(view.nodes, vec!["A", "B"]);
}

#[test]
fn file_span_and_test_are_left_out_of_the_json_when_unset_and_default_on_read() {
    let plain = node("p", None, NodeKind::Package);
    let json = serde_json::to_string(&plain).unwrap();
    assert_eq!(
        json,
        r#"{"id":"p","parent":null,"kind":"package","name":"p"}"#
    );
    assert_eq!(serde_json::from_str::<Node>(&json).unwrap(), plain);

    let function = Node {
        file: Some("a/Foo.java".into()),
        span: Some(cg_core::Span {
            start_line: 3,
            end_line: 5,
        }),
        test: true,
        ..node("f", Some("t"), NodeKind::Function)
    };
    let json = serde_json::to_string(&function).unwrap();
    assert!(
        json.contains(r#""span":{"start_line":3,"end_line":5}"#),
        "{json}"
    );
    assert_eq!(serde_json::from_str::<Node>(&json).unwrap(), function);
}

fn test_node(id: &str, parent: &str, kind: NodeKind) -> Node {
    Node {
        test: true,
        ..node(id, Some(parent), kind)
    }
}

#[test]
fn without_tests_drops_test_nodes_their_descendants_their_edges_and_emptied_containers() {
    let graph = Graph {
        nodes: vec![
            node("M", None, NodeKind::Module),
            node("p", Some("M"), NodeKind::Package),
            node("T", Some("p"), NodeKind::Type),
            node("f", Some("T"), NodeKind::Function),
            test_node("TT", "p", NodeKind::Type),
            // A function below a test type is removed with it, whatever its own flag.
            node("tf", Some("TT"), NodeKind::Function),
            node("TM", None, NodeKind::Module),
            node("tp", Some("TM"), NodeKind::Package),
            test_node("TU", "tp", NodeKind::Type),
        ],
        edges: vec![
            edge("T", "TT", EdgeKind::TypeRef),
            edge("TT", "T", EdgeKind::TypeRef),
            edge("tf", "T", EdgeKind::TypeRef),
            edge("f", "T", EdgeKind::TypeRef),
            edge("TU", "T", EdgeKind::Extends),
        ],
        ..Graph::default()
    };

    let kept = graph.without_tests();

    let ids: Vec<_> = kept.nodes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, ["M", "p", "T", "f"]);
    assert_eq!(kept.edges, vec![edge("f", "T", EdgeKind::TypeRef)]);
}

#[test]
fn without_tests_leaves_a_graph_without_test_nodes_equal() {
    let graph = two_modules();

    assert_eq!(graph.without_tests(), graph);
}

#[test]
fn a_view_edge_counts_the_aggregated_edges_by_kind_and_serializes_the_kinds_as_a_map() {
    let view = two_modules().cut(&expanded(&[]));

    let edge = &view.edges[0];
    assert_eq!(edge.count, 2);
    assert_eq!(
        edge.kinds,
        BTreeMap::from([(EdgeKind::Extends, 1), (EdgeKind::TypeRef, 1)])
    );
    assert_eq!(edge.kinds.values().sum::<usize>(), edge.count);
    let json = serde_json::to_string(edge).unwrap();
    assert_eq!(
        json,
        r#"{"from":"A","to":"B","count":2,"kinds":{"extends":1,"type_ref":1},"added":0,"removed":0}"#
    );
    assert_eq!(serde_json::from_str::<ViewEdge>(&json).unwrap(), *edge);
}

#[test]
fn expandable_lists_the_visible_nodes_that_have_children_in_graph_order() {
    let collapsed = two_modules().cut(&expanded(&[]));
    assert_eq!(collapsed.expandable, vec!["A", "B"]);

    let open = two_modules().cut(&expanded(&["A", "a"]));
    assert_eq!(open.nodes, vec!["A", "a", "T1", "T2", "B"]);
    assert_eq!(
        open.expandable,
        vec!["A", "a", "B"],
        "the leaves T1 and T2 cannot open"
    );
}

#[test]
fn an_import_is_redundant_when_the_importer_or_a_descendant_uses_the_type() {
    let graph = Graph {
        nodes: vec![
            node("User", None, NodeKind::Type),
            node("User#run", Some("User"), NodeKind::Function),
            node("Used", None, NodeKind::Type),
            node("Used.Inner", Some("Used"), NodeKind::Type),
            node("ViaInner", None, NodeKind::Type),
            node("OnlyImported", None, NodeKind::Type),
        ],
        edges: vec![
            edge("User", "Used", EdgeKind::Imports),
            edge("User#run", "Used", EdgeKind::TypeRef),
            edge("User", "OnlyImported", EdgeKind::Imports),
            edge("ViaInner", "Used", EdgeKind::Imports),
            edge("ViaInner", "Used.Inner", EdgeKind::Extends),
        ],
        ..Graph::default()
    };

    let merged = graph.without_redundant_imports();

    assert_eq!(merged.nodes, graph.nodes);
    assert_eq!(
        merged.edges,
        vec![
            edge("User#run", "Used", EdgeKind::TypeRef),
            edge("User", "OnlyImported", EdgeKind::Imports),
            edge("ViaInner", "Used.Inner", EdgeKind::Extends),
        ]
    );
}

/// Module `M`, package `u` that holds `T2` and has the child package `u.f` with `T1`;
/// `T1` uses `T2`.
fn nested_packages() -> Graph {
    Graph {
        nodes: vec![
            node("M", None, NodeKind::Module),
            node("u", Some("M"), NodeKind::Package),
            node("u.f", Some("u"), NodeKind::Package),
            node("T1", Some("u.f"), NodeKind::Type),
            node("T2", Some("u"), NodeKind::Type),
        ],
        edges: vec![edge("T1", "T2", EdgeKind::TypeRef)],
        ..Graph::default()
    }
}

#[test]
fn an_edge_from_a_nested_package_to_its_parent_package_shows_only_while_both_ends_are_visible() {
    let g = nested_packages();
    let kinds = [(EdgeKind::TypeRef, 1)];

    // Both ends sit in `u`: the edge is inside one box.
    assert!(g.cut(&expanded(&[])).edges.is_empty());
    assert!(g.cut(&expanded(&["M"])).edges.is_empty());
    // `T2` and the closed `u.f` are siblings.
    let view = g.cut(&expanded(&["M", "u"]));
    assert_eq!(view.edges, [view_edge("u.f", "T2", &kinds)]);
    // Then the edge runs between the types.
    let view = g.cut(&expanded(&["M", "u", "u.f"]));
    assert_eq!(view.edges, [view_edge("T1", "T2", &kinds)]);
}

#[test]
fn a_value_is_a_leaf_beside_functions_and_value_ref_is_an_edge_kind_of_its_own() {
    let value = node("v", Some("p"), NodeKind::Value);
    let json = serde_json::to_string(&value).unwrap();
    assert!(json.contains(r#""kind":"value""#), "{json}");
    assert_eq!(serde_json::from_str::<Node>(&json).unwrap(), value);
    let reference = edge("f", "v", EdgeKind::ValueRef);
    let json = serde_json::to_string(&reference).unwrap();
    assert!(json.contains(r#""kind":"value_ref""#), "{json}");
    assert_eq!(serde_json::from_str::<Edge>(&json).unwrap(), reference);

    let graph = Graph {
        nodes: vec![
            node("M", None, NodeKind::Module),
            node("p", Some("M"), NodeKind::Package),
            node("f", Some("p"), NodeKind::Function),
            node("v", Some("p"), NodeKind::Value),
            node("N", None, NodeKind::Module),
            node("q", Some("N"), NodeKind::Package),
            node("w", Some("q"), NodeKind::Value),
        ],
        edges: vec![
            edge("f", "w", EdgeKind::ValueRef),
            edge("v", "w", EdgeKind::ValueRef),
            edge("f", "w", EdgeKind::Calls),
        ],
        diff: None,
    };
    let view = graph.cut(&expanded(&["M", "p"]));
    assert_eq!(view.nodes, vec!["M", "p", "f", "v", "N"]);
    assert_eq!(
        view.edges,
        vec![
            view_edge("f", "N", &[(EdgeKind::Calls, 1), (EdgeKind::ValueRef, 1)]),
            view_edge("v", "N", &[(EdgeKind::ValueRef, 1)]),
        ]
    );
    assert_eq!(view.expandable, vec!["M", "p", "N"]);
}

#[test]
fn a_value_ref_makes_an_import_redundant_and_a_test_value_is_removed_with_its_edges() {
    let graph = Graph {
        nodes: vec![
            node("M", None, NodeKind::Module),
            node("a", Some("M"), NodeKind::Package),
            node("f", Some("a"), NodeKind::Function),
            node("b", Some("M"), NodeKind::Package),
            node("v", Some("b"), NodeKind::Value),
            node("c", Some("M"), NodeKind::Package),
            test_node("tv", "c", NodeKind::Value),
            node("g", Some("c"), NodeKind::Function),
        ],
        edges: vec![
            edge("a", "b", EdgeKind::Imports),
            edge("f", "v", EdgeKind::ValueRef),
            edge("g", "tv", EdgeKind::ValueRef),
        ],
        ..Graph::default()
    };

    assert_eq!(
        graph.without_redundant_imports().edges,
        vec![
            edge("f", "v", EdgeKind::ValueRef),
            edge("g", "tv", EdgeKind::ValueRef)
        ]
    );
    let kept = graph.without_tests();
    assert!(kept.node("tv").is_none());
    assert_eq!(
        kept.edges,
        vec![
            edge("a", "b", EdgeKind::Imports),
            edge("f", "v", EdgeKind::ValueRef)
        ]
    );
}
