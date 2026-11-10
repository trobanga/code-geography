//! What every front end shares about its input: the file as data, the rule for the
//! directories that hold no sources, and the hash of the own text of a declaration.

/// One file of the input of a front end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile {
    /// Relative to the root of the repository, with `/` as separator.
    pub path: String,
    /// The content. A front end that needs only the place of a file (`pom.xml`) accepts
    /// an empty text.
    pub text: String,
}

/// Hidden directories, build output (`target`) and npm packages hold copies, not sources.
pub fn holds_no_sources(dir: &str) -> bool {
    dir.starts_with('.') || dir == "target" || dir == "node_modules"
}

/// Whether the file at `path` (relative, `/` as separator) lies outside the
/// directories that `holds_no_sources`.
pub fn outside_excluded_directories(path: &str) -> bool {
    let mut dirs = path.split('/').rev().skip(1);
    dirs.all(|dir| !holds_no_sources(dir))
}

/// 64-bit FNV-1a hash (16 hex digits) of a source text in which each run of whitespace
/// counts as one space and leading and trailing whitespace does not count. Written by
/// hand because `DefaultHasher` is not stable across Rust versions.
pub fn text_hash(text: &str) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in normalized.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}
