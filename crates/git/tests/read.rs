//! Repositories made with the `git` command in a temporary directory; the reader gets the
//! bytes of their `.git` as the viewer gives them, and its answers are compared with `git`.

use cg_git::{Change, Id, Repository};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn git(dir: &Path, args: &[&str]) -> String {
    git_env(dir, args, &[])
}

fn git_env(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> String {
    let out = Command::new("git")
        .envs(env.iter().copied())
        .args(["-c", "user.name=T", "-c", "user.email=t@example.org"])
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(["-c", "gc.auto=0", "-c", "core.autocrlf=false"])
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
        let dir = std::env::temp_dir().join(format!("cg-git-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q"]);
        Repo { dir }
    }

    fn write(&self, path: &str, text: &str) -> &Repo {
        let file = self.dir.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
        self
    }

    fn commit(&self, message: &str) -> String {
        git(&self.dir, &["add", "-A"]);
        git(&self.dir, &["commit", "-q", "-m", message]);
        self.git(&["rev-parse", "HEAD"]).trim().to_string()
    }

    /// A commit by `author` with the author and committer date `date`.
    fn commit_by(&self, message: &str, author: &str, date: &str) -> String {
        git(&self.dir, &["add", "-A"]);
        let env = [
            ("GIT_AUTHOR_NAME", author),
            ("GIT_AUTHOR_DATE", date),
            ("GIT_COMMITTER_DATE", date),
        ];
        git_env(&self.dir, &["commit", "-q", "-m", message], &env);
        self.git(&["rev-parse", "HEAD"]).trim().to_string()
    }

    fn git(&self, args: &[&str]) -> String {
        git(&self.dir, args)
    }

    /// The reader with the files of `.git` that the viewer reads: `HEAD`, everything below
    /// `refs`, `packed-refs`, each pack with its index, each loose object.
    fn reader(&self) -> Repository {
        let dot = self.dir.join(".git");
        let mut repo = Repository::new(&fs::read_to_string(dot.join("HEAD")).unwrap());
        for (name, path) in files_below(&dot, "refs") {
            repo.add_ref(&name, &fs::read_to_string(path).unwrap());
        }
        if let Ok(text) = fs::read_to_string(dot.join("packed-refs")) {
            repo.set_packed_refs(&text);
        }
        let objects = dot.join("objects");
        add_packs(&mut repo, &objects);
        add_loose(&mut repo, &objects);
        repo
    }
}

/// Each pack of `objects/pack` with its index.
fn add_packs(repo: &mut Repository, objects: &Path) {
    for (name, path) in files_below(objects, "pack") {
        if let Some(stem) = name.strip_suffix(".idx") {
            let data = fs::read(objects.join(format!("{stem}.pack"))).unwrap();
            repo.add_pack(fs::read(path).unwrap(), data).unwrap();
        }
    }
}

/// Each loose object of the two-letter directories of `objects`.
fn add_loose(repo: &mut Repository, objects: &Path) {
    for dir in fs::read_dir(objects).unwrap() {
        let dir = dir.unwrap();
        let prefix = dir.file_name().into_string().unwrap();
        if prefix.len() != 2 {
            continue;
        }
        for file in fs::read_dir(dir.path()).unwrap() {
            let file = file.unwrap();
            let rest = file.file_name().into_string().unwrap();
            repo.add_loose(&format!("{prefix}{rest}"), fs::read(file.path()).unwrap())
                .unwrap();
        }
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// The files below `root/sub`, with their paths from `root`.
fn files_below(root: &Path, sub: &str) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let mut todo = vec![sub.to_string()];
    while let Some(rel) = todo.pop() {
        let Ok(entries) = fs::read_dir(root.join(&rel)) else {
            continue;
        };
        for e in entries {
            let e = e.unwrap();
            let name = format!("{rel}/{}", e.file_name().into_string().unwrap());
            if e.file_type().unwrap().is_dir() {
                todo.push(name);
            } else {
                out.push((name, e.path()));
            }
        }
    }
    out
}

fn text_at(repo: &Repository, rev: &str, path: &str) -> String {
    let commit = repo.resolve(rev).unwrap();
    let tree = repo.commit(&commit).unwrap().tree;
    let files = repo.files(&tree, &|_| true, &|_| true).unwrap();
    let (_, id) = files
        .iter()
        .find(|(p, _)| p == path)
        .unwrap_or_else(|| panic!("no {path}"));
    String::from_utf8(repo.blob(id).unwrap()).unwrap()
}

#[test]
fn reads_a_file_at_head_from_loose_objects() {
    let r = Repo::new("loose");
    r.write("src/A.java", "class A {}\n");
    r.commit("one");
    assert!(r.git(&["count-objects", "-v"]).contains("count: 4"));
    assert_eq!(
        text_at(&r.reader(), "HEAD", "src/A.java"),
        r.git(&["show", "HEAD:src/A.java"])
    );
}

/// Versions of one file that git stores as deltas of each other.
fn versions(r: &Repo, n: usize) -> Vec<String> {
    let mut body: String = (0..200)
        .map(|i| format!("    int f{i}() {{ return {i}; }}\n"))
        .collect();
    (0..n)
        .map(|v| {
            body.push_str(&format!("    int g{v}() {{ return {v}; }}\n"));
            r.write("src/A.java", &format!("class A {{\n{body}}}\n"));
            r.commit(&format!("version {v}"))
        })
        .collect()
}

fn pack_files(r: &Repo, ext: &str) -> Vec<PathBuf> {
    fs::read_dir(r.dir.join(".git/objects/pack"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == ext))
        .collect()
}

#[test]
fn reads_every_version_of_a_delta_chain_in_a_pack() {
    let r = Repo::new("pack");
    let commits = versions(&r, 6);
    r.git(&["gc", "-q", "--aggressive"]);
    assert!(r.git(&["count-objects", "-v"]).contains("count: 0"));
    let idx = pack_files(&r, "idx")[0].to_string_lossy().into_owned();
    let stats = r.git(&["verify-pack", "-v", &idx]);
    // Some versions are stored as deltas.
    assert!(stats.contains("chain length = "), "{stats}");
    let reader = r.reader();
    for c in &commits {
        assert_eq!(
            text_at(&reader, c, "src/A.java"),
            r.git(&["show", &format!("{c}:src/A.java")])
        );
    }
}

#[test]
fn reads_objects_spread_over_two_packs_and_loose_objects() {
    let r = Repo::new("packs");
    let first = versions(&r, 2);
    r.git(&["repack", "-q", "-d"]);
    let second = versions(&r, 2);
    r.git(&["repack", "-q", "-d"]);
    r.write("src/B.java", "class B {}\n");
    let third = r.commit("loose");
    assert_eq!(pack_files(&r, "pack").len(), 2);
    let reader = r.reader();
    for c in first.iter().chain(&second).chain([&third]) {
        assert_eq!(
            text_at(&reader, c, "src/A.java"),
            r.git(&["show", &format!("{c}:src/A.java")])
        );
    }
    assert_eq!(text_at(&reader, "HEAD", "src/B.java"), "class B {}\n");
}

#[test]
fn a_commit_has_its_tree_and_its_parents() {
    let r = Repo::new("parents");
    r.write("a.rs", "fn a() {}\n");
    let one = r.commit("one");
    r.write("a.rs", "fn b() {}\n");
    let two = r.commit("two");
    let reader = r.reader();
    let commit = reader.commit(&reader.resolve(&two).unwrap()).unwrap();
    assert_eq!(
        commit.tree.to_string(),
        r.git(&["rev-parse", "HEAD^{tree}"]).trim()
    );
    let parents: Vec<String> = commit.parents.iter().map(|p| p.to_string()).collect();
    assert_eq!(parents, std::slice::from_ref(&one));
    let root = reader.commit(&reader.resolve(&one).unwrap()).unwrap();
    assert!(root.parents.is_empty());
}

#[test]
fn resolves_head_through_a_loose_ref_a_packed_ref_and_a_detached_head() {
    let r = Repo::new("refs");
    r.write("a.rs", "fn a() {}\n");
    let one = r.commit("one");
    assert_eq!(r.reader().resolve("HEAD").unwrap().to_string(), one);
    r.git(&["pack-refs", "--all"]);
    assert!(!r.dir.join(".git/refs/heads/main").exists());
    assert_eq!(r.reader().resolve("HEAD").unwrap().to_string(), one);
    assert_eq!(
        r.reader().resolve("refs/heads/main").unwrap().to_string(),
        one
    );
    r.write("a.rs", "fn b() {}\n");
    let two = r.commit("two");
    // The loose ref is newer than the packed one.
    assert_eq!(r.reader().resolve("HEAD").unwrap().to_string(), two);
    r.git(&["checkout", "-q", "--detach", &one]);
    assert_eq!(r.reader().resolve("HEAD").unwrap().to_string(), one);
}

#[test]
fn an_unborn_branch_does_not_resolve() {
    let r = Repo::new("unborn");
    let err = r.reader().resolve("HEAD").unwrap_err();
    assert!(err.contains("refs/heads/main"), "{err}");
}

#[test]
fn the_files_of_a_tree_are_the_blobs_that_keep_wants() {
    let r = Repo::new("files");
    r.write("src/A.java", "class A {}\n")
        .write("src/b/B.java", "class B {}\n")
        .write("README", "x\n");
    r.commit("one");
    let reader = r.reader();
    let tree = reader
        .commit(&reader.resolve("HEAD").unwrap())
        .unwrap()
        .tree;
    let files: Vec<String> = reader
        .files(&tree, &|_| true, &|p| p.ends_with(".java"))
        .unwrap()
        .into_iter()
        .map(|(p, _)| p)
        .collect();
    assert_eq!(files, ["src/A.java", "src/b/B.java"]);
}

#[test]
fn a_directory_that_keep_dir_rejects_is_not_read() {
    let r = Repo::new("prune");
    r.write("src/A.java", "class A {}\n")
        .write("node_modules/x/B.java", "class B {}\n");
    r.commit("one");
    // Without its tree the directory cannot be read: only pruning gets past it.
    let pruned = r.git(&["rev-parse", "HEAD:node_modules"]);
    let (dir, file) = pruned.trim().split_at(2);
    fs::remove_file(r.dir.join(".git/objects").join(dir).join(file)).unwrap();
    let reader = r.reader();
    let tree = reader
        .commit(&reader.resolve("HEAD").unwrap())
        .unwrap()
        .tree;
    let files: Vec<String> = reader
        .files(&tree, &|d| d != "node_modules", &|_| true)
        .unwrap()
        .into_iter()
        .map(|(p, _)| p)
        .collect();
    assert_eq!(files, ["src/A.java"]);
}

#[cfg(unix)]
#[test]
fn an_executable_is_a_file_and_a_symbolic_link_is_none() {
    let r = Repo::new("modes");
    r.write("run.sh", "echo\n").write("A.java", "class A {}\n");
    std::os::unix::fs::symlink("A.java", r.dir.join("Link.java")).unwrap();
    r.git(&["update-index", "--add", "--chmod=+x", "run.sh"]);
    r.commit("one");
    let reader = r.reader();
    let tree = reader
        .commit(&reader.resolve("HEAD").unwrap())
        .unwrap()
        .tree;
    let files: Vec<String> = reader
        .files(&tree, &|_| true, &|_| true)
        .unwrap()
        .into_iter()
        .map(|(p, _)| p)
        .collect();
    assert_eq!(files, ["A.java", "run.sh"]);
}

#[test]
fn a_missing_object_is_an_error_that_names_it() {
    let r = Repo::new("missing");
    r.write("a.rs", "fn a() {}\n");
    let one = r.commit("one");
    let reader = Repository::new(&format!("{one}\n"));
    let err = reader.commit(&reader.resolve("HEAD").unwrap()).unwrap_err();
    assert!(err.contains(&one), "{err}");
}

#[test]
fn a_commit_has_its_author_its_date_with_the_offset_and_its_message() {
    let r = Repo::new("meta");
    let dates = [
        "2026-11-20T10:00:00+0130",
        "2026-11-21T23:15:09-0800",
        // A leap day that the offset moves past midnight in UTC. (`git commit` refuses a date
        // before 1970, so none is compared here.)
        "2024-02-29T23:59:59+0545",
        "2024-03-01T01:30:00+0545",
        "2026-11-22T00:00:00+0000",
    ];
    for (i, date) in dates.iter().enumerate() {
        r.write("a.rs", &format!("fn a{i}() {{}}\n"));
        let id = r.commit_by(
            &format!("Title {i}\n\nBody with ümlaut\nand more\n"),
            &format!("Ada Ünder {i}"),
            date,
        );
        let reader = r.reader();
        let commit = reader.commit(&reader.resolve(&id).unwrap()).unwrap();
        assert_eq!(commit.author, r.git(&["log", "-1", "--format=%an"]).trim());
        // Newer `git` writes `Z` for the offset 0, older ones `+00:00`: both are ISO 8601.
        let expected = r.git(&["log", "-1", "--format=%aI"]);
        let expected = expected.trim().strip_suffix('Z').map_or_else(
            || expected.trim().to_string(),
            |head| format!("{head}+00:00"),
        );
        assert_eq!(commit.date, expected);
        assert_eq!(
            commit.message.trim_end(),
            r.git(&["log", "-1", "--format=%B"]).trim_end()
        );
    }
    let reader = r.reader();
    let last = reader.commit(&reader.resolve("HEAD").unwrap()).unwrap();
    assert_eq!(last.date, "2026-11-22T00:00:00+00:00");
}

#[test]
fn an_abbreviated_id_resolves_among_loose_and_packed_objects() {
    let r = Repo::new("abbrev");
    r.write("a.rs", "fn a() {}\n");
    let packed = r.commit("packed");
    r.git(&["gc", "-q"]);
    assert!(r.git(&["count-objects", "-v"]).contains("count: 0"));
    r.write("a.rs", "fn b() {}\n");
    let loose = r.commit("loose");
    assert!(r.git(&["count-objects", "-v"]).contains("count: 3"));
    let reader = r.reader();
    for full in [&packed, &loose] {
        for len in [7, 12, 39, 40] {
            let short = &full[..len];
            assert_eq!(reader.resolve(short).unwrap().to_string(), *full);
            assert_eq!(
                reader.resolve(&short.to_uppercase()).unwrap().to_string(),
                *full
            );
        }
    }
    // Too short, or no object: no ref.
    for name in ["abcdef", "0000000", "main"] {
        let err = reader.resolve(name).unwrap_err();
        assert!(err.contains(&format!("no ref {name}")), "{err}");
    }
}

/// Two contents of blobs whose ids share the first 7 hex digits (a birthday search over
/// about 20 000 candidates).
fn colliding_blobs() -> (String, String) {
    let mut seen = std::collections::HashMap::new();
    for i in 0u32.. {
        let text = format!("blob {i}\n");
        let id = gix_object::compute_hash(
            gix_hash::Kind::Sha1,
            gix_object::Kind::Blob,
            text.as_bytes(),
        )
        .unwrap();
        let b = id.as_bytes();
        let key = u32::from_be_bytes([b[0], b[1], b[2], b[3] >> 4]);
        if let Some(other) = seen.insert(key, text.clone()) {
            return (other, text);
        }
    }
    unreachable!()
}

#[test]
fn an_abbreviated_id_that_two_objects_share_is_ambiguous() {
    let r = Repo::new("ambiguous");
    let (one, two) = colliding_blobs();
    r.write("one.txt", &one).write("two.txt", &two);
    let head = r.commit("both");
    let ids: Vec<String> = ["one.txt", "two.txt"]
        .iter()
        .map(|f| {
            r.git(&["rev-parse", &format!("HEAD:{f}")])
                .trim()
                .to_string()
        })
        .collect();
    assert_eq!(ids[0][..7], ids[1][..7]);
    let check = |reader: &Repository| {
        let err = reader.resolve(&ids[0][..7]).unwrap_err();
        assert!(err.contains("ambiguous"), "{err}");
        assert!(reader.resolve(&head[..7]).is_ok());
        // Long enough to tell them apart.
        let len = (7..40).find(|n| ids[0][..*n] != ids[1][..*n]).unwrap();
        for id in &ids {
            assert_eq!(reader.resolve(&id[..len]).unwrap().to_string(), *id);
        }
    };
    check(&r.reader());
    r.git(&["gc", "-q"]);
    assert!(r.git(&["count-objects", "-v"]).contains("count: 0"));
    check(&r.reader());
}

/// What `git diff --name-status --no-renames` says between two trees, as the reader's pairs.
fn name_status(r: &Repo, base: &str, head: &str) -> Vec<(String, Change)> {
    r.git(&["diff", "--name-status", "--no-renames", base, head])
        .lines()
        .map(|line| {
            let (status, path) = line.split_once('\t').unwrap();
            let change = match status {
                "A" => Change::Added,
                "D" => Change::Removed,
                "M" => Change::Modified,
                other => panic!("status {other}"),
            };
            (path.to_string(), change)
        })
        .collect()
}

fn tree_of(reader: &Repository, rev: &str) -> Id {
    reader.commit(&reader.resolve(rev).unwrap()).unwrap().tree
}

const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

#[test]
fn the_changes_between_two_trees_are_those_of_a_name_status_diff() {
    let r = Repo::new("changes");
    r.write("keep.rs", "fn keep() {}\n")
        .write("mod.rs", "fn m() {}\n")
        .write("gone.rs", "fn gone() {}\n")
        .write("src/deep/a/b.rs", "fn b() {}\n")
        .write("src/deep/same.rs", "fn same() {}\n")
        .write("was_file", "now a directory\n")
        .write("was_dir/x.rs", "fn x() {}\n")
        .write("was_dir/y/z.rs", "fn z() {}\n");
    let first = r.commit("one");
    std::fs::remove_file(r.dir.join("gone.rs")).unwrap();
    std::fs::remove_file(r.dir.join("was_file")).unwrap();
    std::fs::remove_dir_all(r.dir.join("was_dir")).unwrap();
    r.write("mod.rs", "fn m2() {}\n")
        .write("added.rs", "fn added() {}\n")
        .write("src/deep/a/b.rs", "fn b2() {}\n")
        .write("src/new/n.rs", "fn n() {}\n")
        .write("was_file/inner.rs", "fn inner() {}\n")
        .write("was_dir", "now a file\n");
    let second = r.commit("two");
    let reader = r.reader();
    let (t1, t2) = (tree_of(&reader, &first), tree_of(&reader, &second));
    let got = reader.changes(Some(&t1), &t2).unwrap();
    assert_eq!(got, name_status(&r, &first, &second));
    assert!(got.contains(&("was_file".to_string(), Change::Removed)));
    assert!(got.contains(&("was_file/inner.rs".to_string(), Change::Added)));
    assert!(got.contains(&("was_dir".to_string(), Change::Added)));
    assert!(got.contains(&("was_dir/y/z.rs".to_string(), Change::Removed)));
    // The other way round: added and removed swap.
    let back = reader.changes(Some(&t2), &t1).unwrap();
    assert_eq!(back, name_status(&r, &second, &first));
    // No base: every file is added (a root commit).
    let root = reader.changes(None, &t1).unwrap();
    let all: Vec<(String, Change)> = name_status(&r, EMPTY_TREE, &first);
    assert_eq!(root, all);
    assert!(root.iter().all(|(_, c)| *c == Change::Added));
    // The same tree: nothing.
    assert!(reader.changes(Some(&t1), &t1).unwrap().is_empty());
}

#[test]
fn a_subtree_with_the_same_id_on_both_sides_is_not_read() {
    let r = Repo::new("changes-skip");
    r.write("same/a.rs", "fn a() {}\n")
        .write("same/deep/b.rs", "fn b() {}\n")
        .write("other.rs", "fn o() {}\n");
    let first = r.commit("one");
    r.write("other.rs", "fn o2() {}\n");
    let second = r.commit("two");
    let unchanged = r.git(&["rev-parse", "HEAD:same"]);
    let (dir, file) = unchanged.trim().split_at(2);
    std::fs::remove_file(r.dir.join(".git/objects").join(dir).join(file)).unwrap();
    let reader = r.reader();
    let got = reader
        .changes(Some(&tree_of(&reader, &first)), &tree_of(&reader, &second))
        .unwrap();
    assert_eq!(got, [("other.rs".to_string(), Change::Modified)]);
}

#[cfg(unix)]
#[test]
fn a_change_of_the_mode_only_is_no_change_and_a_symbolic_link_is_no_file() {
    let r = Repo::new("changes-mode");
    r.write("run.sh", "echo\n").write("A.java", "class A {}\n");
    let first = r.commit("one");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(r.dir.join("run.sh"), std::fs::Permissions::from_mode(0o755)).unwrap();
    std::os::unix::fs::symlink("A.java", r.dir.join("Link.java")).unwrap();
    r.commit("two");
    let reader = r.reader();
    let tree = tree_of(&reader, "HEAD");
    assert!(
        r.git(&["diff", "--name-status", &first, "HEAD"])
            .contains("run.sh")
    );
    assert!(
        reader
            .changes(Some(&tree_of(&reader, &first)), &tree)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn first_parents_of_a_shallow_clone_end_where_its_history_ends() {
    let r = Repo::new("shallow-origin");
    for v in 1..=4 {
        r.write("a.rs", &format!("fn v{v}() {{}}\n"));
        r.commit(&format!("version {v}"));
    }
    let dir = std::env::temp_dir().join(format!("cg-git-shallow-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let url = format!("file://{}", r.dir.display());
    git(
        &std::env::temp_dir(),
        &["clone", "-q", "--depth", "2", &url, dir.to_str().unwrap()],
    );
    let clone = Repo { dir };
    let reader = clone.reader();
    let head = reader.resolve("HEAD").unwrap();

    let listed = reader.first_parents(&head, 10).unwrap();

    // `git log` of the clone stops at the commit that `.git/shallow` names; its parent is absent.
    let expected: Vec<String> = clone
        .git(&["log", "--first-parent", "--format=%H", "HEAD"])
        .lines()
        .map(str::to_string)
        .collect();
    let ids: Vec<String> = listed.iter().map(|(id, _)| id.to_string()).collect();
    assert_eq!(ids, expected);
    assert_eq!(ids.len(), 2);
}

#[test]
fn first_parents_follow_the_first_parent_past_a_merge_to_the_root() {
    let r = Repo::new("firstparents");
    r.write("a.rs", "fn a() {}\n");
    r.commit("root");
    r.write("a.rs", "fn b() {}\n");
    r.commit("main 1");
    r.git(&["checkout", "-q", "-b", "side"]);
    r.write("s.rs", "fn s() {}\n");
    r.commit("side 1");
    r.write("s.rs", "fn t() {}\n");
    r.commit("side 2");
    r.git(&["checkout", "-q", "main"]);
    r.write("a.rs", "fn c() {}\n");
    r.commit("main 2");
    r.git(&["merge", "-q", "--no-ff", "-m", "merge", "side"]);
    r.write("a.rs", "fn d() {}\n");
    r.commit("main 3");
    let reader = r.reader();
    let head = reader.resolve("HEAD").unwrap();
    let listed = |n: usize| -> Vec<String> {
        reader
            .first_parents(&head, n)
            .unwrap()
            .into_iter()
            .map(|(id, _)| id.to_string())
            .collect()
    };
    let expected: Vec<String> = r
        .git(&["log", "--first-parent", "--format=%H", "HEAD"])
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(expected.len(), 5);
    assert_eq!(listed(100), expected);
    assert_eq!(listed(3), expected[..3]);
    assert!(listed(0).is_empty());
    // Each entry carries its commit.
    let (id, commit) = reader.first_parents(&head, 1).unwrap().remove(0);
    assert_eq!(id, head);
    assert_eq!(commit, reader.commit(&head).unwrap());
}
