//! Repositories made with the `git` command in a temporary directory; the reader gets the
//! bytes of their `.git` as the viewer gives them, and its answers are compared with `git`.

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
