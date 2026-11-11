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

/// One spec of a `:require` or `:use` clause, with the prefix already joined.
pub struct Spec {
    /// The namespace, as written.
    pub name: String,
    /// The name after `:as`.
    pub alias: Option<String>,
    /// The names after `:refer`.
    pub refer: Vec<String>,
    /// `:as-alias`: the namespace is not loaded.
    pub as_alias: bool,
}

/// The specs of the `:require` and `:use` clauses of an `ns` form, in source order.
pub fn specs(ns_form: Ts, source: &str) -> Vec<Spec> {
    let mut found = Vec::new();
    for clause in flat_items(ns_form).into_iter().skip(2) {
        if clause.kind() != "list_lit" {
            continue;
        }
        let parts = flat_items(clause);
        let Some((head, rest)) = parts.split_first() else {
            continue;
        };
        if matches!(keyword(*head, source), Some("require" | "use")) {
            for spec in rest {
                collect(*spec, "", source, &mut found);
            }
        }
    }
    found
}

/// The namespaces that the `:require` and `:use` clauses of an `ns` form name, as written,
/// in source order. `:as-alias` loads nothing and gives no name. A name that no namespace
/// of the repository has is for the caller to drop.
pub fn requires(ns_form: Ts, source: &str) -> Vec<String> {
    specs(ns_form, source)
        .into_iter()
        .filter(|s| !s.as_alias)
        .map(|s| s.name)
        .collect()
}

/// The specs of one element of a `:require`: a symbol, a vector `[ns :as a]`, or a prefix
/// list `(a.b [c :as x] d)`, whose elements are below the prefix.
fn collect(spec: Ts, prefix: &str, source: &str, out: &mut Vec<Spec>) {
    let join = |name: &str| {
        if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}.{name}")
        }
    };
    match spec.kind() {
        "sym_lit" => out.extend(plain_symbol(spec, source).map(|name| Spec {
            name: join(name),
            alias: None,
            refer: Vec::new(),
            as_alias: false,
        })),
        "vec_lit" => {
            let parts = flat_items(spec);
            let Some(name) = parts.first().and_then(|p| plain_symbol(*p, source)) else {
                return;
            };
            // The symbol after an option such as `:as`.
            let after = |key: &str| {
                let at = parts
                    .iter()
                    .position(|p| keyword(*p, source) == Some(key))?;
                plain_symbol(*parts.get(at + 1)?, source).map(str::to_string)
            };
            out.push(Spec {
                name: join(name),
                alias: after("as"),
                refer: parts
                    .iter()
                    .position(|p| keyword(*p, source) == Some("refer"))
                    .and_then(|at| parts.get(at + 1))
                    .filter(|names| names.kind() == "vec_lit")
                    .map(|names| {
                        flat_items(*names)
                            .into_iter()
                            .filter_map(|n| plain_symbol(n, source))
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default(),
                as_alias: parts
                    .iter()
                    .any(|p| keyword(*p, source) == Some("as-alias")),
            });
        }
        "list_lit" => {
            let parts = flat_items(spec);
            if let Some((first, rest)) = parts.split_first()
                && let Some(name) = plain_symbol(*first, source)
            {
                let prefix = join(name);
                for part in rest {
                    collect(*part, &prefix, source, out);
                }
            }
        }
        _ => {}
    }
}
