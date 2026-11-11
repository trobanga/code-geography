use cg_core::{Change, EdgeKind};
use cg_index::{DiffSpec, SourceFile, diff_sources};

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
            // Not Clojure: the rule of ADR 8 stays.
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
            ("src/test/java/a/FooTest.java", Some("mod:"), true),
            ("src/main/java/a/Foo.java", Some("mod:"), false),
            ("README.md", Some("mod:"), false),
        ]
    );
}
