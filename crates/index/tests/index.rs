use cg_core::{EdgeKind, NodeKind};
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
        "mix.exs",
        "lib/a.ex",
        "test/a_test.exs",
        "a.ts",
        "a.tsx",
        "package.json",
        "tsconfig.json",
    ] {
        assert!(is_input(path), "{path}");
    }
    for path in [
        "README.md",
        "a.js",
        "web/node_modules/package.json",
        "target/x.rs",
        "a/node_modules/B.java",
        "src/a.cljs",
        "project.clj",
        "config/config.exs",
        "lib/a.eex",
        "deps/x/lib/a.ex",
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
    assert_eq!(graph.node("val:a/x").unwrap().kind, NodeKind::Value);
    assert_eq!(report.clojure.value_nodes, 1);
    assert_eq!((graph, report), index_sources("repo", &reversed));
}

#[test]
fn elixir_and_rust_in_one_directory_share_the_module_and_the_report_has_an_elixir_part() {
    let files = [
        source("Cargo.toml", "[package]\nname = \"tool\"\n"),
        source("src/lib.rs", "pub struct Bar;\n"),
        source("mix.exs", "def project, do: [app: :tool_ex]\n"),
        source(
            "lib/a.ex",
            "defmodule A do\n  alias B\n  def f(x), do: x\nend\n",
        ),
        source("lib/b.ex", "defmodule B do\nend\n"),
    ];
    let mut reversed = files.clone();
    reversed.reverse();

    let (graph, report) = index_sources("repo", &files);

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
        graph.node("pkg::A").unwrap().parent.as_deref(),
        Some("mod:")
    );
    assert!(graph.node("fn:A.f/1").is_some());
    assert!(
        graph
            .edges
            .iter()
            .any(|e| e.from == "pkg::A" && e.to == "pkg::B")
    );
    assert_eq!(report.elixir.references["alias"].resolved, 1);
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
    assert_eq!(report.clojure.calls.unresolved, 1);
    assert_eq!(report.clojure.calls.to_value, 1);
    assert_eq!(report.clojure.var_without_node, 0);
    assert_eq!(report.calls.to_function, 3);
    assert_eq!(report.calls.external, 1);
    assert_eq!(report.calls.unresolved, 1);
    assert_eq!(report.calls.to_value, 1);
    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["clojure"]["calls"]["to_function"], 2);
    assert_eq!(json["calls"]["to_value"], 1);
    assert_eq!(json["clojure"]["unqualified"]["to_function"], 1);
    assert_eq!(json["clojure"]["function_values"], 0);
}

#[test]
fn the_typescript_calls_are_added_to_the_calls_of_the_report_and_stay_visible() {
    let files = [
        source("pom.xml", ""),
        source(
            "src/main/java/a/Foo.java",
            "package a;\nclass Foo {\n    void one() {}\n    void two() { one(); }\n}\n",
        ),
        source("package.json", r#"{"name":"app"}"#),
        source(
            "a.ts",
            "export function a() {\n  b();\n  console.log(1);\n  c();\n  items.map(b);\n}\nfunction b() {}\n",
        ),
    ];

    let (_, report) = index_sources("repo", &files);

    assert_eq!(report.typescript.calls.to_function, 2);
    assert_eq!(report.typescript.calls.external, 3);
    assert_eq!(report.typescript.function_values, 1);
    assert_eq!(report.calls.to_function, 3);
    assert_eq!(report.calls.external, 3);
    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["typescript"]["calls"]["to_function"], 2);
    assert_eq!(json["typescript"]["function_values"], 1);
    assert_eq!(json["typescript"]["jsx_elements"], 0);
    assert_eq!(json["calls"]["to_function"], 3);
}

#[test]
fn the_elixir_calls_are_added_to_the_calls_of_the_report_and_stay_visible() {
    let files = [
        source("pom.xml", ""),
        source(
            "src/main/java/a/Foo.java",
            "package a;\nclass Foo {\n    void one() {}\n    void two() { one(); }\n}\n",
        ),
        source("mix.exs", "def project, do: [app: :demo]\n"),
        source(
            "lib/a.ex",
            "defmodule A do\n  def f, do: {g(), Enum.to_list([]), nobody()}\n  def g, do: 1\nend\n",
        ),
    ];

    let (graph, report) = index_sources("repo", &files);

    assert_eq!(report.elixir.calls.to_function, 1);
    assert_eq!(report.elixir.calls.external, 1);
    assert_eq!(report.elixir.calls.unresolved, 1);
    assert_eq!(report.elixir.unresolved_calls.local_unknown, 1);
    assert_eq!(report.calls.to_function, 2);
    assert_eq!(report.calls.external, 1);
    assert_eq!(report.calls.unresolved, 1);
    assert!(
        graph
            .edge("fn:A.f/0", "fn:A.g/0", cg_core::EdgeKind::Calls)
            .is_some()
    );
    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["elixir"]["calls"]["to_function"], 1);
    assert_eq!(json["elixir"]["unresolved_calls"]["local_unknown"], 1);
    assert_eq!(json["elixir"]["behind_outside_use"], 0);
}

const FOO: &str = "package a.b;\npublic class Foo {}\n";

#[test]
fn a_java_file_below_a_deps_edn_module_belongs_to_that_module() {
    let files = [
        source("deps.edn", "{}"),
        source("modules/x/deps.edn", "{:paths [\"src\" \"java\"]}"),
        source("modules/x/java/a/b/Foo.java", FOO),
        source("modules/x/impl/deps.edn", "{}"),
        source(
            "modules/x/impl/src/a/Bar.java",
            "package a;\nclass Bar {}\n",
        ),
    ];
    let mut reversed = files.clone();
    reversed.reverse();

    let (graph, _) = index_sources("repo", &files);

    let pkg = graph.node("pkg:modules/x:a.b").expect("package of Foo");
    assert_eq!(pkg.parent.as_deref(), Some("mod:modules/x"));
    assert_eq!(
        graph.node("type:a.b.Foo").unwrap().parent.as_deref(),
        Some("pkg:modules/x:a.b")
    );
    assert_eq!(
        graph
            .node("pkg:modules/x/impl:a")
            .unwrap()
            .parent
            .as_deref(),
        Some("mod:modules/x/impl")
    );
    // The module node of a directory with a `deps.edn` is one node.
    let modules = graph.nodes.iter().filter(|n| n.kind == NodeKind::Module);
    assert_eq!(modules.count(), 3);
    assert!(graph.node("pkg:a.b").is_none());
    assert_eq!((graph, ()), (index_sources("repo", &reversed).0, ()));
}

#[test]
fn a_java_file_in_a_deps_edn_module_is_test_code_by_the_source_directories_of_its_deps_edn() {
    let deps = "{:paths [\"src\" \"java\"] :aliases {:test {:extra-paths [\"java-test\"]}}}";
    let java = |package: &str| format!("package {package};\nclass C {{}}\n");
    let files = [
        source("modules/x/deps.edn", deps),
        source("modules/x/java/m/C.java", &java("m")),
        source("modules/x/java-test/t/C.java", &java("t")),
        // The directory is `src`, a main path: the rule `src/<not main>` does not apply.
        source("modules/x/src/test/java/s/C.java", &java("s")),
        // Outside every source directory: read, and not test code.
        source("modules/x/other/o/C.java", &java("o")),
        // A Maven module keeps the rule of ADR 8, also below a `deps.edn`.
        source("modules/y/deps.edn", "{}"),
        source("modules/y/pom.xml", ""),
        source("modules/y/src/test/java/p/C.java", &java("p")),
    ];

    let (graph, _) = index_sources("repo", &files);

    let test = |id: &str| graph.node(id).unwrap_or_else(|| panic!("{id}")).test;
    assert!(!test("type:m.C"));
    assert!(test("type:t.C"));
    assert!(!test("type:s.C"));
    assert!(!test("type:o.C"));
    assert!(test("type:p.C"));
}

fn java_class(package: &str) -> String {
    format!("package {package};\npublic class C {{}}\n")
}

fn clj_ns(namespace: &str) -> String {
    format!("(ns {namespace})\n(defn f [])\n")
}

#[test]
fn java_packages_and_clojure_namespaces_of_one_module_nest_as_one_set() {
    let deps = "{:paths [\"src\" \"java\"]}";
    let files = [
        source("m/deps.edn", deps),
        source("m/java/a/b/x/C.java", &java_class("a.b.x")),
        source("m/java/a/c/z/C.java", &java_class("a.c.z")),
        source("m/src/a/b/y.clj", &clj_ns("a.b.y")),
        // The same name in both languages is one package with the children of both.
        source("m/src/a/c/z.clj", &clj_ns("a.c.z")),
    ];
    let mut reversed = files.clone();
    reversed.reverse();

    let (graph, _) = index_sources("repo", &files);

    let parent = |id: &str| {
        graph
            .node(id)
            .unwrap_or_else(|| panic!("{id}"))
            .parent
            .clone()
    };
    let name = |id: &str| graph.node(id).unwrap().name.clone();
    // Neither front end alone has a node `a.b`: below it Java has only `x`, Clojure only
    // `y`. Together two segments follow it.
    assert_eq!(parent("pkg:m:a.b").as_deref(), Some("mod:m"));
    assert_eq!(parent("pkg:m:a.b.x").as_deref(), Some("pkg:m:a.b"));
    assert_eq!(name("pkg:m:a.b.x"), "x");
    assert_eq!(parent("pkg:m:a.b.y").as_deref(), Some("pkg:m:a.b"));
    assert_eq!(name("pkg:m:a.b.y"), "y");
    assert_eq!(parent("pkg:m:a.c.z").as_deref(), Some("mod:m"));
    assert_eq!(name("pkg:m:a.c.z"), "a.c.z");
    let same = graph.nodes.iter().filter(|n| n.id == "pkg:m:a.c.z");
    assert_eq!(same.count(), 1);
    assert_eq!(parent("type:a.c.z.C").as_deref(), Some("pkg:m:a.c.z"));
    assert_eq!(parent("fn:a.c.z/f").as_deref(), Some("pkg:m:a.c.z"));
    assert_eq!(graph, index_sources("repo", &reversed).0);
}

#[test]
fn a_prefix_package_of_both_languages_is_test_code_if_all_packages_below_it_are() {
    let deps =
        "{:paths [\"src\" \"java\"] :aliases {:test {:extra-paths [\"test\" \"java-test\"]}}}";
    let files = [
        // Module `t`: the Java package and the namespace below `a.b` are test code.
        source("t/deps.edn", deps),
        source("t/java-test/a/b/x/C.java", &java_class("a.b.x")),
        source("t/test/a/b/y.clj", &clj_ns("a.b.y")),
        source("t/java/a/c/z/C.java", &java_class("a.c.z")),
        // Module `m`: one of them is main code.
        source("m/deps.edn", deps),
        source("m/java-test/n/b/x/C.java", &java_class("n.b.x")),
        source("m/src/n/b/y.clj", &clj_ns("n.b.y")),
        source("m/java/n/c/z/C.java", &java_class("n.c.z")),
    ];

    let (graph, _) = index_sources("repo", &files);

    let test = |id: &str| graph.node(id).unwrap_or_else(|| panic!("{id}")).test;
    assert!(test("pkg:t:a.b"));
    assert!(test("pkg:t:a.b.x"));
    assert!(test("pkg:t:a.b.y"));
    assert!(!test("pkg:t:a.c.z"));
    assert!(!test("pkg:m:n.b"));
    assert!(test("pkg:m:n.b.x"));
    assert!(!test("pkg:m:n.b.y"));
}

#[test]
fn a_package_name_in_both_languages_is_test_code_only_if_both_are() {
    let deps =
        "{:paths [\"src\" \"java\"] :aliases {:test {:extra-paths [\"test\" \"java-test\"]}}}";
    let files = [
        source("m/deps.edn", deps),
        source("m/java-test/a/C.java", &java_class("a")),
        source("m/src/a.clj", &clj_ns("a")),
        source("m/java-test/b/C.java", &java_class("b")),
        source("m/test/b.clj", &clj_ns("b")),
    ];

    let (graph, _) = index_sources("repo", &files);

    assert!(!graph.node("pkg:m:a").unwrap().test);
    assert!(graph.node("pkg:m:b").unwrap().test);
}

#[test]
fn a_clojure_import_of_a_java_class_gives_an_imports_edge_to_the_type_also_a_nested_one() {
    let files = [
        source("m/deps.edn", "{:paths [\"src\" \"java\"]}"),
        source(
            "m/java/a/b/Outer.java",
            "package a.b;\npublic class Outer {\n    public static class Inner {}\n    public interface Api {}\n}\n",
        ),
        source(
            "m/java/a/b/Plain.java",
            "package a.b;\npublic interface Plain {}\n",
        ),
        source(
            "m/src/user.clj",
            "(ns user (:import [a.b Plain Outer Outer$Inner Outer$Api Missing] (java.util Date)))\n",
        ),
        // Another module sees the same classes: the ids hold no module.
        source("n/deps.edn", "{}"),
        source("n/src/other.clj", "(ns other (:import a.b.Outer$Inner))\n"),
    ];

    let (graph, _) = index_sources("repo", &files);

    let mut imports: Vec<_> = graph
        .edges
        .iter()
        .filter(|e| e.kind == cg_core::EdgeKind::Imports)
        .map(|e| (e.from.as_str(), e.to.as_str()))
        .collect();
    imports.sort();
    assert_eq!(
        imports,
        [
            ("pkg:m:user", "type:a.b.Outer"),
            ("pkg:m:user", "type:a.b.Outer.Api"),
            ("pkg:m:user", "type:a.b.Outer.Inner"),
            ("pkg:m:user", "type:a.b.Plain"),
            ("pkg:n:other", "type:a.b.Outer.Inner"),
        ]
    );
}

#[test]
fn a_clojure_function_that_names_a_java_class_gives_a_type_ref_edge_and_the_report_counts_it() {
    let files = [
        source("m/deps.edn", "{:paths [\"src\" \"java\"]}"),
        source(
            "m/java/a/b/Outer.java",
            "package a.b;\npublic class Outer {\n    public static class Inner {}\n}\n",
        ),
        source(
            "m/java/a/b/Plain.java",
            "package a.b;\npublic class Plain {}\n",
        ),
        source(
            "m/src/user.clj",
            "(ns user (:import [a.b Plain Outer$Inner] [java.util Date]))\n\
             (defn f [] [(Plain.) (Outer$Inner/make) (java.util.Date.) (Date.)])\n\
             (defn g [] (a.b.Outer/make))\n",
        ),
    ];

    let (graph, report) = index_sources("repo", &files);

    let mut refs: Vec<_> = graph
        .edges
        .iter()
        .filter(|e| e.kind == cg_core::EdgeKind::TypeRef)
        .map(|e| (e.from.as_str(), e.to.as_str()))
        .collect();
    refs.sort();
    assert_eq!(
        refs,
        [
            ("fn:user/f", "type:a.b.Outer.Inner"),
            ("fn:user/f", "type:a.b.Plain"),
            ("fn:user/g", "type:a.b.Outer"),
        ]
    );
    assert_eq!(report.clojure.classes.to_type, 3);
    assert_eq!(report.clojure.classes.external, 2);
    assert_eq!(report.calls.to_type, 3);
    assert_eq!(report.calls.external, 2);
}

#[test]
fn a_clojure_record_that_names_a_java_interface_implements_it() {
    let files = [
        source("m/deps.edn", "{:paths [\"src\" \"java\"]}"),
        source(
            "m/java/a/b/Api.java",
            "package a.b;\npublic interface Api {\n    interface Nested {}\n}\n",
        ),
        source(
            "m/src/user.clj",
            "(ns user (:import [a.b Api Api$Nested]))\n\
             (defrecord R [x] Api (m [_] 1))\n\
             (deftype T [x] a.b.Api$Nested)\n",
        ),
    ];

    let (graph, _) = index_sources("repo", &files);

    let mut found: Vec<_> = graph
        .edges
        .iter()
        .filter(|e| e.kind == cg_core::EdgeKind::Implements)
        .map(|e| (e.from.as_str(), e.to.as_str()))
        .collect();
    found.sort();
    assert_eq!(
        found,
        [
            ("type:user/R", "type:a.b.Api"),
            ("type:user/T", "type:a.b.Api.Nested")
        ]
    );
}

#[test]
fn a_namespace_gets_no_import_edge_to_a_java_type_that_is_inside_it() {
    let files = [
        source("m/deps.edn", "{:paths [\"src\" \"java\"]}"),
        source("m/java/a/b/Foo.java", "package a.b;\npublic class Foo {}\n"),
        source(
            "m/java/a/b/c/Bar.java",
            "package a.b.c;\npublic class Bar {}\n",
        ),
        source("m/java/a/Up.java", "package a;\npublic class Up {}\n"),
        source(
            "m/src/a/b.clj",
            "(ns a.b (:import [a.b Foo] [a.b.c Bar] [a Up]))\n(defn f [] [(Foo.) (Bar.) (Up.)])\n",
        ),
    ];

    let (graph, _) = index_sources("repo", &files);

    let mut edges: Vec<_> = graph
        .edges
        .iter()
        .map(|e| (e.from.as_str(), e.to.as_str(), e.kind))
        .collect();
    edges.sort();
    // Foo and Bar are inside the namespace node `a.b`. Only `Up` is outside it.
    assert_eq!(
        edges,
        [
            ("fn:a.b/f", "type:a.Up", cg_core::EdgeKind::TypeRef),
            ("fn:a.b/f", "type:a.b.Foo", cg_core::EdgeKind::TypeRef),
            ("fn:a.b/f", "type:a.b.c.Bar", cg_core::EdgeKind::TypeRef),
            ("pkg:m:a.b", "type:a.Up", cg_core::EdgeKind::Imports),
        ]
    );
}

#[test]
fn extend_type_on_a_java_class_puts_the_method_below_the_class_and_the_class_implements_the_protocol()
 {
    let files = [
        source("pom.xml", ""),
        source("src/main/java/a/b/Foo.java", FOO),
        source("deps.edn", "{}"),
        source(
            "src/p.clj",
            "(ns p)\n(defprotocol Store (-get [s]))\n(def limit 1)\n",
        ),
        source(
            "src/user.clj",
            "(ns user (:require [p]) (:import [a.b Foo]))\n\
             (extend-type Foo p/Store (-get [x] p/limit))\n",
        ),
    ];

    let (graph, report) = index_sources("repo", &files);

    let method = graph.node("ext:a.b.Foo#p/Store/-get").expect("method node");
    assert_eq!(method.parent.as_deref(), Some("type:a.b.Foo"));
    assert_eq!(method.name, "-get");
    assert!(graph.has_edge(
        "type:a.b.Foo",
        "type:p/Store",
        cg_core::EdgeKind::Implements
    ));
    assert!(graph.has_edge(
        "ext:a.b.Foo#p/Store/-get",
        "val:p/limit",
        cg_core::EdgeKind::ValueRef
    ));
    assert_eq!(report.clojure.extend_nodes, 1);
    assert_eq!(report.calls.to_value, 1);
}

/// The site entries of the one edge `(from, to, kind)`: file and lines.
fn sites(
    graph: &cg_core::Graph,
    from: &str,
    to: &str,
    kind: cg_core::EdgeKind,
) -> Vec<(Option<String>, Vec<u32>)> {
    graph
        .edge(from, to, kind)
        .unwrap_or_else(|| panic!("no edge {from} -> {to} {kind:?}"))
        .sites
        .iter()
        .map(|s| (s.file.clone(), s.lines.clone()))
        .collect()
}

#[test]
fn the_edges_of_each_front_end_keep_their_sites_in_the_joined_graph() {
    use cg_core::EdgeKind;
    let files = [
        source("j/pom.xml", ""),
        source(
            "j/src/main/java/a/Foo.java",
            "package a;\nclass Foo {\n    void f() {\n        new Bar().g();\n        new Bar().g();\n    }\n}\n",
        ),
        source(
            "j/src/main/java/a/Bar.java",
            "package a;\nclass Bar {\n    void g() {}\n}\n",
        ),
        source(
            "r/Cargo.toml",
            "[package]\nname = \"r\"\nversion = \"0.1.0\"\n",
        ),
        source(
            "r/src/lib.rs",
            "pub fn f() {\n    g();\n}\n\npub fn g() {}\n",
        ),
        source("m/deps.edn", "{:paths [\"src\" \"java\"]}"),
        source(
            "m/java/x/Plain.java",
            "package x;\npublic interface Plain {}\n",
        ),
        source(
            "m/src/user.clj",
            "(ns user\n  (:import\n   [x Plain]))\n\n(defn f []\n  Plain)\n",
        ),
    ];

    let (graph, _) = index_sources("repo", &files);

    assert_eq!(
        sites(&graph, "fn:a.Foo#f()", "fn:a.Bar#g()", EdgeKind::Calls),
        [(None, vec![4, 5])]
    );
    assert_eq!(
        sites(&graph, "fn:r::f", "fn:r::g", EdgeKind::Calls),
        [(None, vec![2])]
    );
    // A namespace is a package without a file: the entry names the file.
    assert_eq!(
        sites(&graph, "pkg:m:user", "type:x.Plain", EdgeKind::Imports),
        [(Some("m/src/user.clj".to_string()), vec![3])]
    );
    assert_eq!(
        sites(&graph, "fn:user/f", "type:x.Plain", EdgeKind::TypeRef),
        [(None, vec![6])]
    );
    assert!(graph.edges.iter().all(|e| !e.sites.is_empty()));
}

#[test]
fn typescript_sources_give_a_graph_and_the_typescript_report() {
    let files = [
        source("package.json", "{\"name\":\"app\"}"),
        source(
            "src/a.ts",
            "import { B } from './b';\nexport class A extends B {}\n",
        ),
        source("src/b.ts", "export class B {}\n"),
    ];
    let mut reversed = files.clone();
    reversed.reverse();

    let (graph, report) = index_sources("repo", &files);

    for id in [
        "mod:",
        "pkg::/src/a",
        "pkg::/src/b",
        "type::/src/a#A",
        "type::/src/b#B",
    ] {
        assert!(graph.node(id).is_some(), "{id}");
    }
    let has = |from: &str, to: &str, kind: EdgeKind| {
        graph
            .edges
            .iter()
            .any(|e| e.from == from && e.to == to && e.kind == kind)
    };
    assert!(has("pkg::/src/a", "type::/src/b#B", EdgeKind::Imports));
    assert!(has("type::/src/a#A", "type::/src/b#B", EdgeKind::Extends));
    assert_eq!(report.typescript.imports.resolved, 1);
    assert_eq!((graph, report), index_sources("repo", &reversed));
}

#[test]
fn typescript_and_rust_in_one_directory_share_the_module() {
    let files = [
        source("Cargo.toml", "[package]\nname = \"tool\"\n"),
        source("src/lib.rs", "pub struct Bar;\n"),
        source("package.json", "{\"name\":\"app\"}"),
        source("web/package.json", "{\"name\":\"web\"}"),
        source("src/a.ts", "export class A {}\n"),
        source("web/b.ts", "export class B {}\n"),
    ];

    let (graph, _) = index_sources("repo", &files);

    let modules: Vec<_> = graph
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Module)
        .map(|n| n.id.as_str())
        .collect();
    assert_eq!(modules, ["mod:", "mod:web"]);
    assert!(graph.node("type:tool::Bar").is_some());
    assert_eq!(
        graph.node("pkg::/src/a").unwrap().parent.as_deref(),
        Some("mod:")
    );
    assert!(graph.node("type::/src/a#A").is_some());
    assert!(graph.node("type:web:/b#B").is_some());
}
