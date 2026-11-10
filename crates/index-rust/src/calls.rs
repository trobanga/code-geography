//! Pass 4: the call edges. An edge exists only when the declarations in the repository
//! prove the target (ADR 0001, ADR 0014): the function that is called, or the type of
//! the receiver.

use crate::declare::{Decl, container_id};
use crate::edges::type_head;
use crate::model::{Found, Model};
use crate::modules::ModInfo;
use crate::syntax::{named_children, path_of, text};
use crate::types::{Cx, Ty, Types, Wrapper};
use crate::world::{Entity, Res, World};
use cg_core::{CallReport, Edge, EdgeKind, NodeKind};
use std::collections::{BTreeSet, HashMap};
use tree_sitter::Node as Ts;

/// What the call sites say beyond the four counts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    /// Of the calls to a function: those where the function is named but not called.
    pub function_values: usize,
    /// Names followed by `(` in the arguments of macros: call sites that are not read.
    pub macro_calls: usize,
}

/// Adds the call edges to `edges` and counts the call sites.
pub fn compute<'t>(
    world: &World<'_, 't>,
    packages: &[(&str, &str)],
    mods: &[ModInfo<'t>],
    decls: &[Decl<'t>],
    edges: &mut BTreeSet<Edge>,
) -> (CallReport, Stats) {
    let model = Model::build(world, mods, decls);
    let types = Types {
        world,
        mods,
        decls,
        model: &model,
    };
    let mut report = CallReport::default();
    let mut stats = Stats::default();
    for (f, decl) in decls.iter().enumerate() {
        if decl.kind != NodeKind::Function {
            continue;
        }
        let Some(body) = decl.item.child_by_field_name("body") else {
            continue;
        };
        let cx = types.fn_cx(f);
        let mut walker = Walker {
            types: &types,
            source: mods[decl.module].source,
            cx,
            from: &decl.id,
            edges: &mut *edges,
            report: &mut report,
            stats: &mut stats,
            scopes: vec![HashMap::new()],
        };
        if let Some(parameters) = decl.item.child_by_field_name("parameters") {
            for parameter in named_children(parameters) {
                walker.bind_parameter(parameter);
            }
        }
        walker.visit(body);
    }
    for (index, m) in mods.iter().enumerate() {
        let (dir, crate_name) = packages[m.package];
        let container = container_id(dir, crate_name, m);
        // The initializers of constants and statics, in the mod and in impls and traits.
        let mut initializers: Vec<(String, Option<Ts>, Ts)> = Vec::new();
        for item in &m.items {
            match item.kind() {
                "const_item" | "static_item" => {
                    initializers.push((container.clone(), None, *item));
                }
                "impl_item" | "trait_item" => {
                    let cx = types.item_cx(index, Some(*item));
                    let from = match (cx.self_type, item.kind()) {
                        (Some(Entity::Type(t)), _) => decls[t].id.clone(),
                        _ => item
                            .child_by_field_name("trait")
                            .and_then(|t| {
                                type_head(world, &cx.ctx(), t, m.source)
                                    .map(|d| decls[d].id.clone())
                            })
                            .unwrap_or_else(|| container.clone()),
                    };
                    for member in members(*item, "const_item") {
                        initializers.push((from.clone(), Some(*item), member));
                    }
                }
                _ => {}
            }
        }
        for (from, holder, item) in initializers {
            let Some(value) = item.child_by_field_name("value") else {
                continue;
            };
            let mut walker = Walker {
                types: &types,
                source: m.source,
                cx: types.item_cx(index, holder),
                from: &from,
                edges: &mut *edges,
                report: &mut report,
                stats: &mut stats,
                scopes: vec![HashMap::new()],
            };
            walker.visit(value);
        }
    }
    (report, stats)
}

#[derive(Debug, Clone, Copy)]
enum Kind {
    Function,
    Type,
    External,
    Unresolved,
}

struct Walker<'c, 'w, 't> {
    types: &'c Types<'c, 'w, 't>,
    cx: Cx,
    source: &'t str,
    from: &'c str,
    edges: &'c mut BTreeSet<Edge>,
    report: &'c mut CallReport,
    stats: &'c mut Stats,
    /// The local variables in scope, innermost last.
    scopes: Vec<HashMap<String, Ty>>,
}

impl<'t> Walker<'_, '_, 't> {
    fn local(&self, name: &str) -> Option<&Ty> {
        self.scopes.iter().rev().find_map(|scope| scope.get(name))
    }

    fn bind(&mut self, name: &str, ty: Ty) {
        // A capital letter starts a constant or a variant, which a pattern does not bind.
        if name.chars().next().is_some_and(char::is_uppercase) {
            return;
        }
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_string(), ty);
        }
    }

    /// Binds the names that a pattern introduces.
    fn bind_pattern(&mut self, pattern: Ts<'t>, ty: Ty) {
        match pattern.kind() {
            "identifier" => {
                let name = text(pattern, self.source);
                self.bind(name, ty);
            }
            "mut_pattern" | "ref_pattern" | "reference_pattern" => {
                for child in named_children(pattern) {
                    self.bind_pattern(child, ty.clone());
                }
            }
            "tuple_struct_pattern" | "struct_pattern" => {
                let head = pattern.child_by_field_name("type");
                for child in named_children(pattern)
                    .into_iter()
                    .filter(|c| Some(*c) != head)
                {
                    self.bind_pattern(child, Ty::Unknown);
                }
            }
            "field_pattern" => match pattern.child_by_field_name("pattern") {
                Some(inner) => self.bind_pattern(inner, Ty::Unknown),
                None => {
                    if let Some(name) = pattern.child_by_field_name("name") {
                        let name = text(name, self.source);
                        self.bind(name, Ty::Unknown);
                    }
                }
            },
            "match_pattern" => {
                let guard = pattern.child_by_field_name("condition");
                for child in named_children(pattern)
                    .into_iter()
                    .filter(|c| Some(*c) != guard)
                {
                    self.bind_pattern(child, Ty::Unknown);
                }
            }
            "tuple_pattern" | "slice_pattern" | "or_pattern" | "captured_pattern" => {
                for child in named_children(pattern) {
                    self.bind_pattern(child, Ty::Unknown);
                }
            }
            _ => {}
        }
    }

    fn scoped(&mut self, body: impl FnOnce(&mut Self)) {
        self.scopes.push(HashMap::new());
        body(self);
        self.scopes.pop();
    }

    /// Reads the calls in `node` and gives the type of the expression.
    fn visit(&mut self, node: Ts<'t>) -> Ty {
        match node.kind() {
            "call_expression" => self.call(node),
            "block" | "unsafe_block" | "async_block" | "const_block" => {
                self.scoped(|w| w.visit_children(node));
                Ty::Unknown
            }
            "let_declaration" => {
                let mut value = node.child_by_field_name("value").map(|v| self.visit(v));
                if let Some(written) = node.child_by_field_name("type") {
                    value = Some(self.types.ty(&self.cx, written));
                }
                if let Some(alternative) = node.child_by_field_name("alternative") {
                    self.visit(alternative);
                }
                if let Some(pattern) = node.child_by_field_name("pattern") {
                    self.bind_pattern(pattern, value.unwrap_or(Ty::Unknown));
                }
                Ty::Unknown
            }
            "let_condition" => {
                if let Some(value) = node.child_by_field_name("value") {
                    self.visit(value);
                }
                if let Some(pattern) = node.child_by_field_name("pattern") {
                    self.bind_pattern(pattern, Ty::Unknown);
                }
                Ty::Unknown
            }
            "if_expression" | "while_expression" => {
                let mut rest = Vec::new();
                self.scoped(|w| {
                    for child in named_children(node) {
                        if child.kind() == "else_clause" {
                            rest.push(child);
                        } else {
                            w.visit(child);
                        }
                    }
                });
                for child in rest {
                    self.visit(child);
                }
                Ty::Unknown
            }
            "for_expression" => {
                if let Some(value) = node.child_by_field_name("value") {
                    self.visit(value);
                }
                self.scoped(|w| {
                    if let Some(pattern) = node.child_by_field_name("pattern") {
                        w.bind_pattern(pattern, Ty::Unknown);
                    }
                    if let Some(body) = node.child_by_field_name("body") {
                        w.visit(body);
                    }
                });
                Ty::Unknown
            }
            "match_arm" => {
                self.scoped(|w| {
                    if let Some(pattern) = node.child_by_field_name("pattern") {
                        w.bind_pattern(pattern, Ty::Unknown);
                        if let Some(guard) = pattern.child_by_field_name("condition") {
                            w.visit(guard);
                        }
                    }
                    if let Some(value) = node.child_by_field_name("value") {
                        w.visit(value);
                    }
                });
                Ty::Unknown
            }
            "closure_expression" => {
                self.scoped(|w| {
                    if let Some(parameters) = node.child_by_field_name("parameters") {
                        for parameter in named_children(parameters) {
                            w.bind_parameter(parameter);
                        }
                    }
                    if let Some(body) = node.child_by_field_name("body") {
                        w.visit(body);
                    }
                });
                Ty::Unknown
            }
            "function_item" => {
                self.nested_function(node);
                Ty::Unknown
            }
            "self" => self.cx.self_ty.clone(),
            "identifier" => {
                let name = text(node, self.source);
                match self.local(name) {
                    Some(ty) => ty.clone(),
                    None => self.path_value(&[name]),
                }
            }
            "scoped_identifier" => match path_of(node, self.source) {
                Some(path) => self.path_value(&path),
                None => Ty::Unknown,
            },
            "parenthesized_expression" | "reference_expression" => {
                let mut ty = Ty::Unknown;
                for child in named_children(node) {
                    ty = self.visit(child);
                }
                ty
            }
            "unary_expression" => {
                let ty = named_children(node)
                    .into_iter()
                    .fold(Ty::Unknown, |_, child| self.visit(child));
                if text(node, self.source).starts_with('*') {
                    ty
                } else {
                    Ty::Unknown
                }
            }
            "struct_expression" => {
                let ty = node
                    .child_by_field_name("name")
                    .map_or(Ty::Unknown, |name| self.types.ty(&self.cx, name));
                if let Some(body) = node.child_by_field_name("body") {
                    self.visit(body);
                }
                ty
            }
            "await_expression" | "try_expression" => {
                let ty = named_children(node)
                    .into_iter()
                    .fold(Ty::Unknown, |_, child| self.visit(child));
                match (node.kind(), ty) {
                    ("await_expression", Ty::Wrap(Wrapper::Future, inner))
                    | ("try_expression", Ty::Wrap(Wrapper::Option | Wrapper::Result, inner)) => {
                        *inner
                    }
                    _ => Ty::Unknown,
                }
            }
            "field_expression" => {
                let owner = node
                    .child_by_field_name("value")
                    .map_or(Ty::Unknown, |value| self.visit(value));
                let name = node
                    .child_by_field_name("field")
                    .map_or("", |field| text(field, self.source));
                match owner {
                    Ty::Repo(owner) => self.types.field_ty(owner, name),
                    _ => Ty::Unknown,
                }
            }
            "string_literal" | "raw_string_literal" | "integer_literal" | "float_literal"
            | "boolean_literal" | "char_literal" | "array_expression" | "tuple_expression" => {
                self.visit_children(node);
                Ty::External
            }
            "type_cast_expression" => {
                if let Some(value) = node.child_by_field_name("value") {
                    self.visit(value);
                }
                node.child_by_field_name("type")
                    .map_or(Ty::Unknown, |written| self.types.ty(&self.cx, written))
            }
            "macro_invocation" => {
                self.count_macro_calls(node);
                Ty::Unknown
            }
            "macro_definition" | "attribute_item" => Ty::Unknown,
            _ => {
                self.visit_children(node);
                Ty::Unknown
            }
        }
    }

    /// The arguments of a macro are tokens, and the macro decides what they mean: no edge.
    /// A name that a `(` follows is counted as a call site, to know the size of the gap.
    fn count_macro_calls(&mut self, call: Ts<'t>) {
        let mut tokens = Vec::new();
        for child in named_children(call) {
            if child.kind() == "token_tree" {
                leaves(child, self.source, &mut tokens);
            }
        }
        for pair in tokens.windows(2) {
            let name = pair[0];
            let word = name
                .chars()
                .next()
                .is_some_and(|c| c.is_alphabetic() || c == '_');
            if word && !starts_uppercase(name) && pair[1] == "(" {
                self.stats.macro_calls += 1;
            }
        }
    }

    fn visit_children(&mut self, node: Ts<'t>) {
        // Items declared in a block are visible in the whole block.
        for child in named_children(node) {
            if child.kind() == "function_item"
                && let Some(name) = child.child_by_field_name("name")
            {
                let name = text(name, self.source);
                self.bind(name, Ty::Unknown);
            }
        }
        for child in named_children(node) {
            self.visit(child);
        }
    }

    fn bind_parameter(&mut self, parameter: Ts<'t>) {
        match parameter.child_by_field_name("pattern") {
            Some(pattern) if parameter.kind() == "parameter" => {
                let ty = match parameter.child_by_field_name("type") {
                    Some(written) => self.types.ty(&self.cx, written),
                    None => Ty::Unknown,
                };
                self.bind_pattern(pattern, ty);
            }
            _ => self.bind_pattern(parameter, Ty::Unknown),
        }
    }

    /// A function in the body of another: its calls count for the enclosing function.
    fn nested_function(&mut self, item: Ts<'t>) {
        let outer = std::mem::take(&mut self.scopes);
        self.scopes.push(HashMap::new());
        if let Some(parameters) = item.child_by_field_name("parameters") {
            for parameter in named_children(parameters) {
                self.bind_parameter(parameter);
            }
        }
        if let Some(body) = item.child_by_field_name("body") {
            self.visit(body);
        }
        self.scopes = outer;
    }

    fn call(&mut self, node: Ts<'t>) -> Ty {
        let function = node.child_by_field_name("function");
        let first_argument = match node.child_by_field_name("arguments") {
            Some(arguments) => named_children(arguments)
                .into_iter()
                .map(|argument| self.visit(argument))
                .collect::<Vec<_>>()
                .into_iter()
                .next(),
            None => None,
        };
        let Some(function) = function else {
            return Ty::Unknown;
        };
        if function.kind() == "field_expression" {
            return self.method_call(function);
        }
        match path_of(function, self.source) {
            Some(path) => self.path_call(&path, first_argument.unwrap_or(Ty::Unknown)),
            None => {
                self.visit(function);
                self.tally(Kind::Unresolved);
                Ty::Unknown
            }
        }
    }

    fn path_call(&mut self, path: &[&str], first_argument: Ty) -> Ty {
        if let [name] = path
            && self.local(name).is_some()
        {
            self.tally(Kind::Unresolved);
            return Ty::Unknown;
        }
        match self.types.world.resolve(&self.cx.ctx(), path) {
            Res::Item(Entity::Function(target), consumed) if consumed == path.len() => {
                self.call_function(target, None)
            }
            Res::Item(Entity::Type(owner), consumed) if consumed == path.len() => {
                self.types.unalias(owner).map_or(Ty::Unknown, Ty::Repo)
            }
            Res::Item(Entity::Type(owner), consumed) if consumed + 1 == path.len() => {
                match self.types.unalias(owner) {
                    Some(owner) => self.associated(owner, path[consumed]),
                    None => Ty::Unknown,
                }
            }
            Res::External if path.ends_with(&["new"]) && path.len() >= 2 => {
                self.tally(Kind::External);
                match path[path.len() - 2] {
                    "Box" | "Arc" | "Rc" => Ty::Ptr(Box::new(first_argument)),
                    _ => Ty::Unknown,
                }
            }
            Res::Hidden if path.len() == 2 && self.cx.bounds.contains_key(path[0]) => {
                let traits = self.cx.bounds[path[0]].clone();
                self.declared_method(&Ty::Bounds(traits.clone()), &traits, path[1])
            }
            Res::External | Res::Hidden => {
                // A capital letter: the constructor of a variant, not a call.
                if !starts_uppercase(path[path.len() - 1]) {
                    self.tally(Kind::External);
                }
                Ty::Unknown
            }
            Res::Unresolved => {
                if !starts_uppercase(path[path.len() - 1]) {
                    self.tally(Kind::Unresolved);
                }
                Ty::Unknown
            }
            Res::Item(..) => Ty::Unknown,
        }
    }

    /// A path in the place of a value. A function that it names is used as a value
    /// (`.map(parse)`), which counts as a call; a type names a unit struct.
    fn path_value(&mut self, path: &[&str]) -> Ty {
        match self.types.world.resolve(&self.cx.ctx(), path) {
            Res::Item(Entity::Function(target), consumed) if consumed == path.len() => {
                self.function_value(target);
                Ty::Unknown
            }
            Res::Item(Entity::Type(owner), consumed) if consumed == path.len() => Ty::Repo(owner),
            Res::Item(Entity::Type(owner), consumed) if consumed + 1 == path.len() => {
                if self.types.model.has_variant(owner, path[consumed]) {
                    return Ty::Repo(owner);
                }
                if let Found::One(target) =
                    self.types
                        .model
                        .function(self.types.decls, owner, path[consumed])
                {
                    self.function_value(target);
                }
                Ty::Unknown
            }
            _ => Ty::Unknown,
        }
    }

    fn function_value(&mut self, target: usize) {
        self.stats.function_values += 1;
        self.call_function(target, None);
    }

    /// Counts and records a call of the function `target`; gives the type that it returns.
    /// `receiver` is the type that a method of a trait is called on.
    fn call_function(&mut self, target: usize, receiver: Option<usize>) -> Ty {
        self.tally(Kind::Function);
        let to = self.types.decls[target].id.clone();
        self.add(&to);
        self.types.ret_ty(target, receiver)
    }

    fn call_type(&mut self, owner: usize) -> Ty {
        self.tally(Kind::Type);
        let to = self.types.decls[owner].id.clone();
        self.add(&to);
        Ty::Unknown
    }

    /// `T::name(..)`: a function of the type, or a variant that is built like a call.
    fn associated(&mut self, owner: usize, name: &str) -> Ty {
        if self.types.model.has_variant(owner, name) {
            return Ty::Repo(owner);
        }
        match self.types.model.function(self.types.decls, owner, name) {
            Found::One(target) => self.call_function(target, Some(owner)),
            Found::None if self.types.is_trait(owner) => {
                self.external_call(&Ty::Repo(owner), name);
                Ty::Unknown
            }
            Found::None if is_std_provided(name) => {
                self.tally(Kind::External);
                // `D::default()` and `D::from(x)` build a `D`.
                match name {
                    "default" | "from" => Ty::Repo(owner),
                    _ => Ty::Unknown,
                }
            }
            Found::Several | Found::None => self.call_type(owner),
        }
    }

    /// `receiver.name(..)`.
    fn method_call(&mut self, function: Ts<'t>) -> Ty {
        let receiver = match function.child_by_field_name("value") {
            Some(value) => self.visit(value),
            None => Ty::Unknown,
        };
        let name = function
            .child_by_field_name("field")
            .map_or("", |f| text(f, self.source));
        self.method(receiver, name)
    }

    /// The call of the method `name` on a value of type `receiver`.
    fn method(&mut self, receiver: Ty, name: &str) -> Ty {
        match &receiver {
            Ty::Repo(owner) => match self.types.model.function(self.types.decls, *owner, name) {
                Found::One(target) => self.call_function(target, Some(*owner)),
                Found::Several => self.call_type(*owner),
                Found::None if self.types.is_trait(*owner) => self.external_call(&receiver, name),
                Found::None if is_std_provided(name) => self.external_call(&receiver, name),
                Found::None => self.call_type(*owner),
            },
            Ty::Bounds(traits) => {
                let traits = traits.clone();
                self.declared_method(&receiver, &traits, name)
            }
            Ty::Ptr(inner) => match inner.as_ref() {
                Ty::Repo(owner) => self.declared_method(&receiver, &[*owner], name),
                Ty::Bounds(traits) => {
                    let traits = traits.clone();
                    self.declared_method(&receiver, &traits, name)
                }
                other => self.method(other.clone(), name),
            },
            Ty::Wrap(wrapper, inner) => {
                let kept = matches!(wrapper, Wrapper::Option | Wrapper::Result)
                    && matches!(name, "unwrap" | "expect" | "unwrap_or_default");
                if kept {
                    self.tally(Kind::External);
                    *inner.clone()
                } else {
                    self.external_call(&receiver, name)
                }
            }
            Ty::External => self.external_call(&receiver, name),
            Ty::Unknown => {
                self.tally(Kind::Unresolved);
                Ty::Unknown
            }
        }
    }

    /// A call that the repository does not declare: `std` or a dependency provides it.
    /// `clone` gives the type of the receiver.
    fn external_call(&mut self, receiver: &Ty, name: &str) -> Ty {
        self.tally(Kind::External);
        if name == "clone" {
            receiver.clone()
        } else {
            Ty::Unknown
        }
    }

    /// A method that the types `owners` declare, or that comes from elsewhere (external).
    fn declared_method(&mut self, receiver: &Ty, owners: &[usize], name: &str) -> Ty {
        let mut found: Vec<(usize, Found)> = owners
            .iter()
            .map(|o| (*o, self.types.model.function(self.types.decls, *o, name)))
            .filter(|(_, f)| *f != Found::None)
            .collect();
        found.dedup_by_key(|(_, f)| *f);
        match found.as_slice() {
            [] => self.external_call(receiver, name),
            [(owner, Found::One(target))] => self.call_function(*target, Some(*owner)),
            [(owner, _)] if owners.len() == 1 => self.call_type(*owner),
            _ => {
                self.tally(Kind::Unresolved);
                Ty::Unknown
            }
        }
    }

    fn tally(&mut self, kind: Kind) {
        match kind {
            Kind::Function => self.report.to_function += 1,
            Kind::Type => self.report.to_type += 1,
            Kind::External => self.report.external += 1,
            Kind::Unresolved => self.report.unresolved += 1,
        }
    }

    fn add(&mut self, to: &str) {
        self.edges.insert(Edge {
            from: self.from.to_string(),
            to: to.to_string(),
            kind: EdgeKind::Calls,
            change: None,
        });
    }
}

/// The items of an impl or a trait that have the given kind.
fn members<'t>(item: Ts<'t>, kind: &str) -> Vec<Ts<'t>> {
    item.child_by_field_name("body")
        .map(named_children)
        .unwrap_or_default()
        .into_iter()
        .filter(|c| c.kind() == kind)
        .collect()
}

/// The leaf tokens of a token tree, in the order of the source.
fn leaves<'a>(node: Ts, source: &'a str, tokens: &mut Vec<&'a str>) {
    if node.child_count() == 0 {
        tokens.push(text(node, source));
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        leaves(child, source, tokens);
    }
}

fn starts_uppercase(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_uppercase)
}

/// The names that `std` traits and derives provide for most types: a call of one of them on
/// a type of the repository that declares no such function is not a call of the type.
const STD_PROVIDED: [&str; 31] = [
    "clone",
    "clone_from",
    "to_string",
    "to_owned",
    "into",
    "from",
    "try_into",
    "try_from",
    "from_str",
    "default",
    "eq",
    "ne",
    "cmp",
    "partial_cmp",
    "lt",
    "le",
    "gt",
    "ge",
    "max",
    "min",
    "hash",
    "fmt",
    "as_ref",
    "as_mut",
    "borrow",
    "borrow_mut",
    "deref",
    "deref_mut",
    "drop",
    "serialize",
    "deserialize",
];

fn is_std_provided(name: &str) -> bool {
    STD_PROVIDED.contains(&name)
}
