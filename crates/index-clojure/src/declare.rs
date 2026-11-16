//! The declarations of a file: the forms at its top level that make nodes.

use crate::Report;
use crate::syntax::{flat_items, line_of, plain_symbol, string, symbol, text};
use cg_core::NodeKind;
use std::collections::HashSet;
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
    /// The other forms of a method that a type writes as several forms, one per arity
    /// (`(m [this] ..) (m [this x] ..)`), in source order. `node` is the first.
    pub more_forms: Vec<Ts<'t>>,
    /// Whether the forms of all arities follow one another: then the span of the node
    /// runs from the first to the last, else it is the span of the first.
    pub adjacent: bool,
    /// The declaration also imports its class into the namespace (`defrecord`, `deftype`,
    /// `definterface`), so that the bare class name is valid in the rest of the file.
    pub imports_class: bool,
}

/// A node whose id and parent depend on the declarations of other files: the id is made
/// after all files are read (see `extend`).
pub struct LateDecl<'t> {
    /// `id`, `name` and `parent` are not set.
    pub decl: Decl<'t>,
    pub spec: Spec,
}

/// What a late declaration says, as written.
#[derive(Debug, Clone)]
pub enum Spec {
    /// `(defmethod multi dispatch ..)`: the multi as written (`alias/name` or `name`) and the
    /// text of the dispatch value with each run of whitespace as one space.
    Method { multi: String, dispatch: String },
    /// One method of `extend-protocol` or `extend-type`, for one type and one protocol.
    Extend {
        protocol: String,
        ty: String,
        method: String,
    },
}

/// What `declarations` found.
pub struct Declared<'t> {
    pub decls: Vec<Decl<'t>>,
    pub late: Vec<LateDecl<'t>>,
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

/// A top-level `def` or `defonce` with a name: the name, whether the value is a literal
/// `(fn ..)` or `(fn* ..)` (then the var is a function), and the position of the first item of
/// the value (after the name and an optional docstring); `None` if the name is not a plain
/// symbol. A `def` without value has the position after the last item.
pub fn def_form<'a>(items: &[Ts], source: &'a str) -> Option<(&'a str, bool, usize)> {
    let name = plain_symbol(*items.get(1)?, source)?;
    let value = match items.get(2) {
        Some(doc) if string(*doc, source).is_some() && items.len() > 3 => 3,
        _ => 2,
    };
    let is_fn = items.get(value).is_some_and(|v| {
        v.kind() == "list_lit"
            && flat_items(*v)
                .first()
                .and_then(|h| plain_symbol(*h, source))
                .is_some_and(|h| h == "fn" || h == "fn*")
    });
    Some((name, is_fn, value))
}

/// The declarations among the top-level forms `forms` of a file that names the namespace
/// `namespace`. The report counts the forms that give no node, by their head, and the
/// forms that name a var that an earlier form of the namespace declared.
pub fn declarations<'t>(
    forms: &[Ts<'t>],
    source: &str,
    namespace: &str,
    report: &mut Report,
) -> Declared<'t> {
    let mut decls = Vec::new();
    let mut late = Vec::new();
    // The names of the functions and values of the namespace: the first form that declares a
    // name gives the node.
    let mut claimed: HashSet<&str> = HashSet::new();
    for form in forms {
        let items = flat_items(*form);
        let head = head(*form, source);
        match head.as_deref() {
            Some("defmethod") => {
                match method(&items, *form, source) {
                    Some(made) => late.push(made),
                    None => {
                        *report
                            .forms_without_node
                            .entry("defmethod".to_string())
                            .or_default() += 1;
                    }
                }
                continue;
            }
            Some(kind @ ("extend-protocol" | "extend-type")) => {
                let made = extensions(&items, kind == "extend-protocol", source);
                if made.is_empty() {
                    *report
                        .forms_without_node
                        .entry(kind.to_string())
                        .or_default() += 1;
                }
                late.extend(made);
                continue;
            }
            Some("extend") => report.extend_forms += 1,
            _ => {}
        }
        let made = match head.as_deref() {
            Some("defn" | "defn-" | "defmacro" | "defmulti") => {
                function(&items, *form, source, namespace).map(|f| vec![f])
            }
            Some("def" | "defonce") => def(&items, *form, source, namespace).map(|f| vec![f]),
            Some(kind @ ("defprotocol" | "definterface")) => {
                declared_type(&items, *form, source, namespace, 2, kind == "definterface")
            }
            Some("defrecord" | "deftype") => {
                declared_type(&items, *form, source, namespace, 3, true)
            }
            _ => None,
        };
        match made {
            Some(made) => {
                let var = matches!(
                    head.as_deref(),
                    Some("defn" | "defn-" | "defmacro" | "defmulti" | "def" | "defonce")
                );
                if var && !claimed.insert(plain_symbol(items[1], source).unwrap_or_default()) {
                    *report
                        .duplicate_declarations
                        .entry(head.clone().unwrap_or_default())
                        .or_default() += 1;
                } else {
                    decls.extend(made);
                }
            }
            None => {
                let key = match (&head, form.kind()) {
                    (Some(head), _) => head.clone(),
                    (None, "list_lit") => "<list without a symbol head>".to_string(),
                    (None, kind) => format!("<{kind}>"),
                };
                *report.forms_without_node.entry(key).or_default() += 1;
            }
        }
    }
    Declared { decls, late }
}

/// `(defmethod multi dispatch-value ..)`: a late declaration named by both.
fn method<'t>(items: &[Ts<'t>], form: Ts<'t>, source: &str) -> Option<LateDecl<'t>> {
    let multi = qualified_text(*items.get(1)?, source)?;
    let dispatch = text(*items.get(2)?, source)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    Some(LateDecl {
        decl: late_decl(form, Vec::new(), true),
        spec: Spec::Method { multi, dispatch },
    })
}

/// The type, the protocol and the name of a method, with the forms of the method and their
/// position among the items.
type Extension<'t> = (String, String, String, Vec<(usize, Ts<'t>)>);

/// The methods of an `extend-protocol` (`protocol_first`: `(extend-protocol P T1 (m ..) T2 ..)`)
/// or `extend-type` (`(extend-type T P1 (m ..) P2 ..)`) form. The forms of one method name
/// under one type and protocol are one node, as for a record.
fn extensions<'t>(items: &[Ts<'t>], protocol_first: bool, source: &str) -> Vec<LateDecl<'t>> {
    let Some(first) = items.get(1).and_then(|f| written(*f, source)) else {
        return Vec::new();
    };
    // The item that the first stands for stays; the others change with the symbols.
    let mut current: Option<String> = None;
    let mut found: Vec<Extension<'t>> = Vec::new();
    for (position, item) in items.iter().enumerate().skip(2) {
        if item.kind() == "list_lit" {
            let Some(method) = flat_items(*item)
                .first()
                .and_then(|m| plain_symbol(*m, source))
            else {
                continue;
            };
            let Some(current) = &current else { continue };
            let (ty, protocol) = if protocol_first {
                (current.clone(), first.clone())
            } else {
                (first.clone(), current.clone())
            };
            match found
                .iter_mut()
                .find(|(t, p, m, _)| *t == ty && *p == protocol && m == method)
            {
                Some((_, _, _, forms)) => forms.push((position, *item)),
                None => found.push((ty, protocol, method.to_string(), vec![(position, *item)])),
            }
        } else if let Some(name) = written(*item, source) {
            current = Some(name);
        }
    }
    found
        .into_iter()
        .map(|(ty, protocol, method, forms)| LateDecl {
            decl: late_decl(
                forms[0].1,
                forms[1..].iter().map(|(_, f)| *f).collect(),
                forms.windows(2).all(|w| w[1].0 == w[0].0 + 1),
            ),
            spec: Spec::Extend {
                protocol,
                ty,
                method,
            },
        })
        .collect()
}

/// The pairs of type and protocol that an `extend-protocol` or `extend-type` form names,
/// as written, also for a type without methods. The third is the line (1-based) of the
/// symbol that varies inside the form: the type of an `extend-protocol`, the protocol of an
/// `extend-type`.
pub fn extend_pairs(
    items: &[Ts],
    protocol_first: bool,
    source: &str,
) -> Vec<(String, String, u32)> {
    let Some(first) = items.get(1).and_then(|f| written(*f, source)) else {
        return Vec::new();
    };
    items
        .iter()
        .skip(2)
        .filter_map(|item| Some((written(*item, source)?, line_of(*item))))
        .map(|(other, line)| {
            if protocol_first {
                (other, first.clone(), line)
            } else {
                (first.clone(), other, line)
            }
        })
        .collect()
}

fn late_decl<'t>(node: Ts<'t>, more_forms: Vec<Ts<'t>>, adjacent: bool) -> Decl<'t> {
    Decl {
        id: String::new(),
        name: String::new(),
        kind: NodeKind::Function,
        parent: None,
        node,
        members: Vec::new(),
        more_forms,
        adjacent,
        imports_class: false,
    }
}

/// A symbol as written, with its namespace part.
pub fn qualified_text(form: Ts, source: &str) -> Option<String> {
    let (namespace, name) = symbol(form, source)?;
    Some(match namespace {
        Some(namespace) => format!("{namespace}/{name}"),
        None => name.to_string(),
    })
}

/// A type or protocol name as written: a symbol, or `nil`.
fn written(form: Ts, source: &str) -> Option<String> {
    match form.kind() {
        "sym_lit" => qualified_text(form, source),
        "nil_lit" => Some("nil".to_string()),
        _ => None,
    }
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
        more_forms: Vec::new(),
        adjacent: true,
        imports_class: false,
    })
}

/// A `def` or `defonce`: a function if the value is a literal `fn`, else a value.
fn def<'t>(items: &[Ts<'t>], form: Ts<'t>, source: &str, namespace: &str) -> Option<Decl<'t>> {
    let (name, is_fn, _) = def_form(items, source)?;
    let (prefix, kind) = if is_fn {
        ("fn", NodeKind::Function)
    } else {
        ("val", NodeKind::Value)
    };
    Some(Decl {
        id: format!("{prefix}:{namespace}/{name}"),
        name: name.to_string(),
        kind,
        parent: None,
        node: form,
        members: Vec::new(),
        more_forms: Vec::new(),
        adjacent: true,
        imports_class: false,
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
    imports_class: bool,
) -> Option<Vec<Decl<'t>>> {
    let name = plain_symbol(*items.get(1)?, source)?;
    let id = format!("type:{namespace}/{name}");
    // The methods with their position among the items; the forms of one name are one node.
    let methods: Vec<(usize, Ts<'t>)> = items
        .iter()
        .copied()
        .enumerate()
        .skip(first_member)
        .filter(|(_, m)| m.kind() == "list_lit")
        .collect();
    let mut decls = vec![Decl {
        id: id.clone(),
        name: name.to_string(),
        kind: NodeKind::Type,
        parent: None,
        node: form,
        members: methods.iter().map(|(_, m)| *m).collect(),
        more_forms: Vec::new(),
        adjacent: true,
        imports_class,
    }];
    let mut by_name: Vec<(&str, Vec<(usize, Ts<'t>)>)> = Vec::new();
    for (position, method) in &methods {
        let Some(first) = flat_items(*method).first().copied() else {
            continue;
        };
        let Some(method_name) = plain_symbol(first, source) else {
            continue;
        };
        match by_name.iter_mut().find(|(n, _)| *n == method_name) {
            Some((_, forms)) => forms.push((*position, *method)),
            None => by_name.push((method_name, vec![(*position, *method)])),
        }
    }
    for (method_name, forms) in by_name {
        decls.push(Decl {
            id: format!("fn:{namespace}/{name}#{method_name}"),
            name: method_name.to_string(),
            kind: NodeKind::Function,
            parent: Some(id.clone()),
            node: forms[0].1,
            members: Vec::new(),
            more_forms: forms[1..].iter().map(|(_, f)| *f).collect(),
            adjacent: forms.windows(2).all(|w| w[1].0 == w[0].0 + 1),
            imports_class: false,
        });
    }
    Some(decls)
}
