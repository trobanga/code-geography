//! The overlay of the working tree against `HEAD` (ADR 0039), on a repository made with
//! the `git` command; the viewer gives the same bytes of `.git` and texts of the inputs.

use cg_core::{Change, Graph};
use cg_git::Repository;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "user.name=T", "-c", "user.email=t@example.org"])
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(["-c", "gc.auto=0"])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git runs");
    assert!(out.status.success(), "git {args:?}");
    String::from_utf8(out.stdout).expect("utf-8")
}

fn repo(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cg-worktree-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q"]);
    dir
}

fn write(dir: &Path, path: &str, text: &str) {
    let file = dir.join(path);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, text).unwrap();
}

/// `HEAD`, the loose refs and the loose objects: the tests make no pack.
fn reader(dir: &Path) -> Repository {
    let dot = dir.join(".git");
    let mut repo = Repository::new(&fs::read_to_string(dot.join("HEAD")).unwrap());
    let main = dot.join("refs/heads/main");
    if let Ok(text) = fs::read_to_string(main) {
        repo.add_ref("refs/heads/main", &text);
    }
    for d in fs::read_dir(dot.join("objects")).unwrap() {
        let d = d.unwrap();
        let prefix = d.file_name().into_string().unwrap();
        if prefix.len() == 2 {
            for f in fs::read_dir(d.path()).unwrap() {
                let f = f.unwrap();
                let id = format!("{prefix}{}", f.file_name().into_string().unwrap());
                repo.add_loose(&id, fs::read(f.path()).unwrap()).unwrap();
            }
        }
    }
    repo
}

/// The inputs of the working tree as the walk reads them.
fn inputs(dir: &Path, paths: &[&str]) -> (Vec<String>, Vec<String>) {
    let texts = paths
        .iter()
        .filter_map(|p| fs::read_to_string(dir.join(p)).ok())
        .collect();
    let paths = paths
        .iter()
        .filter(|p| dir.join(p).exists())
        .map(|p| p.to_string())
        .collect();
    (paths, texts)
}

fn overlay(dir: &Path, paths: &[&str]) -> Graph {
    let (paths, texts) = inputs(dir, paths);
    let json = cg_wasm::LocalRepo::new(reader(dir))
        .worktree_overlay(&paths, &texts, "r")
        .unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    serde_json::from_value(value["graph"].clone()).unwrap()
}

fn changes(graph: &Graph) -> Vec<(String, Change)> {
    let diff = graph.diff.as_ref().expect("an overlay");
    diff.files
        .iter()
        .map(|f| (f.path.clone(), f.change))
        .collect()
}

const ALL: [&str; 4] = [
    "pom.xml",
    "src/main/java/p/A.java",
    "src/main/java/p/B.java",
    "src/main/java/p/C.java",
];

#[test]
fn uncommitted_changes_are_the_overlay_against_head() {
    let dir = repo("changes");
    write(
        &dir,
        "pom.xml",
        "<project><artifactId>m</artifactId></project>\n",
    );
    write(&dir, "src/main/java/p/A.java", "package p;\nclass A {}\n");
    write(&dir, "src/main/java/p/B.java", "package p;\nclass B {}\n");
    write(&dir, "notes.txt", "not an input\n");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "one"]);
    let head = git(&dir, &["rev-parse", "--short=7", "HEAD"]);
    write(
        &dir,
        "src/main/java/p/A.java",
        "package p;\nclass A { B b; }\n",
    );
    fs::remove_file(dir.join("src/main/java/p/B.java")).unwrap();
    write(&dir, "src/main/java/p/C.java", "package p;\nclass C {}\n");
    write(&dir, "notes.txt", "changed, but not an input\n");

    let graph = overlay(&dir, &ALL);
    assert_eq!(
        changes(&graph),
        [
            ("src/main/java/p/A.java".to_string(), Change::Modified),
            ("src/main/java/p/B.java".to_string(), Change::Removed),
            ("src/main/java/p/C.java".to_string(), Change::Added),
        ]
    );
    let diff = graph.diff.as_ref().unwrap();
    assert_eq!(diff.base, head.trim());
    assert_eq!(diff.head, "working tree");
    let node = |name: &str| graph.nodes.iter().find(|n| n.name == name).unwrap();
    assert_eq!(node("C").change, Some(Change::Added));
    assert_eq!(node("B").change, Some(Change::Removed));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn the_overlay_carries_the_texts_of_the_inputs_at_head() {
    let dir = repo("base");
    write(
        &dir,
        "pom.xml",
        "<project><artifactId>m</artifactId></project>\n",
    );
    write(&dir, "src/main/java/p/A.java", "package p;\nclass A {}\n");
    write(&dir, "src/main/java/p/B.java", "package p;\nclass B {}\n");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "one"]);
    write(
        &dir,
        "src/main/java/p/A.java",
        "package p;\nclass A { int x; }\n",
    );
    fs::remove_file(dir.join("src/main/java/p/B.java")).unwrap();
    write(&dir, "src/main/java/p/C.java", "package p;\nclass C {}\n");
    let (paths, texts) = inputs(&dir, &ALL);
    let json = cg_wasm::LocalRepo::new(reader(&dir))
        .worktree_overlay(&paths, &texts, "r")
        .unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(
        value["base"],
        serde_json::json!({
            "pom.xml": "<project><artifactId>m</artifactId></project>\n",
            "src/main/java/p/A.java": "package p;\nclass A {}\n",
            "src/main/java/p/B.java": "package p;\nclass B {}\n",
        })
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_clean_working_tree_is_an_overlay_with_no_change() {
    let dir = repo("clean");
    write(
        &dir,
        "pom.xml",
        "<project><artifactId>m</artifactId></project>\n",
    );
    write(&dir, "src/main/java/p/A.java", "package p;\nclass A {}\n");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "one"]);
    let graph = overlay(&dir, &ALL);
    assert!(changes(&graph).is_empty());
    assert!(graph.nodes.iter().all(|n| n.change.is_none()));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_byte_order_mark_that_the_browser_drops_is_no_change() {
    let dir = repo("bom");
    write(
        &dir,
        "pom.xml",
        "<project><artifactId>m</artifactId></project>\n",
    );
    write(
        &dir,
        "src/main/java/p/A.java",
        "\u{feff}package p;\nclass A {}\n",
    );
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "one"]);
    let (paths, _) = inputs(&dir, &ALL);
    // `File.text()` decodes without the mark.
    let texts = vec![
        fs::read_to_string(dir.join("pom.xml")).unwrap(),
        "package p;\nclass A {}\n".to_string(),
    ];
    let json = cg_wasm::LocalRepo::new(reader(&dir))
        .worktree_overlay(&paths, &texts, "r")
        .unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let graph: Graph = serde_json::from_value(value["graph"].clone()).unwrap();
    assert!(changes(&graph).is_empty());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_tracked_file_that_the_walk_does_not_read_is_not_removed() {
    let dir = repo("excluded");
    write(
        &dir,
        "pom.xml",
        "<project><artifactId>m</artifactId></project>\n",
    );
    write(&dir, "src/main/java/p/A.java", "package p;\nclass A {}\n");
    write(
        &dir,
        "target/generated/p/G.java",
        "package p;\nclass G {}\n",
    );
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "one"]);
    // The walk does not list `target`, so it never gives `G.java`.
    let graph = overlay(&dir, &ALL);
    assert!(changes(&graph).is_empty(), "{:?}", changes(&graph));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_repository_with_no_commit_is_an_error() {
    let dir = repo("unborn");
    write(&dir, "src/main/java/p/A.java", "package p;\nclass A {}\n");
    let (paths, texts) = inputs(&dir, &ALL);
    let err = cg_wasm::LocalRepo::new(reader(&dir))
        .worktree_overlay(&paths, &texts, "r")
        .unwrap_err();
    assert!(err.contains("no commit"), "{err}");
    let _ = fs::remove_dir_all(&dir);
}
