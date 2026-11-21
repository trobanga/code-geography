//! Browser binding of the graph cut. `Explorer` holds one graph and answers
//! `view` calls with the JSON of a `cg_core::View`.

use cg_core::{Focus, Graph};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
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
    Ok(Worktree::index(paths, texts, root_name)?.json())
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

/// A tree of a commit with the index of its inputs.
struct Indexed {
    root_name: String,
    files: Vec<cg_index::SourceFile>,
    graph: Graph,
    report: cg_index::Report,
}

impl Indexed {
    fn texts(&self) -> serde_json::Map<String, Value> {
        self.files
            .iter()
            .map(|f| (f.path.clone(), f.text.clone().into()))
            .collect()
    }
}

/// The index of the working tree of a chosen directory: made once, shown at once, and the
/// head of the overlay against `HEAD` (`LocalRepo::worktree_overlay`), which does not index
/// it again.
pub struct Worktree(Rc<Indexed>);

impl Worktree {
    /// Indexes the inputs: `paths[i]` holds `texts[i]` (as for `index_sources`).
    pub fn index(paths: &[String], texts: &[String], root_name: &str) -> Result<Worktree, String> {
        let files = source_files(paths, texts, "paths", "texts")?;
        Ok(Worktree(Rc::new(index_files(root_name, files, &mut 0))))
    }

    /// The JSON `{graph, report}` that `index_sources` gives for the same input.
    pub fn json(&self) -> String {
        serde_json::to_string(&json!({ "graph": self.0.graph, "report": self.0.report }))
            .expect("an index serializes to JSON")
    }
}

/// A local repository (ADR 0039): the objects of a `.git` and the trees that were indexed
/// by the last overlay (at most two), so that stepping down a list of commits indexes one
/// tree per step: the head of one step is the base of the next. The cache is by tree id
/// and root name.
pub struct LocalRepo {
    repo: cg_git::Repository,
    cache: HashMap<cg_git::Id, Rc<Indexed>>,
}

impl LocalRepo {
    pub fn new(repo: cg_git::Repository) -> LocalRepo {
        LocalRepo {
            repo,
            cache: HashMap::new(),
        }
    }

    /// The repository, to add refs and objects before the first overlay.
    pub fn repo_mut(&mut self) -> &mut cg_git::Repository {
        &mut self.repo
    }

    /// The first-parent history from `from` (a name `resolve` takes; default `HEAD`), at
    /// most `n` commits, as a JSON array of `{sha, parents, title, author, date, message}`:
    /// the `Commit` of `web/src/lib/forge.ts`. The next page starts at the first parent of
    /// the last commit listed.
    pub fn log(&self, from: Option<&str>, n: usize) -> Result<String, String> {
        let from = self.resolve_commit(from.unwrap_or("HEAD"))?;
        let commits: Vec<Value> = self
            .repo
            .first_parents(&from, n)?
            .into_iter()
            .map(|(id, commit)| {
                json!({
                    "sha": id.to_string(),
                    "parents": commit.parents.iter().map(|p| p.to_string()).collect::<Vec<_>>(),
                    "title": commit.message.lines().next().unwrap_or(""),
                    "author": commit.author,
                    "date": commit.date,
                    "message": commit.message,
                })
            })
            .collect();
        serde_json::to_string(&commits).map_err(|e| e.to_string())
    }

    /// The full id of the commit that `name` names: a ref, `HEAD`, a full or an abbreviated
    /// id. An object that is no commit is an error.
    pub fn resolve(&self, name: &str) -> Result<String, String> {
        self.resolve_commit(name).map(|id| id.to_string())
    }

    fn resolve_commit(&self, name: &str) -> Result<cg_git::Id, String> {
        let id = self.repo.resolve(name)?;
        self.repo
            .commit(&id)
            .map_err(|_| format!("{name} is no commit"))?;
        Ok(id)
    }

    /// The files of the tree `tree` that the walk of a directory would read and an indexer
    /// wants, with their texts as the browser reads a file.
    fn inputs_of(&self, tree: &cg_git::Id) -> Result<Vec<cg_index::SourceFile>, String> {
        self.repo
            .files(tree, &walk_lists, &is_input)?
            .into_iter()
            .map(|(path, id)| {
                let text = blob_text(&self.repo.blob(&id)?);
                Ok(cg_index::SourceFile { path, text })
            })
            .collect()
    }

    /// The index of the inputs of `tree`: from the cache, else `reuse` when it holds the same
    /// sources, else made (`runs` counts it).
    fn indexed(
        &self,
        tree: &cg_git::Id,
        root_name: &str,
        runs: &mut u32,
        reuse: Option<&Rc<Indexed>>,
    ) -> Result<Rc<Indexed>, String> {
        if let Some(found) = self.cache.get(tree).filter(|i| i.root_name == root_name) {
            return Ok(Rc::clone(found));
        }
        let files = self.inputs_of(tree)?;
        if let Some(ready) = reuse.filter(|r| cg_index::same_sources(&files, &r.files)) {
            return Ok(Rc::clone(ready));
        }
        Ok(Rc::new(index_files(root_name, files, runs)))
    }

    /// The overlay of the commit `head` against the commit `base` (None: against nothing, for
    /// a root commit), as JSON `{graph, report, base, head, indexes}`: `base` and `head` map
    /// the path of each input at that commit to its text, for the source view; `indexes` is
    /// how many times the indexers ran for this call (a tree that the last overlay indexed is
    /// not indexed again). The names are the first 7 digits of the ids (`none` for no base).
    /// The report is the one of the head.
    pub fn commit_overlay(
        &mut self,
        base: Option<&str>,
        head: &str,
        root_name: &str,
    ) -> Result<String, String> {
        let head_id = self.resolve_commit(head)?;
        let base_id = base.map(|name| self.resolve_commit(name)).transpose()?;
        let head_tree = self.repo.commit(&head_id)?.tree;
        let base_tree = base_id
            .map(|id| self.repo.commit(&id).map(|c| c.tree))
            .transpose()?;
        let mut runs = 0;
        let after = self.indexed(&head_tree, root_name, &mut runs, None)?;
        let before = match &base_tree {
            Some(tree) if *tree == head_tree => Rc::clone(&after),
            Some(tree) => self.indexed(tree, root_name, &mut runs, None)?,
            None => Rc::new(index_files(root_name, Vec::new(), &mut runs)),
        };
        let changed = self
            .repo
            .changes(base_tree.as_ref(), &head_tree)?
            .into_iter()
            .map(|(path, change)| {
                let change = match change {
                    cg_git::Change::Added => cg_core::Change::Added,
                    cg_git::Change::Removed => cg_core::Change::Removed,
                    cg_git::Change::Modified => cg_core::Change::Modified,
                };
                (path, change)
            })
            .collect();
        let spec = cg_index::DiffSpec {
            base: base_id.map_or("none".to_string(), |id| id.to_hex_with_len(7).to_string()),
            head: head_id.to_hex_with_len(7).to_string(),
            changed,
        };
        let graph = cg_index::diff_indexed(
            (&before.files, &before.graph),
            (&after.files, &after.graph),
            spec,
        );
        let answer = serde_json::to_string(&json!({
            "graph": graph,
            "report": after.report,
            "base": before.texts(),
            "head": after.texts(),
            "indexes": runs,
        }))
        .map_err(|e| e.to_string())?;
        self.cache.clear();
        self.cache.insert(head_tree, after);
        if let Some(tree) = base_tree {
            self.cache.insert(tree, before);
        }
        Ok(answer)
    }

    /// The overlay of the working tree against `HEAD` (ADR 0039): the inputs of the commit
    /// at `HEAD` are the base, the index of `worktree` (the inputs that the walk read) the
    /// head, which is not indexed again. A file is changed when its text differs; an input
    /// that `HEAD` lacks is `added`, also when it is not tracked. JSON `{graph, report, base,
    /// indexes}` like `diff_sources`; `base` maps the path of each input at `HEAD` to its
    /// text there, for the source view; `indexes` is as for `commit_overlay` (the index of
    /// `worktree` is not counted; a `HEAD` with the sources of the working tree takes its
    /// index and runs nothing).
    pub fn worktree_overlay(&mut self, worktree: &Worktree) -> Result<String, String> {
        let head = &worktree.0;
        let commit = self
            .repo
            .resolve("HEAD")
            .map_err(|e| format!("no commit at HEAD: {e}"))?;
        let tree = self.repo.commit(&commit)?.tree;
        let mut runs = 0;
        let before = self.indexed(&tree, &head.root_name, &mut runs, Some(head))?;
        let spec = cg_index::DiffSpec {
            base: commit.to_hex_with_len(7).to_string(),
            head: "working tree".to_string(),
            changed: text_changes(&before.files, &head.files),
        };
        let overlay = cg_index::diff_indexed(
            (&before.files, &before.graph),
            (&head.files, &head.graph),
            spec,
        );
        let answer = serde_json::to_string(&json!({
            "graph": overlay,
            "report": head.report,
            "base": before.texts(),
            "indexes": runs,
        }))
        .map_err(|e| e.to_string())?;
        self.cache.clear();
        self.cache.insert(tree, before);
        Ok(answer)
    }
}

/// Indexes `files`; `runs` counts it.
fn index_files(root_name: &str, files: Vec<cg_index::SourceFile>, runs: &mut u32) -> Indexed {
    *runs += 1;
    let (graph, report) = cg_index::index_sources(root_name, &files);
    Indexed {
        root_name: root_name.to_string(),
        files,
        graph,
        report,
    }
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
pub struct JsGitRepo(LocalRepo);

#[wasm_bindgen(js_class = GitRepo)]
impl JsGitRepo {
    /// `new GitRepo(head)`: `head` is the text of `.git/HEAD`.
    #[wasm_bindgen(constructor)]
    pub fn new(head: &str) -> JsGitRepo {
        JsGitRepo(LocalRepo::new(cg_git::Repository::new(head)))
    }

    /// `addRef(name, text)`: a file below `.git/refs`, `name` from `.git` (`refs/heads/main`).
    #[wasm_bindgen(js_name = addRef)]
    pub fn add_ref(&mut self, name: &str, text: &str) {
        self.0.repo_mut().add_ref(name, text);
    }

    /// `setPackedRefs(text)`: the text of `.git/packed-refs`.
    #[wasm_bindgen(js_name = setPackedRefs)]
    pub fn set_packed_refs(&mut self, text: &str) {
        self.0.repo_mut().set_packed_refs(text);
    }

    /// `addPack(index, data)`: a `.idx` file and its `.pack` file; throws on a broken pack.
    #[wasm_bindgen(js_name = addPack)]
    pub fn add_pack(&mut self, index: Vec<u8>, data: Vec<u8>) -> Result<(), JsError> {
        self.0
            .repo_mut()
            .add_pack(index, data)
            .map_err(|e| JsError::new(&e))
    }

    /// `addLoose(id, bytes)`: the file `.git/objects/xx/yyyy…` with `id` = `xxyyyy…`.
    #[wasm_bindgen(js_name = addLoose)]
    pub fn add_loose(&mut self, id: &str, bytes: Vec<u8>) -> Result<(), JsError> {
        self.0
            .repo_mut()
            .add_loose(id, bytes)
            .map_err(|e| JsError::new(&e))
    }

    /// `log(from, n)`: JSON array of `{sha, parents, title, author, date, message}` for the
    /// first-parent history of `from` (default `HEAD`), at most `n` commits.
    pub fn log(&self, from: Option<String>, n: usize) -> Result<String, JsError> {
        self.0.log(from.as_deref(), n).map_err(|e| JsError::new(&e))
    }

    /// `resolve(name)`: the full id of the commit that `name` names (a ref, `HEAD`, a full or
    /// an abbreviated id); throws for anything else.
    pub fn resolve(&self, name: &str) -> Result<String, JsError> {
        self.0.resolve(name).map_err(|e| JsError::new(&e))
    }

    /// `commitOverlay(base, head, rootName)`: JSON of `{graph, report, base, head, indexes}`
    /// with the overlay of the commit `head` against `base` (`undefined`: a root commit).
    #[wasm_bindgen(js_name = commitOverlay)]
    pub fn commit_overlay(
        &mut self,
        base: Option<String>,
        head: &str,
        root_name: &str,
    ) -> Result<String, JsError> {
        self.0
            .commit_overlay(base.as_deref(), head, root_name)
            .map_err(|e| JsError::new(&e))
    }
}

/// The JavaScript class `Worktree`: the index of the working tree of a chosen directory
/// (see `Worktree` above); `free()` drops it.
#[wasm_bindgen(js_name = Worktree)]
pub struct JsWorktree(Worktree);

#[wasm_bindgen(js_class = Worktree)]
impl JsWorktree {
    /// `new Worktree(paths, texts, rootName)`: indexes the inputs; throws when `paths` and
    /// `texts` differ in length.
    #[wasm_bindgen(constructor)]
    pub fn new(
        paths: Vec<String>,
        texts: Vec<String>,
        root_name: &str,
    ) -> Result<JsWorktree, JsError> {
        Worktree::index(&paths, &texts, root_name)
            .map(JsWorktree)
            .map_err(|message| JsError::new(&message))
    }

    /// `json()`: JSON of `{graph, report}`, as `indexSources` gives it.
    pub fn json(&self) -> String {
        self.0.json()
    }
}

/// `worktreeOverlay(repo, worktree)`: JSON of `{graph, report, base, indexes}` with the
/// overlay of the working tree against `HEAD`; throws when git cannot give `HEAD`.
#[wasm_bindgen(js_name = worktreeOverlay)]
pub fn js_worktree_overlay(repo: &mut JsGitRepo, worktree: &JsWorktree) -> Result<String, JsError> {
    repo.0
        .worktree_overlay(&worktree.0)
        .map_err(|message| JsError::new(&message))
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
