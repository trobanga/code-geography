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
        stderr.contains("calls: 1 to function, 0 to type, 0 to value, 1 external, 1 unresolved\n"),
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
            "clojure: 1 files outside the source directories, 0 below no deps.edn, 1 without ns form, 1 duplicate namespaces; 1 top-level forms without a node\n"
        ),
        "{stderr}"
    );
    assert!(
        stderr.contains("clojure forms without a node: declare 1\n"),
        "{stderr}"
    );
}

#[test]
fn index_prints_what_the_elixir_front_end_resolved_and_did_not_read() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("repo");
    fs::create_dir_all(source.join("app/lib")).unwrap();
    fs::create_dir_all(source.join("app/src")).unwrap();
    fs::create_dir_all(source.join("stray")).unwrap();
    fs::write(
        source.join("app/mix.exs"),
        "def project, do: [app: :demo]\n",
    )
    .unwrap();
    fs::write(
        source.join("app/lib/a.ex"),
        "defmodule Demo.A do
  use Demo.B
  alias Demo.B
  alias Demo.B.Missing
  import :lists
  defstruct [:x]
  schema \"x\" do
  end
end
defmodule Demo.B do
end
",
    )
    .unwrap();
    fs::write(source.join("app/lib/dup.ex"), "defmodule Demo.B do\nend\n").unwrap();
    fs::write(source.join("app/src/x.ex"), "defmodule Demo.X do\nend\n").unwrap();
    fs::write(source.join("stray/z.ex"), "defmodule Z do\nend\n").unwrap();

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
        "elixir: 2 module references resolved, 1 external, 1 unresolved, 0 dynamic; 1 files outside the source directories, 1 in no project, 1 duplicate modules; 2 forms without a node\n",
        "elixir references: alias 1/0/1/0, import 0/1/0/0, use 1/0/0/0\n",
        "elixir unresolved: Demo.B.Missing 1\n",
        "elixir forms without a node: defstruct 1, schema 1\n",
    ] {
        assert!(stderr.contains(line), "{line}{stderr}");
    }
}

#[test]
fn index_prints_how_the_clojure_calls_were_resolved() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("repo");
    fs::create_dir_all(source.join("src")).unwrap();
    fs::write(source.join("deps.edn"), "{}").unwrap();
    fs::write(
        source.join("src/x.clj"),
        "(ns x)\n(defn f [])\n(declare v)\n(def w 1)\n",
    )
    .unwrap();
    fs::write(
        source.join("src/user.clj"),
        "(ns user (:require [x :as a]))\n(defn g [] (a/f) (map a/f []) (a/v) (a/w) (Math/abs 1) (a/none))\n(defn h [] (g))\n(def d (a/f))\n(defmethod mm :k [x] x)\n(extend-type String P (m [x] x))\n(a/f)\n",
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
        "clojure calls: 4 to function (1 as a value), 0 to type, 1 to value, 1 external, 2 unresolved (1 var without node); 1 more in code without a function node, not read\n",
        "clojure code without a function node, by head: other 1\n",
        "clojure nodes: 2 values, 1 defmethod, 1 extend methods; 0 duplicate declarations, 0 extend calls with a map not read\n",
        "clojure unqualified: 1 to function (0 as a value), 0 to type, 0 to value, 0 a local could hide, 0 ambiguous\n",
        "clojure unresolved: x/none 1\n",
    ] {
        assert!(stderr.contains(line), "{line}\n{stderr}");
    }
}

#[test]
fn index_prints_how_the_clojure_class_references_were_resolved() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("repo");
    fs::create_dir_all(source.join("src")).unwrap();
    fs::create_dir_all(source.join("java/a")).unwrap();
    fs::write(source.join("deps.edn"), "{:paths [\"src\" \"java\"]}").unwrap();
    fs::write(
        source.join("java/a/Foo.java"),
        "package a;\npublic class Foo {}\n",
    )
    .unwrap();
    fs::write(
        source.join("src/user.clj"),
        "(ns user (:import [a Foo] [java.util Date]))\n(defn f [] [(Foo.) (Foo.) (Date.)])\n(defn g [Foo] Foo)\n",
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
    let line = "clojure classes: 2 to a type of the repository, 1 external, 2 a local could hide\n";
    assert!(stderr.contains(line), "{line}\n{stderr}");
}

#[test]
fn index_prints_what_the_typescript_front_end_resolved_and_did_not_read() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("repo");
    fs::create_dir_all(source.join("app/src")).unwrap();
    fs::create_dir_all(source.join("stray")).unwrap();
    fs::write(source.join("app/package.json"), "{\"name\":\"app\"}").unwrap();
    fs::write(
        source.join("app/src/a.ts"),
        "import { B, helper } from './b';\nimport React from 'react';\nimport { M } from './missing';\nexport class A extends B {}\nexport function run(items: string[]) {\n  const f = make();\n  helper();\n  items.map(helper);\n  new B();\n  f.x();\n}\n",
    )
    .unwrap();
    fs::write(
        source.join("app/src/b.ts"),
        "export class B {}\nexport function helper() {}\n",
    )
    .unwrap();
    fs::write(source.join("stray/z.ts"), "export class Z {}\n").unwrap();

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
        "typescript: 1 files not read (no package.json above), 0 not TypeScript, 0 with syntax errors\n",
        "typescript imports: 1 resolved, 1 external, 1 unresolved, 0 other files\n",
        "typescript names: 2 to a declaration, 0 to a package\n",
        "typescript heritage: 1 resolved, 0 external, 0 unresolved\n",
        "typescript calls: 2 to function (1 as a value), 1 to type, 2 external, 1 unresolved; 0 of the call sites are JSX elements\n",
    ] {
        assert!(stderr.contains(line), "{line}{stderr}");
    }
}
