//! The declarations of a file: the forms at its top level that make nodes.

use crate::syntax::{flat_items, plain_symbol, symbol};
use cg_core::NodeKind;
use std::collections::BTreeMap;
use tree_sitter::Node as Ts;

/// A declaration that becomes a node.
pub struct Decl<'t> {
    pub id: String,
    pub name: String,
    pub kind: NodeKind,
    /// The id of the type that holds the declaration; `None`: the namespace.
    pub parent: Option<String>,
    pub node: Ts<'t>,
    /// The nodes below this one, whose text is not part of the own text.
    pub members: Vec<Ts<'t>>,
}

/// The head of a list form: its first symbol, with the namespace as written.
pub fn head(form: Ts, source: &str) -> Option<String> {
    if form.kind() != "list_lit" {
        return None;
    }
    let first = *flat_items(form).first()?;
    let (namespace, name) = symbol(first, source)?;
    Some(match namespace {
        Some(namespace) => format!("{namespace}/{name}"),
        None => name.to_string(),
    })
}

/// The declarations among the top-level forms `forms` of a file that names the namespace
/// `namespace`. `without_node` counts the forms that give no node, by their head.
pub fn declarations<'t>(
    forms: &[Ts<'t>],
    source: &str,
    namespace: &str,
    without_node: &mut BTreeMap<String, usize>,
) -> Vec<Decl<'t>> {
    let mut decls = Vec::new();
    for form in forms {
        let items = flat_items(*form);
        let head = head(*form, source);
        let made = match head.as_deref() {
            Some("defn" | "defn-" | "defmacro" | "defmulti") => {
                function(&items, *form, source, namespace).map(|f| vec![f])
            }
            Some("defprotocol" | "definterface") => {
                declared_type(&items, *form, source, namespace, 2)
            }
            Some("defrecord" | "deftype") => declared_type(&items, *form, source, namespace, 3),
            _ => None,
        };
        match made {
            Some(made) => decls.extend(made),
            None => {
                let key = match (&head, form.kind()) {
                    (Some(head), _) => head.clone(),
                    (None, "list_lit") => "<list without a symbol head>".to_string(),
                    (None, kind) => format!("<{kind}>"),
                };
                *without_node.entry(key).or_default() += 1;
            }
        }
    }
    decls
}

fn function<'t>(items: &[Ts<'t>], form: Ts<'t>, source: &str, namespace: &str) -> Option<Decl<'t>> {
    let name = plain_symbol(*items.get(1)?, source)?;
    Some(Decl {
        id: format!("fn:{namespace}/{name}"),
        name: name.to_string(),
        kind: NodeKind::Function,
        parent: None,
        node: form,
        members: Vec::new(),
    })
}

/// A type and its methods. The methods are the lists among the items from `first_member`
/// on (after the name, and for a record or type after the vector of fields), whose first
/// item is a symbol: signatures of a protocol or interface, methods of a record or type.
fn declared_type<'t>(
    items: &[Ts<'t>],
    form: Ts<'t>,
    source: &str,
    namespace: &str,
    first_member: usize,
) -> Option<Vec<Decl<'t>>> {
    let name = plain_symbol(*items.get(1)?, source)?;
    let id = format!("type:{namespace}/{name}");
    let methods: Vec<Ts<'t>> = items
        .iter()
        .skip(first_member)
        .copied()
        .filter(|m| m.kind() == "list_lit")
        .collect();
    let mut decls = vec![Decl {
        id: id.clone(),
        name: name.to_string(),
        kind: NodeKind::Type,
        parent: None,
        node: form,
        members: methods.clone(),
    }];
    for method in methods {
        let Some(first) = flat_items(method).first().copied() else {
            continue;
        };
        let Some(method_name) = plain_symbol(first, source) else {
            continue;
        };
        decls.push(Decl {
            id: format!("fn:{namespace}/{name}#{method_name}"),
            name: method_name.to_string(),
            kind: NodeKind::Function,
            parent: Some(id.clone()),
            node: method,
            members: Vec::new(),
        });
    }
    Some(decls)
}
