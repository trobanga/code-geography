//! What a call site reaches (ADR 36).

use crate::member::Member;
use crate::subject::{Env, Subject};
use crate::syntax::text;
use tree_sitter::Node as Ts;

/// What a call site reaches.
pub enum Outcome {
    /// Function nodes.
    Edges(Vec<usize>),
    /// A type: no function node says it better.
    ToType(usize),
    External,
    Unresolved,
}

/// The node that holds the name of a callee: the property of a member expression.
pub fn name_of(expression: Ts) -> Ts {
    match expression.kind() {
        "member_expression" => expression
            .child_by_field_name("property")
            .unwrap_or(expression),
        _ => expression,
    }
}

/// What the call of `function` reaches.
pub fn callee(env: &Env, function: Ts) -> Outcome {
    match function.kind() {
        "identifier" => as_callee(env.expression(function)),
        "member_expression" => member_callee(env, function),
        "super" => instantiate(env, env.superclass()),
        _ => Outcome::Unresolved,
    }
}

/// What `new class(..)` reaches.
pub fn constructed(env: &Env, class: Ts) -> Outcome {
    instantiate(env, env.expression(class))
}

fn as_callee(subject: Subject) -> Outcome {
    match subject {
        Subject::Funcs(functions) => Outcome::Edges(functions),
        Subject::External => Outcome::External,
        _ => Outcome::Unresolved,
    }
}

/// The creation of a value of `subject`: the constructor of a class, else the class.
fn instantiate(env: &Env, subject: Subject) -> Outcome {
    match subject {
        Subject::Types(ty) => match env.constructor(ty) {
            Some(constructor) => Outcome::Edges(vec![constructor]),
            None => Outcome::ToType(ty),
        },
        other => as_callee(other),
    }
}

/// `receiver.name(..)`.
fn member_callee(env: &Env, function: Ts) -> Outcome {
    let receiver = function
        .child_by_field_name("object")
        .map_or(Subject::Unknown, |object| env.expression(object));
    let name = function
        .child_by_field_name("property")
        .map_or("", |property| text(property, env.source()));
    match receiver {
        Subject::Types(ty) => member_of_type(env, ty, name),
        _ => as_callee(env.property(&receiver, name)),
    }
}

/// `name(..)` on a value of the type `ty`.
fn member_of_type(env: &Env, ty: usize, name: &str) -> Outcome {
    match env.find_member(ty, name) {
        Member::Func(function) => Outcome::Edges(vec![function]),
        Member::Field(owner, _) => Outcome::ToType(owner),
        Member::Ambiguous => Outcome::ToType(ty),
        Member::Missing => Outcome::External,
        Member::Unknown => Outcome::Unresolved,
    }
}
