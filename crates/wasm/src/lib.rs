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
    ) -> String {
        let graph = &self.variants[variant(include_tests, merge_imports)];
        let expanded: HashSet<String> = expanded.iter().cloned().collect();
        let focus = Focus {
            nodes: focus_nodes.iter().cloned().collect(),
            changes: focus_changes,
            neighbors,
            unfolded: unfolded.iter().cloned().collect(),
        };
        serde_json::to_string(&graph.cut_focused(&expanded, &focus))
            .expect("a view serializes to JSON")
    }
}

/// Whether the Java indexer wants the file at `path`. See `cg_index_java::is_input`.
pub fn is_input(path: &str) -> bool {
    cg_index_java::is_input(path)
}

/// Indexes Java sources that are given as data: `paths[i]` (relative to the root, with
/// `/`) holds `texts[i]`. Returns the JSON `{"graph": ..., "report": ...}`.
pub fn index_java(paths: &[String], texts: &[String], root_name: &str) -> Result<String, String> {
    if paths.len() != texts.len() {
        return Err(format!("{} paths but {} texts", paths.len(), texts.len()));
    }
    let files: Vec<cg_index_java::SourceFile> = paths
        .iter()
        .zip(texts)
        .map(|(path, text)| cg_index_java::SourceFile {
            path: path.clone(),
            text: text.clone(),
        })
        .collect();
    let (graph, report) = cg_index_java::index_sources(root_name, &files);
    serde_json::to_string(&serde_json::json!({ "graph": graph, "report": report }))
        .map_err(|e| e.to_string())
}

/// `isInput(path)`: true for a `.java` file or a `pom.xml` that the indexer wants.
#[wasm_bindgen(js_name = isInput)]
pub fn js_is_input(path: &str) -> bool {
    is_input(path)
}

/// `indexJava(paths, texts, rootName)`: JSON of `{graph, report}`; throws on an error.
#[wasm_bindgen(js_name = indexJava)]
pub fn js_index_java(
    paths: Vec<String>,
    texts: Vec<String>,
    root_name: &str,
) -> Result<String, JsError> {
    index_java(&paths, &texts, root_name).map_err(|message| JsError::new(&message))
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
    ) -> String {
        self.0.focus_view_json(
            &expanded,
            include_tests,
            merge_imports,
            &focus_nodes,
            focus_changes,
            neighbors,
            &unfolded,
        )
    }
}
