use cg_core::{holds_no_sources, outside_excluded_directories, text_hash};

#[test]
fn hidden_target_and_node_modules_directories_hold_no_sources() {
    for dir in [".git", ".hidden", "target", "node_modules"] {
        assert!(holds_no_sources(dir), "{dir}");
    }
    for dir in ["src", "targets", "modules", "a.b"] {
        assert!(!holds_no_sources(dir), "{dir}");
    }
}

#[test]
fn only_the_directories_of_a_path_count_not_its_file_name() {
    assert!(outside_excluded_directories("a/b/c.rs"));
    assert!(outside_excluded_directories("target"));
    assert!(outside_excluded_directories(".hidden"));
    assert!(!outside_excluded_directories("a/target/c.rs"));
    assert!(!outside_excluded_directories(".git/x"));
}

#[test]
fn the_text_hash_ignores_the_amount_of_whitespace_and_has_16_hex_digits() {
    let hash = text_hash("fn  f() {\n    1\n}\n");

    assert_eq!(hash, text_hash(" fn f() { 1 }"));
    assert_ne!(hash, text_hash("fn f() { 2 }"));
    assert_eq!(hash.len(), 16);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}
