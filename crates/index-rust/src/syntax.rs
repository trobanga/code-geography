//! Small helpers on tree-sitter nodes.

use tree_sitter::Node as Ts;

pub fn named_children(node: Ts) -> Vec<Ts> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

pub fn text<'a>(node: Ts, source: &'a str) -> &'a str {
    &source[node.byte_range()]
}

/// The `#[...]` items right above an item, nearest first. Comments between them do not
/// interrupt the run.
pub fn attribute_items(item: Ts) -> Vec<Ts> {
    let mut found = Vec::new();
    let mut previous = item.prev_named_sibling();
    while let Some(node) = previous {
        match node.kind() {
            "attribute_item" => found.push(node),
            "line_comment" | "block_comment" => {}
            _ => break,
        }
        previous = node.prev_named_sibling();
    }
    found
}

/// The path of the attribute in an `attribute_item`, for example `cfg`.
pub fn attribute_path<'a>(attribute_item: Ts, source: &'a str) -> &'a str {
    attribute_of(attribute_item)
        .and_then(|a| named_children(a).first().map(|n| text(*n, source)))
        .unwrap_or("")
}

fn attribute_of(attribute_item: Ts) -> Option<Ts> {
    named_children(attribute_item)
        .into_iter()
        .find(|c| c.kind() == "attribute")
}

/// The attributes of an item as `(path, arguments)`, for example `("cfg", "(test)")`.
pub fn attributes<'a>(item: Ts, source: &'a str) -> Vec<(&'a str, &'a str)> {
    attribute_items(item)
        .into_iter()
        .filter_map(attribute_of)
        .map(|attribute| {
            let path = named_children(attribute)
                .first()
                .map_or("", |n| text(*n, source));
            let arguments = attribute
                .child_by_field_name("arguments")
                .map_or("", |n| text(n, source));
            (path, arguments)
        })
        .collect()
}

/// Where the item starts: its first attribute counts, a comment does not.
pub fn start_of(item: Ts) -> (usize, usize) {
    let first = attribute_items(item).last().copied().unwrap_or(item);
    (first.start_byte(), first.start_position().row)
}

/// Whether the item is compiled for tests only: `#[cfg(test)]`, `#[cfg(all(test, ..))]`
/// or `#[test]`. A `not` anywhere in the condition rules it out.
pub fn is_test_item(item: Ts, source: &str) -> bool {
    attributes(item, source)
        .into_iter()
        .any(|(path, arguments)| {
            path == "test"
                || (path == "cfg"
                    && !arguments.contains("not")
                    && arguments
                        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
                        .any(|word| word == "test"))
        })
}

/// The segments of a path as written, without generic arguments: `a::b::C` gives
/// `["a", "b", "C"]`. `None` for a path that starts at a qualified type
/// (`<T as Trait>::X`) or a macro variable.
pub fn path_of<'a>(node: Ts, source: &'a str) -> Option<Vec<&'a str>> {
    match node.kind() {
        "identifier" | "type_identifier" | "crate" | "self" | "super" => {
            Some(vec![text(node, source)])
        }
        "generic_type" | "generic_type_with_turbofish" => {
            path_of(node.child_by_field_name("type")?, source)
        }
        "scoped_identifier" | "scoped_type_identifier" => {
            let mut path = path_of(node.child_by_field_name("path")?, source)?;
            path.push(text(node.child_by_field_name("name")?, source));
            Some(path)
        }
        _ => None,
    }
}
