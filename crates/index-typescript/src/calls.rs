//! Pass 3: the `calls` edges (ADR 36).

use crate::bindings::line;
use crate::edges::{Collected, Files};
use crate::scope::{Scope, is_class, is_function_like};
use crate::subject::{Env, Subject};
use crate::syntax::{named_children, namespace_parts, text};
use crate::target::{Outcome, callee, constructed, name_of};
use cg_core::EdgeKind;
use tree_sitter::Node as Ts;

/// Reads the call sites of all files.
pub fn collect(files: &Files, found: &mut Collected) {
    for file in 0..files.sources.len() {
        let source = &files.sources[file];
        let mut walk = Walk {
            files,
            file,
            out: found,
            from: crate::declare::container_id(source.module, &source.package),
            namespace: Vec::new(),
            stack: Vec::new(),
            this_class: None,
        };
        walk.visit(source.tree.root_node());
    }
}

/// What a node changed in the walk.
struct Frame {
    /// The source of the edges before the node, if the node changed it.
    from: Option<String>,
    /// The number of scopes before the node.
    stack: usize,
    /// What `this` was before the node.
    this_class: Option<usize>,
    /// The number of namespaces before the node.
    namespace: usize,
}

/// The walk through the syntax tree of one file.
struct Walk<'f, 'a> {
    files: &'f Files<'a>,
    file: usize,
    out: &'f mut Collected,
    /// The id of the node that the code at the walk belongs to.
    from: String,
    namespace: Vec<String>,
    stack: Vec<Scope<'a>>,
    /// The class that `this` is: a node, or `None` where `this` is unknown.
    this_class: Option<usize>,
}

impl<'a> Walk<'_, 'a> {
    fn env(&self) -> Env<'_, 'a> {
        Env {
            files: self.files,
            file: self.file,
            namespace: &self.namespace,
            stack: &self.stack,
            this_class: self.this_class,
        }
    }

    fn visit(&mut self, node: Ts<'a>) {
        let frame = self.enter(node);
        match node.kind() {
            "call_expression" => {
                self.call(node);
                self.arguments(node);
            }
            "new_expression" => {
                self.instance(node);
                self.arguments(node);
            }
            "jsx_self_closing_element" | "jsx_opening_element" => self.element(node),
            "jsx_attribute" => self.attribute(node),
            _ => {}
        }
        for child in named_children(node) {
            self.visit(child);
        }
        self.leave(frame);
    }

    /// Switches the source of the edges if `node` is a node of the graph, and opens the
    /// scope of a function.
    fn enter(&mut self, node: Ts<'a>) -> Frame {
        let source = self.files.sources[self.file].text;
        let mut frame = Frame {
            from: None,
            stack: self.stack.len(),
            this_class: self.this_class,
            namespace: self.namespace.len(),
        };
        if is_function_like(node.kind()) {
            self.stack.push(Scope::of_function(node, source));
        } else if is_class(node.kind()) {
            self.stack.push(Scope::of_class(node, source));
        }
        if let Some(class) = self.this_in(node) {
            self.this_class = class;
        }
        let entered = self.enter_namespace(node, source);
        let id = match self.declaration(node).or_else(|| self.exported(node)) {
            Some(index) => Some(self.files.decls[index].id.clone()),
            None => entered.then(|| self.files.container(self.file, &self.namespace)),
        };
        frame.from = id.map(|id| std::mem::replace(&mut self.from, id));
        frame
    }

    fn leave(&mut self, frame: Frame) {
        self.stack.truncate(frame.stack);
        self.namespace.truncate(frame.namespace);
        self.this_class = frame.this_class;
        if let Some(from) = frame.from {
            self.from = from;
        }
    }

    /// Adds the names of a namespace to the path of the walk; whether `node` is one.
    fn enter_namespace(&mut self, node: Ts<'a>, source: &str) -> bool {
        if !matches!(node.kind(), "internal_module" | "module") {
            return false;
        }
        match namespace_parts(node, source) {
            Some(parts) => {
                self.namespace.extend(parts.into_iter().map(str::to_string));
                true
            }
            None => false,
        }
    }

    /// The declaration that `node` is, if it is a node of the graph.
    fn declaration(&self, node: Ts<'a>) -> Option<usize> {
        let range = node.byte_range();
        let (index, kind) = *self
            .files
            .node_at
            .get(&(self.file, range.start, range.end))?;
        (kind == node.kind()).then_some(index)
    }

    /// The declaration of an `export` statement: its decorators belong to it.
    fn exported(&self, node: Ts<'a>) -> Option<usize> {
        let exported = node
            .child_by_field_name("declaration")
            .or_else(|| node.child_by_field_name("value"));
        exported
            .filter(|_| node.kind() == "export_statement")
            .and_then(|inner| self.declaration(inner))
    }

    /// What `this` is inside `node`, if `node` changes it: a class that is a node, or
    /// nothing in a class that is none and in a function that binds `this`.
    fn this_in(&self, node: Ts<'a>) -> Option<Option<usize>> {
        match node.kind() {
            kind if is_class(kind) => Some(self.declaration(node)),
            "function_declaration"
            | "generator_function_declaration"
            | "function_expression"
            | "generator_function" => Some(None),
            "method_definition" if node.parent().is_none_or(|p| p.kind() != "class_body") => {
                Some(None)
            }
            _ => None,
        }
    }

    fn call(&mut self, node: Ts<'a>) {
        // `import('..')` is an import (ADR 34), not a call.
        let Some(function) = node
            .child_by_field_name("function")
            .filter(|function| function.kind() != "import")
        else {
            return;
        };
        let outcome = callee(&self.env(), function);
        self.record(outcome, line(name_of(function)));
    }

    fn instance(&mut self, node: Ts<'a>) {
        let Some(class) = node.child_by_field_name("constructor") else {
            return;
        };
        let outcome = constructed(&self.env(), class);
        self.record(outcome, line(name_of(class)));
    }

    /// A JSX element: a call site if its tag is a capitalized name or a member.
    fn element(&mut self, node: Ts<'a>) {
        let Some(tag) = node.child_by_field_name("name") else {
            return;
        };
        let source = self.files.sources[self.file].text;
        let is_component = match tag.kind() {
            "identifier" => text(tag, source).starts_with(char::is_uppercase),
            "member_expression" => true,
            _ => false,
        };
        if is_component {
            self.out.jsx_elements += 1;
            let outcome = constructed(&self.env(), tag);
            self.record(outcome, line(name_of(tag)));
        }
    }

    /// The arguments of a call: a name of a function is a function value.
    fn arguments(&mut self, node: Ts<'a>) {
        let list = node.child_by_field_name("arguments");
        for argument in list.map(named_children).unwrap_or_default() {
            self.function_value(argument);
        }
    }

    /// The value of a JSX attribute: `onClick={onSave}`.
    fn attribute(&mut self, node: Ts<'a>) {
        let expression = named_children(node)
            .into_iter()
            .find(|child| child.kind() == "jsx_expression");
        let value = expression.and_then(|e| named_children(e).into_iter().next());
        if let Some(value) = value {
            self.function_value(value);
        }
    }

    /// A name that reaches a function node, in a place that hands the function on: a call
    /// of the function by the callee.
    fn function_value(&mut self, value: Ts<'a>) {
        if !matches!(value.kind(), "identifier" | "member_expression") {
            return;
        }
        if let Subject::Funcs(functions) = self.env().expression(value) {
            self.out.function_values += 1;
            self.record(Outcome::Edges(functions), line(name_of(value)));
        }
    }

    fn record(&mut self, outcome: Outcome, line: u32) {
        let path = self.files.sources[self.file].path;
        match outcome {
            Outcome::Edges(functions) => {
                self.out.calls.to_function += 1;
                for to in functions {
                    let to = &self.files.decls[to].id;
                    self.out
                        .edges
                        .add(&self.from, to, EdgeKind::Calls, path, line);
                }
            }
            Outcome::ToType(ty) => {
                self.out.calls.to_type += 1;
                let to = &self.files.decls[ty].id;
                self.out
                    .edges
                    .add(&self.from, to, EdgeKind::Calls, path, line);
            }
            Outcome::External => self.out.calls.external += 1,
            Outcome::Unresolved => self.out.calls.unresolved += 1,
        }
    }
}
