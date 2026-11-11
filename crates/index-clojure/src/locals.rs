//! The names in a top-level form that a local could bind. The rule is conservative: a name
//! that occurs anywhere in the form at a place where a binder can stand is taken as
//! possibly bound, and no edge is made for it. No model of the binding forms of
//! `clojure.core` or of a macro is needed.

use crate::syntax::{items, keyword, plain_symbol};
use std::collections::HashSet;
use tree_sitter::Node as Ts;

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
        "map_lit" => {
            // A map in a vector is taken as a destructuring form: all its symbols.
            if parent == Some("vec_lit") {
                for i in 0..children.len() {
                    add(symbol_at(i));
                }
            }
            // After `:as` the name, after `:or` the names that get a default. The
            // vectors after `:keys`, `:syms` and `:strs` are vectors like any other.
            for (i, key) in children.iter().enumerate() {
                match keyword(*key, source) {
                    Some("as") => add(symbol_at(i + 1)),
                    Some("or") => {
                        if let Some(defaults) =
                            children.get(i + 1).filter(|d| d.kind() == "map_lit")
                        {
                            for name in items(*defaults).into_iter().step_by(2) {
                                add(plain_symbol(name, source));
                            }
                        }
                    }
                    _ => {}
                }
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
