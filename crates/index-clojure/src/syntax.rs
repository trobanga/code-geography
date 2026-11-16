//! Reading the tree that the grammar gives: lists, vectors, symbols, keywords. What a
//! list means is decided by the callers.

use tree_sitter::Node as Ts;

/// The named children of a collection that are forms of their own: no comment, no `#_`
/// form, no metadata (which belongs to the form that follows it).
pub fn items(node: Ts) -> Vec<Ts> {
    (0..node.named_child_count())
        .filter_map(|i| node.named_child(i as u32))
        .filter(|c| {
            !matches!(
                c.kind(),
                "comment" | "dis_expr" | "meta_lit" | "old_meta_lit"
            )
        })
        .collect()
}

/// Like `items`, but a reader conditional is replaced by the forms of all its branches
/// (`#?(:clj a :cljs b)` gives `a` and `b`; `#?@(:clj [a b])` gives `a` and `b`).
pub fn flat_items(node: Ts) -> Vec<Ts> {
    let mut out = Vec::new();
    for item in items(node) {
        flatten(item, &mut out);
    }
    out
}

fn flatten<'t>(form: Ts<'t>, out: &mut Vec<Ts<'t>>) {
    match form.kind() {
        "read_cond_lit" => {
            for branch in items(form).into_iter().skip(1).step_by(2) {
                flatten(branch, out);
            }
        }
        "splicing_read_cond_lit" => {
            for branch in items(form).into_iter().skip(1).step_by(2) {
                if matches!(branch.kind(), "vec_lit" | "list_lit") {
                    for item in items(branch) {
                        flatten(item, out);
                    }
                } else {
                    flatten(branch, out);
                }
            }
        }
        _ => out.push(form),
    }
}

/// A symbol as `(namespace, name)`, without its metadata.
pub fn symbol<'a>(form: Ts, source: &'a str) -> Option<(Option<&'a str>, &'a str)> {
    if form.kind() != "sym_lit" {
        return None;
    }
    let name = form.child_by_field_name("name")?;
    let namespace = form.child_by_field_name("namespace");
    Some((namespace.map(|n| text(n, source)), text(name, source)))
}

/// The name of a symbol without a namespace.
pub fn plain_symbol<'a>(form: Ts, source: &'a str) -> Option<&'a str> {
    match symbol(form, source)? {
        (None, name) => Some(name),
        _ => None,
    }
}

/// The name of a keyword (`:require` gives `require`), without namespace.
pub fn keyword<'a>(form: Ts, source: &'a str) -> Option<&'a str> {
    if form.kind() != "kwd_lit" {
        return None;
    }
    Some(text(form.child_by_field_name("name")?, source))
}

/// The content of a string literal, as written between the quotes.
pub fn string<'a>(form: Ts, source: &'a str) -> Option<&'a str> {
    if form.kind() != "str_lit" {
        return None;
    }
    let raw = text(form, source);
    raw.strip_prefix('"')?.strip_suffix('"')
}

pub fn text<'a>(node: Ts, source: &'a str) -> &'a str {
    &source[node.byte_range()]
}

/// The line on which `node` starts: 1-based.
pub fn line_of(node: Ts) -> u32 {
    node.start_position().row as u32 + 1
}

/// Whether the metadata of a symbol marks it private (`^:private`, `^{:private true}`).
pub fn private(form: Ts, source: &str) -> bool {
    (0..form.named_child_count())
        .filter_map(|i| form.named_child(i as u32))
        .any(|c| {
            matches!(c.kind(), "meta_lit" | "old_meta_lit") && text(c, source).contains(":private")
        })
}
