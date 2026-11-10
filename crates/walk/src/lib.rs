//! The one walk over a directory that all front ends share: it reads the files that a
//! front end wants into `SourceFile`s. `cg-core` has no I/O, so the walk lives here.

use cg_core::{SourceFile, holds_no_sources};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The files below `root` for which `is_input` holds, read as UTF-8 (invalid bytes
/// become U+FFFD), sorted by path. `is_input` gets the path relative to `root` with `/`
/// as separator. Directories that `cg_core::holds_no_sources` rejects are not entered.
pub fn read_sources(root: &Path, is_input: impl Fn(&str) -> bool) -> io::Result<Vec<SourceFile>> {
    let mut paths: Vec<(PathBuf, String)> = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            if path.is_dir() {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
                if !name.is_some_and(|n| holds_no_sources(&n)) {
                    pending.push(path);
                }
            } else {
                let relative = slash_path(path.strip_prefix(root).unwrap_or(&path));
                if is_input(&relative) {
                    paths.push((path, relative));
                }
            }
        }
    }
    paths.sort();
    paths
        .into_iter()
        .map(|(path, relative)| {
            Ok(SourceFile {
                path: relative,
                text: String::from_utf8_lossy(&fs::read(&path)?).into_owned(),
            })
        })
        .collect()
}

/// The path with `/` as separator, whatever the platform.
fn slash_path(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}
