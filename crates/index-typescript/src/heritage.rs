//! The names in the `extends` and `implements` clauses of a class or interface.

use crate::bindings::line;
use crate::syntax::{named_children, text};
use cg_core::EdgeKind;
use tree_sitter::Node as Ts;

/// A name as written: `B`, `x.B`, `a.b.C`.
pub struct Written {
    /// The segments, the last is the name of the type.
    pub path: Vec<String>,
    /// The line of the last segment.
    pub line: u32,
}

/// One name of a clause. `name` is `None` for an expression that is no name
/// (`extends mixin(A)`).
pub struct Clause {
    pub kind: EdgeKind,
    pub name: Option<Written>,
}

/// The names in the clauses of the class or interface `node`. Other nodes have none.
pub fn clauses(node: Ts, source: &str) -> Vec<Clause> {
    named_children(node)
        .into_iter()
        .flat_map(|child| match child.kind() {
            // A class: `extends` and `implements` in the heritage.
            "class_heritage" => class_clauses(child, source),
            // An interface: `extends` lists types.
            "extends_type_clause" => listed(child, "type", EdgeKind::Extends, source),
            _ => Vec::new(),
        })
        .collect()
}

/// The clauses in the heritage of a class.
fn class_clauses(heritage: Ts, source: &str) -> Vec<Clause> {
    named_children(heritage)
        .into_iter()
        .flat_map(|clause| match clause.kind() {
            "extends_clause" => listed(clause, "value", EdgeKind::Extends, source),
            "implements_clause" => named_children(clause)
                .into_iter()
                .filter(|ty| ty.kind() != "comment")
                .map(|ty| Clause {
                    kind: EdgeKind::Implements,
                    name: written(ty, source),
                })
                .collect(),
            _ => Vec::new(),
        })
        .collect()
}

/// A clause of `kind` for each child of `parent` in the field `field`.
fn listed(parent: Ts, field: &str, kind: EdgeKind, source: &str) -> Vec<Clause> {
    let mut cursor = parent.walk();
    parent
        .children_by_field_name(field, &mut cursor)
        .map(|value| Clause {
            kind,
            name: written(value, source),
        })
        .collect()
}

/// The name that a node writes, without type arguments; `None` if it is no plain name.
fn written(node: Ts, source: &str) -> Option<Written> {
    match node.kind() {
        "identifier" | "type_identifier" => Some(Written {
            path: vec![text(node, source).to_string()],
            line: line(node),
        }),
        "generic_type" => written(node.child_by_field_name("name")?, source),
        "member_expression" | "nested_identifier" => {
            join_fields(node, "object", "property", source)
        }
        "nested_type_identifier" => join_fields(node, "module", "name", source),
        _ => None,
    }
}

/// The name of the field `head` of `node` followed by the segment in the field `last`.
fn join_fields(node: Ts, head: &str, last: &str, source: &str) -> Option<Written> {
    join(
        node.child_by_field_name(head)?,
        node.child_by_field_name(last)?,
        source,
    )
}

/// The name of `head` followed by the segment `last`.
fn join(head: Ts, last: Ts, source: &str) -> Option<Written> {
    let mut path = written(head, source)?.path;
    path.push(text(last, source).to_string());
    Some(Written {
        path,
        line: line(last),
    })
}
