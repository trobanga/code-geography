//! A measurement, not part of the graph: how many unqualified symbols at the head of a
//! list name a function of the repository, and how many of these a local could hide.
//! The answer decides whether a scope analysis is worth building. A later stage replaces
//! this module.

use crate::syntax::{items, plain_symbol};
use serde::Serialize;
use std::collections::HashSet;
use tree_sitter::Node as Ts;

/// Unqualified heads of lists in the code of functions, named as a function of the same
/// namespace or as a function that `:refer` brings from a namespace of the repository.
/// Each `..._maybe_local` is the part whose name occurs in the same top-level form where a
/// local can be bound: a vector element, a symbol of a map in a vector, the name after
/// `as->`'s first argument, after `catch <Class>`, in `(fn name [..])` or in `letfn`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct UnqualifiedHeads {
    pub same_namespace: usize,
    pub same_namespace_maybe_local: usize,
    pub referred: usize,
    pub referred_maybe_local: usize,
}

/// The names in `form` that a local could bind.
pub fn locals(form: Ts, source: &str) -> HashSet<String> {
    let mut names = HashSet::new();
    collect(form, None, source, &mut names);
    names
}

fn collect(form: Ts, parent: Option<&str>, source: &str, names: &mut HashSet<String>) {
    let children = items(form);
    let symbol_at = |i: usize| children.get(i).and_then(|c| plain_symbol(*c, source));
    let mut add = |name: Option<&str>| names.extend(name.map(str::to_string));
    match form.kind() {
        "sym_lit" if parent == Some("vec_lit") => add(plain_symbol(form, source)),
        "map_lit" if parent == Some("vec_lit") => {
            for i in 0..children.len() {
                add(symbol_at(i));
            }
        }
        "list_lit" => match symbol_at(0) {
            Some("as->") => add(symbol_at(2)),
            Some("catch") => add(symbol_at(2)),
            Some("fn") => add(symbol_at(1)),
            Some("letfn") => {
                let bindings = children.get(1).filter(|b| b.kind() == "vec_lit");
                for binding in bindings.iter().flat_map(|b| items(**b)) {
                    add(items(binding)
                        .first()
                        .and_then(|f| plain_symbol(*f, source)));
                }
            }
            _ => {}
        },
        _ => {}
    }
    for child in children {
        collect(child, Some(form.kind()), source, names);
    }
}
