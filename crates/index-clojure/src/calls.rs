//! The edges from the code of a function to what it names: `calls` to functions.
//! A qualified symbol (`alias/name`, `namespace/name`) is resolved through the `ns` form: a
//! local name never has a namespace part. An unqualified symbol is resolved through the
//! namespace of the file and its `:refer`s, but only if no local could hide it (`locals`)
//! and no two sources declare it.

use crate::declare::head;
use crate::syntax::{flat_items, items, keyword, plain_symbol, private, symbol};
use crate::{Report, locals};
use cg_core::{Edge, EdgeKind};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use tree_sitter::{Node as Ts, Tree};

/// What a name in a namespace stands for.
pub enum Target {
    /// The function node with this id.
    Function(String),
    /// The type node with this id, named as a protocol or interface (`interface`) or
    /// through a constructor function of a record or type.
    Type {
        id: String,
        interface: bool,
        /// A class without a var (`definterface`): valid unqualified, never as `a/Name`.
        class_only: bool,
    },
    /// A var that no node stands for (`def`, `defonce`).
    Var,
}

/// The vars of one namespace of the repository, by name.
#[derive(Default)]
pub struct Vars {
    names: HashMap<String, Target>,
    /// The names of `defn-` and of vars with `^:private`: `:refer :all` does not bring them.
    private: HashSet<String>,
}

/// The vars that the top-level `forms` of a file declare in `namespace`.
pub fn vars(forms: &[Ts], source: &str, namespace: &str) -> Vars {
    let mut vars = Vars::default();
    for form in forms {
        let items = flat_items(*form);
        let Some(name) = items.get(1).and_then(|n| plain_symbol(*n, source)) else {
            continue;
        };
        match head(*form, source).as_deref() {
            Some("defn" | "defn-" | "defmacro" | "defmulti") => {
                if head(*form, source).as_deref() == Some("defn-") || private(items[1], source) {
                    vars.private.insert(name.to_string());
                }
                vars.names.insert(
                    name.to_string(),
                    Target::Function(format!("fn:{namespace}/{name}")),
                );
            }
            // The methods of a protocol are vars of the namespace, those of an interface
            // are not. The name of both is a type.
            Some(kind @ ("defprotocol" | "definterface")) => {
                vars.names.insert(
                    name.to_string(),
                    Target::Type {
                        id: format!("type:{namespace}/{name}"),
                        interface: true,
                        class_only: kind == "definterface",
                    },
                );
                if kind == "defprotocol" {
                    let methods = items.iter().skip(2).filter(|m| m.kind() == "list_lit");
                    for method in methods {
                        if let Some(method_name) = flat_items(*method)
                            .first()
                            .and_then(|m| plain_symbol(*m, source))
                        {
                            vars.names.insert(
                                method_name.to_string(),
                                Target::Function(format!("fn:{namespace}/{name}#{method_name}")),
                            );
                        }
                    }
                }
            }
            // A declared name is a var; a `defn` of it makes it a function.
            Some("declare") => {
                for declared in items.iter().skip(1) {
                    if let Some(declared) = plain_symbol(*declared, source) {
                        vars.names
                            .entry(declared.to_string())
                            .or_insert(Target::Var);
                    }
                }
            }
            Some("def" | "defonce") => {
                if private(items[1], source) {
                    vars.private.insert(name.to_string());
                }
                vars.names.entry(name.to_string()).or_insert(Target::Var);
            }
            // The constructor functions that the declaration generates.
            Some(kind @ ("defrecord" | "deftype")) => {
                let constructors = if kind == "defrecord" {
                    vec![format!("->{name}"), format!("map->{name}")]
                } else {
                    vec![format!("->{name}")]
                };
                for constructor in constructors {
                    vars.names.insert(
                        constructor,
                        Target::Type {
                            id: format!("type:{namespace}/{name}"),
                            interface: false,
                            class_only: false,
                        },
                    );
                }
            }
            _ => {}
        }
    }
    vars
}

/// A file that is read, with what the first pass found out about it.
pub struct File<'s> {
    pub source: &'s str,
    pub tree: Tree,
    /// The `ns` form, as `Node::id`.
    pub ns_form: usize,
    pub namespace: String,
    /// The aliases of the `ns` form: alias to namespace.
    pub aliases: HashMap<String, String>,
    /// The names that `:refer` (or `:only` of a `:use`) brings: name to namespaces.
    pub refers: HashMap<String, Vec<String>>,
    /// The namespaces whose public vars are all brought (`:refer :all`, `:use` without
    /// `:only`), each with the names that it leaves out.
    pub refer_all: Vec<(String, Vec<String>)>,
    /// The declarations that became nodes: `Node::id` of the form to the id of the node.
    pub owned: HashMap<usize, String>,
}

/// What a name means in one file.
#[derive(Clone, Copy)]
struct Scope<'a> {
    vars: &'a HashMap<String, Vars>,
    file: &'a File<'a>,
}

/// The result of looking up a qualified symbol.
enum Resolved<'a> {
    /// Its namespace is not one of the repository.
    External,
    Found(&'a Target),
    /// A namespace of the repository, which does not declare the name.
    Missing(&'a str),
}

impl<'a> Scope<'a> {
    /// `namespace/name`, where `namespace` is an alias or the full name of a namespace.
    fn qualified(&self, namespace: &'a str, name: &str) -> Resolved<'a> {
        let namespace = self
            .file
            .aliases
            .get(namespace)
            .map_or(namespace, String::as_str);
        match self.vars.get(namespace) {
            None => Resolved::External,
            Some(vars) => match vars.names.get(name) {
                Some(Target::Type {
                    class_only: true, ..
                })
                | None => Resolved::Missing(namespace),
                Some(target) => Resolved::Found(target),
            },
        }
    }

    /// What an unqualified name can mean in the file: the declarations of its own
    /// namespace and of the namespaces that bring it, by namespace.
    fn candidates(&self, name: &str) -> BTreeMap<&'a str, &'a Target> {
        let file = self.file;
        let mut found = BTreeMap::new();
        let mut look = |namespace: &'a str, only_public: bool| {
            let Some(vars) = self.vars.get(namespace) else {
                return;
            };
            if only_public && vars.private.contains(name) {
                return;
            }
            if let Some(target) = vars.names.get(name) {
                found.insert(namespace, target);
            }
        };
        look(&file.namespace, false);
        for namespace in file.refers.get(name).into_iter().flatten() {
            look(namespace, false);
        }
        for (namespace, except) in &file.refer_all {
            if !except.iter().any(|e| e == name) {
                look(namespace, true);
            }
        }
        found
    }

    /// A symbol without namespace part that a declaration names, if one does and none
    /// other does. Only for places where no local can stand.
    fn unqualified(&self, name: &str) -> Option<&'a Target> {
        let found = self.candidates(name);
        match found.len() {
            1 => found.into_values().next(),
            _ => None,
        }
    }
}

/// The edges that the code of `files` names. `vars` holds the namespaces of the repository.
pub fn read(files: &[File], vars: &HashMap<String, Vars>, report: &mut Report) -> BTreeSet<Edge> {
    let mut edges = BTreeSet::new();
    for file in files {
        for form in flat_items(file.tree.root_node()) {
            if form.id() == file.ns_form {
                continue;
            }
            let form_head = head(form, file.source);
            let scope = Scope { vars, file };
            if matches!(form_head.as_deref(), Some("defrecord" | "deftype"))
                && let Some(from) = file.owned.get(&form.id())
            {
                implemented(form, from, scope, &mut edges);
            }
            // The pieces of code of the form, each with the node that holds it, if any.
            let units: Vec<(Option<&String>, Vec<Ts>)> = match form_head.as_deref() {
                Some("defn" | "defn-" | "defmacro" | "defmulti") => {
                    let code = flat_items(form).into_iter().skip(2).collect();
                    vec![(file.owned.get(&form.id()), code)]
                }
                // The code of a type is in its methods.
                Some("defrecord" | "deftype") => flat_items(form)
                    .into_iter()
                    .skip(3)
                    .filter(|m| m.kind() == "list_lit")
                    .map(|m| {
                        let code = flat_items(m).into_iter().skip(1).collect();
                        (file.owned.get(&m.id()), code)
                    })
                    .collect(),
                Some("defprotocol" | "definterface") => Vec::new(),
                _ => vec![(None, vec![form])],
            };
            let key = match form_head.as_deref() {
                Some("def") => "def",
                Some("defmethod") => "defmethod",
                Some("extend-protocol") => "extend-protocol",
                Some("defrecord") => "defrecord",
                Some("deftype") => "deftype",
                _ => "other",
            };
            let locals = locals::locals(form, file.source);
            for (from, code) in units {
                let mut reader = Reader {
                    source: file.source,
                    scope,
                    from: from.map(String::as_str),
                    key,
                    locals: &locals,
                    // The expansion of a macro calls what its syntax-quoted forms name.
                    in_macro: form_head.as_deref() == Some("defmacro"),
                    edges: &mut edges,
                    report: &mut *report,
                };
                for item in code {
                    reader.read(item);
                }
            }
        }
    }
    edges
}

struct Reader<'a> {
    source: &'a str,
    scope: Scope<'a>,
    /// The node that holds the code; `None`: no node does, and `key` names the head of the
    /// top-level form for the report.
    from: Option<&'a str>,
    key: &'static str,
    /// The names in the top-level form that a local could bind (see `locals`).
    locals: &'a HashSet<String>,
    in_macro: bool,
    edges: &'a mut BTreeSet<Edge>,
    report: &'a mut Report,
}

impl Reader<'_> {
    /// Reads a form of code. `at_head`: the form is the first of a list.
    fn read(&mut self, form: Ts) {
        match form.kind() {
            "sym_lit" => self.symbol(form, false),
            // Data, not code.
            "quoting_lit" => {}
            "syn_quoting_lit" if !self.in_macro => {}
            "list_lit" | "anon_fn_lit" => {
                let items = flat_items(form);
                let first = items.first().and_then(|f| plain_symbol(*f, self.source));
                if matches!(first, Some("quote" | "comment")) {
                    return;
                }
                // The lists in these forms define methods: the first symbol names the
                // method, it does not call anything.
                let defines_methods = matches!(
                    first,
                    Some(
                        "reify"
                            | "proxy"
                            | "specify"
                            | "specify!"
                            | "extend-type"
                            | "extend-protocol"
                    )
                );
                for (i, item) in items.into_iter().enumerate() {
                    if i == 0 {
                        self.head(item);
                    } else if first == Some("letfn") && i == 1 && item.kind() == "vec_lit" {
                        // The head of a binding is the name that it binds, not a call.
                        for binding in flat_items(item) {
                            let skip = usize::from(binding.kind() == "list_lit");
                            for part in flat_items(binding).into_iter().skip(skip) {
                                self.read(part);
                            }
                        }
                    } else if defines_methods && item.kind() == "list_lit" {
                        for method_item in flat_items(item).into_iter().skip(1) {
                            self.read(method_item);
                        }
                    } else {
                        self.read(item);
                    }
                }
            }
            "map_lit" => {
                let entries = items(form);
                for (i, item) in entries.iter().enumerate() {
                    // `{:keys [a/f]}` names the locals that it binds.
                    let binds = i % 2 == 1
                        && item.kind() == "vec_lit"
                        && matches!(keyword(entries[i - 1], self.source), Some("keys" | "syms"));
                    if !binds {
                        self.read(*item);
                    }
                }
            }
            _ => {
                for item in items(form) {
                    self.read(item);
                }
            }
        }
    }

    fn head(&mut self, form: Ts) {
        if form.kind() == "sym_lit" {
            self.symbol(form, true);
        } else {
            self.read(form);
        }
    }

    fn symbol(&mut self, form: Ts, at_head: bool) {
        let Some((namespace, name)) = symbol(form, self.source) else {
            return;
        };
        let Some(namespace) = namespace else {
            self.unqualified(name, at_head);
            return;
        };
        let (to, kind) = match self.scope.qualified(namespace, name) {
            Resolved::External => {
                self.report.calls.external += 1;
                return;
            }
            Resolved::Found(Target::Function(_) | Target::Type { .. }) if self.from.is_none() => {
                *self
                    .report
                    .no_source_node
                    .entry(self.key.to_string())
                    .or_default() += 1;
                return;
            }
            Resolved::Found(Target::Function(id)) => {
                self.report.calls.to_function += 1;
                if !at_head {
                    self.report.function_values += 1;
                }
                (id, EdgeKind::Calls)
            }
            Resolved::Found(Target::Type { id, .. }) => {
                self.report.calls.to_type += 1;
                (id, EdgeKind::TypeRef)
            }
            Resolved::Found(Target::Var) => {
                self.report.var_without_node += 1;
                self.report.calls.unresolved += 1;
                return;
            }
            Resolved::Missing(namespace) => {
                *self
                    .report
                    .unresolved_names
                    .entry(format!("{namespace}/{name}"))
                    .or_default() += 1;
                self.report.calls.unresolved += 1;
                return;
            }
        };
        let Some(from) = self.from else { return };
        self.edges.insert(Edge {
            from: from.to_string(),
            to: to.clone(),
            kind,
            change: None,
        });
    }
}

impl Reader<'_> {
    /// A symbol without namespace part: a function that the namespace of the file or a
    /// `:refer` names.
    fn unqualified(&mut self, name: &str, at_head: bool) {
        let candidates = self.scope.candidates(name);
        // A var that has no node is not a site (the `def` of a constant is named everywhere).
        if candidates.is_empty()
            || (candidates.len() == 1 && candidates.values().all(|t| matches!(t, Target::Var)))
        {
            return;
        }
        // The function itself (recursion): no edge, and no site.
        if candidates.len() == 1
            && candidates
                .values()
                .all(|t| matches!(t, Target::Function(id) if Some(id.as_str()) == self.from))
        {
            return;
        }
        if self.locals.contains(name) {
            // Only a call is a lost call; a local that is named elsewhere is no site.
            if at_head {
                self.report.unqualified.maybe_local += 1;
                self.report.calls.unresolved += 1;
            }
            return;
        }
        if candidates.len() > 1 {
            self.report.unqualified.ambiguous += 1;
            self.report.calls.unresolved += 1;
            return;
        }
        let target = candidates.into_values().next();
        let Some(from) = self.from else {
            // Code that no function node holds: counted by the head of its form.
            if matches!(target, Some(Target::Function(_) | Target::Type { .. })) {
                *self
                    .report
                    .no_source_node
                    .entry(self.key.to_string())
                    .or_default() += 1;
            }
            return;
        };
        let (to, kind) = match target {
            Some(Target::Function(id)) => {
                self.report.unqualified.to_function += 1;
                self.report.calls.to_function += 1;
                if !at_head {
                    self.report.unqualified.function_values += 1;
                    self.report.function_values += 1;
                }
                (id, EdgeKind::Calls)
            }
            Some(Target::Type { id, .. }) => {
                self.report.unqualified.to_type += 1;
                self.report.calls.to_type += 1;
                (id, EdgeKind::TypeRef)
            }
            _ => return,
        };
        self.edges.insert(Edge {
            from: from.to_string(),
            to: to.clone(),
            kind,
            change: None,
        });
    }
}

/// The `implements` edges of a `defrecord` or `deftype` form: each symbol at the top level
/// of its body that names a protocol or interface of the repository. No local can stand
/// there, so also a symbol without namespace part is resolved.
fn implemented(form: Ts, from: &str, scope: Scope, edges: &mut BTreeSet<Edge>) {
    for item in flat_items(form).into_iter().skip(3) {
        let Some((namespace, name)) = symbol(item, scope.file.source) else {
            continue;
        };
        let target = match namespace {
            Some(namespace) => match scope.qualified(namespace, name) {
                Resolved::Found(target) => Some(target),
                _ => None,
            },
            None => scope.unqualified(name),
        };
        if let Some(Target::Type {
            id,
            interface: true,
            ..
        }) = target
        {
            edges.insert(Edge {
                from: from.to_string(),
                to: id.clone(),
                kind: EdgeKind::Implements,
                change: None,
            });
        }
    }
}
