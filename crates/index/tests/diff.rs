use cg_core::{Change, EdgeKind};
use cg_index::{DiffSpec, SourceFile, diff_indexed, diff_sources, index_sources};

fn source(path: &str, text: &str) -> SourceFile {
    SourceFile {
        path: path.to_string(),
        text: text.to_string(),
    }
}

fn changed(items: &[(&str, Change)]) -> Vec<(String, Change)> {
    items.iter().map(|(p, c)| (p.to_string(), *c)).collect()
}

const BASE: &str = "package a;\nclass Base {\n    int one() { return 1; }\n}\n";

#[test]
fn two_source_sets_give_the_overlay_with_marks_revision_names_and_files() {
    let before = [
        source("pom.xml", ""),
        source("src/main/java/a/Base.java", BASE),
        source(
            "src/main/java/a/Gone.java",
            "package a;\nclass Gone extends Base {}\n",
        ),
    ];
    let after = [
        source("pom.xml", ""),
        source("src/main/java/a/Base.java", BASE),
        source(
            "src/main/java/a/Sub.java",
            "package a;\nclass Sub extends Base {}\n",
        ),
        source(
            "src/test/java/a/SubTest.java",
            "package a;\nclass SubTest {}\n",
        ),
    ];
    let spec = DiffSpec {
        base: "v1".into(),
        head: "v2".into(),
        changed: changed(&[
            ("src/main/java/a/Gone.java", Change::Removed),
            ("src/main/java/a/Sub.java", Change::Added),
            ("src/test/java/a/SubTest.java", Change::Added),
        ]),
    };

    let (graph, _report) = diff_sources("repo", &before, &after, spec);

    let nodes: Vec<_> = graph
        .nodes
        .iter()
        .filter_map(|n| n.change.map(|c| (n.id.as_str(), c)))
        .collect();
    assert_eq!(
        nodes,
        [
            ("type:a.Sub", Change::Added),
            ("type:a.SubTest", Change::Added),
            ("type:a.Gone", Change::Removed),
        ]
    );
    assert!(graph.edges.iter().any(|e| e.from == "type:a.Sub"
        && e.to == "type:a.Base"
        && e.kind == EdgeKind::Extends
        && e.change == Some(Change::Added)));
    assert!(graph.edges.iter().any(|e| e.from == "type:a.Gone"
        && e.to == "type:a.Base"
        && e.kind == EdgeKind::Extends
        && e.change == Some(Change::Removed)));
    let info = graph.diff.expect("diff info");
    assert_eq!((info.base.as_str(), info.head.as_str()), ("v1", "v2"));
    let files: Vec<_> = info
        .files
        .iter()
        .map(|f| (f.path.as_str(), f.change, f.module.as_deref(), f.test))
        .collect();
    assert_eq!(
        files,
        [
            (
                "src/main/java/a/Gone.java",
                Change::Removed,
                Some("mod:"),
                false
            ),
            (
                "src/main/java/a/Sub.java",
                Change::Added,
                Some("mod:"),
                false
            ),
            (
                "src/test/java/a/SubTest.java",
                Change::Added,
                Some("mod:"),
                true
            ),
        ]
    );
}

#[test]
fn a_changed_file_that_is_in_neither_source_set_is_still_listed() {
    let before = [source("pom.xml", "")];
    let after = [source("pom.xml", "")];
    let spec = DiffSpec {
        base: "a".into(),
        head: "b".into(),
        changed: changed(&[
            ("README.md", Change::Modified),
            ("docs/x.png", Change::Added),
        ]),
    };

    let (graph, _) = diff_sources("repo", &before, &after, spec);

    let files: Vec<_> = graph
        .diff
        .expect("diff info")
        .files
        .iter()
        .map(|f| (f.path.clone(), f.change, f.module.clone()))
        .collect();
    assert_eq!(
        files,
        [
            ("README.md".to_string(), Change::Modified, None),
            ("docs/x.png".to_string(), Change::Added, None),
        ]
    );
}

#[test]
fn a_changed_clojure_file_is_test_code_by_the_source_directories_of_its_deps_edn() {
    let deps =
        "{:paths [\"src\"] :aliases {:test {:extra-paths [\"test\" \"src/blaze/testing\"]}}}";
    let before = [
        source("deps.edn", deps),
        source("modules/db/deps.edn", deps),
        source("modules/gone/deps.edn", deps),
    ];
    let after = [
        source("deps.edn", deps),
        source("modules/db/deps.edn", deps),
    ];
    let spec = DiffSpec {
        base: "a".into(),
        head: "b".into(),
        changed: changed(&[
            ("modules/db/src/blaze/db/api.clj", Change::Modified),
            ("modules/db/test/blaze/db/api_test.clj", Change::Modified),
            ("modules/db/src/blaze/testing/util.cljc", Change::Added),
            ("modules/db/build.clj", Change::Modified),
            ("modules/gone/test/x_test.clj", Change::Removed),
            ("modules/gone/src/x.clj", Change::Removed),
            ("src/blaze/other.clj", Change::Modified),
            // A Java file of a `deps.edn` module follows the `deps.edn` too: `src` is a main
            // path here. Not Clojure and not Java: the rule of ADR 8 stays.
            ("src/test/java/a/FooTest.java", Change::Modified),
            ("src/main/java/a/Foo.java", Change::Modified),
            ("README.md", Change::Modified),
        ]),
    };

    let (graph, _) = diff_sources("repo", &before, &after, spec);

    let info = graph.diff.expect("diff info");
    let files: Vec<_> = info
        .files
        .iter()
        .map(|f| (f.path.as_str(), f.module.as_deref(), f.test))
        .collect();
    assert_eq!(
        files,
        [
            (
                "modules/db/src/blaze/db/api.clj",
                Some("mod:modules/db"),
                false
            ),
            (
                "modules/db/test/blaze/db/api_test.clj",
                Some("mod:modules/db"),
                true
            ),
            (
                "modules/db/src/blaze/testing/util.cljc",
                Some("mod:modules/db"),
                true
            ),
            ("modules/db/build.clj", Some("mod:modules/db"), false),
            (
                "modules/gone/test/x_test.clj",
                Some("mod:modules/gone"),
                true
            ),
            ("modules/gone/src/x.clj", Some("mod:modules/gone"), false),
            ("src/blaze/other.clj", Some("mod:"), false),
            ("src/test/java/a/FooTest.java", Some("mod:"), false),
            ("src/main/java/a/Foo.java", Some("mod:"), false),
            ("README.md", Some("mod:"), false),
        ]
    );
}

#[test]
fn a_changed_elixir_file_is_test_code_by_the_directory_test_of_its_mix_project() {
    let mix = "def project, do: [app: :app]\n";
    let before = [source("mix.exs", mix), source("gone/mix.exs", mix)];
    let after = [source("mix.exs", mix)];
    let spec = DiffSpec {
        base: "a".into(),
        head: "b".into(),
        changed: changed(&[
            ("test/a_test.exs", Change::Modified),
            ("test/support/h.ex", Change::Added),
            ("lib/a.ex", Change::Modified),
            // ADR 8 alone would call this test code.
            ("lib/src/x/a.ex", Change::Modified),
            ("gone/test/x_test.exs", Change::Removed),
            ("gone/lib/x.ex", Change::Removed),
        ]),
    };

    let (graph, _) = diff_sources("repo", &before, &after, spec);

    let info = graph.diff.expect("diff info");
    let files: Vec<_> = info
        .files
        .iter()
        .map(|f| (f.path.as_str(), f.test))
        .collect();
    assert_eq!(
        files,
        [
            ("test/a_test.exs", true),
            ("test/support/h.ex", true),
            ("lib/a.ex", false),
            ("lib/src/x/a.ex", false),
            ("gone/test/x_test.exs", true),
            ("gone/lib/x.ex", false),
        ]
    );
}

#[test]
fn a_changed_java_file_in_a_deps_edn_module_follows_the_source_directories_of_its_deps_edn() {
    let deps = "{:paths [\"src\" \"java\"] :aliases {:test {:extra-paths [\"java-test\"]}}}";
    let before = [
        source("modules/gone/deps.edn", deps),
        source("modules/db/deps.edn", deps),
        source("modules/mvn/pom.xml", ""),
        source("modules/mvn/deps.edn", deps),
    ];
    let after = [
        source("modules/db/deps.edn", deps),
        source("modules/mvn/pom.xml", ""),
        source("modules/mvn/deps.edn", deps),
    ];
    let spec = DiffSpec {
        base: "a".into(),
        head: "b".into(),
        changed: changed(&[
            ("modules/db/java/a/Foo.java", Change::Modified),
            ("modules/db/java-test/a/FooTest.java", Change::Modified),
            // `src` is a main path of the module: ADR 8 would say test code.
            ("modules/db/src/test/java/a/Bar.java", Change::Modified),
            ("modules/db/other/a/Baz.java", Change::Modified),
            ("modules/gone/java-test/a/Gone.java", Change::Removed),
            // A module with a `pom.xml` keeps the rule of ADR 8.
            ("modules/mvn/src/test/java/a/Mvn.java", Change::Modified),
            ("modules/mvn/java-test/a/Mvn2.java", Change::Modified),
            // No module at all.
            ("src/test/java/a/Free.java", Change::Modified),
        ]),
    };

    let (graph, _) = diff_sources("repo", &before, &after, spec);

    let files: Vec<_> = graph
        .diff
        .expect("diff info")
        .files
        .iter()
        .map(|f| (f.path.clone(), f.test))
        .collect();
    assert_eq!(
        files,
        [
            ("modules/db/java/a/Foo.java".to_string(), false),
            ("modules/db/java-test/a/FooTest.java".to_string(), true),
            ("modules/db/src/test/java/a/Bar.java".to_string(), false),
            ("modules/db/other/a/Baz.java".to_string(), false),
            ("modules/gone/java-test/a/Gone.java".to_string(), true),
            ("modules/mvn/src/test/java/a/Mvn.java".to_string(), true),
            ("modules/mvn/java-test/a/Mvn2.java".to_string(), false),
            ("src/test/java/a/Free.java".to_string(), true),
        ]
    );
}

#[test]
fn a_changed_def_is_a_modified_value_and_a_new_defmethod_an_added_function() {
    let user = |value: &str, extra: &str| {
        source(
            "src/user.clj",
            &format!("(ns user)\n(defmulti m :k)\n(def limit {value})\n{extra}"),
        )
    };
    let before = [source("deps.edn", "{}"), user("1", "")];
    let after = [
        source("deps.edn", "{}"),
        user("2", "(defmethod m :a [x] limit)\n"),
    ];
    let spec = DiffSpec {
        base: "v1".into(),
        head: "v2".into(),
        changed: changed(&[("src/user.clj", Change::Modified)]),
    };

    let (graph, _) = diff_sources("repo", &before, &after, spec);

    let nodes: Vec<_> = graph
        .nodes
        .iter()
        .filter_map(|n| n.change.map(|c| (n.id.as_str(), c)))
        .collect();
    assert_eq!(
        nodes,
        [
            ("val:user/limit", Change::Modified),
            ("fn:user/user/m :a", Change::Added)
        ]
    );
    let edges: Vec<_> = graph
        .edges
        .iter()
        .filter_map(|e| {
            e.change
                .map(|c| (e.from.as_str(), e.to.as_str(), e.kind, c))
        })
        .collect();
    assert_eq!(
        edges,
        [
            (
                "fn:user/user/m :a",
                "fn:user/m",
                EdgeKind::Implements,
                Change::Added
            ),
            (
                "fn:user/user/m :a",
                "val:user/limit",
                EdgeKind::ValueRef,
                Change::Added
            ),
        ]
    );
}

/// The site entries of an edge: file and lines.
type Entries = Vec<(Option<String>, Vec<u32>)>;

/// The mark and the site entries of the one edge `(from, to, kind)`.
fn edge_sites(
    graph: &cg_core::Graph,
    from: &str,
    to: &str,
    kind: EdgeKind,
) -> (Option<Change>, Entries) {
    let edge = graph
        .edge(from, to, kind)
        .unwrap_or_else(|| panic!("no edge {from} -> {to} {kind:?}"));
    (
        edge.change,
        edge.sites
            .iter()
            .map(|s| (s.file.clone(), s.lines.clone()))
            .collect(),
    )
}

const CALLEE: &str =
    "package a;\nclass Bar {\n    void g() {}\n    void old() {}\n    void fresh() {}\n}\n";

fn spec(changed_files: &[(&str, Change)]) -> DiffSpec {
    DiffSpec {
        base: "v1".into(),
        head: "v2".into(),
        changed: changed(changed_files),
    }
}

#[test]
fn a_call_that_moved_to_another_line_is_no_changed_edge_and_has_its_line_at_the_head() {
    let foo = "src/main/java/a/Foo.java";
    let before = [
        source("pom.xml", ""),
        source("src/main/java/a/Bar.java", CALLEE),
        source(
            foo,
            "package a;\nclass Foo {\n    void f(Bar b) {\n        b.g();\n    }\n}\n",
        ),
    ];
    let after = [
        source("pom.xml", ""),
        source("src/main/java/a/Bar.java", CALLEE),
        source(
            foo,
            "package a;\nclass Foo {\n    void f(Bar b) {\n        int x = 1;\n\n        b.g();\n    }\n}\n",
        ),
    ];

    let (graph, _) = diff_sources("repo", &before, &after, spec(&[(foo, Change::Modified)]));

    assert_eq!(
        edge_sites(&graph, "fn:a.Foo#f(Bar)", "fn:a.Bar#g()", EdgeKind::Calls),
        (None, vec![(None, vec![6])])
    );
    assert!(graph.edges.iter().all(|e| e.change.is_none()));
}

#[test]
fn a_removed_edge_has_its_lines_at_the_base_and_an_added_edge_at_the_head() {
    let foo = "src/main/java/a/Foo.java";
    let before = [
        source("pom.xml", ""),
        source("src/main/java/a/Bar.java", CALLEE),
        source(
            foo,
            "package a;\nclass Foo {\n    void f(Bar b) {\n        b.old();\n        b.g();\n        b.old();\n    }\n}\n",
        ),
    ];
    let after = [
        source("pom.xml", ""),
        source("src/main/java/a/Bar.java", CALLEE),
        source(
            foo,
            "package a;\nclass Foo {\n    void f(Bar b) {\n        b.g();\n        b.fresh();\n    }\n}\n",
        ),
    ];

    let (graph, _) = diff_sources("repo", &before, &after, spec(&[(foo, Change::Modified)]));

    let f = "fn:a.Foo#f(Bar)";
    assert_eq!(
        edge_sites(&graph, f, "fn:a.Bar#old()", EdgeKind::Calls),
        (Some(Change::Removed), vec![(None, vec![4, 6])])
    );
    assert_eq!(
        edge_sites(&graph, f, "fn:a.Bar#fresh()", EdgeKind::Calls),
        (Some(Change::Added), vec![(None, vec![5])])
    );
    assert_eq!(
        edge_sites(&graph, f, "fn:a.Bar#g()", EdgeKind::Calls),
        (None, vec![(None, vec![4])])
    );
}

#[test]
fn a_removed_edge_of_a_function_whose_file_moved_names_the_file_of_the_base() {
    let (old, new) = ("src/main/java/a/Foo.java", "extra/java/a/Foo.java");
    let before = [
        source("pom.xml", ""),
        source("src/main/java/a/Bar.java", CALLEE),
        source(
            old,
            "package a;\nclass Foo {\n    void f(Bar b) {\n        b.old();\n    }\n}\n",
        ),
    ];
    let after = [
        source("pom.xml", ""),
        source("src/main/java/a/Bar.java", CALLEE),
        source(
            new,
            "package a;\nclass Foo {\n    void f(Bar b) {\n        b.g();\n    }\n}\n",
        ),
    ];

    let (graph, _) = diff_sources(
        "repo",
        &before,
        &after,
        spec(&[(old, Change::Removed), (new, Change::Added)]),
    );

    let f = "fn:a.Foo#f(Bar)";
    assert_eq!(graph.node(f).unwrap().file.as_deref(), Some(new));
    // The ids hold no path: the nodes of a moved file are not added. Their marks follow
    // their text alone.
    assert_eq!(graph.node(f).unwrap().change, Some(Change::Modified));
    assert_eq!(graph.node("type:a.Foo").unwrap().change, None);
    assert_eq!(
        edge_sites(&graph, f, "fn:a.Bar#old()", EdgeKind::Calls),
        (
            Some(Change::Removed),
            vec![(Some(old.to_string()), vec![4])]
        )
    );
    assert_eq!(
        edge_sites(&graph, f, "fn:a.Bar#g()", EdgeKind::Calls),
        (Some(Change::Added), vec![(None, vec![4])])
    );
}

#[test]
fn a_changed_typescript_file_is_test_code_by_its_name_not_by_the_directory_src() {
    let package = "{\"name\":\"app\"}";
    let before = [source("package.json", package)];
    let after = [source("package.json", package)];
    let spec = DiffSpec {
        base: "a".into(),
        head: "b".into(),
        changed: changed(&[
            // ADR 8 alone would call this test code.
            ("src/lib/x.ts", Change::Modified),
            ("src/lib/x.test.ts", Change::Modified),
            ("src/lib/y.spec.tsx", Change::Added),
            ("src/__tests__/z.ts", Change::Added),
            ("src/main/w.ts", Change::Removed),
        ]),
    };

    let (graph, _) = diff_sources("repo", &before, &after, spec);

    let info = graph.diff.expect("diff info");
    let files: Vec<_> = info
        .files
        .iter()
        .map(|f| (f.path.as_str(), f.test))
        .collect();
    assert_eq!(
        files,
        [
            ("src/lib/x.ts", false),
            ("src/lib/x.test.ts", true),
            ("src/lib/y.spec.tsx", true),
            ("src/__tests__/z.ts", true),
            ("src/main/w.ts", false),
        ]
    );
}

/// Equal source sets (a clean working tree, ADR 0039) give the overlay of the graph with
/// itself: no marks, the names and the files of the spec, the report of the head.
#[test]
fn equal_source_sets_give_the_overlay_of_the_graph_with_itself() {
    let sub = "package a;\nclass Sub extends Base {\n    int two() { return one(); }\n}\n";
    let base = [
        source("pom.xml", ""),
        source("src/main/java/a/Base.java", BASE),
        source("src/main/java/a/Sub.java", sub),
    ];
    // Another order: the sides are sets.
    let head = [base[2].clone(), base[0].clone(), base[1].clone()];
    let spec = DiffSpec {
        base: "abc1234".into(),
        head: "working tree".into(),
        changed: changed(&[("README.md", Change::Modified)]),
    };

    let (graph, report) = diff_sources("repo", &base, &head, spec);

    let (plain, plain_report) = cg_index::index_sources("repo", &head);
    let itself = cg_core::diff(&plain, &plain);
    assert_eq!(graph.nodes, itself.nodes);
    assert_eq!(graph.edges, itself.edges);
    assert!(graph.nodes.iter().all(|n| n.change.is_none()));
    let diff = graph.diff.unwrap();
    assert_eq!(
        (diff.base.as_str(), diff.head.as_str(), diff.files.len()),
        ("abc1234", "working tree", 1)
    );
    assert_eq!(
        serde_json::to_value(&report).unwrap(),
        serde_json::to_value(&plain_report).unwrap()
    );
}

#[test]
fn indexed_sides_give_the_graph_that_diff_sources_gives() {
    let before = [
        source("pom.xml", ""),
        source("src/main/java/a/Base.java", BASE),
        source(
            "src/main/java/a/Gone.java",
            "package a;\nclass Gone extends Base {}\n",
        ),
    ];
    let after = [
        source("pom.xml", ""),
        source("src/main/java/a/Base.java", BASE),
        source(
            "src/main/java/a/Sub.java",
            "package a;\nclass Sub extends Base {}\n",
        ),
    ];
    let spec = DiffSpec {
        base: "v1".into(),
        head: "v2".into(),
        changed: changed(&[
            ("src/main/java/a/Gone.java", Change::Removed),
            ("src/main/java/a/Sub.java", Change::Added),
        ]),
    };

    let (expected, _) = diff_sources("repo", &before, &after, spec.clone());
    let (base_graph, _) = index_sources("repo", &before);
    let (head_graph, _) = index_sources("repo", &after);
    let graph = diff_indexed((&before, &base_graph), (&after, &head_graph), spec);

    assert_eq!(
        serde_json::to_value(&graph).unwrap(),
        serde_json::to_value(&expected).unwrap()
    );
    assert!(graph.nodes.iter().any(|n| n.change.is_some()));
}
