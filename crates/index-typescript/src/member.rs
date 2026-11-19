//! The members of the classes and interfaces of the repository (ADR 36).

use crate::declare::{has_members_kind, member_name};
use crate::edges::{Place, Reach};
use crate::heritage::clauses;
use crate::scope::{Scope, Typing, declared_typing};
use crate::subject::{Env, Subject};
use crate::syntax::{named_children, text};
use cg_core::{EdgeKind, NodeKind};
use std::collections::HashSet;
use tree_sitter::Node as Ts;

/// What a type declares under a name.
pub enum Member<'a> {
    /// A function node.
    Func(usize),
    /// A field with no function node: the type that declares it, and its written type.
    Field(usize, Option<Typing<'a>>),
    /// Several things at the nearest level that declares the name.
    Ambiguous,
    /// Nothing that the type or its supertypes declare, and each supertype is a class or
    /// interface of the repository or outside it: a member of a library or of `Object`.
    Missing,
    /// Nothing that the type or its supertypes declare, and a supertype is a name of the
    /// repository that reaches no class or interface: it can declare the member.
    Unknown,
}

impl<'a> Env<'_, 'a> {
    /// The member `name` of the type `ty` (an index of the declarations): in the type, else
    /// in its supertypes, the nearest level first.
    pub fn find_member(&self, ty: usize, name: &str) -> Member<'a> {
        let mut level = vec![ty];
        let mut seen = HashSet::from([ty]);
        let mut open = false;
        while !level.is_empty() {
            let mut hits: Vec<Member<'a>> = level
                .iter()
                .filter_map(|&t| self.own_member(t, name))
                .collect();
            match hits.len() {
                0 => {}
                1 => return hits.remove(0),
                _ => return Member::Ambiguous,
            }
            let mut next = Vec::new();
            for &t in &level {
                let (supertypes, unknown) = self.supertypes(t);
                open |= unknown;
                next.extend(supertypes.into_iter().filter(|&s| seen.insert(s)));
            }
            level = next;
        }
        if open {
            Member::Unknown
        } else {
            Member::Missing
        }
    }

    /// The constructor of the class `ty`, if it declares one.
    pub fn constructor(&self, ty: usize) -> Option<usize> {
        let files = self.files;
        let mut own = files.named(&files.decls[ty].id, "constructor");
        own.find(|&i| files.decls[i].kind == NodeKind::Function)
    }

    /// The class that the class around `this` extends.
    pub fn superclass(&self) -> Subject {
        let Some(class) = self.this_class else {
            return Subject::Unknown;
        };
        let decl = &self.files.decls[class];
        let source = self.files.sources[decl.source].text;
        let extends = clauses(decl.node, source)
            .into_iter()
            .find(|clause| clause.kind == EdgeKind::Extends)
            .and_then(|clause| clause.name);
        let at = Place {
            source: decl.source,
            namespace: &decl.namespace,
        };
        match extends.map(|name| self.files.type_target(&at, &name.path)) {
            Some(Reach::Types(types)) => self.one_type(&types),
            Some(Reach::External) => Subject::External,
            Some(Reach::Unresolved) | None => Subject::Unknown,
        }
    }

    /// What the type `ty` itself declares under `name`.
    fn own_member(&self, ty: usize, name: &str) -> Option<Member<'a>> {
        let files = self.files;
        let decl = &files.decls[ty];
        let mut own = files.named(&decl.id, name);
        if let Some(function) = own.find(|&i| files.decls[i].kind == NodeKind::Function) {
            return Some(Member::Func(function));
        }
        let source = files.sources[decl.source].text;
        field(decl.node, name, source).map(|typing| Member::Field(ty, typing))
    }

    /// The classes and interfaces of the repository that the type `ty` extends or
    /// implements, and whether a clause names something of the repository that is none of
    /// them (a name that reaches no type, a `type` alias, an expression).
    fn supertypes(&self, ty: usize) -> (Vec<usize>, bool) {
        let decl = &self.files.decls[ty];
        let source = self.files.sources[decl.source].text;
        let at = Place {
            source: decl.source,
            namespace: &decl.namespace,
        };
        let mut found = Vec::new();
        let mut unknown = false;
        for clause in clauses(decl.node, source) {
            let reach = clause.name.map_or(Reach::Unresolved, |name| {
                self.files.type_target(&at, &name.path)
            });
            match reach {
                Reach::Types(types) => {
                    let (members, other): (Vec<usize>, Vec<usize>) = types
                        .into_iter()
                        .partition(|&t| has_members_kind(self.files.decls[t].node.kind()));
                    unknown |= !other.is_empty();
                    found.extend(members);
                }
                Reach::External => {}
                Reach::Unresolved => unknown = true,
            }
        }
        (found, unknown)
    }

    /// What the field `name` of `owner` is written to be, with `owner` around the type.
    pub fn field_subject(&self, owner: usize, typing: Typing<'a>) -> Subject {
        let decl = &self.files.decls[owner];
        let source = self.files.sources[decl.source].text;
        let stack = [Scope::of_class(decl.node, source)];
        let env = Env {
            files: self.files,
            file: decl.source,
            namespace: &decl.namespace,
            stack: &stack,
            this_class: None,
        };
        env.typing(typing)
    }
}

/// The field `name` of the class or interface `node`: `Some(typing)` if there is one.
fn field<'a>(node: Ts<'a>, name: &str, source: &str) -> Option<Option<Typing<'a>>> {
    let body = node.child_by_field_name("body")?;
    named_children(body)
        .into_iter()
        .find_map(|member| match member.kind() {
            "public_field_definition" | "property_signature" => {
                (member_text(member, source).as_deref() == Some(name))
                    .then(|| declared_typing(member, true))
            }
            "method_definition"
                if member_text(member, source).as_deref() == Some("constructor") =>
            {
                parameter_property(member, name, source)
            }
            _ => None,
        })
}

/// The name of a member as written.
fn member_text(member: Ts, source: &str) -> Option<String> {
    member_name(member.child_by_field_name("name")?, source)
}

/// A parameter of the constructor `member` that is a field too: `private s: Store`.
fn parameter_property<'a>(member: Ts<'a>, name: &str, source: &str) -> Option<Option<Typing<'a>>> {
    let list = member.child_by_field_name("parameters")?;
    named_children(list).into_iter().find_map(|parameter| {
        let pattern = parameter.child_by_field_name("pattern")?;
        let is_field = (0..parameter.child_count())
            .filter_map(|i| parameter.child(i))
            .any(|c| matches!(c.kind(), "accessibility_modifier" | "readonly"));
        (is_field && text(pattern, source) == name).then(|| declared_typing(parameter, false))
    })
}
