//! What a written type denotes (ADR 36).

use crate::declare::has_members_kind;
use crate::edges::{Place, Reach};
use crate::heritage::written;
use crate::scope::{Binding, Typing};
use crate::subject::{Env, Subject};
use crate::syntax::{named_children, text};
use tree_sitter::Node as Ts;

impl<'a> Env<'_, 'a> {
    /// The subject that a variable is written to be: untyped, it is unknown.
    pub fn typed(&self, binding: &Binding<'a>) -> Subject {
        binding.typing.map_or(Subject::Unknown, |t| self.typing(t))
    }

    /// The subject that `typing` writes.
    pub fn typing(&self, typing: Typing<'a>) -> Subject {
        if typing.constructed {
            self.constructed(typing.node)
        } else {
            self.written_type(typing.node)
        }
    }

    /// The subject of `new class(..)`.
    pub(crate) fn constructed(&self, class: Ts<'a>) -> Subject {
        match self.expression(class) {
            Subject::Types(ty) => Subject::Types(ty),
            Subject::External => Subject::External,
            _ => Subject::Unknown,
        }
    }

    /// Whether a type parameter or a type of a function hides the type `name`.
    fn hidden(&self, name: &str) -> bool {
        self.stack.iter().any(|scope| scope.types.contains(name))
    }

    /// The subject of a type as it is written.
    fn written_type(&self, node: Ts<'a>) -> Subject {
        match node.kind() {
            // `any` and `unknown` say nothing about the value.
            "predefined_type" if matches!(text(node, self.source()), "any" | "unknown") => {
                Subject::Unknown
            }
            "predefined_type" | "array_type" => Subject::External,
            "type_identifier" | "generic_type" | "nested_type_identifier" => self.named_type(node),
            "union_type" => self.union_type(node),
            _ => Subject::Unknown,
        }
    }

    /// A union of one name and `null` or `undefined`.
    fn union_type(&self, node: Ts<'a>) -> Subject {
        let mut members = Vec::new();
        self.union_members(node, &mut members);
        match members.as_slice() {
            [one] => self.written_type(*one),
            _ => Subject::Unknown,
        }
    }

    /// The members of a union that are neither `null` nor `undefined`.
    fn union_members(&self, node: Ts<'a>, members: &mut Vec<Ts<'a>>) {
        for member in named_children(node) {
            match member.kind() {
                "union_type" => self.union_members(member, members),
                _ if matches!(text(member, self.source()), "null" | "undefined") => {}
                _ => members.push(member),
            }
        }
    }

    /// A name of a type: `Store`, `Store<A>`, `ns.Store`.
    fn named_type(&self, node: Ts<'a>) -> Subject {
        let Some(name) = written(node, self.source()) else {
            return Subject::Unknown;
        };
        if self.hidden(&name.path[0]) {
            return Subject::Unknown;
        }
        let at = Place {
            source: self.file,
            namespace: self.namespace,
        };
        match self.files.type_target(&at, &name.path) {
            Reach::Types(types) => self.one_type(&types),
            Reach::External => Subject::External,
            Reach::Unresolved => Subject::Unknown,
        }
    }

    /// The one class or interface among `types`: a type alias and an enum have no members.
    pub(crate) fn one_type(&self, types: &[usize]) -> Subject {
        match types {
            [one] if has_members_kind(self.files.decls[*one].node.kind()) => Subject::Types(*one),
            _ => Subject::Unknown,
        }
    }
}
