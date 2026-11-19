//! Small helpers on tree-sitter nodes.

use tree_sitter::Node as Ts;

pub fn named_children(node: Ts) -> Vec<Ts> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

pub fn text<'a>(node: Ts, source: &'a str) -> &'a str {
    &source[node.byte_range()]
}

/// The names of the namespace `node` (`internal_module`, `module`): `A.B` gives `A` and
/// `B`. `None` for `declare module "x"`, which declares things outside the file.
pub fn namespace_parts<'a>(node: Ts, source: &'a str) -> Option<Vec<&'a str>> {
    let name = node
        .child_by_field_name("name")
        .filter(|n| n.kind() != "string")?;
    Some(text(name, source).split('.').map(str::trim).collect())
}
