use cg_core::{Change, DiffInfo, FileChange, Graph, Node, NodeKind, file_changes};

fn module(id: &str) -> Node {
    Node {
        id: id.into(),
        parent: None,
        kind: NodeKind::Module,
        name: id.into(),
        file: None,
        span: None,
        test: false,
        hash: None,
        change: None,
    }
}

fn graph(modules: &[&str]) -> Graph {
    Graph {
        nodes: modules.iter().map(|id| module(id)).collect(),
        ..Graph::default()
    }
}

fn changed(paths: &[(&str, Change)]) -> Vec<(String, Change)> {
    paths.iter().map(|(p, c)| (p.to_string(), *c)).collect()
}

#[test]
fn a_file_belongs_to_the_module_with_the_longest_directory_prefix() {
    let graph = graph(&["mod:", "mod:svc", "mod:svc/sub", "mod:svcx"]);
    let files = file_changes(
        &graph,
        &changed(&[
            ("pom.xml", Change::Modified),
            ("README.md", Change::Added),
            ("svc/pom.xml", Change::Modified),
            ("svc/src/main/A.java", Change::Removed),
            ("svc/sub/src/main/B.java", Change::Added),
            ("svcy/pom.xml", Change::Modified),
        ]),
    );

    let modules: Vec<_> = files
        .iter()
        .map(|f| (f.path.as_str(), f.module.as_deref()))
        .collect();
    assert_eq!(
        modules,
        vec![
            ("pom.xml", Some("mod:")),
            ("README.md", Some("mod:")),
            ("svc/pom.xml", Some("mod:svc")),
            ("svc/src/main/A.java", Some("mod:svc")),
            ("svc/sub/src/main/B.java", Some("mod:svc/sub")),
            ("svcy/pom.xml", Some("mod:")),
        ]
    );
    assert_eq!(files[3].change, Change::Removed);
}

#[test]
fn a_file_outside_each_module_has_none_and_the_test_flag_follows_the_source_set_rule() {
    let graph = graph(&["mod:svc"]);
    let files = file_changes(
        &graph,
        &changed(&[
            ("docs/README.md", Change::Added),
            ("svc/src/test/java/T.java", Change::Added),
            ("svc/src/main/java/M.java", Change::Added),
            ("svcx/src/it/X.java", Change::Added),
        ]),
    );

    assert_eq!(
        files,
        vec![
            FileChange {
                path: "docs/README.md".into(),
                change: Change::Added,
                module: None,
                test: false
            },
            FileChange {
                path: "svc/src/test/java/T.java".into(),
                change: Change::Added,
                module: Some("mod:svc".into()),
                test: true
            },
            FileChange {
                path: "svc/src/main/java/M.java".into(),
                change: Change::Added,
                module: Some("mod:svc".into()),
                test: false
            },
            FileChange {
                path: "svcx/src/it/X.java".into(),
                change: Change::Added,
                module: None,
                test: true
            },
        ]
    );
}

#[test]
fn files_are_in_the_diff_json_only_when_there_are_some() {
    let bare = DiffInfo {
        base: "a".into(),
        head: "b".into(),
        files: vec![],
    };
    let json = serde_json::to_string(&bare).unwrap();
    assert_eq!(json, r#"{"base":"a","head":"b"}"#);
    assert_eq!(serde_json::from_str::<DiffInfo>(&json).unwrap(), bare);

    let with = DiffInfo {
        files: vec![FileChange {
            path: "pom.xml".into(),
            change: Change::Modified,
            module: None,
            test: false,
        }],
        ..bare
    };
    let json = serde_json::to_string(&with).unwrap();
    assert!(
        json.contains(
            r#""files":[{"path":"pom.xml","change":"modified","module":null,"test":false}]"#
        ),
        "{json}"
    );
    assert_eq!(serde_json::from_str::<DiffInfo>(&json).unwrap(), with);
}

#[test]
fn the_filters_keep_the_diff_info_with_its_files() {
    let info = DiffInfo {
        base: "a".into(),
        head: "b".into(),
        files: file_changes(
            &graph(&["mod:"]),
            &changed(&[("pom.xml", Change::Modified)]),
        ),
    };
    let graph = Graph {
        diff: Some(info.clone()),
        ..graph(&["mod:"])
    };

    assert_eq!(graph.without_tests().diff, Some(info.clone()));
    assert_eq!(graph.without_redundant_imports().diff, Some(info));
}
