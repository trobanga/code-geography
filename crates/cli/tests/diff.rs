use cg_core::{Change, EdgeKind, Graph};
use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::TempDir;

fn git(repo: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.org"])
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write(repo: &Path, path: &str, content: &str) {
    let full = repo.join(path);
    fs::create_dir_all(full.parent().unwrap()).unwrap();
    fs::write(full, content).unwrap();
}

fn commit(repo: &Path, message: &str) {
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-q", "-m", message]);
}

const BASE: &str =
    "package a;\nclass Base {\n    int one() { return 1; }\n    int two() { return 2; }\n}\n";

/// Two commits. The second changes `Base.one`, adds `Sub extends Base`, deletes
/// `Gone`, which extended `Base`.
fn two_commits() -> TempDir {
    let dir = TempDir::new().unwrap();
    let repo = dir.path();
    git(repo, &["init", "-q"]);
    write(repo, "a/Base.java", BASE);
    write(
        repo,
        "a/Gone.java",
        "package a;\nclass Gone extends Base {}\n",
    );
    write(repo, "a/Keep.java", "package a;\nclass Keep {}\n");
    commit(repo, "first");
    write(
        repo,
        "a/Base.java",
        &BASE.replace("return 1;", "return 11;"),
    );
    write(
        repo,
        "a/Sub.java",
        "package a;\nclass Sub extends Base {}\n",
    );
    fs::remove_file(repo.join("a/Gone.java")).unwrap();
    commit(repo, "second");
    dir
}

fn cg_diff(repo: &Path, args: &[&str], output: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cg"))
        .arg("diff")
        .arg(repo)
        .args(args)
        .arg("-o")
        .arg(output)
        .output()
        .unwrap()
}

fn read(output: &Path) -> Graph {
    serde_json::from_str(&fs::read_to_string(output).unwrap()).unwrap()
}

fn node_changes(graph: &Graph) -> Vec<(&str, Change)> {
    graph
        .nodes
        .iter()
        .filter_map(|n| n.change.map(|c| (n.id.as_str(), c)))
        .collect()
}

#[test]
fn diff_of_one_revision_marks_nodes_and_edges_against_its_parent() {
    let dir = two_commits();
    let output = dir.path().join("out/diff.json");
    fs::create_dir_all(output.parent().unwrap()).unwrap();

    let run = cg_diff(dir.path(), &["HEAD"], &output);

    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let graph = read(&output);
    assert_eq!(
        node_changes(&graph),
        // Order of the diff graph: the nodes of `after`, then the removed ones.
        [
            ("fn:a.Base#one()", Change::Modified),
            ("type:a.Sub", Change::Added),
            ("type:a.Gone", Change::Removed),
        ]
    );
    let marked: Vec<_> = graph
        .edges
        .iter()
        .filter_map(|e| {
            e.change
                .map(|c| (e.from.as_str(), e.to.as_str(), e.kind, c))
        })
        .collect();
    // The indexer gives a supertype also a `type_ref` edge.
    let (gone, sub, base) = ("type:a.Gone", "type:a.Sub", "type:a.Base");
    assert_eq!(
        marked,
        [
            (gone, base, EdgeKind::Extends, Change::Removed),
            (gone, base, EdgeKind::TypeRef, Change::Removed),
            (sub, base, EdgeKind::Extends, Change::Added),
            (sub, base, EdgeKind::TypeRef, Change::Added),
        ]
    );
    let info = graph.diff.expect("diff info");
    assert_eq!((info.base.as_str(), info.head.as_str()), ("HEAD^", "HEAD"));
}

#[test]
fn diff_of_a_root_commit_compares_with_the_empty_graph() {
    let dir = two_commits();
    let output = dir.path().join("diff.json");

    // `HEAD~1` is the first commit and has no parent.
    let run = cg_diff(dir.path(), &["HEAD~1"], &output);

    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let graph = read(&output);
    assert!(!graph.nodes.is_empty());
    assert!(graph.nodes.iter().all(|n| n.change == Some(Change::Added)));
    assert!(graph.edges.iter().all(|e| e.change == Some(Change::Added)));
    let info = graph.diff.unwrap();
    assert_eq!(
        (info.base.as_str(), info.head.as_str()),
        ("HEAD~1^", "HEAD~1")
    );
}

#[test]
fn an_unknown_revision_fails_with_the_message_of_git_and_writes_no_file() {
    let dir = two_commits();
    let output = dir.path().join("diff.json");

    let run = cg_diff(dir.path(), &["no-such-revision"], &output);

    // Exit code 1 from `main`, not 101 from a panic.
    assert_eq!(run.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(stderr.contains("no-such-revision"), "{stderr}");
    assert!(!stderr.contains("panicked"), "{stderr}");
    assert!(!output.exists());
}

#[test]
fn diff_of_a_range_compares_its_two_ends_and_keeps_the_spelling_in_the_diff_info() {
    let dir = two_commits();
    write(
        dir.path(),
        "a/Keep.java",
        "package a;\nclass Keep { int field; }\n",
    );
    commit(dir.path(), "third");
    let output = dir.path().join("diff.json");

    let run = cg_diff(dir.path(), &["HEAD~2..HEAD"], &output);

    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let graph = read(&output);
    assert_eq!(
        node_changes(&graph),
        [
            ("fn:a.Base#one()", Change::Modified),
            ("type:a.Keep", Change::Modified),
            ("type:a.Sub", Change::Added),
            ("type:a.Gone", Change::Removed),
        ]
    );
    let info = graph.diff.unwrap();
    assert_eq!((info.base.as_str(), info.head.as_str()), ("HEAD~2", "HEAD"));
}

#[test]
fn a_revision_that_looks_like_an_option_is_refused() {
    let dir = two_commits();
    let output = dir.path().join("diff.json");

    let run = cg_diff(dir.path(), &["--list"], &output);

    assert_eq!(run.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(stderr.contains("not a revision"), "{stderr}");
    assert!(!output.exists());
}
