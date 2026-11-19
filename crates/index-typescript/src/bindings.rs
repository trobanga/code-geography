//! What the top-level statements of a file import and export.

use crate::layout::Source;
use crate::syntax::{named_children, text};
use std::collections::{HashMap, HashSet};
use tree_sitter::Node as Ts;

/// An `import` statement with a source or an `export .. from`.
pub struct Link {
    pub specifier: String,
    /// The line of the string that names the file.
    pub line: u32,
    /// The names that are read from the file, each with its line: `A` of `{ A as B }`,
    /// `default` for a default import.
    pub names: Vec<(String, u32)>,
    /// The file as a whole: `import * as x`, `import './x'`, `export *`.
    pub whole: bool,
}

/// What a local name of an import stands for.
#[derive(Clone)]
pub enum What {
    /// `import { A }`, `import { A as B }`: the name in the other file.
    Name(String),
    /// `import A`.
    Default,
    /// `import * as A`.
    Namespace,
}

#[derive(Clone)]
pub struct Import {
    pub specifier: String,
    pub what: What,
}

/// What an exported name stands for.
#[derive(Clone)]
pub enum Export {
    /// A name of this file: a declaration, or an import that this file exports again.
    Local(String),
    /// `export { name } from 'specifier'`.
    From { specifier: String, name: String },
}

#[derive(Default)]
pub struct Bindings {
    pub links: Vec<Link>,
    /// The imports by local name.
    pub imports: HashMap<String, Import>,
    /// The exports by exported name.
    pub exports: HashMap<String, Export>,
    /// The specifiers of `export * from`, in order.
    pub stars: Vec<String>,
    /// The names that the top-level statements declare, exported or not: also what is no
    /// node (a `const`, an `enum` member's enum).
    pub declared: HashSet<String>,
}

/// The line (1-based) of a syntax node.
pub fn line(node: Ts) -> u32 {
    node.start_position().row as u32 + 1
}

/// The text of a string node without its quotes.
pub fn unquoted<'a>(node: Ts, source: &'a str) -> &'a str {
    let written = text(node, source);
    written
        .get(1..written.len().saturating_sub(1))
        .unwrap_or("")
}

/// The specifiers of `{ A, B as C }`: the `name` and the local name (the alias, else the
/// name), with the line of the name.
fn specifiers<'t>(list: Ts<'t>, source: &str) -> Vec<(Ts<'t>, String)> {
    named_children(list)
        .into_iter()
        .filter_map(|specifier| {
            let name = specifier.child_by_field_name("name")?;
            let local = specifier.child_by_field_name("alias").unwrap_or(name);
            Some((name, text(local, source).to_string()))
        })
        .collect()
}

impl Bindings {
    pub fn of(source: &Source) -> Bindings {
        let mut bindings = Bindings::default();
        for statement in named_children(source.tree.root_node()) {
            match statement.kind() {
                "import_statement" => bindings.import(source.text, statement),
                "export_statement" => {
                    bindings.export(source.text, statement);
                    if let Some(declaration) = statement.child_by_field_name("declaration") {
                        bindings
                            .declared
                            .extend(declared_names(declaration, source.text));
                    }
                }
                _ => bindings
                    .declared
                    .extend(declared_names(statement, source.text)),
            }
        }
        bindings.dynamic_imports(source.text, source.tree.root_node());
        bindings
    }

    /// Each `import('..')` with one string as argument, anywhere in the tree.
    fn dynamic_imports(&mut self, source: &str, root: Ts) {
        let mut pending = vec![root];
        while let Some(node) = pending.pop() {
            if node.kind() == "call_expression" {
                self.dynamic_import(source, node);
            }
            // Reversed, so that the first child is the next one.
            let mut cursor = node.walk();
            let children: Vec<Ts> = node.children(&mut cursor).collect();
            pending.extend(children.into_iter().rev());
        }
    }

    fn dynamic_import(&mut self, source: &str, call: Ts) {
        let is_import = call
            .child_by_field_name("function")
            .is_some_and(|f| f.kind() == "import");
        let Some(arguments) = call.child_by_field_name("arguments") else {
            return;
        };
        let arguments = named_children(arguments);
        if let (true, [argument]) = (is_import, arguments.as_slice())
            && argument.kind() == "string"
        {
            self.links.push(Link {
                specifier: unquoted(*argument, source).to_string(),
                line: line(*argument),
                names: Vec::new(),
                whole: true,
            });
        }
    }

    /// A link to the file named by the `source` field of `statement`, if it has one.
    fn link(statement: Ts, source: &str) -> Option<Link> {
        let from = statement.child_by_field_name("source")?;
        Some(Link {
            specifier: unquoted(from, source).to_string(),
            line: line(from),
            names: Vec::new(),
            whole: false,
        })
    }

    fn import(&mut self, source: &str, statement: Ts) {
        let Some(mut link) = Self::link(statement, source) else {
            return;
        };
        let clause = named_children(statement)
            .into_iter()
            .find(|c| c.kind() == "import_clause");
        for part in clause.map(named_children).unwrap_or_default() {
            self.import_part(source, part, &mut link);
        }
        link.whole |= link.names.is_empty();
        self.links.push(link);
    }

    /// The names that one part of an import clause binds: a default, `{ .. }` or `* as`.
    fn import_part(&mut self, source: &str, part: Ts, link: &mut Link) {
        match part.kind() {
            "identifier" => {
                link.names.push(("default".to_string(), line(part)));
                self.bind(text(part, source), &link.specifier, What::Default);
            }
            "named_imports" => {
                for (name, local) in specifiers(part, source) {
                    let name_text = text(name, source).to_string();
                    link.names.push((name_text.clone(), line(name)));
                    self.bind(&local, &link.specifier, What::Name(name_text));
                }
            }
            "namespace_import" => {
                link.whole = true;
                if let Some(local) = named_children(part).into_iter().next() {
                    self.bind(text(local, source), &link.specifier, What::Namespace);
                }
            }
            _ => {}
        }
    }

    fn bind(&mut self, local: &str, specifier: &str, what: What) {
        self.imports.entry(local.to_string()).or_insert(Import {
            specifier: specifier.to_string(),
            what,
        });
    }

    fn export(&mut self, source: &str, statement: Ts) {
        let clause = named_children(statement)
            .into_iter()
            .find(|c| c.kind() == "export_clause");
        match Self::link(statement, source) {
            Some(link) => self.reexport(source, statement, link, clause),
            None => self.local_export(source, statement, clause),
        }
    }

    /// An export without a source: `export { a, b as c }` or a declaration.
    fn local_export(&mut self, source: &str, statement: Ts, clause: Option<Ts>) {
        match clause {
            Some(clause) => {
                for (name, exported) in specifiers(clause, source) {
                    let name = text(name, source).to_string();
                    self.export_as(&exported, Export::Local(name));
                }
            }
            None => self.declaration(source, statement),
        }
    }

    /// An export with a source: `export { a } from`, `export * from`.
    fn reexport(&mut self, source: &str, statement: Ts, mut link: Link, clause: Option<Ts>) {
        match clause {
            Some(clause) => {
                for (node, exported) in specifiers(clause, source) {
                    let name = text(node, source).to_string();
                    link.names.push((name.clone(), line(node)));
                    let from = Export::From {
                        specifier: link.specifier.clone(),
                        name,
                    };
                    self.export_as(&exported, from);
                }
            }
            None => {
                // `export * from`, `export * as ns from`, and `export type * from`, which
                // the grammar does not know.
                let namespace = named_children(statement)
                    .iter()
                    .any(|c| c.kind() == "namespace_export");
                if !namespace {
                    self.stars.push(link.specifier.clone());
                }
            }
        }
        link.whole = link.names.is_empty();
        self.links.push(link);
    }

    fn export_as(&mut self, exported: &str, export: Export) {
        self.exports.entry(exported.to_string()).or_insert(export);
    }

    /// `export class A {}`, `export const a = 1`, `export default class A {}`,
    /// `export default a;`.
    fn declaration(&mut self, source: &str, statement: Ts) {
        if let Some(value) = statement.child_by_field_name("value") {
            let local = if value.kind() == "identifier" {
                text(value, source)
            } else {
                "default"
            };
            self.export_as("default", Export::Local(local.to_string()));
        } else if let Some(declaration) = statement.child_by_field_name("declaration") {
            self.export_declared(source, declaration, is_default_export(statement));
        }
    }

    /// The names of `declaration`, exported by name or, after `default`, as the default.
    fn export_declared(&mut self, source: &str, declaration: Ts, is_default: bool) {
        for name in declared_names(declaration, source) {
            if is_default {
                self.export_as("default", Export::Local(name));
            } else {
                self.export_as(&name.clone(), Export::Local(name));
            }
        }
        if is_default && !self.exports.contains_key("default") {
            self.export_as("default", Export::Local("default".to_string()));
        }
    }
}

/// Whether the export statement has the keyword `default`.
fn is_default_export(statement: Ts) -> bool {
    let mut cursor = statement.walk();
    statement
        .children(&mut cursor)
        .any(|c| c.kind() == "default")
}

/// The names that a declaration introduces.
fn declared_names(declaration: Ts, source: &str) -> Vec<String> {
    match declaration.kind() {
        "ambient_declaration" => named_children(declaration)
            .into_iter()
            .flat_map(|inner| declared_names(inner, source))
            .collect(),
        "lexical_declaration" | "variable_declaration" => named_children(declaration)
            .into_iter()
            .filter(|c| c.kind() == "variable_declarator")
            .filter_map(|c| c.child_by_field_name("name"))
            .filter(|n| n.kind() == "identifier")
            .map(|n| text(n, source).to_string())
            .collect(),
        "internal_module" | "module" => declaration
            .child_by_field_name("name")
            .filter(|n| n.kind() != "string")
            .map(|n| {
                text(n, source)
                    .split('.')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string()
            })
            .into_iter()
            .collect(),
        _ => declaration
            .child_by_field_name("name")
            .map(|n| text(n, source).to_string())
            .into_iter()
            .collect(),
    }
}
