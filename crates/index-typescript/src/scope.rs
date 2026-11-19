//! The names that a scope binds (ADR 36): a file, a namespace, a function, a class.

use crate::syntax::{named_children, namespace_parts, text};
use std::collections::{HashMap, HashSet};
use tree_sitter::Node as Ts;

/// What a scope is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    File,
    Namespace,
    Function,
    Class,
}

/// How a name is declared.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum How {
    /// `function f() {}`, also an overload signature.
    Function,
    /// `const x = ..`.
    Const,
    Class,
    Namespace,
    /// `let x`, `var x` (can be assigned again), a parameter, an enum, a binding of
    /// `catch` or `for`, two declarations of the name.
    Other,
}

/// What a variable is written to be: the type of its annotation, or the class of `new T()`.
#[derive(Clone, Copy)]
pub struct Typing<'t> {
    pub node: Ts<'t>,
    pub constructed: bool,
}

#[derive(Clone)]
pub struct Binding<'t> {
    pub how: How,
    pub typing: Option<Typing<'t>>,
}

/// The names of a scope. Values and types are looked up apart.
pub struct Scope<'t> {
    pub kind: Kind,
    /// The namespaces around a file or namespace scope, including its own name.
    pub path: Vec<String>,
    pub values: HashMap<String, Binding<'t>>,
    /// The type parameters and the types that a function declares.
    pub types: HashSet<String>,
}

impl<'t> Scope<'t> {
    fn new(kind: Kind, path: Vec<String>) -> Self {
        Scope {
            kind,
            path,
            values: HashMap::new(),
            types: HashSet::new(),
        }
    }

    /// The scope of a function: its parameters, its own name if it is an expression, and
    /// the names that its body declares outside the functions in it.
    pub fn of_function(function: Ts<'t>, source: &'t str) -> Self {
        let mut scope = Scope::new(Kind::Function, Vec::new());
        let mut collector = Collector {
            scope: &mut scope,
            source,
        };
        collector.parameters(function);
        if matches!(
            function.kind(),
            "function_expression" | "generator_function"
        ) {
            collector.name(function, How::Other);
        }
        if let Some(body) = function.child_by_field_name("body") {
            collector.visit(body);
        }
        scope
    }

    /// The scope of a class: its type parameters, and the name of a class expression.
    pub fn of_class(class: Ts<'t>, source: &'t str) -> Self {
        let mut scope = Scope::new(Kind::Class, Vec::new());
        let mut collector = Collector {
            scope: &mut scope,
            source,
        };
        collector.type_parameters(class);
        if class.kind() == "class" {
            collector.name(class, How::Other);
            collector.type_name(class);
        }
        scope
    }

    /// The scope of the statements that the children of `holder` are: a file or a namespace.
    pub fn of_statements(kind: Kind, path: Vec<String>, holder: Ts<'t>, source: &'t str) -> Self {
        let mut scope = Scope::new(kind, path);
        let mut collector = Collector {
            scope: &mut scope,
            source,
        };
        for statement in named_children(holder) {
            collector.visit(statement);
        }
        scope
    }

    /// Takes the names of a second body of the same namespace. Which body sees which name
    /// is not followed: every name of either body becomes one that gives no edge.
    pub fn absorb(&mut self, other: Scope<'t>) {
        let unknown = Binding {
            how: How::Other,
            typing: None,
        };
        self.values.extend(other.values);
        for binding in self.values.values_mut() {
            *binding = unknown.clone();
        }
        self.types.extend(other.types);
    }

    fn bind(&mut self, name: &str, binding: Binding<'t>, source: &str) {
        match self.values.get_mut(name) {
            None => {
                self.values.insert(name.to_string(), binding);
            }
            Some(old) => *old = merged(old, binding, source),
        }
    }
}

/// The scopes of the namespaces among the statements of `holder`, which are in the
/// namespace `path`, and of the namespaces in them.
pub fn namespace_scopes<'t>(
    holder: Ts<'t>,
    path: &[String],
    source: &'t str,
    out: &mut Vec<Scope<'t>>,
) {
    for statement in named_children(holder) {
        namespace_in(statement, path, source, out);
    }
}

fn namespace_in<'t>(node: Ts<'t>, path: &[String], source: &'t str, out: &mut Vec<Scope<'t>>) {
    match node.kind() {
        "export_statement" | "ambient_declaration" | "expression_statement" => {
            for child in named_children(node) {
                namespace_in(child, path, source, out);
            }
        }
        "internal_module" | "module" => {
            let parts = namespace_parts(node, source);
            if let (Some(parts), Some(body)) = (parts, node.child_by_field_name("body")) {
                let mut inner = path.to_vec();
                inner.extend(parts.into_iter().map(str::to_string));
                out.push(Scope::of_statements(
                    Kind::Namespace,
                    inner.clone(),
                    body,
                    source,
                ));
                namespace_scopes(body, &inner, source, out);
            }
        }
        _ => {}
    }
}

/// Two declarations of one name in a scope.
fn merged<'t>(old: &Binding<'t>, new: Binding<'t>, source: &str) -> Binding<'t> {
    let same_kind =
        old.how == new.how && matches!(old.how, How::Function | How::Class | How::Namespace);
    let same_typing = match (old.typing, new.typing) {
        (Some(a), Some(b)) => {
            a.constructed == b.constructed && text(a.node, source) == text(b.node, source)
        }
        _ => false,
    };
    Binding {
        how: if same_kind { old.how } else { How::Other },
        typing: if same_typing { old.typing } else { None },
    }
}

/// Reads the names that a body declares, outside the functions in it.
struct Collector<'s, 't> {
    scope: &'s mut Scope<'t>,
    source: &'t str,
}

impl<'t> Collector<'_, 't> {
    fn visit(&mut self, node: Ts<'t>) {
        match node.kind() {
            "function_declaration"
            | "generator_function_declaration"
            | "function_signature"
            | "class_declaration"
            | "abstract_class_declaration"
            | "interface_declaration"
            | "type_alias_declaration"
            | "enum_declaration" => self.declaration(node),
            kind if is_own_scope(kind) => {}
            "internal_module" | "module" | "import_alias" => self.namespace_name(node),
            "variable_declarator" => self.declarator(node),
            "catch_clause" | "for_in_statement" => self.binding_statement(node),
            _ => self.children(node),
        }
    }

    /// A function, class, interface, alias or enum: a value name for a function, class or
    /// enum, and a type name for all but a function.
    fn declaration(&mut self, node: Ts<'t>) {
        match node.kind() {
            "class_declaration" | "abstract_class_declaration" => self.name(node, How::Class),
            "enum_declaration" => self.name(node, How::Other),
            "interface_declaration" | "type_alias_declaration" => {}
            _ => return self.name(node, How::Function),
        }
        self.type_name(node);
    }

    /// `catch (e)` binds `e`; `for (const x of y)` binds `x`, but `for (x of y)` does not.
    fn binding_statement(&mut self, node: Ts<'t>) {
        let bound = if node.kind() == "catch_clause" {
            node.child_by_field_name("parameter")
        } else if node.child_by_field_name("kind").is_some() {
            node.child_by_field_name("left")
        } else {
            None
        };
        if let Some(pattern) = bound {
            self.pattern(pattern);
        }
        self.children(node);
    }

    /// The first name of a namespace: its body is a scope of its own.
    fn namespace(&mut self, node: Ts<'t>) {
        if let Some(first) = namespace_parts(node, self.source).and_then(|p| p.first().copied()) {
            let binding = Binding {
                how: How::Namespace,
                typing: None,
            };
            self.scope.bind(first, binding, self.source);
        }
    }

    /// The name that a namespace or an `import x = A.b` binds.
    fn namespace_name(&mut self, node: Ts<'t>) {
        if node.kind() == "import_alias" {
            self.alias(node);
        } else {
            self.namespace(node);
        }
    }

    /// `import x = A.b`: `x` is a name that we do not follow.
    fn alias(&mut self, node: Ts<'t>) {
        let name = named_children(node).into_iter().next();
        if let Some(name) = name.filter(|n| n.kind() == "identifier") {
            let binding = Binding {
                how: How::Other,
                typing: None,
            };
            self.scope
                .bind(text(name, self.source), binding, self.source);
        }
    }

    fn children(&mut self, node: Ts<'t>) {
        for child in named_children(node) {
            self.visit(child);
        }
    }

    /// The parameters of a function, and its type parameters.
    fn parameters(&mut self, function: Ts<'t>) {
        self.type_parameters(function);
        if let Some(parameter) = function.child_by_field_name("parameter") {
            self.pattern(parameter);
        }
        let list = function.child_by_field_name("parameters");
        for parameter in list.map(named_children).unwrap_or_default() {
            if let Some(pattern) = parameter.child_by_field_name("pattern") {
                let typing = parameter.child_by_field_name("type").and_then(annotation);
                self.bind_pattern(pattern, typing);
            }
        }
    }

    /// The names of the type parameters of a function or class.
    fn type_parameters(&mut self, node: Ts<'t>) {
        let list = node.child_by_field_name("type_parameters");
        for parameter in list.map(named_children).unwrap_or_default() {
            if let Some(name) = parameter.child_by_field_name("name") {
                self.scope.types.insert(text(name, self.source).to_string());
            }
        }
    }

    /// A pattern with the type that is written for it: only a plain name has it.
    fn bind_pattern(&mut self, node: Ts<'t>, typing: Option<Typing<'t>>) {
        if node.kind() == "identifier" {
            let binding = Binding {
                how: How::Other,
                typing,
            };
            self.scope
                .bind(text(node, self.source), binding, self.source);
        } else {
            self.pattern(node);
        }
    }

    /// The names that a pattern binds: `x`, `{ a, b: c }`, `[a, ...rest]`.
    fn pattern(&mut self, node: Ts<'t>) {
        match node.kind() {
            "identifier" | "shorthand_property_identifier_pattern" => {
                let binding = Binding {
                    how: How::Other,
                    typing: None,
                };
                self.scope
                    .bind(text(node, self.source), binding, self.source);
            }
            "pair_pattern" => self.pattern_of(node, "value"),
            "object_assignment_pattern" | "assignment_pattern" => self.pattern_of(node, "left"),
            "object_pattern" | "array_pattern" | "rest_pattern" => {
                for child in named_children(node) {
                    self.pattern(child);
                }
            }
            _ => {}
        }
    }

    fn pattern_of(&mut self, node: Ts<'t>, field: &str) {
        if let Some(inner) = node.child_by_field_name(field) {
            self.pattern(inner);
        }
    }

    /// The name of a declaration that declares a type.
    fn type_name(&mut self, node: Ts<'t>) {
        if let Some(name) = node.child_by_field_name("name") {
            self.scope.types.insert(text(name, self.source).to_string());
        }
    }

    /// The name of a declaration.
    fn name(&mut self, node: Ts<'t>, how: How) {
        if let Some(name) = node.child_by_field_name("name") {
            let binding = Binding { how, typing: None };
            self.scope
                .bind(text(name, self.source), binding, self.source);
        }
    }

    /// `x = ..` of a `const`, `let` or `var`.
    fn declarator(&mut self, node: Ts<'t>) {
        let Some(name) = node.child_by_field_name("name") else {
            return;
        };
        if name.kind() != "identifier" {
            self.pattern(name);
            return;
        }
        let constant = node.parent().is_some_and(is_const);
        let how = if constant { How::Const } else { How::Other };
        let typing = declared_typing(node, constant);
        let binding = Binding { how, typing };
        self.scope
            .bind(text(name, self.source), binding, self.source);
    }
}

/// The type of a variable or field `node`: the annotation, else the class of `new T()`
/// if `new_counts`.
pub fn declared_typing(node: Ts, new_counts: bool) -> Option<Typing> {
    if let Some(written) = node.child_by_field_name("type") {
        return annotation(written);
    }
    let value = node.child_by_field_name("value")?;
    let class = value.child_by_field_name("constructor");
    class
        .filter(|_| new_counts && value.kind() == "new_expression")
        .map(|node| Typing {
            node,
            constructed: true,
        })
}

/// The type that a `type_annotation` writes.
fn annotation(node: Ts) -> Option<Typing> {
    named_children(node).into_iter().next().map(|node| Typing {
        node,
        constructed: false,
    })
}

/// A kind of node that is a class.
pub fn is_class(kind: &str) -> bool {
    matches!(
        kind,
        "class_declaration" | "abstract_class_declaration" | "class"
    )
}

/// A kind of node whose names are not those of the scope around it: a function or a class
/// expression.
fn is_own_scope(kind: &str) -> bool {
    is_function_like(kind) || kind == "class"
}

/// A kind of node that is a function with a scope of its own, or a static block, which
/// has one too.
pub fn is_function_like(kind: &str) -> bool {
    matches!(
        kind,
        "class_static_block"
            | "function_declaration"
            | "generator_function_declaration"
            | "function_expression"
            | "generator_function"
            | "arrow_function"
            | "method_definition"
    )
}

/// Whether `declaration` is a `const`.
fn is_const(declaration: Ts) -> bool {
    declaration.kind() == "lexical_declaration"
        && declaration.child(0).is_some_and(|c| c.kind() == "const")
}
