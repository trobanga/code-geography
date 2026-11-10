//! Browser binding of the graph cut. `Explorer` holds one graph and answers
//! `view` calls with the JSON of a `cg_core::View`.

use cg_core::Graph;
use std::collections::HashSet;
use wasm_bindgen::prelude::*;

/// A graph and its variant without the test nodes, ready to be cut.
pub struct Explorer {
    all: Graph,
    without_tests: Graph,
}

impl Explorer {
    pub fn from_json(graph_json: &str) -> Result<Explorer, String> {
        let all: Graph =
            serde_json::from_str(graph_json).map_err(|e| format!("invalid graph JSON: {e}"))?;
        let without_tests = all.without_tests();
        Ok(Explorer { all, without_tests })
    }

    pub fn view_json(&self, expanded: &[String], include_tests: bool) -> String {
        let graph = if include_tests {
            &self.all
        } else {
            &self.without_tests
        };
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

    pub fn view(&self, expanded: Vec<String>, include_tests: bool) -> String {
        self.0.view_json(&expanded, include_tests)
    }
}
