use cg_core::{Change, Edge, EdgeKind, Graph, Node, NodeKind, View};
use cg_wasm::Explorer;

fn node(id: &str, parent: Option<&str>, kind: NodeKind, test: bool) -> Node {
    Node {
        id: id.into(),
        parent: parent.map(Into::into),
        kind,
        name: id.into(),
        file: None,
        span: None,
        test,
        hash: None,
        change: None,
    }
}

/// Module `M` with the package `p`: type `T` and test type `TT`, which uses `T`.
/// Module `TM` holds only a test type.
fn graph_json() -> String {
    let graph = Graph {
        nodes: vec![
            node("M", None, NodeKind::Module, false),
            node("p", Some("M"), NodeKind::Package, false),
            node("T", Some("p"), NodeKind::Type, false),
            node("TT", Some("p"), NodeKind::Type, true),
            node("TM", None, NodeKind::Module, false),
            node("tp", Some("TM"), NodeKind::Package, false),
            node("TU", Some("tp"), NodeKind::Type, true),
        ],
        edges: vec![
            Edge {
                from: "TT".into(),
                to: "T".into(),
                kind: EdgeKind::Imports,
                change: None,
            },
            Edge {
                from: "TT".into(),
                to: "T".into(),
                kind: EdgeKind::TypeRef,
                change: None,
            },
        ],
        ..Graph::default()
    };
    serde_json::to_string(&graph).unwrap()
}

fn view(explorer: &Explorer, expanded: &[&str], include_tests: bool) -> View {
    let expanded: Vec<String> = expanded.iter().map(|id| id.to_string()).collect();
    serde_json::from_str(&explorer.view_json(&expanded, include_tests, false)).unwrap()
}

#[test]
fn the_view_json_is_the_cut_of_the_graph_with_or_without_the_tests() {
    let explorer = Explorer::from_json(&graph_json()).unwrap();

    let all = view(&explorer, &["M", "p"], true);
    assert_eq!(all.nodes, ["M", "p", "T", "TT", "TM"]);
    assert_eq!(all.edges[0].count, 2);

    let without = view(&explorer, &["M", "p"], false);
    assert_eq!(without.nodes, ["M", "p", "T"]);
    assert_eq!(without.edges, []);
    assert_eq!(without.expandable, ["M", "p"]);
}

#[test]
fn invalid_json_gives_an_error_message() {
    let Err(message) = Explorer::from_json("{not json") else {
        panic!("expected an error");
    };

    assert!(!message.is_empty());
}

#[test]
fn merge_imports_leaves_out_the_imports_that_a_use_covers() {
    let explorer = Explorer::from_json(&graph_json()).unwrap();
    let expanded = ["M".to_string(), "p".to_string()];

    let merged: View = serde_json::from_str(&explorer.view_json(&expanded, true, true)).unwrap();

    assert_eq!(merged.edges[0].count, 1);
    assert_eq!(merged.edges[0].kinds.get(&EdgeKind::TypeRef), Some(&1));
}

#[test]
fn the_marks_of_a_diff_graph_reach_the_view_in_each_variant() {
    let marked = |id: &str, parent: Option<&str>, kind, test, change| Node {
        change: Some(change),
        ..node(id, parent, kind, test)
    };
    let graph = Graph {
        nodes: vec![
            node("M", None, NodeKind::Module, false),
            node("p", Some("M"), NodeKind::Package, false),
            marked("T", Some("p"), NodeKind::Type, false, Change::Added),
            marked("TT", Some("p"), NodeKind::Type, true, Change::Modified),
            node("U", Some("p"), NodeKind::Type, false),
        ],
        edges: vec![
            Edge {
                from: "T".into(),
                to: "U".into(),
                kind: EdgeKind::Imports,
                change: Some(Change::Added),
            },
            Edge {
                from: "T".into(),
                to: "U".into(),
                kind: EdgeKind::TypeRef,
                change: Some(Change::Removed),
            },
        ],
        ..Graph::default()
    };
    let explorer = Explorer::from_json(&serde_json::to_string(&graph).unwrap()).unwrap();
    let expanded = ["M".to_string(), "p".to_string()];

    let with_tests: View =
        serde_json::from_str(&explorer.view_json(&expanded, true, false)).unwrap();
    assert_eq!(with_tests.changes["M"].added, 1);
    assert_eq!(with_tests.changes["M"].modified, 1);
    assert_eq!(
        (with_tests.edges[0].added, with_tests.edges[0].removed),
        (1, 1)
    );

    let without: View = serde_json::from_str(&explorer.view_json(&expanded, false, false)).unwrap();
    assert_eq!(without.changes["M"].modified, 0);
    assert_eq!(without.changes["T"].added, 1);

    let merged: View = serde_json::from_str(&explorer.view_json(&expanded, false, true)).unwrap();
    assert_eq!((merged.edges[0].count, merged.edges[0].removed), (1, 1));
}

#[test]
fn focus_view_json_folds_in_the_chosen_variant_and_passes_each_option() {
    let explorer = Explorer::from_json(&graph_json()).unwrap();
    let expanded = ["M".to_string(), "p".to_string()];
    let focus = ["T".to_string()];
    let focus_view = |include_tests, changes, neighbors, unfolded: &[String]| -> View {
        serde_json::from_str(&explorer.focus_view_json(
            &expanded,
            include_tests,
            false,
            &focus,
            changes,
            neighbors,
            unfolded,
        ))
        .unwrap()
    };

    let folded = focus_view(true, false, false, &[]);
    assert_eq!(folded.nodes, ["M", "p", "T", "rest:p", "TM"]);
    assert_eq!(folded.rest["p"], 1);

    // Without tests `TT` does not exist, so nothing folds.
    let without = focus_view(false, false, false, &[]);
    assert_eq!(without.nodes, ["M", "p", "T"]);
    assert!(without.rest.is_empty());

    // `TT` uses `T`: a neighbor, so kept.
    let near = focus_view(true, false, true, &[]);
    assert_eq!(near.nodes, ["M", "p", "T", "TT", "TM"]);

    let unfolded = focus_view(true, false, false, &["p".to_string()]);
    assert_eq!(unfolded.nodes, ["M", "p", "T", "TT", "TM"]);
}
