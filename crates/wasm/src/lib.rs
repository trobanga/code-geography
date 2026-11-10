//! Browser binding of the graph cut. `Explorer` holds one graph and answers
//! `view` calls with the JSON of a `cg_core::View`.

use cg_core::Graph;
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
}
