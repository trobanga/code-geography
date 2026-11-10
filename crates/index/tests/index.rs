use cg_core::NodeKind;
use cg_index::{SourceFile, index, index_sources, index_with_report, is_input};
use std::fs;
use tempfile::TempDir;

fn source(path: &str, text: &str) -> SourceFile {
    SourceFile {
        path: path.to_string(),
        text: text.to_string(),
    }
}

#[test]
fn is_input_is_true_when_any_front_end_wants_the_file() {
    for path in ["pom.xml", "a/B.java", "Cargo.toml", "src/lib.rs"] {
        assert!(is_input(path), "{path}");
    }
    for path in [
        "README.md",
        "web/package.json",
        "target/x.rs",
        "a/node_modules/B.java",
    ] {
        assert!(!is_input(path), "{path}");
    }
}

#[test]
fn java_and_rust_sources_of_one_repository_give_one_graph_with_both_reports() {
    let files = [
        source("pom.xml", ""),
        source("src/main/java/a/Foo.java", "package a;\nclass Foo {}\n"),
        source("Cargo.toml", "[package]\nname = \"tool\"\n"),
        source("src/lib.rs", "pub struct Bar;\n"),
        source("src/orphan.rs", ""),
    ];
    let mut reversed = files.clone();
    reversed.reverse();

    let (graph, report) = index_sources("repo", &files);

    assert!(graph.node("type:a.Foo").is_some());
    assert!(graph.node("type:tool::Bar").is_some());
    // Both sit in the module of the root directory: one node.
    let modules: Vec<_> = graph
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Module)
        .collect();
    assert_eq!(modules.len(), 1);
    assert_eq!(
        graph.node("type:tool::Bar").unwrap().parent.as_deref(),
        Some("mod:")
    );
    assert_eq!(
        graph.node("pkg::a").unwrap().parent.as_deref(),
        Some("mod:")
    );
    assert_eq!(report.rust.unreached_files, ["src/orphan.rs"]);
    assert_eq!(report.calls.to_function, 0);
    assert_eq!((graph, report), index_sources("repo", &reversed));
}

#[test]
fn index_walks_a_directory_and_leaves_out_what_no_front_end_wants() {
    let dir = TempDir::new().unwrap();
    for (path, text) in [
        ("Cargo.toml", "[package]\nname = \"tool\"\n"),
        ("src/lib.rs", "pub mod a;\n"),
        ("src/a.rs", "pub struct A;\nuse std::fmt;\n"),
        ("target/debug/x.rs", "pub struct Copy;\n"),
        ("src/a/Foo.java", "package a;\nclass Foo {}\n"),
        ("notes.txt", "x"),
    ] {
        let full = dir.path().join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, text).unwrap();
    }

    let graph = index(dir.path()).unwrap();
    let (again, report) = index_with_report(dir.path()).unwrap();

    assert_eq!(graph, again);
    assert!(graph.node("type:tool::a::A").is_some());
    assert!(graph.node("type:a.Foo").is_some());
    assert!(graph.nodes.iter().all(|n| n.name != "Copy"));
    assert!(report.rust.paths.external >= 1);
}
