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
    assert_eq!(
        graph,
        cg_index_java::index(&dir.path().join("repo")).unwrap()
    );
    assert!(graph.has_edge("type:a.Sub", "type:a.Base", EdgeKind::Extends));
}
