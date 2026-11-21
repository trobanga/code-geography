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

fn worktree(paths: &[String], texts: &[String]) -> cg_wasm::Worktree {
    cg_wasm::Worktree::index(paths, texts, "r").unwrap()
}

fn overlay_json(dir: &Path, paths: &[&str]) -> serde_json::Value {
    let (paths, texts) = inputs(dir, paths);
    let json = cg_wasm::LocalRepo::new(reader(dir))
        .worktree_overlay(&worktree(&paths, &texts))
        .unwrap();
    serde_json::from_str(&json).unwrap()
}

fn overlay(dir: &Path, paths: &[&str]) -> Graph {
    serde_json::from_value(overlay_json(dir, paths)["graph"].clone()).unwrap()
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
fn the_index_of_the_working_tree_is_the_index_of_its_sources() {
    let paths = vec!["pom.xml".to_string(), "src/main/java/p/A.java".to_string()];
    let texts = vec![
        "<project><artifactId>m</artifactId></project>\n".to_string(),
        "package p;\nclass A {}\n".to_string(),
    ];
    let worktree = cg_wasm::Worktree::index(&paths, &texts, "r").unwrap();
    assert_eq!(
        worktree.json(),
        cg_wasm::index_sources(&paths, &texts, "r").unwrap()
    );
    let err = cg_wasm::Worktree::index(&paths, &texts[..1], "r").err();
    assert!(err.is_some_and(|e| e.contains("paths")));
}

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
    let value = overlay_json(&dir, &ALL);
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
    let value = overlay_json(&dir, &ALL);
    let graph: Graph = serde_json::from_value(value["graph"].clone()).unwrap();
    assert!(changes(&graph).is_empty());
    assert!(graph.nodes.iter().all(|n| n.change.is_none()));
    // The base is the index of the working tree: nothing ran again.
    assert_eq!(value["indexes"], 0);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_changed_working_tree_indexes_head_once_and_then_from_the_cache() {
    let dir = repo("runs");
    write(
        &dir,
        "pom.xml",
        "<project><artifactId>m</artifactId></project>\n",
    );
    write(&dir, "src/main/java/p/A.java", "package p;\nclass A {}\n");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "one"]);
    write(
        &dir,
        "src/main/java/p/A.java",
        "package p;\nclass A { int x; }\n",
    );
    let (paths, mut texts) = inputs(&dir, &ALL);
    let mut local = cg_wasm::LocalRepo::new(reader(&dir));
    let first = local.worktree_overlay(&worktree(&paths, &texts)).unwrap();
    let first: serde_json::Value = serde_json::from_str(&first).unwrap();
    assert_eq!(first["indexes"], 1);
    let graph: Graph = serde_json::from_value(first["graph"].clone()).unwrap();
    assert_eq!(
        changes(&graph),
        [("src/main/java/p/A.java".to_string(), Change::Modified)]
    );
    // HEAD is cached now: a second edit indexes nothing but the working tree.
    texts[1] = "package p;\nclass A { int y; }\n".to_string();
    let second = local.worktree_overlay(&worktree(&paths, &texts)).unwrap();
    let second: serde_json::Value = serde_json::from_str(&second).unwrap();
    assert_eq!(second["indexes"], 0);
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
        .worktree_overlay(&worktree(&paths, &texts))
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
        .worktree_overlay(&worktree(&paths, &texts))
        .unwrap_err();
    assert!(err.contains("no commit"), "{err}");
    let _ = fs::remove_dir_all(&dir);
}
