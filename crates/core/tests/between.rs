use cg_core::{Edge, EdgeKind, Graph, Node, NodeKind};
use std::collections::HashSet;

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
    }
}

fn expanded(ids: &[&str]) -> HashSet<String> {
    ids.iter().map(|id| id.to_string()).collect()
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

#[test]
fn between_two_leaves_are_the_edges_from_the_one_to_the_other() {
    let graph = two_modules();

    assert_eq!(
        graph.edges_between("T1", "U"),
        vec![&edge("T1", "U", EdgeKind::Extends)]
    );
    assert!(graph.edges_between("U", "T1").is_empty());
}

#[test]
fn between_two_containers_are_the_edges_of_their_descendants_in_graph_order() {
    let graph = Graph {
        edges: vec![
            edge("T1", "U", EdgeKind::Extends),
            edge("U", "T1", EdgeKind::Calls),
            edge("T2", "U", EdgeKind::TypeRef),
            edge("T1", "T2", EdgeKind::TypeRef),
        ],
        ..two_modules()
    };

    assert_eq!(
        graph.edges_between("A", "B"),
        vec![
            &edge("T1", "U", EdgeKind::Extends),
            &edge("T2", "U", EdgeKind::TypeRef),
        ]
    );
    assert_eq!(
        graph.edges_between("a", "U"),
        vec![
            &edge("T1", "U", EdgeKind::Extends),
            &edge("T2", "U", EdgeKind::TypeRef),
        ]
    );
    assert_eq!(
        graph.edges_between("B", "A"),
        vec![&edge("U", "T1", EdgeKind::Calls)]
    );
}

#[test]
fn between_a_box_and_itself_or_its_descendant_is_empty() {
    let graph = Graph {
        edges: vec![
            edge("T1", "T2", EdgeKind::TypeRef),
            edge("T2", "T1", EdgeKind::TypeRef),
            edge("a", "T1", EdgeKind::Imports),
            edge("T1", "T1", EdgeKind::Calls),
        ],
        ..two_modules()
    };

    assert!(graph.edges_between("T1", "T1").is_empty());
    assert!(graph.edges_between("a", "a").is_empty());
    assert!(graph.edges_between("a", "T1").is_empty());
    assert!(graph.edges_between("T1", "a").is_empty());
    assert!(graph.edges_between("A", "T2").is_empty());
    assert!(graph.edges_between("T2", "A").is_empty());
}

#[test]
fn between_a_box_that_the_graph_lacks_is_empty() {
    let graph = Graph {
        edges: vec![
            edge("T1", "U", EdgeKind::Extends),
            edge("ghost", "U", EdgeKind::Calls),
            edge("T1", "ghost", EdgeKind::Calls),
        ],
        ..two_modules()
    };

    assert!(graph.edges_between("ghost", "U").is_empty());
    assert!(graph.edges_between("T1", "ghost").is_empty());
    assert!(graph.edges_between("ghost", "ghost2").is_empty());
}

#[test]
fn between_two_boxes_of_a_cut_are_as_many_edges_as_its_lines_at_them_and_below_them_count() {
    let graph = Graph {
        nodes: vec![
            node("A", None, NodeKind::Module),
            node("a", Some("A"), NodeKind::Package),
            node("T1", Some("a"), NodeKind::Type),
            node("T1.f", Some("T1"), NodeKind::Function),
            node("T2", Some("a"), NodeKind::Type),
            node("B", None, NodeKind::Module),
            node("b", Some("B"), NodeKind::Package),
            node("U", Some("b"), NodeKind::Type),
            node("U.g", Some("U"), NodeKind::Function),
            node("V", Some("b"), NodeKind::Type),
        ],
        edges: vec![
            edge("T1", "U", EdgeKind::Extends),
            edge("T1.f", "U.g", EdgeKind::Calls),
            edge("U.g", "T1.f", EdgeKind::Calls),
            edge("T2", "V", EdgeKind::TypeRef),
            edge("V", "T2", EdgeKind::TypeRef),
            edge("T1", "T2", EdgeKind::TypeRef),
            edge("T1.f", "T2", EdgeKind::Calls),
            edge("U", "V", EdgeKind::Imports),
            edge("V", "T1.f", EdgeKind::Calls),
        ],
        ..Graph::default()
    };

    let all = ["A", "a", "T1", "B", "b", "U"];
    for open in [
        expanded(&[]),
        expanded(&["A"]),
        expanded(&["A", "a"]),
        expanded(&["A", "a", "T1"]),
        expanded(&["A", "a", "B", "b"]),
        expanded(&all),
    ] {
        let view = graph.cut(&open);
        assert!(!view.edges.is_empty());
        let at_or_below = |id: &str, top: &str| {
            std::iter::successors(graph.node(id), |n| graph.node(n.parent.as_deref()?))
                .any(|n| n.id == top)
        };
        // An open box shows its children with their own lines, so the lines of the cut
        // between two boxes are those at the boxes and those below them.
        for from in &view.nodes {
            for to in &view.nodes {
                let nested = at_or_below(from, to) || at_or_below(to, from);
                let lines: usize = view
                    .edges
                    .iter()
                    .filter(|e| !nested && at_or_below(&e.from, from) && at_or_below(&e.to, to))
                    .map(|e| e.count)
                    .sum();
                assert_eq!(graph.edges_between(from, to).len(), lines, "{from} -> {to}");
            }
        }
    }
}
