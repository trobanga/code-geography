//! Nodes that need the declarations of all files before they have an id: the methods of
//! `defmulti` (`defmethod`) and of `extend-protocol` and `extend-type`. The id of a method
//! names the multi, the type and the protocol by their full names, which an alias or the
//! declarations of another file decide.

use crate::Report;
use crate::calls::{File, Scope, Vars};
use crate::declare::Spec;
use cg_core::{ClassTable, Span};
use std::collections::{BTreeSet, HashMap};

/// A declaration of a file whose node is made by `place`.
pub struct Late {
    /// `Node::id` of the forms that the node holds: the first and the other arities.
    pub forms: Vec<usize>,
    pub hash: String,
    pub span: Span,
    pub spec: Spec,
}

/// A node that `place` made, for the file at `file` in the slice that was given.
pub struct Placed {
    pub file: usize,
    pub id: String,
    pub parent: Option<String>,
    pub name: String,
    pub hash: String,
    pub span: Span,
}

/// Gives the late declarations of all `files` an id, a parent and a name, in the order of
/// the files and of the source. A node whose id is in `seen` (or was made earlier) is a
/// duplicate: no node, counted in the report. The forms of the others are added to `owned`.
pub fn place(
    files: &mut [File],
    vars: &HashMap<String, Vars>,
    classes: &ClassTable,
    seen: &mut BTreeSet<String>,
    report: &mut Report,
) -> Vec<Placed> {
    let mut placed = Vec::new();
    for (index, file) in files.iter_mut().enumerate() {
        let late = std::mem::take(&mut file.late);
        let mut owned = Vec::new();
        let scope = Scope {
            vars,
            classes,
            file: &*file,
        };
        for item in late {
            let (id, parent, name, head) = match &item.spec {
                Spec::Method { multi, dispatch } => {
                    let id = format!(
                        "fn:{}/{} {dispatch}",
                        scope.file.namespace,
                        scope.full_name(multi)
                    );
                    (id, None, format!("{multi} {dispatch}"), "defmethod")
                }
                Spec::Extend {
                    protocol,
                    ty,
                    method,
                } => {
                    let ty_node = scope.type_node(ty);
                    let protocol_node = scope.protocol_node(protocol);
                    let ty_key = ty_node.map_or_else(
                        || scope.class_text(ty),
                        |id| id.strip_prefix("type:").unwrap_or(id).to_string(),
                    );
                    let protocol_key = protocol_node.map_or_else(
                        || scope.full_name(protocol),
                        |id| id.strip_prefix("type:").unwrap_or(id).to_string(),
                    );
                    let id = format!("ext:{ty_key}#{protocol_key}/{method}");
                    let (parent, name) = match (ty_node, protocol_node) {
                        (Some(t), _) => (Some(t.to_string()), method.clone()),
                        (None, Some(p)) => (Some(p.to_string()), format!("{method} for {ty}")),
                        (None, None) => (None, format!("{protocol}/{method} for {ty}")),
                    };
                    (id, parent, name, "extend")
                }
            };
            if !seen.insert(id.clone()) {
                *report
                    .duplicate_declarations
                    .entry(head.to_string())
                    .or_default() += 1;
                continue;
            }
            match head {
                "defmethod" => report.method_nodes += 1,
                _ => report.extend_nodes += 1,
            }
            owned.extend(item.forms.iter().map(|f| (*f, id.clone())));
            placed.push(Placed {
                file: index,
                id,
                parent,
                name,
                hash: item.hash,
                span: item.span,
            });
        }
        file.owned.extend(owned);
    }
    placed
}
