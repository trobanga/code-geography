//! `deps.edn`: where the source directories of a module are (ADR 23).

use crate::syntax::{flat_items, items, keyword, string};
use tree_sitter::{Node as Ts, Parser};

/// The source directories of one module, relative to its directory (empty: the directory
/// itself), each with whether it comes from an alias (`:extra-paths`) and not from the
/// top-level `:paths`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceDirs(Vec<(String, bool)>);

impl Default for SourceDirs {
    /// What a `deps.edn` without `:paths` means.
    fn default() -> Self {
        SourceDirs(vec![("src".to_string(), false)])
    }
}

impl SourceDirs {
    /// Reads the text of a `deps.edn`. Text that is no map gives the default.
    pub fn parse(parser: &mut Parser, text: &str) -> SourceDirs {
        let Some(tree) = parser.parse(text, None) else {
            return SourceDirs::default();
        };
        let forms = flat_items(tree.root_node());
        let Some(map) = forms.iter().find(|f| f.kind() == "map_lit") else {
            return SourceDirs::default();
        };
        let mut dirs = Vec::new();
        let mut has_paths = false;
        for (key, value) in entries(*map) {
            match keyword(key, text) {
                Some("paths") => {
                    has_paths = true;
                    dirs.extend(paths(value, text).map(|p| (p, false)));
                }
                Some("aliases") => {
                    for (_, alias) in entries(value) {
                        for (key, value) in entries(alias) {
                            if keyword(key, text) == Some("extra-paths") {
                                dirs.extend(paths(value, text).map(|p| (p, true)));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        if !has_paths {
            dirs.push(("src".to_string(), false));
        }
        SourceDirs(dirs)
    }

    /// For a file below the module, given by its path relative to the module directory:
    /// `None` if no source directory contains it, else whether it is test code, that is,
    /// whether the longest directory that contains it comes from an alias only.
    pub fn test_code(&self, relative: &str) -> Option<bool> {
        self.0
            .iter()
            .filter(|(dir, _)| {
                dir.is_empty()
                    || relative
                        .strip_prefix(dir.as_str())
                        .is_some_and(|rest| rest.starts_with('/'))
            })
            // The longest directory; on a tie `:paths` beats an alias.
            .max_by_key(|(dir, alias)| (dir.len(), !alias))
            .map(|(_, alias)| *alias)
    }
}

/// The key-value pairs of a map form.
fn entries(map: Ts) -> Vec<(Ts, Ts)> {
    if map.kind() != "map_lit" {
        return Vec::new();
    }
    let items = items(map);
    items
        .as_chunks::<2>()
        .0
        .iter()
        .map(|[k, v]| (*k, *v))
        .collect()
}

/// The directories that a vector of strings names, normalized: no `./` in front, no `/`
/// at the end, `.` as the empty string. A directory above the module is dropped.
fn paths<'a>(vector: Ts<'a>, text: &'a str) -> impl Iterator<Item = String> + 'a {
    let strings = if matches!(vector.kind(), "vec_lit" | "list_lit") {
        flat_items(vector)
    } else {
        Vec::new()
    };
    strings
        .into_iter()
        .filter_map(move |s| string(s, text))
        .map(|p| {
            let p = p.trim_end_matches('/');
            let p = p.strip_prefix("./").unwrap_or(p);
            if p == "." { "" } else { p }.to_string()
        })
        .filter(|p| p != ".." && !p.starts_with("../"))
}
