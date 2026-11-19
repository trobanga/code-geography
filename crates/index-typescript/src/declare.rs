//! Pass 1: the type and function nodes of the files.

use crate::layout::Source;
use crate::syntax::{named_children, text};
use cg_core::{NodeKind, Span};
use std::collections::HashMap;
use tree_sitter::Node as Ts;

/// A type or function that becomes a node.
pub struct Decl<'t> {
    pub id: String,
    pub kind: NodeKind,
    /// The name that the node shows: `Foo`, `open`.
    pub name: String,
    pub parent: String,
    /// The index of the file in whose source the declaration stands.
    pub source: usize,
    /// The syntax node of the declaration itself (the class, the function, the declarator).
    pub node: Ts<'t>,
    /// The namespaces around the declaration, outermost first. A member of a type has those
    /// of the type.
    pub namespace: Vec<String>,
    pub test: bool,
    pub hash: String,
    pub span: Span,
    /// A function with a body replaces an overload signature of the same id.
    pub has_body: bool,
    /// The syntax nodes of the declarations of the same id that this one absorbed: the
    /// setter beside a getter, a `static m` beside `m`, an overload signature.
    pub also: Vec<Ts<'t>>,
}

/// The id of the node of a package: the module node for the `index.ts` of its directory.
pub fn container_id(module: &str, package: &str) -> String {
    if package.is_empty() {
        format!("mod:{module}")
    } else {
        format!("pkg:{module}:/{package}")
    }
}

/// The declarations of a file that are nodes.
struct Declarer<'a, 't> {
    source: &'a Source<'a>,
    index: usize,
    /// `<dir>:/<path>`, the part of each id below the module.
    prefix: String,
    found: Vec<Decl<'t>>,
    namespaces: Vec<Namespace>,
}

/// A `namespace`: a package below the package of its file.
pub struct Namespace {
    pub id: String,
    pub name: String,
    pub parent: String,
    pub test: bool,
}

/// What pass 1 found in all files.
pub struct Declared<'t> {
    pub decls: Vec<Decl<'t>>,
    pub namespaces: Vec<Namespace>,
}

/// Where a statement stands: the node that holds it.
#[derive(Clone)]
struct Scope {
    container: String,
    /// The namespaces around the statement, outermost first.
    namespace: Vec<String>,
}

/// A member of a class or interface that is a function node.
struct Member<'t> {
    name: String,
    node: Ts<'t>,
    has_body: bool,
}

/// The name of a member, if it is written out: an identifier, a private name or a string.
pub(crate) fn member_name(name: Ts, source: &str) -> Option<String> {
    let written = text(name, source);
    match name.kind() {
        "property_identifier" | "private_property_identifier" => Some(written.to_string()),
        "string" => Some(crate::bindings::unquoted(name, source).to_string()),
        _ => None,
    }
}

/// A kind of node that declares a type.
fn is_type_kind(kind: &str) -> bool {
    matches!(
        kind,
        "class_declaration"
            | "abstract_class_declaration"
            | "interface_declaration"
            | "type_alias_declaration"
            | "enum_declaration"
    )
}

/// A kind of node that declares a function or an overload signature.
fn is_function_kind(kind: &str) -> bool {
    matches!(
        kind,
        "function_declaration" | "generator_function_declaration" | "function_signature"
    )
}

/// A kind of node that declares variables.
fn is_variable_kind(kind: &str) -> bool {
    matches!(kind, "lexical_declaration" | "variable_declaration")
}

/// A kind of node that has members: a class or an interface.
pub(crate) fn has_members_kind(kind: &str) -> bool {
    matches!(
        kind,
        "class" | "class_declaration" | "abstract_class_declaration" | "interface_declaration"
    )
}

/// Whether a value is an arrow function or a function expression.
fn is_function_value(value: Ts) -> bool {
    matches!(value.kind(), "arrow_function" | "function_expression")
}

/// Declares the types and the functions of each file, in the order of `sources` and of the
/// source in a file.
pub fn declare<'t>(sources: &'t [Source]) -> Declared<'t> {
    let mut found = Vec::new();
    let mut namespaces: Vec<Namespace> = Vec::new();
    for (index, source) in sources.iter().enumerate() {
        let mut declarer = Declarer {
            source,
            index,
            prefix: format!("{}:/{}", source.module, source.package),
            found: Vec::new(),
            namespaces: Vec::new(),
        };
        let scope = Scope {
            container: container_id(source.module, &source.package),
            namespace: Vec::new(),
        };
        for statement in named_children(source.tree.root_node()) {
            declarer.statement(&scope, statement, statement);
        }
        found.extend(declarer.found);
        namespaces.extend(declarer.namespaces);
    }
    // The same namespace in two files of one package is one node.
    let mut seen = std::collections::HashSet::new();
    namespaces.retain(|n| seen.insert(n.id.clone()));
    Declared {
        decls: dedup(found),
        namespaces,
    }
}

/// One id, one node: the first declaration gives it, but a declaration with a body replaces
/// an overload signature.
fn dedup(declarations: Vec<Decl>) -> Vec<Decl> {
    let mut at: HashMap<String, usize> = HashMap::new();
    let mut kept: Vec<Decl> = Vec::new();
    for decl in declarations {
        match at.get(&decl.id) {
            Some(&position) => {
                let mut decl = decl;
                if decl.has_body && !kept[position].has_body {
                    std::mem::swap(&mut kept[position], &mut decl);
                }
                kept[position].also.push(decl.node);
                kept[position].also.extend(decl.also);
            }
            None => {
                at.insert(decl.id.clone(), kept.len());
                kept.push(decl);
            }
        }
    }
    kept
}

impl<'t> Declarer<'_, 't> {
    /// A statement of a file or of a namespace. `outer` is the outermost statement node,
    /// where the span starts.
    fn statement(&mut self, scope: &Scope, outer: Ts<'t>, node: Ts<'t>) {
        match node.kind() {
            "export_statement" => self.export_statement(scope, outer, node),
            "ambient_declaration" | "expression_statement" => {
                for inner in named_children(node) {
                    self.statement(scope, outer, inner);
                }
            }
            "internal_module" | "module" => self.namespace(scope, node),
            _ => self.declaration_statement(scope, outer, node),
        }
    }

    /// A statement that declares a type, a function or variables.
    fn declaration_statement(&mut self, scope: &Scope, outer: Ts<'t>, node: Ts<'t>) {
        match node.kind() {
            kind if is_type_kind(kind) => self.type_statement(scope, outer, node),
            kind if is_function_kind(kind) => self.function_decl(scope, outer, node),
            kind if is_variable_kind(kind) => self.variable_decl(scope, outer, node),
            _ => {}
        }
    }

    /// A class, interface, type alias or enum with a name.
    fn type_statement(&mut self, scope: &Scope, outer: Ts<'t>, node: Ts<'t>) {
        if let Some(name) = node.child_by_field_name("name") {
            self.type_decl(scope, outer, node, text(name, self.source.text));
        }
    }

    /// `export <declaration>` or `export default <expression>`.
    fn export_statement(&mut self, scope: &Scope, outer: Ts<'t>, node: Ts<'t>) {
        if let Some(inner) = node.child_by_field_name("declaration") {
            self.statement(scope, outer, inner);
        } else if let Some(value) = node.child_by_field_name("value") {
            // `export default <expression>`: only a class or a function is a node.
            match value.kind() {
                "class" => self.type_decl(scope, outer, value, "default"),
                "function_expression" | "generator_function" | "arrow_function" => {
                    let id = format!("fn:{}#{}", self.prefix, self.qualified(scope, "default"));
                    self.push(
                        NodeKind::Function,
                        id,
                        "default",
                        scope,
                        outer,
                        value,
                        true,
                        &[],
                    );
                }
                _ => {}
            }
        }
    }

    /// A function declaration or an overload signature.
    fn function_decl(&mut self, scope: &Scope, outer: Ts<'t>, node: Ts<'t>) {
        let Some(name) = node.child_by_field_name("name") else {
            return;
        };
        let name = text(name, self.source.text);
        let id = format!("fn:{}#{}", self.prefix, self.qualified(scope, name));
        let has_body = node.kind() != "function_signature";
        self.push(
            NodeKind::Function,
            id,
            name,
            scope,
            outer,
            node,
            has_body,
            &[],
        );
    }

    /// The declarators of a `const`, `let` or `var` whose value is a function.
    fn variable_decl(&mut self, scope: &Scope, outer: Ts<'t>, node: Ts<'t>) {
        let declarators: Vec<Ts<'t>> = named_children(node)
            .into_iter()
            .filter(|c| c.kind() == "variable_declarator")
            .collect();
        for declarator in &declarators {
            let is_function = declarator
                .child_by_field_name("value")
                .is_some_and(is_function_value);
            let name = declarator.child_by_field_name("name");
            let Some(name) = name.filter(|n| is_function && n.kind() == "identifier") else {
                continue;
            };
            let name = text(name, self.source.text);
            let id = format!("fn:{}#{}", self.prefix, self.qualified(scope, name));
            // One of several declarators has the span of its own.
            let span_node = if declarators.len() > 1 {
                *declarator
            } else {
                outer
            };
            self.push(
                NodeKind::Function,
                id,
                name,
                scope,
                span_node,
                *declarator,
                true,
                &[],
            );
        }
    }

    /// `N.M.name`: the name below the namespaces around it.
    fn qualified(&self, scope: &Scope, name: &str) -> String {
        scope
            .namespace
            .iter()
            .map(String::as_str)
            .chain([name])
            .collect::<Vec<_>>()
            .join(".")
    }

    /// A namespace: a package for each of its names, and its statements.
    fn namespace(&mut self, scope: &Scope, node: Ts<'t>) {
        // `declare module "name" {}` declares things outside the file: no parts.
        let (Some(parts), Some(body)) = (
            crate::syntax::namespace_parts(node, self.source.text),
            node.child_by_field_name("body"),
        ) else {
            return;
        };
        let mut scope = scope.clone();
        for part in parts {
            scope.namespace.push(part.to_string());
            let id = format!("pkg:{}#{}", self.prefix, scope.namespace.join("."));
            self.namespaces.push(Namespace {
                id: id.clone(),
                name: part.to_string(),
                parent: scope.container.clone(),
                test: self.source.test,
            });
            scope.container = id;
        }
        for statement in named_children(body) {
            self.statement(&scope, statement, statement);
        }
    }

    /// A type and its members.
    fn type_decl(&mut self, scope: &Scope, outer: Ts<'t>, node: Ts<'t>, name: &str) {
        let qualified = self.qualified(scope, name);
        let id = format!("type:{}#{qualified}", self.prefix);
        let members = self.members(node);
        let cut: Vec<Ts<'t>> = members.iter().map(|m| m.node).collect();
        self.push(
            NodeKind::Type,
            id.clone(),
            name,
            scope,
            outer,
            node,
            true,
            &cut,
        );
        for member in members {
            let member_id = format!("fn:{}#{qualified}.{}", self.prefix, member.name);
            let member_scope = Scope {
                container: id.clone(),
                namespace: scope.namespace.clone(),
            };
            self.push(
                NodeKind::Function,
                member_id,
                &member.name,
                &member_scope,
                member.node,
                member.node,
                member.has_body,
                &[],
            );
        }
    }

    /// The members of a class or interface that are function nodes: methods, accessors,
    /// signatures, and fields whose value is a function.
    fn members(&self, node: Ts<'t>) -> Vec<Member<'t>> {
        let body = Some(node)
            .filter(|n| has_members_kind(n.kind()))
            .and_then(|n| n.child_by_field_name("body"));
        body.map(named_children)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|member| self.member(member))
            .collect()
    }

    /// `member` as a function node, if it is one and has a written name.
    fn member(&self, member: Ts<'t>) -> Option<Member<'t>> {
        let has_body = match member.kind() {
            "method_definition" => true,
            "method_signature" | "abstract_method_signature" => false,
            "public_field_definition"
                if member
                    .child_by_field_name("value")
                    .is_some_and(is_function_value) =>
            {
                true
            }
            _ => return None,
        };
        let name = member_name(member.child_by_field_name("name")?, self.source.text)?;
        Some(Member {
            name,
            node: member,
            has_body,
        })
    }

    /// Pushes a declaration. The hash is that of the text of `outer` without the texts of
    /// `cut`, the member nodes.
    #[allow(clippy::too_many_arguments)]
    fn push(
        &mut self,
        kind: NodeKind,
        id: String,
        name: &str,
        scope: &Scope,
        outer: Ts<'t>,
        node: Ts<'t>,
        has_body: bool,
        cut: &[Ts<'t>],
    ) {
        let mut own = String::new();
        let mut position = outer.start_byte();
        for member in cut {
            own.push_str(&self.source.text[position..member.start_byte()]);
            position = member.end_byte();
        }
        own.push_str(&self.source.text[position..outer.end_byte()]);
        self.found.push(Decl {
            id,
            kind,
            name: name.to_string(),
            parent: scope.container.clone(),
            source: self.index,
            node,
            namespace: scope.namespace.clone(),
            test: self.source.test,
            hash: cg_core::text_hash(&own),
            span: Span {
                start_line: outer.start_position().row as u32 + 1,
                end_line: outer.end_position().row as u32 + 1,
            },
            has_body,
            also: Vec::new(),
        });
    }
}
