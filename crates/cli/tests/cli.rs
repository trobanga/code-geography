use cg_core::{EdgeKind, Graph};
use std::fs;
use std::process::Command;
use tempfile::TempDir;

#[test]
fn index_writes_a_graph_file_that_core_reads_again() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("repo/a");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("Base.java"), "package a;\nclass Base {}\n").unwrap();
    fs::write(
        source.join("Sub.java"),
        "package a;\nclass Sub extends Base {}\n",
    )
    .unwrap();
    let output = dir.path().join("graph.json");

    let status = Command::new(env!("CARGO_BIN_EXE_cg"))
        .arg("index")
        .arg(dir.path().join("repo"))
        .arg("-o")
        .arg(&output)
        .status()
        .unwrap();

    assert!(status.success());
    let graph: Graph = serde_json::from_str(&fs::read_to_string(&output).unwrap()).unwrap();
    assert_eq!(graph, cg_index::index(&dir.path().join("repo")).unwrap());
    assert!(graph.has_edge("type:a.Sub", "type:a.Base", EdgeKind::Extends));
}

#[test]
fn the_usage_text_names_both_commands() {
    let output = Command::new(env!("CARGO_BIN_EXE_cg")).output().unwrap();

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("cg index <repo>"), "{stderr}");
    assert!(stderr.contains("cg diff <repo> <rev>"), "{stderr}");
}

#[test]
fn index_prints_how_the_calls_were_resolved() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("repo/a");
    fs::create_dir_all(&source).unwrap();
    fs::write(
        source.join("T.java"),
        "package a;\nclass T {\n    void m() {}\n    void n(T t) { t.m(); Math.abs(1); t.gone.x(); }\n}\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_cg"))
        .arg("index")
        .arg(dir.path().join("repo"))
        .arg("-o")
        .arg(dir.path().join("graph.json"))
        .output()
        .unwrap();

    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("calls: 1 to function, 0 to type, 1 external, 1 unresolved\n"),
        "{stderr}"
    );
}

#[test]
fn index_prints_what_the_rust_front_end_could_not_follow() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("repo");
    fs::create_dir_all(source.join("src")).unwrap();
    fs::write(source.join("Cargo.toml"), "[package]\nname = \"tool\"\n").unwrap();
    fs::write(
        source.join("src/lib.rs"),
        "use crate::gone::G;\nuse std::fmt::Debug;\npub struct S;\npub fn f(s: S) {}\n",
    )
    .unwrap();
    fs::write(source.join("src/orphan.rs"), "pub struct O;\n").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_cg"))
        .arg("index")
        .arg(&source)
        .arg("-o")
        .arg(dir.path().join("graph.json"))
        .output()
        .unwrap();

    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "rust: 1 paths resolved, 1 external, 1 unresolved; 1 files not reached, 0 #[path] attributes\n"
        ),
        "{stderr}"
    );
}

#[test]
fn index_prints_how_the_rust_calls_were_resolved() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("repo");
    fs::create_dir_all(source.join("src")).unwrap();
    fs::write(source.join("Cargo.toml"), "[package]\nname = \"tool\"\n").unwrap();
    fs::write(
        source.join("src/lib.rs"),
        "fn g() {}\nfn h() {}\npub fn f(x: Vec<u8>, y: u8) {\n    g();\n    x.len();\n    y.gone();\n    let _ = Some(h);\n    println!(\"{}\", g());\n}\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_cg"))
        .arg("index")
        .arg(&source)
        .arg("-o")
        .arg(dir.path().join("graph.json"))
        .output()
        .unwrap();

    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "rust calls: 2 to function (1 as a value), 0 to type, 2 external, 0 unresolved; 1 more in macro arguments, not read\n"
        ),
        "{stderr}"
    );
}

#[test]
fn index_prints_what_the_clojure_front_end_did_not_read() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("repo");
    fs::create_dir_all(source.join("src")).unwrap();
    fs::write(source.join("deps.edn"), "{}").unwrap();
    fs::write(
        source.join("src/a.clj"),
        "(ns a)\n(def x 1)\n(def y 2)\n(declare z)\n(defn f [])\n",
    )
    .unwrap();
    fs::write(source.join("src/b.clj"), "(ns a)\n").unwrap();
    fs::write(source.join("src/c.clj"), "(defn g [])\n").unwrap();
    fs::write(source.join("build.clj"), "(ns build)\n").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_cg"))
        .arg("index")
        .arg(&source)
        .arg("-o")
        .arg(dir.path().join("graph.json"))
        .output()
        .unwrap();

    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "clojure: 1 files outside the source directories, 0 below no deps.edn, 1 without ns form, 1 duplicate namespaces; 3 top-level forms without a node\n"
        ),
        "{stderr}"
    );
    assert!(
        stderr.contains("clojure forms without a node: def 2, declare 1\n"),
        "{stderr}"
    );
}

#[test]
fn index_prints_how_the_clojure_calls_were_resolved() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("repo");
    fs::create_dir_all(source.join("src")).unwrap();
    fs::write(source.join("deps.edn"), "{}").unwrap();
    fs::write(source.join("src/x.clj"), "(ns x)\n(defn f [])\n(def v 1)\n").unwrap();
    fs::write(
        source.join("src/user.clj"),
        "(ns user (:require [x :as a]))\n(defn g [] (a/f) (map a/f []) (a/v) (Math/abs 1) (a/none))\n(defn h [] (g))\n(def d (a/f))\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_cg"))
        .arg("index")
        .arg(&source)
        .arg("-o")
        .arg(dir.path().join("graph.json"))
        .output()
        .unwrap();

    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    for line in [
        "clojure calls: 2 to function (1 as a value), 0 to type, 1 external, 2 unresolved (1 var without node); 1 more in code without a function node, not read\n",
        "clojure code without a function node, by head: def 1\n",
        "clojure unqualified heads: 1 of the same namespace (0 a local could hide), 0 referred (0 a local could hide)\n",
        "clojure unresolved: x/none 1\n",
    ] {
        assert!(stderr.contains(line), "{line}\n{stderr}");
    }
}
