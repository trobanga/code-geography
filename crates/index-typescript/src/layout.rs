//! Where a file lives: its module (the nearest `package.json`), its package path and
//! whether it is test code (ADR 34).

use std::collections::BTreeMap;
use tree_sitter::Tree;

/// A file that is read: its syntax tree and its place in the module tree.
pub struct Source<'a> {
    pub path: &'a str,
    pub text: &'a str,
    pub tree: Tree,
    /// The directory of the module (its `package.json`), relative to the root.
    pub module: &'a str,
    /// The path in the module without extension and without a trailing `/index`: the
    /// package of the file. Empty for the `index.ts` of the module directory.
    pub package: String,
    pub test: bool,
}

/// The `package.json` files by directory, with the `name` of each, if it has one.
pub type Manifests = BTreeMap<String, Option<String>>;

/// Reads the `package.json` files among `files` (path, text). A file that is not valid
/// JSON counts as a `package.json` without a name.
pub fn manifests<'a>(files: impl Iterator<Item = (&'a str, &'a str)>) -> Manifests {
    files
        .filter(|(path, _)| *path == "package.json" || path.ends_with("/package.json"))
        .map(|(path, text)| {
            let dir = path.strip_suffix("package.json").unwrap_or_default();
            let dir = dir.strip_suffix('/').unwrap_or(dir);
            let name = serde_json::from_str::<serde_json::Value>(crate::config::without_bom(text))
                .ok()
                .and_then(|json| json.get("name")?.as_str().map(str::to_string))
                .filter(|name| !name.is_empty());
            (dir.to_string(), name)
        })
        .collect()
}

/// The test rule of ADR 34: a file named `test`, `spec`, `*.test` or `*.spec` (with `.ts` or
/// `.tsx`), and each file below a directory `__tests__` or `__mocks__`. A pure function of
/// the path, whatever its module.
pub fn is_test_file(path: &str) -> bool {
    let (dirs, name) = path.rsplit_once('/').unwrap_or(("", path));
    let stem = name
        .strip_suffix(".tsx")
        .or_else(|| name.strip_suffix(".ts"))
        .unwrap_or(name);
    matches!(stem, "test" | "spec")
        || stem.ends_with(".test")
        || stem.ends_with(".spec")
        || dirs
            .split('/')
            .any(|dir| dir == "__tests__" || dir == "__mocks__")
}

/// Whether the file at `path` is a source file, not a configuration file.
pub fn is_typescript_path(path: &str) -> bool {
    path.ends_with(".ts") || path.ends_with(".tsx")
}

/// Whether a `.ts` file is not TypeScript: it starts with `<` (after a BOM and white
/// space) or holds a NUL character. A Qt translation, a video stream.
pub fn is_not_typescript(path: &str, text: &str) -> bool {
    path.ends_with(".ts")
        && (text.contains('\0')
            || text
                .trim_start_matches(|c: char| c == '\u{feff}' || c.is_whitespace())
                .starts_with('<'))
}

/// The name of the module in `dir`: the `name` of its `package.json`, else the last
/// segment of the directory, else the root name.
pub fn module_name(manifests: &Manifests, dir: &str, root_name: &str) -> String {
    manifests
        .get(dir)
        .cloned()
        .flatten()
        .or_else(|| {
            dir.rsplit('/')
                .next()
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| root_name.to_string())
}

/// The directory of the nearest `package.json` above the file at `path`.
pub fn module_dir<'m>(manifests: &'m Manifests, path: &str) -> Option<&'m str> {
    let mut dir = path;
    while let Some((above, _)) = dir.rsplit_once('/') {
        dir = above;
        if let Some((found, _)) = manifests.get_key_value(dir) {
            return Some(found);
        }
    }
    manifests.get_key_value("").map(|(found, _)| found.as_str())
}

/// The path of the file relative to the module directory, without `.ts` or `.tsx` and
/// without a trailing `index` segment: the package it belongs to.
pub fn package_path(path: &str, module_dir: &str) -> String {
    let relative = path
        .strip_prefix(module_dir)
        .map_or(path, |rest| rest.strip_prefix('/').unwrap_or(rest));
    let stem = relative
        .strip_suffix(".tsx")
        .or_else(|| relative.strip_suffix(".ts"))
        .unwrap_or(relative);
    if stem == "index" {
        String::new()
    } else {
        stem.strip_suffix("/index").unwrap_or(stem).to_string()
    }
}
