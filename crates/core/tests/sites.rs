use cg_core::{Edge, EdgeKind, EdgeSet, FileSites, Graph, Node, NodeKind};

fn edge(from: &str, to: &str, kind: EdgeKind, sites: Vec<FileSites>) -> Edge {
    Edge {
        from: from.into(),
        to: to.into(),
        kind,
        change: None,
        sites,
    }
}

#[test]
fn an_edge_without_sites_has_no_sites_field_and_one_with_sites_round_trips() {
    let plain = edge("A", "B", EdgeKind::Calls, Vec::new());
    assert_eq!(
        serde_json::to_string(&plain).unwrap(),
        r#"{"from":"A","to":"B","kind":"calls"}"#
    );
    let parsed: Graph =
        serde_json::from_str(r#"{"nodes":[],"edges":[{"from":"A","to":"B","kind":"calls"}]}"#)
            .unwrap();
    assert_eq!(parsed.edges, vec![plain]);

    let without_file = edge(
        "A",
        "B",
        EdgeKind::Calls,
        vec![FileSites {
            file: None,
            lines: vec![3, 9],
        }],
    );
    assert!(
        serde_json::to_string(&without_file)
            .unwrap()
            .contains(r#""sites":[{"lines":[3,9]}]"#)
    );

    let with_file = edge(
        "A",
        "B",
        EdgeKind::Calls,
        vec![FileSites {
            file: Some("x/Y.java".into()),
            lines: vec![4],
        }],
    );
    let json = serde_json::to_string(&with_file).unwrap();
    assert!(json.contains(r#""sites":[{"file":"x/Y.java","lines":[4]}]"#));

    let graph = Graph {
        nodes: Vec::new(),
        edges: vec![without_file, with_file],
        diff: None,
    };
    let again: Graph = serde_json::from_str(&serde_json::to_string(&graph).unwrap()).unwrap();
    assert_eq!(again, graph);
}

fn node(id: &str, file: Option<&str>) -> Node {
    Node {
        id: id.into(),
        parent: None,
        kind: NodeKind::Function,
        name: id.into(),
        file: file.map(Into::into),
        span: None,
        test: false,
        hash: None,
        change: None,
    }
}

fn sites(file: Option<&str>, lines: &[u32]) -> FileSites {
    FileSites {
        file: file.map(Into::into),
        lines: lines.to_vec(),
    }
}

#[test]
fn the_file_of_a_site_is_left_out_when_it_is_the_file_of_the_from_node() {
    let nodes = [
        node("A", Some("a.rs")),
        node("C", Some("c.rs")),
        node("D", None),
    ];
    let mut set = EdgeSet::new();
    set.add("A", "B", EdgeKind::Calls, "a.rs", 1);
    set.add("C", "B", EdgeKind::Calls, "other.rs", 2);
    set.add("D", "B", EdgeKind::Calls, "d.rs", 3);
    set.add("E", "B", EdgeKind::Calls, "e.rs", 4);
    // Two files: the entry without file first, the others by path.
    set.add("A", "T", EdgeKind::Calls, "z.rs", 7);
    set.add("A", "T", EdgeKind::Calls, "b.rs", 8);
    set.add("A", "T", EdgeKind::Calls, "a.rs", 9);

    let edges = set.into_edges(&nodes);

    let sites_of = |from: &str, to: &str| {
        edges
            .iter()
            .find(|e| e.from == from && e.to == to)
            .unwrap()
            .sites
            .clone()
    };
    assert_eq!(sites_of("A", "B"), vec![sites(None, &[1])]);
    assert_eq!(sites_of("C", "B"), vec![sites(Some("other.rs"), &[2])]);
    assert_eq!(sites_of("D", "B"), vec![sites(Some("d.rs"), &[3])]);
    assert_eq!(sites_of("E", "B"), vec![sites(Some("e.rs"), &[4])]);
    assert_eq!(
        sites_of("A", "T"),
        vec![
            sites(None, &[9]),
            sites(Some("b.rs"), &[8]),
            sites(Some("z.rs"), &[7])
        ]
    );
}

#[test]
fn the_edges_come_ordered_by_from_to_kind_without_marks() {
    let mut set = EdgeSet::new();
    set.add("B", "A", EdgeKind::Calls, "f", 1);
    set.add("A", "C", EdgeKind::TypeRef, "f", 1);
    set.add("A", "C", EdgeKind::Extends, "f", 1);
    set.add("A", "B", EdgeKind::ValueRef, "f", 1);

    let edges = set.into_edges(&[]);

    let keys: Vec<_> = edges
        .iter()
        .map(|e| (e.from.as_str(), e.to.as_str(), e.kind))
        .collect();
    assert_eq!(
        keys,
        vec![
            ("A", "B", EdgeKind::ValueRef),
            ("A", "C", EdgeKind::Extends),
            ("A", "C", EdgeKind::TypeRef),
            ("B", "A", EdgeKind::Calls),
        ]
    );
    assert!(edges.iter().all(|e| e.change.is_none()));
}

#[test]
fn inserting_an_edge_joins_its_sites_with_those_of_the_same_edge() {
    let mut set = EdgeSet::new();
    set.add("A", "B", EdgeKind::Calls, "a.rs", 5);
    set.insert(edge(
        "A",
        "B",
        EdgeKind::Calls,
        vec![
            sites(Some("a.rs"), &[2, 5]),
            sites(None, &[7]),
            sites(Some("b.rs"), &[1]),
        ],
    ));
    set.insert(edge("A", "B", EdgeKind::Calls, vec![sites(None, &[3])]));
    set.insert(edge("A", "C", EdgeKind::Calls, Vec::new()));

    let edges = set.into_edges(&[node("A", Some("a.rs"))]);

    assert_eq!(
        edges,
        vec![
            edge(
                "A",
                "B",
                EdgeKind::Calls,
                vec![sites(None, &[2, 3, 5, 7]), sites(Some("b.rs"), &[1])]
            ),
            edge("A", "C", EdgeKind::Calls, Vec::new()),
        ]
    );
}

#[test]
fn collecting_and_extending_join_the_sites_of_the_same_edge() {
    let first = edge("A", "B", EdgeKind::Calls, vec![sites(None, &[1])]);
    let second = edge(
        "A",
        "B",
        EdgeKind::Calls,
        vec![sites(None, &[4]), sites(Some("z.rs"), &[2])],
    );
    let third = edge("A", "B", EdgeKind::Calls, vec![sites(Some("z.rs"), &[9])]);
    let joined = vec![edge(
        "A",
        "B",
        EdgeKind::Calls,
        vec![sites(None, &[1, 4]), sites(Some("z.rs"), &[2, 9])],
    )];

    let collected: EdgeSet = [first.clone(), second.clone(), third.clone()]
        .into_iter()
        .collect();
    let mut extended = EdgeSet::new();
    extended.insert(first);
    extended.extend([second, third]);

    assert_eq!(collected.into_edges(&[]), joined);
    assert_eq!(extended.into_edges(&[]), joined);
}

#[test]
fn retain_drops_the_edges_that_the_predicate_rejects() {
    let mut set = EdgeSet::new();
    set.add("A", "B", EdgeKind::Calls, "f", 1);
    set.add("A", "B", EdgeKind::TypeRef, "f", 2);
    set.add("A", "C", EdgeKind::Calls, "f", 3);
    set.add("C", "A", EdgeKind::Calls, "f", 4);

    set.retain(|from, to| from == "A" && to == "C" || from == "C");

    let keys: Vec<_> = set
        .into_edges(&[])
        .iter()
        .map(|e| (e.from.clone(), e.to.clone()))
        .collect();
    assert_eq!(
        keys,
        vec![
            ("A".to_string(), "C".to_string()),
            ("C".to_string(), "A".to_string())
        ]
    );
}

#[test]
fn adding_the_same_edge_twice_joins_its_lines_ascending_and_once() {
    let mut set = EdgeSet::new();
    set.add("A", "B", EdgeKind::Calls, "a.rs", 9);
    set.add("A", "B", EdgeKind::Calls, "a.rs", 3);
    set.add("A", "B", EdgeKind::Calls, "a.rs", 9);

    assert_eq!(
        set.into_edges(&[]),
        vec![edge(
            "A",
            "B",
            EdgeKind::Calls,
            vec![FileSites {
                file: Some("a.rs".into()),
                lines: vec![3, 9],
            }]
        )]
    );
}
