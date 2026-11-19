//! Small helpers on tree-sitter nodes.

use tree_sitter::Node as Ts;

pub fn named_children(node: Ts) -> Vec<Ts> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

pub fn text<'a>(node: Ts, source: &'a str) -> &'a str {
    &source[node.byte_range()]
}
