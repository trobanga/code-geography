//! The commit list and the overlay of commits of a local repository (ADR 0039), on a
//! repository made with the `git` command; the viewer gives the same bytes of `.git`.

use cg_core::{Change, Graph};
use cg_git::Repository;
use cg_wasm::LocalRepo;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn run(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> String {
    let out = Command::new("git")
        .envs(env.iter().copied())
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
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf-8")
}

struct Repo {
    dir: PathBuf,
}

impl Repo {
    fn new(name: &str) -> Repo {
        let dir = std::env::temp_dir().join(format!("cg-commits-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        run(&dir, &["init", "-q"], &[]);
        Repo { dir }
    }

    fn write(&self, path: &str, text: &str) -> &Repo {
        let file = self.dir.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
        self
    }

    fn remove(&self, path: &str) -> &Repo {
        fs::remove_file(self.dir.join(path)).unwrap();
        self
    }

    fn git(&self, args: &[&str]) -> String {
        run(&self.dir, args, &[])
    }

    /// Commits everything; each commit gets its own author and a date one day later.
    fn commit(&self, message: &str) -> String {
        let n = self
            .git(&["rev-list", "--count", "--all"])
            .trim()
            .to_string();
        let day = 10 + n.parse::<u32>().unwrap();
        let date = format!("2026-11-{day:02}T10:00:00+0130");
        let author = format!("Author {n}");
        let env = [
            ("GIT_AUTHOR_NAME", author.as_str()),
            ("GIT_AUTHOR_DATE", date.as_str()),
            ("GIT_COMMITTER_DATE", date.as_str()),
        ];
        run(&self.dir, &["add", "-A"], &[]);
        run(&self.dir, &["commit", "-q", "-m", message], &env);
        self.git(&["rev-parse", "HEAD"]).trim().to_string()
    }

    /// A reader of the loose objects and refs of the repository.
    fn local(&self) -> LocalRepo {
        let dot = self.dir.join(".git");
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
        LocalRepo::new(repo)
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

const POM: &str = "<project><artifactId>m</artifactId></project>\n";

/// Three commits: a root with `A`; `A` gets a field and `B` appears; `B` goes, `C` comes.
/// Returns the repository and the ids of the commits, oldest first.
fn history(name: &str) -> (Repo, [String; 3]) {
    let r = Repo::new(name);
    r.write("pom.xml", POM)
        .write("src/main/java/p/A.java", "package p;\nclass A {}\n")
        .write("notes.txt", "not an input\n");
    let one = r.commit("one\n\nthe first commit\nwith a body");
    r.write("src/main/java/p/A.java", "package p;\nclass A { B b; }\n")
        .write("src/main/java/p/B.java", "package p;\nclass B {}\n");
    let two = r.commit("two");
    r.remove("src/main/java/p/B.java")
        .write("src/main/java/p/C.java", "package p;\nclass C {}\n")
        .write("notes.txt", "changed, still not an input\n");
    let three = r.commit("three");
    (r, [one, two, three])
}

fn json(text: Result<String, String>) -> Value {
    serde_json::from_str(&text.unwrap()).unwrap()
}

#[test]
fn the_log_lists_commits_from_head_in_pages() {
    let (r, [one, two, three]) = history("log");
    let local = r.local();
    let page = json(local.log(None, 2));
    let page = page.as_array().unwrap();
    assert_eq!(page.len(), 2);
    for (entry, sha) in page.iter().zip([&three, &two]) {
        assert_eq!(entry["sha"], sha.as_str());
        let show = |format: &str| r.git(&["log", "-1", &format!("--format={format}"), sha]);
        assert_eq!(entry["title"], show("%s").trim());
        assert_eq!(entry["author"], show("%an").trim());
        assert_eq!(entry["date"], show("%aI").trim());
        assert_eq!(
            entry["message"].as_str().unwrap().trim_end(),
            show("%B").trim_end()
        );
    }
    assert_eq!(page[0]["parents"], serde_json::json!([two]));
    assert_eq!(page[0]["date"], "2026-11-12T10:00:00+01:30");

    // The next page starts at the parent of the last commit listed.
    let next = page[1]["parents"][0].as_str().unwrap();
    let rest = json(local.log(Some(next), 2));
    let rest = rest.as_array().unwrap();
    assert_eq!(rest.len(), 1);
    assert_eq!(rest[0]["sha"], one.as_str());
    assert_eq!(rest[0]["parents"], serde_json::json!([]));
    assert_eq!(rest[0]["title"], "one");
    assert_eq!(
        rest[0]["message"].as_str().unwrap().trim_end(),
        "one\n\nthe first commit\nwith a body"
    );
}

fn graph_of(value: &Value) -> Graph {
    serde_json::from_value(value["graph"].clone()).unwrap()
}

fn files_of(graph: &Graph) -> Vec<(String, Change)> {
    graph
        .diff
        .as_ref()
        .expect("an overlay")
        .files
        .iter()
        .map(|f| (f.path.clone(), f.change))
        .collect()
}

fn node<'a>(graph: &'a Graph, name: &str) -> &'a cg_core::Node {
    graph.nodes.iter().find(|n| n.name == name).unwrap()
}

#[test]
fn a_commit_against_its_parent_is_an_overlay_with_marks_texts_and_names() {
    let (r, [_, two, three]) = history("overlay");
    let mut local = r.local();
    let value = json(local.commit_overlay(Some(&two), &three, "r"));
    let graph = graph_of(&value);
    assert_eq!(
        files_of(&graph),
        [
            ("notes.txt".to_string(), Change::Modified),
            ("src/main/java/p/B.java".to_string(), Change::Removed),
            ("src/main/java/p/C.java".to_string(), Change::Added),
        ]
    );
    assert_eq!(node(&graph, "C").change, Some(Change::Added));
    assert_eq!(node(&graph, "B").change, Some(Change::Removed));
    assert_eq!(node(&graph, "A").change, None);
    let diff = graph.diff.as_ref().unwrap();
    assert_eq!(diff.base, &two[..7]);
    assert_eq!(diff.head, &three[..7]);
    assert_eq!(
        value["base"],
        serde_json::json!({
            "pom.xml": POM,
            "src/main/java/p/A.java": "package p;\nclass A { B b; }\n",
            "src/main/java/p/B.java": "package p;\nclass B {}\n",
        })
    );
    assert_eq!(
        value["head"],
        serde_json::json!({
            "pom.xml": POM,
            "src/main/java/p/A.java": "package p;\nclass A { B b; }\n",
            "src/main/java/p/C.java": "package p;\nclass C {}\n",
        })
    );
    // The report is the one of the head.
    assert!(value["report"]["calls"].is_object());
    // An abbreviated id and a ref name the same commits.
    let short = json(local.commit_overlay(Some(&two[..9]), "HEAD", "r"));
    assert_eq!(short["graph"], value["graph"]);
}

/// The inputs of the working tree of `history`, as the walk reads them.
fn worktree_inputs(r: &Repo) -> (Vec<String>, Vec<String>) {
    let paths: Vec<String> = [
        "pom.xml",
        "src/main/java/p/A.java",
        "src/main/java/p/C.java",
    ]
    .iter()
    .map(|p| p.to_string())
    .collect();
    let texts = paths
        .iter()
        .map(|p| fs::read_to_string(r.dir.join(p)).unwrap())
        .collect();
    (paths, texts)
}

fn indexes(value: &Value) -> u64 {
    value["indexes"].as_u64().expect("a count of indexes")
}

#[test]
fn stepping_down_the_history_indexes_one_tree_per_step() {
    let (r, [one, two, three]) = history("indexes");
    let mut local = r.local();
    // Both sides are new.
    assert_eq!(
        indexes(&json(local.commit_overlay(Some(&two), &three, "r"))),
        2
    );
    // The head is the base of the step above: one tree is new.
    assert_eq!(
        indexes(&json(local.commit_overlay(Some(&one), &two, "r"))),
        1
    );
    // The same overlay again: both trees are kept.
    assert_eq!(
        indexes(&json(local.commit_overlay(Some(&one), &two, "r"))),
        0
    );
    // The other step was dropped from the cache: it costs one tree again.
    assert_eq!(
        indexes(&json(local.commit_overlay(Some(&two), &three, "r"))),
        1
    );
}

#[test]
fn the_working_tree_and_a_commit_share_the_tree_of_head() {
    let (r, [_, two, three]) = history("shared");
    let mut local = r.local();
    // A clean working tree is indexed once, and its tree is then kept.
    let (paths, texts) = worktree_inputs(&r);
    let clean = json(local.worktree_overlay(&paths, &texts, "r"));
    assert_eq!(indexes(&clean), 1);
    assert_eq!(
        indexes(&json(local.commit_overlay(Some(&two), &three, "r"))),
        1
    );
    // A changed working tree: the head is new, the tree of `HEAD` is kept.
    r.write("src/main/java/p/C.java", "package p;\nclass C { int x; }\n");
    let (paths, texts) = worktree_inputs(&r);
    let dirty = json(local.worktree_overlay(&paths, &texts, "r"));
    assert_eq!(indexes(&dirty), 1);
    let (paths, texts) = worktree_inputs(&r);
    assert_eq!(
        indexes(&json(local.worktree_overlay(&paths, &texts, "r"))),
        1
    );
}

#[test]
fn resolve_gives_the_full_id_of_a_commit_and_refuses_other_objects() {
    let (r, [one, _, three]) = history("resolve");
    let local = r.local();
    assert_eq!(local.resolve("HEAD").unwrap(), three);
    assert_eq!(local.resolve("refs/heads/main").unwrap(), three);
    assert_eq!(local.resolve(&one[..7]).unwrap(), one);
    assert_eq!(local.resolve(&one.to_uppercase()).unwrap(), one);
    // A blob and a tree are no commits.
    for object in ["HEAD:pom.xml", "HEAD^{tree}"] {
        let id = r.git(&["rev-parse", object]).trim().to_string();
        let err = local.resolve(&id).unwrap_err();
        assert!(err.contains("no commit"), "{err}");
        assert!(local.resolve(&id[..8]).is_err());
    }
    let err = local.resolve("v1.0").unwrap_err();
    assert!(err.contains("no ref v1.0"), "{err}");
}

#[test]
fn two_commits_with_the_same_tree_are_indexed_once() {
    let (r, [_, _, three]) = history("sametree");
    r.git(&["commit", "-q", "--allow-empty", "-m", "empty"]);
    let mut local = r.local();
    let value = json(local.commit_overlay(Some(&three), "HEAD", "r"));
    assert_eq!(indexes(&value), 1);
    assert!(files_of(&graph_of(&value)).is_empty());
}

#[test]
fn a_root_commit_is_compared_with_nothing_and_all_is_added() {
    let (r, [one, _, _]) = history("root");
    let mut local = r.local();
    let value = json(local.commit_overlay(None, &one, "r"));
    let graph = graph_of(&value);
    assert_eq!(
        files_of(&graph),
        [
            ("notes.txt".to_string(), Change::Added),
            ("pom.xml".to_string(), Change::Added),
            ("src/main/java/p/A.java".to_string(), Change::Added),
        ]
    );
    assert_eq!(node(&graph, "A").change, Some(Change::Added));
    let diff = graph.diff.as_ref().unwrap();
    assert_eq!(diff.base, "none");
    assert_eq!(diff.head, &one[..7]);
    assert_eq!(value["base"], serde_json::json!({}));
    assert_eq!(
        value["head"],
        serde_json::json!({
            "pom.xml": POM,
            "src/main/java/p/A.java": "package p;\nclass A {}\n",
        })
    );
}
