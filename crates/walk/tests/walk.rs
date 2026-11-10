use cg_walk::read_sources;
use std::fs;
use tempfile::TempDir;

fn tree(files: &[(&str, &[u8])]) -> TempDir {
    let dir = TempDir::new().unwrap();
    for (path, content) in files {
        let full = dir.path().join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, content).unwrap();
    }
    dir
}

#[test]
fn it_reads_the_wanted_files_with_relative_paths_sorted_and_skips_directories_without_sources() {
    let dir = tree(&[
        ("b/Two.java", b"two"),
        ("a/One.java", b"one"),
        ("notes.txt", b"x"),
        (".git/Hidden.java", b"hidden"),
        ("target/Built.java", b"built"),
        ("web/node_modules/Dep.java", b"dep"),
    ]);

    let files = read_sources(dir.path(), |path| path.ends_with(".java")).unwrap();

    let found: Vec<(&str, &str)> = files
        .iter()
        .map(|f| (f.path.as_str(), f.text.as_str()))
        .collect();
    assert_eq!(found, [("a/One.java", "one"), ("b/Two.java", "two")]);
}

#[test]
fn text_that_is_not_utf8_is_read_with_replacement_characters() {
    let dir = tree(&[("a.rs", &[b'f', 0xff, b'o'])]);

    let files = read_sources(dir.path(), |_| true).unwrap();

    assert_eq!(files[0].text, "f\u{fffd}o");
}

#[test]
fn a_root_that_cannot_be_read_is_an_error() {
    let dir = TempDir::new().unwrap();

    assert!(read_sources(&dir.path().join("missing"), |_| true).is_err());
}
