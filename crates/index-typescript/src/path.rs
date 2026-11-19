//! Paths relative to the root of the repository, with `/` as separator.

/// The directory of the file at `path`: empty for a file in the root.
pub fn dir_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// `dir` and a relative `specifier` as one path, without `.` and `..` segments; `None` if
/// the path leaves the root.
pub fn join(dir: &str, specifier: &str) -> Option<String> {
    let mut segments: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
    for segment in specifier.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            other => segments.push(other),
        }
    }
    Some(segments.join("/"))
}
