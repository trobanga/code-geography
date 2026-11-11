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
            false,
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

#[test]
fn focus_view_json_passes_the_kind_fold() {
    // Package `p` holds the sub-package `q` and the type `T`.
    let graph = Graph {
        nodes: vec![
            node("M", None, NodeKind::Module, false),
            node("p", Some("M"), NodeKind::Package, false),
            node("q", Some("p"), NodeKind::Package, false),
            node("T", Some("p"), NodeKind::Type, false),
        ],
        ..Graph::default()
    };
    let explorer = Explorer::from_json(&serde_json::to_string(&graph).unwrap()).unwrap();
    let expanded = ["M".to_string(), "p".to_string()];
    let view = |fold_mixed| -> View {
        serde_json::from_str(&explorer.focus_view_json(
            &expanded,
            true,
            false,
            &[],
            false,
            false,
            &[],
            fold_mixed,
        ))
        .unwrap()
    };

    assert_eq!(view(false).nodes, ["M", "p", "q", "T"]);
    let folded = view(true);
    assert_eq!(folded.nodes, ["M", "p", "q", "rest:p"]);
    assert_eq!(folded.rest["p"], 1);
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

#[test]
fn index_sources_returns_the_graph_and_the_report_of_the_sources_as_json() {
    let paths = strings(&[
        "pom.xml",
        "src/main/java/a/Foo.java",
        "src/main/java/b/Bar.java",
    ]);
    let texts = strings(&[
        "",
        "package a;\nimport b.Bar;\nclass Foo extends Bar {}\n",
        "package b;\npublic class Bar {}\n",
    ]);

    let json = cg_wasm::index_sources(&paths, &texts, "repo").unwrap();

    let indexed: serde_json::Value = serde_json::from_str(&json).unwrap();
    let graph: Graph = serde_json::from_value(indexed["graph"].clone()).unwrap();
    assert_eq!(graph.node("mod:").unwrap().name, "repo");
    assert!(graph.node("type:a.Foo").is_some());
    assert!(
        graph
            .edges
            .iter()
            .any(|e| (e.from.as_str(), e.to.as_str(), e.kind)
                == ("type:a.Foo", "type:b.Bar", EdgeKind::Extends))
    );
    assert_eq!(indexed["report"]["calls"]["unresolved"], 0);
    assert_eq!(indexed["report"]["rust"]["paths"]["unresolved"], 0);
    // The graph of the result goes into an `Explorer`.
    assert!(Explorer::from_json(&indexed["graph"].to_string()).is_ok());
}

#[test]
fn index_sources_refuses_paths_and_texts_of_different_length() {
    let Err(message) = cg_wasm::index_sources(&strings(&["A.java"]), &[], "repo") else {
        panic!("expected an error");
    };

    assert!(message.contains("1") && message.contains("0"), "{message}");
}

#[test]
fn is_input_is_the_rule_of_the_indexer() {
    assert!(cg_wasm::is_input("a/Foo.java"));
    assert!(!cg_wasm::is_input("a/target/Foo.java"));
    assert!(cg_wasm::is_input("src/lib.rs"));
    assert!(cg_wasm::is_input("Cargo.toml"));
    assert!(!cg_wasm::is_input("a/target/lib.rs"));
}

const OLD_BASE: &str = "package a;\nclass Base {}\n";

fn diff_inputs() -> (Vec<String>, Vec<String>, Vec<String>, Vec<String>) {
    (
        strings(&[
            "pom.xml",
            "src/main/java/a/Base.java",
            "src/main/java/a/Gone.java",
        ]),
        strings(&["", OLD_BASE, "package a;\nclass Gone extends Base {}\n"]),
        strings(&[
            "pom.xml",
            "src/main/java/a/Base.java",
            "src/main/java/a/Sub.java",
        ]),
        strings(&["", OLD_BASE, "package a;\nclass Sub extends Base {}\n"]),
    )
}

#[test]
fn diff_sources_returns_the_graph_that_cg_index_gives_for_the_same_input() {
    let (base_paths, base_texts, head_paths, head_texts) = diff_inputs();
    let changed_paths = strings(&[
        "src/main/java/a/Gone.java",
        "src/main/java/a/Sub.java",
        "README.md",
    ]);
    let changed_kinds = strings(&["removed", "added", "modified"]);

    let json = cg_wasm::diff_sources(
        &base_paths,
        &base_texts,
        &head_paths,
        &head_texts,
        "repo",
        "v1",
        "v2",
        &changed_paths,
        &changed_kinds,
    )
    .unwrap();

    let sources = |paths: &[String], texts: &[String]| -> Vec<cg_index::SourceFile> {
        paths
            .iter()
            .zip(texts)
            .map(|(p, t)| cg_index::SourceFile {
                path: p.clone(),
                text: t.clone(),
            })
            .collect()
    };
    let (expected, report) = cg_index::diff_sources(
        "repo",
        &sources(&base_paths, &base_texts),
        &sources(&head_paths, &head_texts),
        cg_index::DiffSpec {
            base: "v1".into(),
            head: "v2".into(),
            changed: vec![
                ("src/main/java/a/Gone.java".into(), Change::Removed),
                ("src/main/java/a/Sub.java".into(), Change::Added),
                ("README.md".into(), Change::Modified),
            ],
        },
    );
    let result: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(result["graph"], serde_json::to_value(&expected).unwrap());
    assert_eq!(result["report"], serde_json::to_value(&report).unwrap());
    // Sanity: the graph is an overlay.
    assert_eq!(result["graph"]["diff"]["base"], "v1");
    assert_eq!(
        result["graph"]["diff"]["files"].as_array().unwrap().len(),
        3
    );
}

#[test]
fn diff_sources_refuses_parallel_arrays_of_different_length_and_unknown_kinds() {
    let (base_paths, base_texts, head_paths, head_texts) = diff_inputs();
    let call =
        |bp: &[String],
         bt: &[String],
         hp: &[String],
         ht: &[String],
         cp: &[String],
         ck: &[String]| { cg_wasm::diff_sources(bp, bt, hp, ht, "repo", "a", "b", cp, ck) };
    let one = strings(&["README.md"]);

    let short_base = call(
        &base_paths,
        &base_texts[..1],
        &head_paths,
        &head_texts,
        &[],
        &[],
    );
    let short_head = call(
        &base_paths,
        &base_texts,
        &head_paths[..1],
        &head_texts,
        &[],
        &[],
    );
    let short_kinds = call(
        &base_paths,
        &base_texts,
        &head_paths,
        &head_texts,
        &one,
        &[],
    );
    let unknown = call(
        &base_paths,
        &base_texts,
        &head_paths,
        &head_texts,
        &one,
        &strings(&["renamed"]),
    );

    assert!(short_base.is_err());
    assert!(short_head.is_err());
    assert!(short_kinds.is_err());
    let message = unknown.unwrap_err();
    assert!(message.contains("renamed"), "{message}");
}
