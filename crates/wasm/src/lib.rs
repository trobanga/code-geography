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

/// Whether some indexer (Java, Rust) wants the file at `path`. See `cg_index::is_input`.
pub fn is_input(path: &str) -> bool {
    cg_index::is_input(path)
}

/// Indexes sources that are given as data: `paths[i]` (relative to the root, with `/`)
/// holds `texts[i]`. Returns the JSON `{"graph": ..., "report": {"calls": ..., "rust":
/// ..., "clojure": ...}}`.
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
