use cg_core::{Edge, EdgeKind, Focus, Graph, Node, NodeKind, ViewEdge};
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
        sites: Vec::new(),
    }
}

fn set(ids: &[&str]) -> HashSet<String> {
    ids.iter().map(|id| id.to_string()).collect()
}

/// Module `A` with package `a` and types `T1`..`T4`; module `B` with package `b` and `U`.
fn graph() -> Graph {
    Graph {
        nodes: vec![
            node("A", None, NodeKind::Module),
            node("a", Some("A"), NodeKind::Package),
            node("T1", Some("a"), NodeKind::Type),
            node("T2", Some("a"), NodeKind::Type),
            node("T3", Some("a"), NodeKind::Type),
            node("T4", Some("a"), NodeKind::Type),
            node("B", None, NodeKind::Module),
            node("b", Some("B"), NodeKind::Package),
            node("U", Some("b"), NodeKind::Type),
        ],
        edges: vec![
            edge("T1", "U", EdgeKind::Extends),
            edge("T2", "U", EdgeKind::TypeRef),
            edge("T3", "T4", EdgeKind::TypeRef),
            edge("T1", "T2", EdgeKind::TypeRef),
        ],
        ..Graph::default()
    }
}

#[test]
fn an_empty_focus_gives_the_plain_cut() {
    let graph = graph();
    for open in [&[][..], &["A"], &["A", "a"], &["A", "a", "B", "b"]] {
        assert_eq!(
            graph.cut_focused(&set(open), &Focus::default()),
            graph.cut(&set(open)),
            "expanded {open:?}"
        );
    }
}

#[test]
fn an_expanded_container_with_a_node_of_interest_folds_the_other_children() {
    let focus = Focus {
        nodes: set(&["T1"]),
        ..Focus::default()
    };
    let view = graph().cut_focused(&set(&["A", "a"]), &focus);

    assert_eq!(view.nodes, vec!["A", "a", "T1", "rest:a", "B"]);
    assert_eq!(view.rest, [("a".to_string(), 3)].into());
    assert!(!view.expandable.contains(&"rest:a".to_string()));
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

#[test]
fn edges_of_folded_nodes_move_to_the_placeholder_and_internal_ones_drop() {
    let focus = Focus {
        nodes: set(&["T1"]),
        ..Focus::default()
    };
    let view = graph().cut_focused(&set(&["A", "a"]), &focus);

    assert_eq!(
        view.edges,
        vec![
            view_edge("T1", "B", &[(EdgeKind::Extends, 1)]),
            view_edge("T1", "rest:a", &[(EdgeKind::TypeRef, 1)]),
            view_edge("rest:a", "B", &[(EdgeKind::TypeRef, 1)]),
        ]
    );
}

#[test]
fn a_container_without_a_node_of_interest_below_it_shows_all_children() {
    let focus = Focus {
        nodes: set(&["T1"]),
        ..Focus::default()
    };
    let view = graph().cut_focused(&set(&["A", "a", "B", "b"]), &focus);

    assert_eq!(view.nodes, vec!["A", "a", "T1", "rest:a", "B", "b", "U"]);
    assert_eq!(view.rest, [("a".to_string(), 3)].into());
}

#[test]
fn an_unfolded_container_shows_all_children() {
    let focus = Focus {
        nodes: set(&["T1"]),
        unfolded: set(&["a"]),
        ..Focus::default()
    };
    let view = graph().cut_focused(&set(&["A", "a"]), &focus);

    assert_eq!(view.nodes, vec!["A", "a", "T1", "T2", "T3", "T4", "B"]);
    assert!(view.rest.is_empty());
}

#[test]
fn a_node_of_interest_inside_a_closed_container_shows_as_that_container() {
    let focus = Focus {
        nodes: set(&["T1"]),
        ..Focus::default()
    };
    let view = graph().cut_focused(&set(&["A"]), &focus);

    // `A` is open and its only child `a` holds the node of interest: nothing folds.
    assert_eq!(view.nodes, vec!["A", "a", "B"]);
    let view = graph().cut_focused(&set(&[]), &focus);
    assert_eq!(view.nodes, vec!["A", "B"]);
    assert!(view.rest.is_empty());
}

#[test]
fn nodes_of_unknown_ids_are_of_no_interest() {
    let focus = Focus {
        nodes: set(&["nope"]),
        ..Focus::default()
    };
    let graph = graph();
    assert_eq!(
        graph.cut_focused(&set(&["A", "a"]), &focus),
        graph.cut(&set(&["A", "a"]))
    );
}

/// `graph()` with `T2` modified and the edge `T3 -> T4` added.
fn marked() -> Graph {
    let mut graph = graph();
    graph.nodes[3].change = Some(cg_core::Change::Modified);
    graph.edges[2].change = Some(cg_core::Change::Added);
    graph
}

#[test]
fn changes_make_marked_nodes_and_ends_of_marked_edges_of_interest() {
    let focus = Focus {
        changes: true,
        ..Focus::default()
    };
    let view = marked().cut_focused(&set(&["A", "a"]), &focus);

    assert_eq!(view.nodes, vec!["A", "a", "T2", "T3", "T4", "rest:a", "B"]);
    assert_eq!(view.rest, [("a".to_string(), 1)].into());
    // The marks of the visible nodes, none on the placeholder.
    assert_eq!(
        view.changes.keys().collect::<Vec<_>>(),
        vec!["A", "T2", "a"]
    );
    assert_eq!(view.changes["a"].modified, 1);
}

#[test]
fn marks_in_folded_subtrees_still_count_for_visible_ancestors_but_not_for_the_placeholder() {
    let focus = Focus {
        nodes: set(&["T1"]),
        ..Focus::default()
    };
    let view = marked().cut_focused(&set(&["A", "a"]), &focus);

    assert_eq!(view.nodes, vec!["A", "a", "T1", "rest:a", "B"]);
    assert_eq!(view.changes.keys().collect::<Vec<_>>(), vec!["A", "a"]);
    assert_eq!(view.changes["a"].modified, 1);
    assert_eq!(view.changes, graph_cut_changes());
}

fn graph_cut_changes() -> std::collections::BTreeMap<String, cg_core::ChangeCount> {
    let plain = marked().cut(&set(&["A", "a"]));
    plain
        .changes
        .into_iter()
        .filter(|(id, _)| id == "A" || id == "a")
        .collect()
}

/// `graph()` plus a type `V` in package `b`, and an edge `T3 -> U`.
fn with_v() -> Graph {
    let mut graph = graph();
    graph.nodes.push(node("V", Some("b"), NodeKind::Type));
    graph.edges.push(edge("T3", "U", EdgeKind::TypeRef));
    graph
}

#[test]
fn neighbors_adds_the_other_end_of_edges_of_a_node_of_interest_one_step_only() {
    let focus = Focus {
        nodes: set(&["T4"]),
        neighbors: true,
        ..Focus::default()
    };
    let view = with_v().cut_focused(&set(&["A", "a", "B", "b"]), &focus);

    // T3 is a neighbor of T4. U is a neighbor of T3 only, so `b` shows all children.
    assert_eq!(
        view.nodes,
        vec!["A", "a", "T3", "T4", "rest:a", "B", "b", "U", "V"]
    );
    assert_eq!(view.rest, [("a".to_string(), 2)].into());
}

#[test]
fn neighbors_follows_edges_that_start_at_a_descendant_of_a_node_of_interest() {
    let focus = Focus {
        nodes: set(&["B"]),
        neighbors: true,
        ..Focus::default()
    };
    let view = with_v().cut_focused(&set(&["A", "a", "B", "b"]), &focus);

    assert_eq!(
        view.nodes,
        vec!["A", "a", "T1", "T2", "T3", "rest:a", "B", "b", "U", "V"]
    );
    assert_eq!(view.rest, [("a".to_string(), 1)].into());
}

#[test]
fn neighbors_without_a_base_set_changes_nothing() {
    let focus = Focus {
        neighbors: true,
        ..Focus::default()
    };
    let graph = graph();
    assert_eq!(
        graph.cut_focused(&set(&["A", "a"]), &focus),
        graph.cut(&set(&["A", "a"]))
    );
}

#[test]
fn a_placeholder_follows_the_kept_subtrees_of_its_container_deepest_first() {
    let graph = Graph {
        nodes: vec![
            node("M", None, NodeKind::Module),
            node("p", Some("M"), NodeKind::Package),
            node("T", Some("p"), NodeKind::Type),
            node("f", Some("T"), NodeKind::Function),
            node("g", Some("T"), NodeKind::Function),
            node("S", Some("p"), NodeKind::Type),
            node("h", Some("S"), NodeKind::Function),
        ],
        ..Graph::default()
    };
    let focus = Focus {
        nodes: set(&["f"]),
        ..Focus::default()
    };
    let view = graph.cut_focused(&set(&["M", "p", "T"]), &focus);

    assert_eq!(view.nodes, vec!["M", "p", "T", "f", "rest:T", "rest:p"]);
    assert_eq!(
        view.rest,
        [("T".to_string(), 1), ("p".to_string(), 1)].into()
    );
    assert_eq!(view.expandable, vec!["M", "p", "T"]);
}

#[test]
fn rest_is_left_out_of_the_json_when_empty_and_defaults_on_read() {
    let graph = graph();
    let plain = graph.cut(&set(&["A"]));
    let json = serde_json::to_string(&plain).unwrap();
    assert!(!json.contains("rest"), "{json}");
    assert_eq!(serde_json::from_str::<cg_core::View>(&json).unwrap(), plain);

    let focus = Focus {
        nodes: set(&["T1"]),
        ..Focus::default()
    };
    let folded = graph.cut_focused(&set(&["A", "a"]), &focus);
    let json = serde_json::to_string(&folded).unwrap();
    assert!(json.contains(r#""rest":{"a":3}"#), "{json}");
}

/// Type `T` with the functions `f` and `g`, and type `S` in package `p`.
fn typed() -> Graph {
    Graph {
        nodes: vec![
            node("M", None, NodeKind::Module),
            node("p", Some("M"), NodeKind::Package),
            node("T", Some("p"), NodeKind::Type),
            node("f", Some("T"), NodeKind::Function),
            node("g", Some("T"), NodeKind::Function),
            node("S", Some("p"), NodeKind::Type),
            node("h", Some("S"), NodeKind::Function),
        ],
        ..Graph::default()
    }
}

#[test]
fn the_descendants_of_an_explicit_node_are_kept() {
    // `g` is marked, so without the rule `f` would fold.
    let mut graph = typed();
    graph.nodes[4].change = Some(cg_core::Change::Modified);
    let focus = Focus {
        nodes: set(&["T"]),
        changes: true,
        ..Focus::default()
    };
    let view = graph.cut_focused(&set(&["M", "p", "T"]), &focus);

    assert_eq!(view.nodes, vec!["M", "p", "T", "f", "g", "rest:p"]);
    assert_eq!(view.rest, [("p".to_string(), 1)].into());
}

#[test]
fn a_modified_type_still_folds_its_unchanged_functions() {
    let mut graph = typed();
    graph.nodes[2].change = Some(cg_core::Change::Modified);
    graph.nodes[4].change = Some(cg_core::Change::Modified);
    let focus = Focus {
        changes: true,
        ..Focus::default()
    };
    let view = graph.cut_focused(&set(&["M", "p", "T"]), &focus);

    assert_eq!(view.nodes, vec!["M", "p", "T", "g", "rest:T", "rest:p"]);
}

/// Module `M` with package `p` (sub-package `q` with type `Q1`, and types `T1`, `T2`)
/// and a leaf package `s` with type `S1`.
fn nested() -> Graph {
    Graph {
        nodes: vec![
            node("M", None, NodeKind::Module),
            node("p", Some("M"), NodeKind::Package),
            node("q", Some("p"), NodeKind::Package),
            node("Q1", Some("q"), NodeKind::Type),
            node("T1", Some("p"), NodeKind::Type),
            node("T2", Some("p"), NodeKind::Type),
            node("s", Some("M"), NodeKind::Package),
            node("S1", Some("s"), NodeKind::Type),
        ],
        edges: vec![
            edge("T1", "Q1", EdgeKind::TypeRef),
            edge("T2", "S1", EdgeKind::TypeRef),
            edge("Q1", "S1", EdgeKind::TypeRef),
            edge("Q1", "T1", EdgeKind::TypeRef),
            edge("T1", "T2", EdgeKind::TypeRef),
        ],
        ..Graph::default()
    }
}

#[test]
fn a_container_with_packages_and_other_children_folds_the_other_children_by_kind() {
    let focus = Focus {
        fold_mixed: true,
        ..Focus::default()
    };
    let view = nested().cut_focused(&set(&["M", "p"]), &focus);

    assert_eq!(view.nodes, vec!["M", "p", "q", "rest:p", "s"]);
    assert_eq!(view.rest, [("p".to_string(), 2)].into());
    assert_eq!(
        view.edges,
        vec![
            view_edge("q", "rest:p", &[(EdgeKind::TypeRef, 1)]),
            view_edge("q", "s", &[(EdgeKind::TypeRef, 1)]),
            view_edge("rest:p", "q", &[(EdgeKind::TypeRef, 1)]),
            view_edge("rest:p", "s", &[(EdgeKind::TypeRef, 1)]),
        ]
    );
}

fn mixed() -> Focus {
    Focus {
        fold_mixed: true,
        ..Focus::default()
    }
}

#[test]
fn an_unfolded_container_shows_all_children_in_spite_of_the_kind_fold() {
    let focus = Focus {
        unfolded: set(&["p"]),
        ..mixed()
    };
    let view = nested().cut_focused(&set(&["M", "p"]), &focus);

    assert_eq!(view.nodes, vec!["M", "p", "q", "T1", "T2", "s"]);
    assert!(view.rest.is_empty());
}

#[test]
fn leaf_packages_types_and_pure_package_containers_do_not_kind_fold() {
    // `s` holds only a type, `M` only packages, `Q1`'s container `q` only a type.
    let view = nested().cut_focused(&set(&["M", "s", "p", "q"]), &mixed());

    assert_eq!(view.nodes, vec!["M", "p", "q", "Q1", "rest:p", "s", "S1"]);
    assert_eq!(view.rest, [("p".to_string(), 2)].into());
}

#[test]
fn a_container_with_a_child_of_interest_follows_the_plain_fold() {
    // `T1` is of interest: `p` keeps `T1` and folds `q` and `T2`, as without the kind fold.
    let focus = Focus {
        nodes: set(&["T1"]),
        ..mixed()
    };
    let view = nested().cut_focused(&set(&["M", "p"]), &focus);

    assert_eq!(view.nodes, vec!["M", "p", "T1", "rest:p", "rest:M"]);
    assert_eq!(
        view.rest,
        [("p".to_string(), 2), ("M".to_string(), 1)].into()
    );
}

#[test]
fn a_node_of_interest_inside_a_sub_package_keeps_that_package_and_folds_the_types() {
    let focus = Focus {
        nodes: set(&["Q1"]),
        ..mixed()
    };
    let view = nested().cut_focused(&set(&["M", "p"]), &focus);

    assert_eq!(view.nodes, vec!["M", "p", "q", "rest:p", "rest:M"]);
}

#[test]
fn nodes_below_an_explicit_node_are_not_protected_from_the_kind_fold() {
    let focus = Focus {
        nodes: set(&["M"]),
        ..mixed()
    };
    let view = nested().cut_focused(&set(&["M", "p"]), &focus);

    assert_eq!(view.nodes, vec!["M", "p", "q", "rest:p", "s"]);
    assert_eq!(view.rest, [("p".to_string(), 2)].into());

    // An explicit type in `p` makes `p` follow the plain rule; below the explicit `M`
    // that keeps all children.
    let focus = Focus {
        nodes: set(&["M", "T1"]),
        ..mixed()
    };
    let view = nested().cut_focused(&set(&["M", "p"]), &focus);
    assert_eq!(view.nodes, vec!["M", "p", "q", "T1", "T2", "s"]);
    assert!(view.rest.is_empty());
}

#[test]
fn the_kind_fold_works_without_any_focus_and_leaves_the_plain_cut_alone_when_off() {
    let graph = nested();
    let open = set(&["M", "p"]);
    assert_eq!(
        graph.cut_focused(&open, &Focus::default()),
        graph.cut(&open)
    );
    assert_eq!(graph.cut(&open).nodes, vec!["M", "p", "q", "T1", "T2", "s"]);
    assert_eq!(
        graph.cut_focused(&open, &mixed()).nodes,
        vec!["M", "p", "q", "rest:p", "s"]
    );
}

#[test]
fn a_kind_placeholder_is_not_expandable_and_visible_ancestors_keep_their_change_counts() {
    let mut graph = nested();
    graph.nodes[4].change = Some(cg_core::Change::Added); // T1, folded
    graph.nodes[3].change = Some(cg_core::Change::Modified); // Q1 below visible q
    let view = graph.cut_focused(&set(&["M", "p"]), &mixed());

    assert!(!view.expandable.contains(&"rest:p".to_string()));
    assert!(view.expandable.contains(&"q".to_string()));
    assert_eq!(view.changes["p"].added, 1);
    assert_eq!(view.changes["M"].added, 1);
    assert_eq!(view.changes["q"].modified, 1);
    assert!(!view.changes.contains_key("rest:p"));
}

#[test]
fn a_value_folds_with_the_other_children_of_a_container_that_holds_packages() {
    let graph = Graph {
        nodes: vec![
            node("M", None, NodeKind::Module),
            node("p", Some("M"), NodeKind::Package),
            node("q", Some("p"), NodeKind::Package),
            node("f", Some("p"), NodeKind::Function),
            node("v", Some("p"), NodeKind::Value),
        ],
        edges: vec![edge("f", "v", EdgeKind::ValueRef)],
        ..Graph::default()
    };
    let view = graph.cut_focused(&set(&["M", "p"]), &mixed());

    assert_eq!(view.nodes, vec!["M", "p", "q", "rest:p"]);
    assert_eq!(view.rest, [("p".to_string(), 2)].into());
    assert!(view.edges.is_empty());
}
