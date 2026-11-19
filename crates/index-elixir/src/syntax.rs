//! Reading the tree that the grammar gives: calls, aliases, keyword pairs. What a call
//! means is decided by the callers.

use tree_sitter::Node as Ts;

pub fn text<'a>(node: Ts, source: &'a str) -> &'a str {
    &source[node.byte_range()]
}

/// The named children of a node that are forms of their own: no comment.
pub fn forms(node: Ts) -> Vec<Ts> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter(|c| c.kind() != "comment")
        .collect()
}

/// The identifier that a call names (`defmodule`, `def`, `alias`): `None` for a call of a
/// remote function (`Foo.bar`), of an anonymous function or of the result of a call.
pub fn call_head<'a>(node: Ts, source: &'a str) -> Option<&'a str> {
    if node.kind() != "call" {
        return None;
    }
    let target = node.child_by_field_name("target")?;
    (target.kind() == "identifier").then(|| text(target, source))
}

/// The node that holds the arguments of a call.
fn arguments_node(call: Ts) -> Option<Ts> {
    let mut cursor = call.walk();
    call.named_children(&mut cursor)
        .find(|c| c.kind() == "arguments")
}

/// The arguments of a call, in order, without the `do` block.
pub fn arguments(call: Ts) -> Vec<Ts> {
    arguments_node(call).map(forms).unwrap_or_default()
}

/// Whether `node` has the operator `op` (`a when b`, `&f`).
pub fn is_operator(node: Ts, source: &str, op: &str) -> bool {
    node.child_by_field_name("operator")
        .is_some_and(|o| text(o, source) == op)
}

/// The head of a function form without its guards: `f(a)` of `f(a) when g(a)`. `None` if a
/// guarded head has no left side.
fn unguarded<'t>(mut head: Ts<'t>, source: &str) -> Option<Ts<'t>> {
    while head.kind() == "binary_operator" && is_operator(head, source, "when") {
        head = head.child_by_field_name("left")?;
    }
    Some(head)
}

/// The name and the arity that the head of a function form declares: the first argument
/// of `def`, without its guards. `None` if the name is not written (`def unquote(n)(x)`).
pub fn function_head<'a>(head: Ts, source: &'a str) -> Option<(&'a str, usize)> {
    let head = unguarded(head, source)?;
    match head.kind() {
        "identifier" => Some((text(head, source), 0)),
        "binary_operator" | "unary_operator" => operator_head(head, source),
        _ => Some((
            call_head(head, source).filter(|n| *n != "unquote")?,
            arguments(head).len(),
        )),
    }
}

/// An operator defined as a function: `a + b` has the name `+` and the arity 2.
fn operator_head<'a>(head: Ts, source: &'a str) -> Option<(&'a str, usize)> {
    let arity = if head.kind() == "binary_operator" {
        2
    } else {
        1
    };
    Some((text(head.child_by_field_name("operator")?, source), arity))
}

/// The number of parameters that the head of a function form gives a default (`b \\ 1`).
pub fn default_count(head: Ts, source: &str) -> usize {
    let Some(head) = unguarded(head, source) else {
        return 0;
    };
    if head.kind() != "call" {
        return 0;
    }
    arguments(head)
        .into_iter()
        .filter(|a| a.kind() == "binary_operator" && is_operator(*a, source, "\\\\"))
        .count()
}

/// The content of a string literal without interpolation.
pub fn plain_string<'a>(node: Ts, source: &'a str) -> Option<&'a str> {
    if node.kind() != "string" {
        return None;
    }
    let mut cursor = node.walk();
    let plain = node
        .children(&mut cursor)
        .all(|c| c.kind() != "interpolation");
    let raw = text(node, source);
    plain
        .then(|| raw.strip_prefix('"')?.strip_suffix('"'))
        .flatten()
}

/// The keyword pairs that end the arguments of a call (`as: C`, `do: x`).
pub fn options<'t, 'a>(call: Ts<'t>, source: &'a str) -> Vec<(&'a str, Ts<'t>)> {
    let Some(arguments) = arguments_node(call) else {
        return Vec::new();
    };
    let mut cursor = arguments.walk();
    let Some(keywords) = arguments
        .named_children(&mut cursor)
        .find(|c| c.kind() == "keywords")
    else {
        return Vec::new();
    };
    let mut cursor = keywords.walk();
    keywords
        .named_children(&mut cursor)
        .filter_map(|p| pair(p, source))
        .collect()
}

/// The forms of each block of a call: the `do` block, and the `else` and similar blocks
/// inside it, or the `do:` and `else:` options. The forms of a `do` block come first.
pub fn blocks<'t>(call: Ts<'t>, source: &str) -> Vec<Vec<Ts<'t>>> {
    let mut out = Vec::new();
    let mut cursor = call.walk();
    for child in call.named_children(&mut cursor) {
        if child.kind() != "do_block" {
            continue;
        }
        let (blocks, own): (Vec<_>, Vec<_>) = forms(child)
            .into_iter()
            .partition(|c| c.kind().ends_with("_block"));
        out.push(own);
        out.extend(blocks.into_iter().map(forms));
    }
    for (key, value) in options(call, source) {
        if matches!(key, "do" | "else") {
            out.push(if value.kind() == "block" {
                forms(value)
            } else {
                vec![value]
            });
        }
    }
    out
}

/// The line on which `node` starts: 1-based.
pub fn line_of(node: Ts) -> u32 {
    node.start_position().row as u32 + 1
}

/// The line on which `node` ends: 1-based.
pub fn end_line_of(node: Ts) -> u32 {
    node.end_position().row as u32 + 1
}

/// The node and all nodes below it, in document order.
pub fn descendants(node: Ts) -> Vec<Ts> {
    let mut out = Vec::new();
    collect(node, &mut out);
    out
}

fn collect<'t>(node: Ts<'t>, out: &mut Vec<Ts<'t>>) {
    out.push(node);
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect(child, out);
    }
}

/// The key and the value of a keyword pair (`app: :x`); the key without its colon. A
/// quoted key (`"a b": 1`) is no key.
pub fn pair<'t, 'a>(node: Ts<'t>, source: &'a str) -> Option<(&'a str, Ts<'t>)> {
    if node.kind() != "pair" {
        return None;
    }
    let key = node.child_by_field_name("key")?;
    if key.kind() != "keyword" {
        return None;
    }
    let name = text(key, source).trim_end().strip_suffix(':')?;
    Some((name, node.child_by_field_name("value")?))
}
