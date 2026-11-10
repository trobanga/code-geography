use cg_core::{Edge, EdgeKind, Graph, Node, NodeKind, View};
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
        edges: vec![Edge {
            from: "TT".into(),
            to: "T".into(),
            kind: EdgeKind::TypeRef,
        }],
    };
    serde_json::to_string(&graph).unwrap()
}

fn view(explorer: &Explorer, expanded: &[&str], include_tests: bool) -> View {
    let expanded: Vec<String> = expanded.iter().map(|id| id.to_string()).collect();
    serde_json::from_str(&explorer.view_json(&expanded, include_tests)).unwrap()
}

#[test]
fn the_view_json_is_the_cut_of_the_graph_with_or_without_the_tests() {
    let explorer = Explorer::from_json(&graph_json()).unwrap();

    let all = view(&explorer, &["M", "p"], true);
    assert_eq!(all.nodes, ["M", "p", "T", "TT", "TM"]);
    assert_eq!(all.edges.len(), 1);

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
