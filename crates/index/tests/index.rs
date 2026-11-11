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
    for path in [
        "pom.xml",
        "a/B.java",
        "Cargo.toml",
        "src/lib.rs",
        "deps.edn",
        "src/a.clj",
        "src/a.cljc",
    ] {
        assert!(is_input(path), "{path}");
    }
    for path in [
        "README.md",
        "web/package.json",
        "target/x.rs",
        "a/node_modules/B.java",
        "src/a.cljs",
        "project.clj",
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
fn clojure_sources_give_a_graph_and_the_clojure_report() {
    let files = [
        source("deps.edn", "{}"),
        source(
            "src/a.clj",
            "(ns a (:require [b]))\n(defn f [])\n(def x 1)\n",
        ),
        source("src/b.clj", "(ns b)\n"),
        source("build.clj", "(ns build)\n"),
    ];
    let mut reversed = files.clone();
    reversed.reverse();

    let (graph, report) = index_sources("repo", &files);

    assert!(graph.node("mod:").is_some());
    assert!(graph.node("fn:a/f").is_some());
    assert!(
        graph
            .edges
            .iter()
            .any(|e| e.from == "pkg::a" && e.to == "pkg::b")
    );
    assert_eq!(report.clojure.files_outside_source_dirs, 1);
    assert_eq!(report.clojure.forms_without_node["def"], 1);
    assert_eq!((graph, report), index_sources("repo", &reversed));
}

#[test]
fn java_and_clojure_in_one_repository_share_the_module_of_a_directory() {
    let files = [
        source("pom.xml", ""),
        source("src/main/java/a/Foo.java", "package a;\nclass Foo {}\n"),
        source("deps.edn", "{}"),
        source("src/c.clj", "(ns c)\n(defn f [])\n"),
    ];

    let (graph, _) = index_sources("repo", &files);

    let modules: Vec<_> = graph
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Module)
        .collect();
    assert_eq!(modules.len(), 1);
    assert_eq!(
        graph.node("pkg::c").unwrap().parent.as_deref(),
        Some("mod:")
    );
    assert_eq!(
        graph.node("pkg::a").unwrap().parent.as_deref(),
        Some("mod:")
    );
    assert!(graph.node("fn:c/f").is_some());
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

#[test]
fn the_calls_of_the_report_are_the_sum_over_the_front_ends_and_the_rust_part_stays_visible() {
    let files = [
        source("pom.xml", ""),
        source(
            "src/main/java/a/Foo.java",
            "package a;\nclass Foo {\n    void one() {}\n    void two() { one(); }\n}\n",
        ),
        source("Cargo.toml", "[package]\nname = \"tool\"\n"),
        source(
            "src/lib.rs",
            "mod m {\n    pub fn one() {}\n}\npub fn two() {\n    m::one();\n    m::one();\n}\n",
        ),
    ];

    let (_, report) = index_sources("repo", &files);

    assert_eq!(report.rust.calls.to_function, 2);
    assert_eq!(report.calls.to_function, 3);
    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["calls"]["to_function"], 3);
    assert_eq!(json["rust"]["calls"]["to_function"], 2);
}

#[test]
fn the_clojure_calls_are_added_to_the_calls_of_the_report_and_stay_visible() {
    let files = [
        source("pom.xml", ""),
        source(
            "src/main/java/a/Foo.java",
            "package a;\nclass Foo {\n    void one() {}\n    void two() { one(); }\n}\n",
        ),
        source("deps.edn", "{}"),
        source("src/x.clj", "(ns x)\n(defn f [])\n(def v 1)\n"),
        source(
            "src/user.clj",
            "(ns user (:require [x :as a]))\n(defn g [] (a/f) (a/v) (Math/abs 1) (a/none))\n(defn h [] (g))\n",
        ),
    ];

    let (_, report) = index_sources("repo", &files);

    assert_eq!(report.clojure.calls.to_function, 2);
    assert_eq!(report.clojure.unqualified.to_function, 1);
    assert_eq!(report.clojure.calls.external, 1);
    assert_eq!(report.clojure.calls.unresolved, 2);
    assert_eq!(report.clojure.var_without_node, 1);
    assert_eq!(report.calls.to_function, 3);
    assert_eq!(report.calls.external, 1);
    assert_eq!(report.calls.unresolved, 2);
    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["clojure"]["calls"]["to_function"], 2);
    assert_eq!(json["clojure"]["unqualified"]["to_function"], 1);
    assert_eq!(json["clojure"]["function_values"], 0);
}
