//! What an expression denotes, as far as a call needs to know (ADR 36).

use crate::bindings::{Import, What};
use crate::edges::Files;
use crate::member::Member;
use crate::resolve::Target;
use crate::scope::{Binding, How, Kind, Scope, is_class};
use crate::syntax::{named_children, text};
use cg_core::NodeKind;
use tree_sitter::Node as Ts;

/// What an expression denotes.
pub enum Subject {
    /// A value of a class or interface of the repository, or the class itself: an index of
    /// the declarations.
    Types(usize),
    /// The function nodes that a name reaches.
    Funcs(Vec<usize>),
    /// A file of the repository as a whole: `import * as ns`. An index of the sources.
    Module(usize),
    /// A namespace of the file, by its path.
    Namespace(Vec<String>),
    /// Unknown, or a name of the repository that reaches nothing we can follow.
    Unknown,
    /// Known, and outside the repository.
    External,
}

/// Where a name is bound.
enum Found<'e, 'a> {
    /// The scope, the number of scopes of the stack up to and including it (0 for the file
    /// and the namespaces), and the binding.
    Bound(&'e Scope<'a>, usize, &'e Binding<'a>),
    /// Bound by an import of the file.
    Import(&'e Import),
    /// Bound by no scope: a global.
    Global,
}

/// The place of an expression: its file, the namespaces around it and the scopes of the
/// functions and classes around it.
#[derive(Clone, Copy)]
pub struct Env<'e, 'a> {
    pub files: &'e Files<'a>,
    pub file: usize,
    pub namespace: &'e [String],
    pub stack: &'e [Scope<'a>],
    /// The class that `this` is, if it is a node.
    pub this_class: Option<usize>,
}

impl<'e, 'a> Env<'e, 'a> {
    /// The innermost scope that binds `name`.
    fn find(&self, name: &str) -> Found<'e, 'a> {
        let files: &'e Files<'a> = self.files;
        let outer = (0..=self.namespace.len()).rev().filter_map(|depth| {
            files
                .scopes
                .get(&(self.file, self.namespace[..depth].join(".")))
        });
        let inner = self.stack.iter().enumerate().rev();
        let inner = inner.map(|(index, scope)| (scope, index + 1));
        inner
            .chain(outer.map(|scope| (scope, 0)))
            .find_map(|(scope, depth)| {
                scope
                    .values
                    .get(name)
                    .map(|b| Found::Bound(scope, depth, b))
            })
            .or_else(|| {
                files.bindings[self.file]
                    .imports
                    .get(name)
                    .map(Found::Import)
            })
            .unwrap_or(Found::Global)
    }

    /// The text of the file.
    pub fn source(&self) -> &'a str {
        self.files.sources[self.file].text
    }

    /// What `node` denotes.
    pub fn expression(&self, node: Ts<'a>) -> Subject {
        match node.kind() {
            "identifier" => self.identifier(text(node, self.source())),
            "this" | "super" => self.receiver(node.kind()),
            "new_expression" => self.created(node),
            "member_expression" => self.member(node),
            "non_null_expression" | "parenthesized_expression" => self.inner(node),
            _ => Subject::Unknown,
        }
    }

    /// `this` or `super`.
    fn receiver(&self, kind: &str) -> Subject {
        if kind == "super" {
            return self.superclass();
        }
        self.this_class.map_or(Subject::Unknown, Subject::Types)
    }

    /// `new C(..)`.
    fn created(&self, node: Ts<'a>) -> Subject {
        node.child_by_field_name("constructor")
            .map_or(Subject::Unknown, |class| self.constructed(class))
    }

    /// What the first child of `node` denotes: `x!` and `(x)` denote `x`.
    fn inner(&self, node: Ts<'a>) -> Subject {
        named_children(node)
            .first()
            .map_or(Subject::Unknown, |inner| self.expression(*inner))
    }

    /// `object.property`.
    fn member(&self, node: Ts<'a>) -> Subject {
        let object = node.child_by_field_name("object");
        let property = node.child_by_field_name("property");
        match (object, property) {
            (Some(object), Some(property)) => {
                self.property(&self.expression(object), text(property, self.source()))
            }
            _ => Subject::Unknown,
        }
    }

    /// The identifier `name`.
    pub fn identifier(&self, name: &str) -> Subject {
        match self.find(name) {
            Found::Bound(scope, depth, binding) => self.bound(scope, depth, name, binding),
            Found::Import(import) => self.imported(import),
            Found::Global => Subject::External,
        }
    }

    /// What the name that `import` binds reaches.
    fn imported(&self, import: &Import) -> Subject {
        match self.files.resolver.resolve(self.file, &import.specifier) {
            Target::Source(file) => match &import.what {
                What::Name(name) => self.reached(&self.files.lookup_unique(file, name)),
                What::Default => self.reached(&self.files.lookup_unique(file, "default")),
                What::Namespace => Subject::Module(file),
            },
            Target::External => Subject::External,
            Target::Module(_) | Target::Unresolved | Target::OtherFile => Subject::Unknown,
        }
    }

    fn bound(&self, scope: &Scope<'a>, depth: usize, name: &str, binding: &Binding<'a>) -> Subject {
        match scope.kind {
            Kind::File | Kind::Namespace => self.declared(scope, name, binding),
            Kind::Function | Kind::Class => {
                let env = Env {
                    stack: &self.stack[..depth],
                    ..*self
                };
                env.typed(binding)
            }
        }
    }

    /// The place of the declarations of a file or a namespace scope.
    fn declaring<'b>(&self, scope: &'b Scope<'a>) -> Env<'b, 'a>
    where
        'e: 'b,
    {
        Env {
            namespace: &scope.path,
            stack: &[],
            ..*self
        }
    }

    /// A name that a file or a namespace declares.
    fn declared(&self, scope: &Scope<'a>, name: &str, binding: &Binding<'a>) -> Subject {
        let container = self.files.container(self.file, &scope.path);
        let named: Vec<usize> = self.files.named(&container, name).collect();
        match binding.how {
            How::Function | How::Class => self.reached(&named),
            How::Const => match self.reached(&named) {
                Subject::Unknown => self.declaring(scope).typed(binding),
                subject => subject,
            },
            How::Namespace => {
                Subject::Namespace([scope.path.as_slice(), &[name.to_string()]].concat())
            }
            How::Other => self.declaring(scope).typed(binding),
        }
    }

    /// `name` in the namespace `path` of the file: a declaration or a namespace in it.
    fn in_namespace(&self, path: &[String], name: &str) -> Subject {
        let container = self.files.container(self.file, path);
        let named: Vec<usize> = self.files.named(&container, name).collect();
        match self.reached(&named) {
            Subject::Unknown => {
                let inner = [path, &[name.to_string()]].concat();
                let exists = self
                    .files
                    .scopes
                    .contains_key(&(self.file, inner.join(".")));
                if exists {
                    Subject::Namespace(inner)
                } else {
                    Subject::Unknown
                }
            }
            subject => subject,
        }
    }

    /// What the property `name` of `subject` denotes, where the subject says it without a
    /// type: a file that is imported as a whole.
    pub fn property(&self, subject: &Subject, name: &str) -> Subject {
        match subject {
            Subject::Types(ty) => self.member_subject(*ty, name),
            Subject::Module(file) => self.reached(&self.files.lookup_unique(*file, name)),
            Subject::Namespace(path) => self.in_namespace(path, name),
            Subject::Funcs(functions) => self.property_of_functions(functions),
            Subject::External => Subject::External,
            _ => Subject::Unknown,
        }
    }

    /// A property of function nodes is one of `Function` (`bind`), unless they are
    /// accessors: the value of a getter is of any type.
    fn property_of_functions(&self, functions: &[usize]) -> Subject {
        let accessor = functions
            .iter()
            .any(|&f| is_accessor(self.files.decls[f].node));
        if accessor {
            Subject::Unknown
        } else {
            Subject::External
        }
    }

    /// What the member `name` of the type `ty` denotes.
    fn member_subject(&self, ty: usize, name: &str) -> Subject {
        match self.find_member(ty, name) {
            Member::Func(function) => Subject::Funcs(vec![function]),
            Member::Field(owner, Some(typing)) => self.field_subject(owner, typing),
            Member::Field(_, None) | Member::Ambiguous | Member::Missing | Member::Unknown => {
                Subject::Unknown
            }
        }
    }

    /// The one class, else the function nodes among `reached`, if there are any.
    fn reached(&self, reached: &[usize]) -> Subject {
        let decls = &self.files.decls;
        let classes: Vec<usize> = reached
            .iter()
            .copied()
            .filter(|&i| is_class(decls[i].node.kind()))
            .collect();
        if let [class] = classes.as_slice() {
            return Subject::Types(*class);
        }
        let functions: Vec<usize> = reached
            .iter()
            .copied()
            .filter(|&i| decls[i].kind == NodeKind::Function)
            .collect();
        if classes.is_empty() && !functions.is_empty() {
            Subject::Funcs(functions)
        } else {
            Subject::Unknown
        }
    }
}

/// Whether the declaration `node` is a `get` or `set` accessor.
fn is_accessor(node: Ts) -> bool {
    node.kind() == "method_definition"
        && (0..node.child_count())
            .filter_map(|i| node.child(i))
            .any(|c| matches!(c.kind(), "get" | "set"))
}
