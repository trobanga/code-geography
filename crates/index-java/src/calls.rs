//! Call edges. A call gets an edge only when the declarations in the repository prove
//! the target (ADR 0001): the function that is called, or, when several overloads fit
//! or none is declared, the type of the receiver.

use super::{
    CallReport, Decl, Scope, Unit, children, is_function_declaration, is_type_declaration, members,
    supertypes, text, type_id,
};
use cg_core::NodeKind;
use std::collections::{HashMap, HashSet};
use tree_sitter::Node as Ts;

/// The static type of an expression, as far as the repository tells.
#[derive(Debug, Clone)]
enum Ty {
    /// A type that is declared in the repository: its id.
    Repo(String),
    /// A type that is known, but not declared in the repository.
    External,
    Unknown(&'static str),
}

struct Method {
    id: String,
    params: usize,
    varargs: bool,
    /// The written return type; unknown for a constructor.
    ret: Ty,
}

impl Method {
    /// Whether a call with `arguments` arguments can reach the method.
    fn fits(&self, arguments: Option<usize>) -> bool {
        match arguments {
            None => true,
            Some(n) if self.varargs => n + 1 >= self.params,
            Some(n) => n == self.params,
        }
    }
}

#[derive(Default)]
struct TypeInfo {
    constructors: Vec<Method>,
    methods: HashMap<String, Vec<Method>>,
    /// The supertypes that are declared in the repository, as ids.
    supers: Vec<String>,
    /// What `super` stands for: the declared superclass, or `Object` or another
    /// type that the repository does not declare.
    superclass: Option<Ty>,
    fields: HashMap<String, Ty>,
}

/// What the declarations of all files say about the types.
pub(crate) struct Model {
    types: HashMap<String, TypeInfo>,
}

impl Model {
    pub(crate) fn build(units: &[Unit], scopes: &[Scope], decls: &[Vec<Decl>]) -> Model {
        let mut types: HashMap<String, TypeInfo> = HashMap::new();
        for ((unit, scope), decls) in units.iter().zip(scopes).zip(decls) {
            for decl in decls {
                let ctx = TypeCtx {
                    scope,
                    source: &unit.source,
                    enclosing: &decl.types,
                    type_parameters: &decl.type_parameters,
                };
                match decl.kind {
                    NodeKind::Type => {
                        let supers = supertypes(decl.node, &unit.source)
                            .into_iter()
                            .filter(|(_, name)| !decl.type_parameters.iter().any(|p| p == name))
                            .filter_map(|(_, name)| scope.resolve(name, &decl.types))
                            .collect();
                        let superclass = (decl.node.kind() == "class_declaration").then(|| {
                            let named = supertypes(decl.node, &unit.source).into_iter().next();
                            named
                                .and_then(|(_, name)| scope.resolve(name, &decl.types))
                                .map_or(Ty::External, Ty::Repo)
                        });
                        let info = types.entry(decl.id.clone()).or_default();
                        info.supers = supers;
                        info.superclass = superclass;
                        info.fields = fields_of(decl, &ctx);
                    }
                    NodeKind::Function => {
                        let Some(owner) = &decl.parent else { continue };
                        if decl.node.kind() == "constructor_declaration" {
                            types
                                .entry(owner.clone())
                                .or_default()
                                .constructors
                                .push(method_of(decl, &ctx));
                        } else {
                            types
                                .entry(owner.clone())
                                .or_default()
                                .methods
                                .entry(decl.name.clone())
                                .or_default()
                                .push(method_of(decl, &ctx));
                        }
                    }
                    _ => {}
                }
            }
        }
        Model { types }
    }
}

/// The fields of a type with their types: the variables of field declarations, the
/// constants of an enum and the components of a record.
fn fields_of<'a>(decl: &Decl<'a>, ctx: &TypeCtx<'a>) -> HashMap<String, Ty> {
    let mut fields = HashMap::new();
    let mut add = |name: Ts<'a>, ty: Ty| {
        fields.insert(text(name, ctx.source).to_string(), ty);
    };
    for member in members(decl.node) {
        match member.kind() {
            "field_declaration" => {
                let Some(declared) = member.child_by_field_name("type") else {
                    continue;
                };
                for declarator in children(member).filter(|c| c.kind() == "variable_declarator") {
                    let ty = if declarator.child_by_field_name("dimensions").is_some() {
                        Ty::External
                    } else {
                        ctx.ty_of(declared)
                    };
                    if let Some(name) = declarator.child_by_field_name("name") {
                        add(name, ty);
                    }
                }
            }
            "enum_constant" => {
                if let Some(name) = member.child_by_field_name("name") {
                    add(name, Ty::Repo(decl.id.clone()));
                }
            }
            _ => {}
        }
    }
    if let Some(components) = decl.node.child_by_field_name("parameters") {
        for component in children(components).filter(|c| c.kind() == "formal_parameter") {
            if let (Some(declared), Some(name)) = (
                component.child_by_field_name("type"),
                component.child_by_field_name("name"),
            ) {
                add(name, ctx.ty_of(declared));
            }
        }
    }
    fields
}

fn method_of<'a>(decl: &Decl<'a>, ctx: &TypeCtx<'a>) -> Method {
    let parameters = decl.node.child_by_field_name("parameters");
    let list: Vec<Ts> = parameters
        .map(|p| children(p).collect())
        .unwrap_or_default();
    Method {
        id: decl.id.clone(),
        params: list.len(),
        varargs: list.iter().any(|p| p.kind() == "spread_parameter"),
        ret: decl
            .node
            .child_by_field_name("type")
            .map_or(Ty::Unknown("constructor"), |t| ctx.ty_of(t)),
    }
}

impl Model {
    /// The type of a field of the type or of its supertypes in the repository, nearest first.
    fn field(&self, type_id: &str, name: &str) -> Option<&Ty> {
        let mut level = vec![type_id];
        let mut seen = HashSet::new();
        while !level.is_empty() {
            let mut next = Vec::new();
            for id in level {
                let Some(info) = self.types.get(id).filter(|_| seen.insert(id)) else {
                    continue;
                };
                if let Some(ty) = info.fields.get(name) {
                    return Some(ty);
                }
                next.extend(info.supers.iter().map(String::as_str));
            }
            level = next;
        }
        None
    }

    /// The methods called `name` that fit `arguments`, declared in the type.
    /// If the type has none, those of its supertypes in the repository, nearest first.
    fn methods(&self, type_id: &str, name: &str, arguments: Option<usize>) -> Vec<&Method> {
        let mut level = vec![type_id];
        let mut seen = HashSet::new();
        while !level.is_empty() {
            let mut found = Vec::new();
            let mut next = Vec::new();
            for id in level {
                let Some(info) = self.types.get(id).filter(|_| seen.insert(id)) else {
                    continue;
                };
                let declared = info.methods.get(name).into_iter().flatten();
                found.extend(declared.filter(|m| m.fits(arguments)));
                next.extend(info.supers.iter().map(String::as_str));
            }
            if !found.is_empty() {
                return found;
            }
            level = next;
        }
        Vec::new()
    }
}

#[derive(Clone)]
enum Outcome<'a> {
    Function(&'a Method),
    Type(String),
    External,
    Unresolved(&'static str),
}

/// What is needed to read a type name in the source.
#[derive(Clone, Copy)]
struct TypeCtx<'a> {
    scope: &'a Scope<'a>,
    source: &'a str,
    /// The qualified names of the types around the name, outermost first.
    enclosing: &'a [String],
    type_parameters: &'a [String],
}

impl<'a> TypeCtx<'a> {
    /// The type that a type name in the source stands for.
    fn ty_of(&self, node: Ts<'a>) -> Ty {
        match node.kind() {
            "type_identifier" | "scoped_type_identifier" => {
                let name = text(node, self.source);
                if self.type_parameters.iter().any(|p| p == name) {
                    Ty::Unknown("type parameter")
                } else {
                    match self.scope.resolve(name, self.enclosing) {
                        Some(id) => Ty::Repo(id),
                        None => Ty::External,
                    }
                }
            }
            "generic_type" => children(node)
                .next()
                .map_or(Ty::Unknown("type"), |head| self.ty_of(head)),
            "array_type"
            | "integral_type"
            | "floating_point_type"
            | "boolean_type"
            | "void_type" => Ty::External,
            _ => Ty::Unknown("type"),
        }
    }
}

/// A body in which `this` and the unqualified names are looked up.
enum Frame {
    Type(String),
    /// The body of an anonymous or local class.
    Anonymous {
        supertype: Ty,
        methods: HashSet<String>,
        fields: HashMap<String, Ty>,
    },
}

pub(crate) struct Walker<'a> {
    model: &'a Model,
    ctx: TypeCtx<'a>,
    /// The ids of the types around the code, outermost first.
    frames: Vec<Frame>,
    /// The variables in scope, the innermost scope last.
    variables: Vec<HashMap<String, Ty>>,
    from: &'a str,
    /// The own type of a function: a call that reaches it gives no edge.
    from_parent: Option<&'a str>,
    report: &'a mut CallReport,
    found: &'a mut Vec<(String, String)>,
    seen: HashMap<usize, Outcome<'a>>,
}

impl<'a> Walker<'a> {
    pub(crate) fn run(
        model: &'a Model,
        unit: &'a Unit,
        scope: &'a Scope<'a>,
        decl: &'a Decl<'a>,
        report: &'a mut CallReport,
        found: &'a mut Vec<(String, String)>,
    ) {
        let ctx = TypeCtx {
            scope,
            source: &unit.source,
            enclosing: &decl.types,
            type_parameters: &decl.type_parameters,
        };
        let mut walker = Walker {
            model,
            ctx,
            frames: decl
                .types
                .iter()
                .map(|t| Frame::Type(type_id("", t)))
                .collect(),
            variables: vec![HashMap::new()],
            from: &decl.id,
            from_parent: decl
                .parent
                .as_deref()
                .filter(|_| decl.kind == NodeKind::Function),
            report,
            found,
            seen: HashMap::new(),
        };
        if decl.kind == NodeKind::Function {
            walker.walk(decl.node);
        } else {
            // Field initializers and initializer blocks; the functions and member
            // types are declarations of their own.
            for member in members(decl.node) {
                if !is_type_declaration(member) && !is_function_declaration(member) {
                    walker.walk(member);
                }
            }
        }
    }

    fn walk(&mut self, node: Ts<'a>) {
        match node.kind() {
            "object_creation_expression" => {
                self.outcome(node);
                let supertype = node.child_by_field_name("type").map(|t| self.ctx.ty_of(t));
                for child in children(node) {
                    match (child.kind(), &supertype) {
                        ("class_body", Some(supertype)) => {
                            self.walk_local_class(supertype.clone(), child);
                        }
                        _ => self.walk(child),
                    }
                }
            }
            "class_declaration"
            | "interface_declaration"
            | "enum_declaration"
            | "record_declaration" => {
                // A local class: its members hide those of the enclosing types.
                let superclass = children(node)
                    .find(|c| c.kind() == "superclass")
                    .and_then(|c| children(c).next())
                    .map_or(Ty::External, |t| self.ctx.ty_of(t));
                match node.child_by_field_name("body") {
                    Some(body) => self.walk_local_class(superclass, body),
                    None => self.walk_children(node),
                }
            }
            "method_invocation" | "method_reference" | "explicit_constructor_invocation" => {
                self.outcome(node);
                self.walk_children(node);
            }
            "method_declaration" | "constructor_declaration" => {
                self.variables.push(HashMap::new());
                if let Some(parameters) = node.child_by_field_name("parameters") {
                    self.declare_parameters(parameters);
                }
                self.walk_children(node);
                self.variables.pop();
            }
            "block" | "constructor_body" | "for_statement" => {
                self.variables.push(HashMap::new());
                self.walk_children(node);
                self.variables.pop();
            }
            "local_variable_declaration" => {
                let declared = node.child_by_field_name("type");
                for declarator in children(node).filter(|c| c.kind() == "variable_declarator") {
                    let value = declarator.child_by_field_name("value");
                    if let Some(value) = value {
                        self.walk(value);
                    }
                    if let (Some(name), Some(declared)) =
                        (declarator.child_by_field_name("name"), declared)
                    {
                        let ty = self.declared_type(declared, value);
                        self.declare(name, ty);
                    }
                }
            }
            "enhanced_for_statement" => {
                if let Some(value) = node.child_by_field_name("value") {
                    self.walk(value);
                }
                self.variables.push(HashMap::new());
                if let (Some(declared), Some(name)) = (
                    node.child_by_field_name("type"),
                    node.child_by_field_name("name"),
                ) {
                    let ty = self.declared_type(declared, None);
                    self.declare(name, ty);
                }
                if let Some(body) = node.child_by_field_name("body") {
                    self.walk(body);
                }
                self.variables.pop();
            }
            "catch_clause" => {
                self.variables.push(HashMap::new());
                if let Some(parameter) =
                    children(node).find(|c| c.kind() == "catch_formal_parameter")
                {
                    let alternatives: Vec<Ts> = children(parameter)
                        .filter(|c| c.kind() == "catch_type")
                        .flat_map(children)
                        .collect();
                    let ty = match alternatives.as_slice() {
                        [one] => self.ctx.ty_of(*one),
                        _ => Ty::Unknown("multi-catch"),
                    };
                    if let Some(name) = parameter.child_by_field_name("name") {
                        self.declare(name, ty);
                    }
                }
                self.walk_children(node);
                self.variables.pop();
            }
            "try_with_resources_statement" => {
                self.variables.push(HashMap::new());
                self.walk_children(node);
                self.variables.pop();
            }
            "resource" => {
                let value = node.child_by_field_name("value");
                if let Some(value) = value {
                    self.walk(value);
                }
                if let (Some(declared), Some(name)) = (
                    node.child_by_field_name("type"),
                    node.child_by_field_name("name"),
                ) {
                    let ty = self.declared_type(declared, value);
                    self.declare(name, ty);
                }
                if value.is_none() {
                    self.walk_children(node);
                }
            }
            "lambda_expression" => {
                self.variables.push(HashMap::new());
                match node.child_by_field_name("parameters") {
                    Some(parameters) if parameters.kind() == "formal_parameters" => {
                        self.declare_parameters(parameters);
                    }
                    Some(parameters) if parameters.kind() == "identifier" => {
                        self.declare(parameters, Ty::Unknown("lambda parameter"));
                    }
                    Some(parameters) => {
                        for parameter in children(parameters) {
                            self.declare(parameter, Ty::Unknown("lambda parameter"));
                        }
                    }
                    None => {}
                }
                if let Some(body) = node.child_by_field_name("body") {
                    self.walk(body);
                }
                self.variables.pop();
            }
            "instanceof_expression" => {
                if let Some(left) = node.child_by_field_name("left") {
                    self.walk(left);
                }
                if let (Some(declared), Some(name)) = (
                    node.child_by_field_name("right"),
                    node.child_by_field_name("name"),
                ) {
                    let ty = self.declared_type(declared, None);
                    self.declare(name, ty);
                }
            }
            "type_pattern" => {
                let mut parts = children(node);
                if let (Some(declared), Some(name)) = (parts.next(), parts.last()) {
                    let ty = self.declared_type(declared, None);
                    self.declare(name, ty);
                }
            }
            _ => self.walk_children(node),
        }
    }

    /// Walks the body of an anonymous or local class below its supertype.
    fn walk_local_class(&mut self, supertype: Ty, body: Ts<'a>) {
        let mut methods = HashSet::new();
        let mut fields = HashMap::new();
        for member in children(body) {
            match member.kind() {
                "method_declaration" => {
                    if let Some(name) = member.child_by_field_name("name") {
                        methods.insert(text(name, self.ctx.source).to_string());
                    }
                }
                "field_declaration" => {
                    let declared = member.child_by_field_name("type");
                    for declarator in children(member).filter(|c| c.kind() == "variable_declarator")
                    {
                        if let (Some(name), Some(declared)) =
                            (declarator.child_by_field_name("name"), declared)
                        {
                            fields.insert(
                                text(name, self.ctx.source).to_string(),
                                self.ctx.ty_of(declared),
                            );
                        }
                    }
                }
                _ => {}
            }
        }
        self.frames.push(Frame::Anonymous {
            supertype,
            methods,
            fields,
        });
        self.walk_children(body);
        self.frames.pop();
    }

    fn walk_children(&mut self, node: Ts<'a>) {
        for child in children(node) {
            self.walk(child);
        }
    }

    fn declare(&mut self, name: Ts<'a>, ty: Ty) {
        let name = text(name, self.ctx.source).to_string();
        if let Some(innermost) = self.variables.last_mut() {
            innermost.insert(name, ty);
        }
    }

    fn declare_parameters(&mut self, parameters: Ts<'a>) {
        for parameter in children(parameters) {
            let (ty, name) = match parameter.kind() {
                "formal_parameter" => (
                    parameter
                        .child_by_field_name("type")
                        .map(|t| self.declared_type(t, None)),
                    parameter.child_by_field_name("name"),
                ),
                "spread_parameter" => (
                    Some(Ty::External),
                    children(parameter)
                        .find(|c| c.kind() == "variable_declarator")
                        .and_then(|d| d.child_by_field_name("name")),
                ),
                _ => continue,
            };
            if let (Some(ty), Some(name)) = (ty, name) {
                self.declare(name, ty);
            }
        }
    }

    /// The type of a variable from its declaration.
    fn declared_type(&self, declared: Ts<'a>, initializer: Option<Ts<'a>>) -> Ty {
        if text(declared, self.ctx.source) != "var" {
            return self.ctx.ty_of(declared);
        }
        match initializer {
            Some(new) if new.kind() == "object_creation_expression" => new
                .child_by_field_name("type")
                .map_or(Ty::Unknown("var"), |t| self.ctx.ty_of(t)),
            _ => Ty::Unknown("var"),
        }
    }

    fn outcome(&mut self, node: Ts<'a>) -> Outcome<'a> {
        if let Some(known) = self.seen.get(&node.id()) {
            return known.clone();
        }
        let outcome = self.compute(node);
        match &outcome {
            Outcome::Function(method) => {
                self.report.to_function += 1;
                self.emit(method.id.clone());
            }
            Outcome::Type(id) => {
                self.report.to_type += 1;
                self.emit(id.clone());
            }
            Outcome::External => self.report.external += 1,
            Outcome::Unresolved(_) => self.report.unresolved += 1,
        }
        self.seen.insert(node.id(), outcome.clone());
        outcome
    }

    fn emit(&mut self, to: String) {
        if to != self.from && Some(to.as_str()) != self.from_parent {
            self.found.push((self.from.to_string(), to));
        }
    }

    fn compute(&mut self, node: Ts<'a>) -> Outcome<'a> {
        match node.kind() {
            "object_creation_expression" => {
                let arguments = argument_count(node);
                let Some(ty) = node.child_by_field_name("type") else {
                    return Outcome::Unresolved("creation without type");
                };
                match self.ctx.ty_of(ty) {
                    Ty::Repo(id) => self.constructor(&id, Some(arguments)),
                    Ty::External => Outcome::External,
                    Ty::Unknown(reason) => Outcome::Unresolved(reason),
                }
            }
            "method_invocation" => {
                let arguments = Some(argument_count(node));
                let Some(name) = node.child_by_field_name("name") else {
                    return Outcome::Unresolved("call without name");
                };
                let name = text(name, self.ctx.source);
                match node.child_by_field_name("object") {
                    Some(object) => {
                        let receiver = self.type_of(object);
                        self.by_receiver(&receiver, name, arguments)
                    }
                    None => self.unqualified(name, arguments),
                }
            }
            "explicit_constructor_invocation" => {
                // `outer.super(...)` has an object; its type is not the one of `super`.
                let target = match node.child_by_field_name("constructor") {
                    Some(c) if node.child_by_field_name("object").is_none() => self.type_of(c),
                    _ => Ty::Unknown("qualified super constructor call"),
                };
                match target {
                    Ty::Repo(id) => self.constructor(&id, Some(argument_count(node))),
                    Ty::External => Outcome::External,
                    Ty::Unknown(reason) => Outcome::Unresolved(reason),
                }
            }
            "method_reference" => {
                let mut parts = children(node);
                let Some(receiver) = parts.next() else {
                    return Outcome::Unresolved("method reference without receiver");
                };
                // `T::new` has no name; the receiver can be a type or an expression.
                let name = parts.last().filter(|n| n.kind() == "identifier");
                let ty = match receiver.kind() {
                    "generic_type"
                    | "array_type"
                    | "scoped_type_identifier"
                    | "type_identifier" => self.ctx.ty_of(receiver),
                    _ => self.type_of(receiver),
                };
                match (name, ty) {
                    (None, Ty::Repo(id)) => self.constructor(&id, None),
                    (Some(name), ty) => {
                        let name = text(name, self.ctx.source);
                        self.by_receiver(&ty, name, None)
                    }
                    (None, Ty::External) => Outcome::External,
                    (None, Ty::Unknown(reason)) => Outcome::Unresolved(reason),
                }
            }
            _ => Outcome::Unresolved("not a call"),
        }
    }

    /// A call `m(args)`: the innermost enclosing type that has the method.
    fn unqualified(&self, name: &str, arguments: Option<usize>) -> Outcome<'a> {
        for frame in self.frames.iter().rev() {
            let id = match frame {
                Frame::Type(id) => id,
                Frame::Anonymous {
                    methods, supertype, ..
                } => {
                    if methods.contains(name) {
                        return Outcome::Unresolved("member of an anonymous class");
                    }
                    match supertype {
                        Ty::Repo(id) => id,
                        _ => continue,
                    }
                }
            };
            match self.model.methods(id, name, arguments).as_slice() {
                [] => {}
                [one] => return Outcome::Function(one),
                _ => return Outcome::Type(id.clone()),
            }
        }
        // A static import that names the method, then those with `*`.
        for (holder, member) in &self.ctx.scope.static_members {
            if *member != Some(name) {
                continue;
            }
            return match self.ctx.scope.resolve_qualified(holder) {
                Some(id) => self.by_receiver(&Ty::Repo(id), name, arguments),
                None => Outcome::External,
            };
        }
        for (holder, _) in self
            .ctx
            .scope
            .static_members
            .iter()
            .filter(|(_, m)| m.is_none())
        {
            let Some(id) = self.ctx.scope.resolve_qualified(holder) else {
                continue;
            };
            match self.model.methods(&id, name, arguments).as_slice() {
                [] => {}
                [one] => return Outcome::Function(one),
                _ => return Outcome::Type(id),
            }
        }
        if is_object_method(name, arguments) {
            return Outcome::External;
        }
        Outcome::Unresolved("no enclosing type has the method")
    }

    /// The method of the receiver's type that fits, else the type itself.
    fn by_receiver(&self, receiver: &Ty, name: &str, arguments: Option<usize>) -> Outcome<'a> {
        match receiver {
            Ty::Repo(id) => match self.model.methods(id, name, arguments).as_slice() {
                [one] => Outcome::Function(one),
                [] if is_object_method(name, arguments) => Outcome::External,
                _ => Outcome::Type(id.clone()),
            },
            Ty::External => Outcome::External,
            Ty::Unknown(reason) => Outcome::Unresolved(reason),
        }
    }

    /// The type of `a.f`, where `a` is an expression or a type, or of a qualified name.
    fn field_access(&mut self, expression: Ts<'a>) -> Ty {
        let (Some(object), Some(field)) = (
            expression.child_by_field_name("object"),
            expression.child_by_field_name("field"),
        ) else {
            return Ty::Unknown("field access");
        };
        // A type written in full, `a.b.T` or `Outer.Inner`, unless a variable has its head.
        let written: String = text(expression, self.ctx.source)
            .split_whitespace()
            .collect();
        let head = written.split('.').next().unwrap_or_default();
        let is_name = written
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '.' | '_' | '$'));
        if is_name && !self.is_variable(head) {
            if let Some(id) = self.ctx.scope.resolve(&written, self.ctx.enclosing) {
                return Ty::Repo(id);
            }
            // A type of another repository, written with its package: `java.util.List`.
            let package = head.starts_with(|c: char| c.is_lowercase());
            if package && looks_like_type(text(field, self.ctx.source)) {
                return Ty::External;
            }
        }
        let owner = self.type_of(object);
        let name = text(field, self.ctx.source);
        match owner {
            // `Outer.this` is the enclosing object of type `Outer`.
            Ty::Repo(_) if field.kind() == "this" => owner,
            Ty::Repo(id) => self
                .model
                .field(&id, name)
                .cloned()
                .unwrap_or(Ty::Unknown("unknown field")),
            Ty::External => Ty::Unknown("field of an external type"),
            Ty::Unknown(reason) => Ty::Unknown(reason),
        }
    }

    /// Whether `name` is a local variable or a field of an enclosing type.
    fn is_variable(&self, name: &str) -> bool {
        self.variables.iter().any(|scope| scope.contains_key(name))
            || self
                .frames
                .iter()
                .any(|frame| self.field_of(frame, name).is_some())
    }

    /// The type of the field `name` of the frame's type, if it has such a field.
    fn field_of(&self, frame: &Frame, name: &str) -> Option<Ty> {
        match frame {
            Frame::Type(id) => self.model.field(id, name).cloned(),
            Frame::Anonymous {
                supertype, fields, ..
            } => fields.get(name).cloned().or_else(|| match supertype {
                Ty::Repo(id) => self.model.field(id, name).cloned(),
                _ => None,
            }),
        }
    }

    /// The static type of an expression.
    fn type_of(&mut self, expression: Ts<'a>) -> Ty {
        match expression.kind() {
            "identifier" => {
                let name = text(expression, self.ctx.source);
                if let Some(ty) = self
                    .variables
                    .iter()
                    .rev()
                    .find_map(|scope| scope.get(name))
                {
                    return ty.clone();
                }
                let field = self
                    .frames
                    .iter()
                    .rev()
                    .find_map(|f| self.field_of(f, name));
                if let Some(ty) = field {
                    return ty;
                }
                match self.ctx.scope.resolve(name, self.ctx.enclosing) {
                    Some(id) => Ty::Repo(id),
                    None if looks_like_type(name) => Ty::External,
                    None => Ty::Unknown("unknown name"),
                }
            }
            "string_literal"
            | "text_block"
            | "character_literal"
            | "decimal_integer_literal"
            | "hex_integer_literal"
            | "octal_integer_literal"
            | "binary_integer_literal"
            | "decimal_floating_point_literal"
            | "hex_floating_point_literal"
            | "true"
            | "false"
            | "class_literal" => Ty::External,
            "field_access" => self.field_access(expression),
            "method_invocation" => match self.outcome(expression) {
                Outcome::Function(method) => method.ret.clone(),
                Outcome::Type(_) => Ty::Unknown("overloaded call"),
                Outcome::External => Ty::Unknown("call on an external type"),
                Outcome::Unresolved(reason) => Ty::Unknown(reason),
            },
            "object_creation_expression" | "cast_expression" => expression
                .child_by_field_name("type")
                .map_or(Ty::Unknown("type"), |t| self.ctx.ty_of(t)),
            "parenthesized_expression" => children(expression)
                .next()
                .map_or(Ty::Unknown("expression"), |inner| self.type_of(inner)),
            "super" => match self.frames.last() {
                Some(Frame::Type(id)) => self
                    .model
                    .types
                    .get(id)
                    .and_then(|info| info.superclass.clone())
                    .unwrap_or(Ty::Unknown("super outside a class")),
                Some(Frame::Anonymous { supertype, .. }) => match supertype {
                    Ty::Repo(id) => Ty::Repo(id.clone()),
                    _ => Ty::External,
                },
                None => Ty::Unknown("super outside a class"),
            },
            "this" => match self.frames.last() {
                Some(Frame::Type(id)) => Ty::Repo(id.clone()),
                Some(Frame::Anonymous { .. }) => Ty::Unknown("this of an anonymous class"),
                None => Ty::Unknown("this outside a type"),
            },
            _ => Ty::Unknown("expression"),
        }
    }

    /// The constructor of `type_id` that fits, else the type itself.
    fn constructor(&self, type_id: &str, arguments: Option<usize>) -> Outcome<'a> {
        let fitting: Vec<&Method> = self
            .model
            .types
            .get(type_id)
            .map(|t| {
                t.constructors
                    .iter()
                    .filter(|c| c.fits(arguments))
                    .collect()
            })
            .unwrap_or_default();
        match fitting.as_slice() {
            [one] => Outcome::Function(one),
            _ => Outcome::Type(type_id.to_string()),
        }
    }
}

fn argument_count(call: Ts) -> usize {
    call.child_by_field_name("arguments")
        .map_or(0, |arguments| children(arguments).count())
}

/// The methods that every class has from `java.lang.Object`. A call of one of them that
/// the type does not override does not reach the type.
fn is_object_method(name: &str, arguments: Option<usize>) -> bool {
    let arities: &[usize] = match name {
        "toString" | "hashCode" | "getClass" | "clone" | "notify" | "notifyAll" | "finalize" => {
            &[0]
        }
        "equals" => &[1],
        "wait" => &[0, 1, 2],
        _ => return false,
    };
    arguments.is_none_or(|n| arities.contains(&n))
}

/// Whether a name that no variable has and the repository does not declare is a type of
/// the JDK or of a library: it is written as a class name.
fn looks_like_type(name: &str) -> bool {
    name.starts_with(|c: char| c.is_uppercase())
}
