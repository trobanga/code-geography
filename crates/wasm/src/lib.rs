//! Browser binding of the graph cut. `Explorer` holds one graph and answers
//! `view` calls with the JSON of a `cg_core::View`.

use cg_core::{Focus, Graph};
use std::collections::HashSet;
use wasm_bindgen::prelude::*;

/// A graph and its filtered variants, ready to be cut.
pub struct Explorer {
    /// Indexed by `variant(include_tests, merge_imports)`.
    variants: [Graph; 4],
}

fn variant(include_tests: bool, merge_imports: bool) -> usize {
    usize::from(include_tests) * 2 + usize::from(merge_imports)
}

impl Explorer {
    pub fn from_json(graph_json: &str) -> Result<Explorer, String> {
        let all: Graph =
            serde_json::from_str(graph_json).map_err(|e| format!("invalid graph JSON: {e}"))?;
        let without_tests = all.without_tests();
        // Order as in `variant`.
        Ok(Explorer {
            variants: [
                without_tests.clone(),
                without_tests.without_redundant_imports(),
                all.clone(),
                all.without_redundant_imports(),
            ],
        })
    }

    /// `merge_imports` leaves out each import that a use of the same type covers.
    pub fn view_json(
        &self,
        expanded: &[String],
        include_tests: bool,
        merge_imports: bool,
    ) -> String {
        let graph = &self.variants[variant(include_tests, merge_imports)];
        let expanded: HashSet<String> = expanded.iter().cloned().collect();
        serde_json::to_string(&graph.cut(&expanded)).expect("a view serializes to JSON")
    }

    /// The JSON list of the edges behind a line from the box `from` to the box `to`, in the
    /// variant that `view_json` cuts. See `cg_core::Graph::edges_between`.
    pub fn edges_between_json(
        &self,
        from: &str,
        to: &str,
        include_tests: bool,
        merge_imports: bool,
    ) -> String {
        let graph = &self.variants[variant(include_tests, merge_imports)];
        serde_json::to_string(&graph.edges_between(from, to)).expect("edges serialize to JSON")
    }

    /// Like `view_json`, but the expanded containers fold the children that the
    /// focus does not name. See `cg_core::Graph::cut_focused`.
    #[allow(clippy::too_many_arguments)]
    pub fn focus_view_json(
        &self,
        expanded: &[String],
        include_tests: bool,
        merge_imports: bool,
        focus_nodes: &[String],
        focus_changes: bool,
        neighbors: bool,
        unfolded: &[String],
        fold_mixed: bool,
    ) -> String {
        let graph = &self.variants[variant(include_tests, merge_imports)];
        let expanded: HashSet<String> = expanded.iter().cloned().collect();
        let focus = Focus {
            nodes: focus_nodes.iter().cloned().collect(),
            changes: focus_changes,
            neighbors,
            unfolded: unfolded.iter().cloned().collect(),
            fold_mixed,
        };
        serde_json::to_string(&graph.cut_focused(&expanded, &focus))
            .expect("a view serializes to JSON")
    }
}

/// Whether some indexer (Java, Rust, Clojure, Elixir, TypeScript) wants the file at `path`. See `cg_index::is_input`.
pub fn is_input(path: &str) -> bool {
    cg_index::is_input(path)
}

/// Indexes sources that are given as data: `paths[i]` (relative to the root, with `/`)
/// holds `texts[i]`. Returns the JSON `{"graph": ..., "report": {"calls": ..., "rust":
/// ..., "clojure": ..., "elixir": ..., "typescript": ...}}`.
pub fn index_sources(
    paths: &[String],
    texts: &[String],
    root_name: &str,
) -> Result<String, String> {
    let files = source_files(paths, texts, "paths", "texts")?;
    let (graph, report) = cg_index::index_sources(root_name, &files);
    serde_json::to_string(&serde_json::json!({ "graph": graph, "report": report }))
        .map_err(|e| e.to_string())
}

fn source_files(
    paths: &[String],
    texts: &[String],
    path_label: &str,
    text_label: &str,
) -> Result<Vec<cg_index::SourceFile>, String> {
    if paths.len() != texts.len() {
        return Err(format!(
            "{} {path_label} but {} {text_label}",
            paths.len(),
            texts.len()
        ));
    }
    Ok(paths
        .iter()
        .zip(texts)
        .map(|(path, text)| cg_index::SourceFile {
            path: path.clone(),
            text: text.clone(),
        })
        .collect())
}

/// The commit overlay of two source sets, as JSON `{"graph": ..., "report": ...}` like
/// `index_sources`; the report is the one of the head. `changed_kinds[i]` is `added`,
/// `removed` or `modified` for `changed_paths[i]`. See `cg_index::diff_sources`.
#[allow(clippy::too_many_arguments)]
pub fn diff_sources(
    base_paths: &[String],
    base_texts: &[String],
    head_paths: &[String],
    head_texts: &[String],
    root_name: &str,
    base_name: &str,
    head_name: &str,
    changed_paths: &[String],
    changed_kinds: &[String],
) -> Result<String, String> {
    let base = source_files(base_paths, base_texts, "base paths", "base texts")?;
    let head = source_files(head_paths, head_texts, "head paths", "head texts")?;
    if changed_paths.len() != changed_kinds.len() {
        return Err(format!(
            "{} changed paths but {} changed kinds",
            changed_paths.len(),
            changed_kinds.len()
        ));
    }
    let changed = changed_paths
        .iter()
        .zip(changed_kinds)
        .map(|(path, kind)| {
            serde_json::from_value::<cg_core::Change>(serde_json::Value::String(kind.clone()))
                .map(|change| (path.clone(), change))
                .map_err(|_| format!("unknown kind of change: {kind}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let spec = cg_index::DiffSpec {
        base: base_name.to_string(),
        head: head_name.to_string(),
        changed,
    };
    let (graph, report) = cg_index::diff_sources(root_name, &base, &head, spec);
    serde_json::to_string(&serde_json::json!({ "graph": graph, "report": report }))
        .map_err(|e| e.to_string())
}

/// The overlay of the working tree against `HEAD` (ADR 0039): the inputs of the commit at
/// `HEAD` are the base, `paths` and `texts` (the inputs that the walk read) the head. A file
/// is changed when its text differs; an input that `HEAD` lacks is `added`, also when git
/// does not track it. JSON `{graph, report, base}` like `diff_sources`; `base` maps the
/// path of each input at `HEAD` to its text there, for the source view.
pub fn worktree_overlay(
    repo: &cg_git::Repository,
    paths: &[String],
    texts: &[String],
    root_name: &str,
) -> Result<String, String> {
    let head = source_files(paths, texts, "paths", "texts")?;
    let commit = repo
        .resolve("HEAD")
        .map_err(|e| format!("no commit at HEAD: {e}"))?;
    let tree = repo.commit(&commit)?.tree;
    let base = repo
        .files(&tree, &walk_lists, &is_input)?
        .into_iter()
        .map(|(path, id)| {
            let text = blob_text(&repo.blob(&id)?);
            Ok(cg_index::SourceFile { path, text })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let before: serde_json::Map<String, serde_json::Value> = base
        .iter()
        .map(|f| (f.path.clone(), f.text.clone().into()))
        .collect();
    let spec = cg_index::DiffSpec {
        base: commit.to_hex_with_len(7).to_string(),
        head: "working tree".to_string(),
        changed: text_changes(&base, &head),
    };
    let (graph, report) = cg_index::diff_sources(root_name, &base, &head, spec);
    serde_json::to_string(&serde_json::json!({ "graph": graph, "report": report, "base": before }))
        .map_err(|e| e.to_string())
}

/// Whether the walk of a chosen directory (`web/src/lib/walk.ts`) lists the directory
/// `dir`: the rule of the indexer is asked for a source file in it.
fn walk_lists(dir: &str) -> bool {
    is_input(&format!("{dir}/X.java"))
}

/// The text of a blob as the browser reads a file (`File.text()`): UTF-8 with the
/// invalid bytes replaced, without a byte order mark.
fn blob_text(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    String::from_utf8_lossy(bytes).into_owned()
}

/// The files whose text differs between `base` and `head`, sorted by path.
fn text_changes(
    base: &[cg_index::SourceFile],
    head: &[cg_index::SourceFile],
) -> Vec<(String, cg_core::Change)> {
    use cg_core::Change;
    let before: std::collections::HashMap<&str, &str> = base
        .iter()
        .map(|f| (f.path.as_str(), f.text.as_str()))
        .collect();
    let after: HashSet<&str> = head.iter().map(|f| f.path.as_str()).collect();
    let mut changed: Vec<(String, Change)> = head
        .iter()
        .filter_map(|f| match before.get(f.path.as_str()) {
            None => Some((f.path.clone(), Change::Added)),
            Some(text) if *text != f.text => Some((f.path.clone(), Change::Modified)),
            Some(_) => None,
        })
        .chain(
            base.iter()
                .filter(|f| !after.contains(f.path.as_str()))
                .map(|f| (f.path.clone(), Change::Removed)),
        )
        .collect();
    changed.sort();
    changed
}

/// `isInput(path)`: true for a `.java` file, a `pom.xml`, a `.rs` file or a `Cargo.toml`
/// that an indexer wants.
#[wasm_bindgen(js_name = isInput)]
pub fn js_is_input(path: &str) -> bool {
    is_input(path)
}

/// `indexSources(paths, texts, rootName)`: JSON of `{graph, report}`; throws on an error.
#[wasm_bindgen(js_name = indexSources)]
pub fn js_index_sources(
    paths: Vec<String>,
    texts: Vec<String>,
    root_name: &str,
) -> Result<String, JsError> {
    index_sources(&paths, &texts, root_name).map_err(|message| JsError::new(&message))
}

/// The JavaScript class `Explorer`: only forwards to the plain type above.
#[wasm_bindgen(js_name = Explorer)]
pub struct JsExplorer(Explorer);

#[wasm_bindgen(js_class = Explorer)]
impl JsExplorer {
    #[wasm_bindgen(constructor)]
    pub fn new(graph_json: &str) -> Result<JsExplorer, JsError> {
        Explorer::from_json(graph_json)
            .map(JsExplorer)
            .map_err(|message| JsError::new(&message))
    }

    pub fn view(&self, expanded: Vec<String>, include_tests: bool, merge_imports: bool) -> String {
        self.0.view_json(&expanded, include_tests, merge_imports)
    }

    #[wasm_bindgen(js_name = edgesBetween)]
    pub fn edges_between(
        &self,
        from: &str,
        to: &str,
        include_tests: bool,
        merge_imports: bool,
    ) -> String {
        self.0
            .edges_between_json(from, to, include_tests, merge_imports)
    }

    #[wasm_bindgen(js_name = focusView)]
    #[allow(clippy::too_many_arguments)]
    pub fn focus_view(
        &self,
        expanded: Vec<String>,
        include_tests: bool,
        merge_imports: bool,
        focus_nodes: Vec<String>,
        focus_changes: bool,
        neighbors: bool,
        unfolded: Vec<String>,
        fold_mixed: bool,
    ) -> String {
        self.0.focus_view_json(
            &expanded,
            include_tests,
            merge_imports,
            &focus_nodes,
            focus_changes,
            neighbors,
            &unfolded,
            fold_mixed,
        )
    }
}

/// The JavaScript class `GitRepo`: a repository built from the files of its `.git`
/// (ADR 0039). See `cg_git::Repository`.
#[wasm_bindgen(js_name = GitRepo)]
pub struct JsGitRepo(cg_git::Repository);

#[wasm_bindgen(js_class = GitRepo)]
impl JsGitRepo {
    /// `new GitRepo(head)`: `head` is the text of `.git/HEAD`.
    #[wasm_bindgen(constructor)]
    pub fn new(head: &str) -> JsGitRepo {
        JsGitRepo(cg_git::Repository::new(head))
    }

    /// `addRef(name, text)`: a file below `.git/refs`, `name` from `.git` (`refs/heads/main`).
    #[wasm_bindgen(js_name = addRef)]
    pub fn add_ref(&mut self, name: &str, text: &str) {
        self.0.add_ref(name, text);
    }

    /// `setPackedRefs(text)`: the text of `.git/packed-refs`.
    #[wasm_bindgen(js_name = setPackedRefs)]
    pub fn set_packed_refs(&mut self, text: &str) {
        self.0.set_packed_refs(text);
    }

    /// `addPack(index, data)`: a `.idx` file and its `.pack` file; throws on a broken pack.
    #[wasm_bindgen(js_name = addPack)]
    pub fn add_pack(&mut self, index: Vec<u8>, data: Vec<u8>) -> Result<(), JsError> {
        self.0.add_pack(index, data).map_err(|e| JsError::new(&e))
    }

    /// `addLoose(id, bytes)`: the file `.git/objects/xx/yyyy…` with `id` = `xxyyyy…`.
    #[wasm_bindgen(js_name = addLoose)]
    pub fn add_loose(&mut self, id: &str, bytes: Vec<u8>) -> Result<(), JsError> {
        self.0.add_loose(id, bytes).map_err(|e| JsError::new(&e))
    }
}

/// `worktreeOverlay(repo, paths, texts, rootName)`: JSON of `{graph, report, base}` with the
/// overlay of the working tree against `HEAD`; throws when git cannot give `HEAD`.
#[wasm_bindgen(js_name = worktreeOverlay)]
pub fn js_worktree_overlay(
    repo: &JsGitRepo,
    paths: Vec<String>,
    texts: Vec<String>,
    root_name: &str,
) -> Result<String, JsError> {
    worktree_overlay(&repo.0, &paths, &texts, root_name).map_err(|message| JsError::new(&message))
}

/// `diffSources(basePaths, baseTexts, headPaths, headTexts, rootName, baseName, headName,
/// changedPaths, changedKinds)`: JSON of `{graph, report}` with the overlay graph; throws
/// when two parallel arrays differ in length or a kind is not `added`, `removed` or
/// `modified`.
#[wasm_bindgen(js_name = diffSources)]
#[allow(clippy::too_many_arguments)]
pub fn js_diff_sources(
    base_paths: Vec<String>,
    base_texts: Vec<String>,
    head_paths: Vec<String>,
    head_texts: Vec<String>,
    root_name: &str,
    base_name: &str,
    head_name: &str,
    changed_paths: Vec<String>,
    changed_kinds: Vec<String>,
) -> Result<String, JsError> {
    diff_sources(
        &base_paths,
        &base_texts,
        &head_paths,
        &head_texts,
        root_name,
        base_name,
        head_name,
        &changed_paths,
        &changed_kinds,
    )
    .map_err(|message| JsError::new(&message))
}
