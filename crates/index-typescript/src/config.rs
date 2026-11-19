//! The configuration files that decide what a specifier reaches: `tsconfig*.json` and
//! `package.json` (ADR 34).

use crate::path::{dir_of, join};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};

/// The `tsconfig*.json` and `package.json` files of the repository, parsed.
pub struct Config {
    /// By path.
    tsconfigs: BTreeMap<String, Value>,
    /// By directory.
    manifests: BTreeMap<String, Value>,
}

/// Patterns with one `*` at most (of `paths`, of `exports`), each with its targets.
pub type Patterns = Vec<(String, Vec<String>)>;

/// What a configuration says about names: `compilerOptions.paths` and `baseUrl`.
pub struct Options {
    /// The patterns of `paths` with their targets.
    pub patterns: Patterns,
    /// The directory that the targets are relative to.
    pub base: String,
    /// The `baseUrl`, as a path from the root.
    pub base_url: Option<String>,
}

impl Config {
    /// Reads the configuration files among `files` (path, text).
    pub fn read<'a>(files: impl Iterator<Item = (&'a str, &'a str)>) -> Self {
        let mut tsconfigs = BTreeMap::new();
        let mut manifests = BTreeMap::new();
        for (path, text) in files {
            if is_tsconfig(path) {
                tsconfigs.insert(path.to_string(), parse_lenient(text));
            } else if path.rsplit('/').next() == Some("package.json") {
                let value =
                    serde_json::from_str(without_bom(text)).unwrap_or_else(|_| empty_object());
                manifests.insert(dir_of(path).to_string(), value);
            }
        }
        Config {
            tsconfigs,
            manifests,
        }
    }

    /// The directories of the `package.json` files whose `name` is `name`.
    pub fn packages_named(&self, name: &str) -> Vec<&str> {
        self.manifests
            .iter()
            .filter(|(_, json)| json.get("name").and_then(Value::as_str) == Some(name))
            .map(|(dir, _)| dir.as_str())
            .collect()
    }

    /// The paths, from the root, that the package in `dir` names as the file of
    /// `subpath` (`None`: the entry of the package), as far as its `package.json` says.
    pub fn entries(&self, dir: &str, subpath: Option<&str>) -> Vec<String> {
        let Some(json) = self.manifests.get(dir) else {
            return Vec::new();
        };
        if let Some(subpath) = subpath {
            return subpath_entries(json, dir, subpath);
        }
        entry_files(json)
            .into_iter()
            .filter_map(|file| join(dir, file))
            .collect()
    }

    /// The directory of the nearest `tsconfig.json` at or above `dir`.
    pub fn tsconfig_dir(&self, dir: &str) -> Option<String> {
        let mut dir = dir;
        loop {
            let path = join(dir, "tsconfig.json")?;
            if self.tsconfigs.contains_key(&path) {
                return Some(dir.to_string());
            }
            if dir.is_empty() {
                return None;
            }
            dir = dir_of(dir);
        }
    }

    /// The configurations of the directory `dir`, which holds a `tsconfig.json`: that file
    /// and the files its `references` name (one level), each a file or a directory.
    pub fn configurations(&self, dir: &str) -> Vec<String> {
        let Some(tsconfig) = join(dir, "tsconfig.json") else {
            return Vec::new();
        };
        let references: Vec<String> = self
            .tsconfigs
            .get(&tsconfig)
            .and_then(|json| json.get("references")?.as_array())
            .into_iter()
            .flatten()
            .filter_map(|reference| join(dir, reference.get("path")?.as_str()?))
            .filter_map(|path| {
                [path.clone(), join(&path, "tsconfig.json")?]
                    .into_iter()
                    .find(|file| self.tsconfigs.contains_key(file))
            })
            .collect();
        let mut all = vec![tsconfig];
        all.extend(references);
        all
    }

    /// The effective options of the configuration file `path`: its own, and for what it
    /// does not set the options of the files it extends.
    pub fn options(&self, path: &str) -> Options {
        let mut found = Found::default();
        self.fill(path, &mut HashSet::new(), &mut found);
        let (patterns, paths_dir) = found.paths.unwrap_or_default();
        Options {
            patterns,
            base: found.base_url.clone().unwrap_or(paths_dir),
            base_url: found.base_url,
        }
    }

    /// Takes what `path` sets and `found` lacks, then goes on to the files that it extends:
    /// the last entry first, as it wins over the earlier ones.
    fn fill(&self, path: &str, visited: &mut HashSet<String>, found: &mut Found) {
        if !visited.insert(path.to_string()) {
            return;
        }
        let Some(json) = self.tsconfigs.get(path) else {
            return;
        };
        found.take_from(json, path);
        let extends = match json.get("extends") {
            Some(Value::String(one)) => vec![one.as_str()],
            Some(Value::Array(entries)) => entries.iter().filter_map(Value::as_str).collect(),
            _ => Vec::new(),
        };
        for entry in extends.into_iter().rev() {
            if let Some(file) = self.extended(path, entry) {
                self.fill(&file, visited, found);
            }
        }
    }

    /// The configuration file that the value `extends`, written in `path`, names: only a
    /// relative value counts, with `.json` as written or appended.
    fn extended(&self, path: &str, extends: &str) -> Option<String> {
        if !(extends.starts_with("./") || extends.starts_with("../")) {
            return None;
        }
        let file = join(dir_of(path), extends)?;
        [file.clone(), format!("{file}.json")]
            .into_iter()
            .find(|file| self.tsconfigs.contains_key(file))
    }
}

/// What the files of a chain of `extends` set so far.
#[derive(Default)]
struct Found {
    /// The patterns of `paths`, and the directory of the file that holds them.
    paths: Option<(Patterns, String)>,
    base_url: Option<String>,
}

/// The paths that the package in `dir` names for `subpath` in its `exports`, or by the
/// directory layout if it has no `exports`.
fn subpath_entries(json: &Value, dir: &str, subpath: &str) -> Vec<String> {
    match json.get("exports") {
        Some(Value::Object(map)) => {
            let keys: Patterns = map
                .iter()
                .filter(|(key, _)| key.starts_with("./"))
                .map(|(key, value)| {
                    let targets = leaves(value).into_iter().map(str::to_string);
                    (key.clone(), targets.collect())
                })
                .collect();
            let wanted = format!("./{subpath}");
            let Some((targets, matched)) = best_pattern(&keys, &wanted) else {
                return Vec::new();
            };
            targets
                .iter()
                .filter_map(|target| join(dir, &target.replace('*', matched)))
                .collect()
        }
        Some(_) => Vec::new(),
        None => join(dir, subpath).into_iter().collect(),
    }
}

/// The files that the entry of a package is: its `exports`, else its `types`, `typings`,
/// `module` or `main`, else `index`.
fn entry_files(json: &Value) -> Vec<&str> {
    match json.get("exports") {
        Some(Value::Object(map)) if map.keys().any(|key| key.starts_with('.')) => {
            map.get(".").map(leaves).unwrap_or_default()
        }
        Some(exports) => leaves(exports),
        None => {
            let fields: Vec<&str> = ["types", "typings", "module", "main"]
                .iter()
                .filter_map(|field| json.get(field)?.as_str())
                .collect();
            if fields.is_empty() {
                vec!["index"]
            } else {
                fields
            }
        }
    }
}

impl Found {
    /// Takes the `paths` and the `baseUrl` of the configuration `json` in `path`, for what
    /// is not set yet.
    fn take_from(&mut self, json: &Value, path: &str) {
        let compiler = json.get("compilerOptions");
        if self.paths.is_none()
            && let Some(paths) = compiler.and_then(|c| c.get("paths")?.as_object())
        {
            self.paths = Some((patterns(paths), dir_of(path).to_string()));
        }
        if self.base_url.is_none() {
            self.base_url = compiler
                .and_then(|c| c.get("baseUrl")?.as_str())
                .and_then(|base_url| join(dir_of(path), base_url));
        }
    }
}

/// The patterns of a `paths` object with their targets.
fn patterns(paths: &serde_json::Map<String, Value>) -> Patterns {
    paths
        .iter()
        .map(|(pattern, targets)| {
            let targets = targets
                .as_array()
                .map(|targets| {
                    targets
                        .iter()
                        .filter_map(|t| t.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            (pattern.clone(), targets)
        })
        .collect()
}

/// The targets of the pattern that fits `specifier` best, with the part that its `*`
/// matches: a pattern without a star beats every pattern with one, else the longest prefix
/// before the star wins.
pub fn best_pattern<'p, 's>(
    patterns: &'p [(String, Vec<String>)],
    specifier: &'s str,
) -> Option<(&'p [String], &'s str)> {
    patterns
        .iter()
        .filter_map(|(pattern, targets)| {
            let matched = match_pattern(pattern, specifier)?;
            let prefix = pattern.find('*').unwrap_or(usize::MAX);
            Some((prefix, targets.as_slice(), matched))
        })
        .max_by_key(|(prefix, _, _)| *prefix)
        .map(|(_, targets, matched)| (targets, matched))
}

/// The part of `specifier` that the `*` of `pattern` matches (empty for a pattern without
/// a star that equals `specifier`); `None` if the pattern does not match.
fn match_pattern<'s>(pattern: &str, specifier: &'s str) -> Option<&'s str> {
    match pattern.split_once('*') {
        None => (pattern == specifier).then_some(""),
        Some((prefix, suffix)) => specifier
            .strip_prefix(prefix)?
            .strip_suffix(suffix)
            // Prefix and suffix must not overlap.
            .filter(|_| specifier.len() >= prefix.len() + suffix.len()),
    }
}

/// All strings in a value, through nested objects and arrays.
fn leaves(value: &Value) -> Vec<&str> {
    match value {
        Value::String(string) => vec![string.as_str()],
        Value::Array(items) => items.iter().flat_map(leaves).collect(),
        Value::Object(map) => map.values().flat_map(leaves).collect(),
        _ => Vec::new(),
    }
}

fn is_tsconfig(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    name == "tsconfig.json" || (name.starts_with("tsconfig.") && name.ends_with(".json"))
}

/// JSON with `//` and `/* */` comments and trailing commas. A text that still does not
/// parse counts as an empty object.
fn parse_lenient(text: &str) -> Value {
    serde_json::from_str(&strip(without_bom(text))).unwrap_or_else(|_| empty_object())
}

/// `text` without a byte order mark at its start, which JSON does not allow.
pub fn without_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

fn empty_object() -> Value {
    Value::Object(Default::default())
}

/// `text` without comments and without commas before `}` or `]`.
fn strip(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        i = match (chars[i], chars.get(i + 1)) {
            ('"', _) => copy_string(&chars, i, &mut out),
            ('/', Some('/')) => skip_line_comment(&chars, i),
            ('/', Some('*')) => skip_block_comment(&chars, i),
            (',', _) => copy_comma(&chars, i, &mut out),
            (c, _) => {
                out.push(c);
                i + 1
            }
        };
    }
    out
}

/// Copies the string that starts at `i` up to its closing quote; returns the index after it.
fn copy_string(chars: &[char], i: usize, out: &mut String) -> usize {
    out.push('"');
    let mut i = i + 1;
    while i < chars.len() && chars[i] != '"' {
        if chars[i] == '\\' && i + 1 < chars.len() {
            out.push('\\');
            i += 1;
        }
        out.push(chars[i]);
        i += 1;
    }
    if i < chars.len() {
        out.push('"');
        i += 1;
    }
    i
}

/// The index of the line break that ends the `//` comment at `i`.
fn skip_line_comment(chars: &[char], i: usize) -> usize {
    chars[i..]
        .iter()
        .position(|&c| c == '\n')
        .map_or(chars.len(), |end| i + end)
}

/// The index after the `/* */` comment at `i`.
fn skip_block_comment(chars: &[char], i: usize) -> usize {
    let mut i = i + 2;
    while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
        i += 1;
    }
    i + 2
}

/// The first character at or after `i` that is neither white space nor in a comment.
fn next_token(chars: &[char], mut i: usize) -> Option<char> {
    loop {
        match (chars.get(i)?, chars.get(i + 1)) {
            ('/', Some('/')) => i = skip_line_comment(chars, i),
            ('/', Some('*')) => i = skip_block_comment(chars, i),
            (c, _) if c.is_whitespace() => i += 1,
            (c, _) => return Some(*c),
        }
    }
}

/// Copies the comma at `i` unless a `}` or `]` follows it; returns the next index.
fn copy_comma(chars: &[char], i: usize, out: &mut String) -> usize {
    if !matches!(next_token(chars, i + 1), Some('}' | ']')) {
        out.push(',');
    }
    i + 1
}
