//! The `ns` form: the name of the namespace and the namespaces that it loads.

use crate::declare::head;
use crate::syntax::{flat_items, keyword, plain_symbol, symbol};
use tree_sitter::Node as Ts;

/// The first top-level `ns` form among `forms` and the name that it gives.
pub fn namespace<'t>(forms: &[Ts<'t>], source: &str) -> Option<(Ts<'t>, String)> {
    let form = *forms
        .iter()
        .find(|f| head(**f, source).as_deref() == Some("ns"))?;
    let name = *flat_items(form).get(1)?;
    match symbol(name, source)? {
        (None, name) => Some((form, name.to_string())),
        _ => None,
    }
}

/// The namespaces that the `:require` and `:use` clauses of an `ns` form name, as written,
/// in source order. `:as-alias` loads nothing and gives no name. A name that no namespace
/// of the repository has is for the caller to drop.
pub fn requires(ns_form: Ts, source: &str) -> Vec<String> {
    let mut names = Vec::new();
    for clause in flat_items(ns_form).into_iter().skip(2) {
        if clause.kind() != "list_lit" {
            continue;
        }
        let parts = flat_items(clause);
        let Some((head, specs)) = parts.split_first() else {
            continue;
        };
        if matches!(keyword(*head, source), Some("require" | "use")) {
            for spec in specs {
                spec_names(*spec, "", source, &mut names);
            }
        }
    }
    names
}

/// The namespaces of one spec of a `:require`: a symbol, a vector `[ns :as a]`, or a prefix
/// list `(a.b [c :as x] d)`, whose elements are below the prefix.
fn spec_names(spec: Ts, prefix: &str, source: &str, names: &mut Vec<String>) {
    let join = |name: &str| {
        if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}.{name}")
        }
    };
    match spec.kind() {
        "sym_lit" => names.extend(plain_symbol(spec, source).map(join)),
        "vec_lit" => {
            let parts = flat_items(spec);
            let as_alias = parts
                .iter()
                .any(|p| keyword(*p, source) == Some("as-alias"));
            if let Some(name) = parts.first().and_then(|p| plain_symbol(*p, source))
                && !as_alias
            {
                names.push(join(name));
            }
        }
        "list_lit" => {
            let parts = flat_items(spec);
            if let Some((first, rest)) = parts.split_first()
                && let Some(name) = plain_symbol(*first, source)
            {
                let prefix = join(name);
                for part in rest {
                    spec_names(*part, &prefix, source, names);
                }
            }
        }
        _ => {}
    }
}
