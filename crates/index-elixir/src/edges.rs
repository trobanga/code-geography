//! The edges between modules: what the references of the walk make of them.

use crate::declare::Declared;
use crate::scope::Name;
use crate::{Report, package_id};
use cg_core::{EdgeKind, EdgeSet, Graph};
use std::collections::{BTreeMap, HashMap};

/// The edges that the references make, between the packages of the modules that the
/// repository declares (`dirs`: the module name to the directory of its project). No edge
/// joins a node with itself or with its ancestor or descendant.
pub fn edges(
    graph: &Graph,
    declared: &Declared,
    dirs: &BTreeMap<&str, &str>,
    report: &mut Report,
) -> EdgeSet {
    let mut edges = EdgeSet::new();
    // A `defimpl` is an edge from its type to its protocol.
    for module in &declared.modules {
        let Some(of) = &module.impl_of else { continue };
        if let (Some(ty_dir), Some(protocol_dir)) =
            (dirs.get(of.ty.as_str()), dirs.get(of.protocol.as_str()))
        {
            edges.add(
                &package_id(ty_dir, &of.ty),
                &package_id(protocol_dir, &of.protocol),
                EdgeKind::Implements,
                &module.file,
                of.line,
            );
        }
    }
    for reference in &declared.references {
        let (Some(from), Name::Module(to)) = (&reference.from, &reference.name) else {
            continue;
        };
        let Some(kind) = reference.kind.edge() else {
            continue;
        };
        let (Some(from_dir), Some(to_dir)) = (dirs.get(from.as_str()), dirs.get(to.as_str()))
        else {
            continue;
        };
        edges.add(
            &package_id(from_dir, from),
            &package_id(to_dir, to),
            kind,
            &reference.file,
            reference.line,
        );
    }
    crate::calls::resolve(declared, dirs, &mut edges, report);
    let tree = NodeTree::new(graph);
    edges.retain(|from, to| !tree.related(from, to));
    edges
}

/// Counts how the references resolve, by kind: to a module of the repository, to a module
/// outside it, to a module that the repository does not declare below one that it declares,
/// or to no written name. The counts are per reference, before edges are joined or dropped.
pub fn count(declared: &Declared, dirs: &BTreeMap<&str, &str>, report: &mut Report) {
    for reference in &declared.references {
        let counts = report
            .references
            .entry(reference.kind.name().to_string())
            .or_default();
        match &reference.name {
            Name::Module(name) if dirs.contains_key(name.as_str()) => counts.resolved += 1,
            Name::Module(name) if below_declared(dirs, name) => {
                counts.unresolved += 1;
                *report.unresolved_names.entry(name.clone()).or_default() += 1;
            }
            Name::Module(_) | Name::Erlang => counts.external += 1,
            Name::Dynamic => counts.dynamic += 1,
        }
    }
}

/// Whether `name` is below a module of the repository (`dirs`): one of the repository that
/// it does not declare. A shared first segment says less: `Mix.Tasks.Mine` does not make
/// `Mix.Task` a module to declare.
pub fn below_declared(dirs: &BTreeMap<&str, &str>, name: &str) -> bool {
    name.match_indices('.')
        .any(|(end, _)| dirs.contains_key(&name[..end]))
}

/// The parent links of a graph.
struct NodeTree<'g>(HashMap<&'g str, Option<&'g str>>);

impl<'g> NodeTree<'g> {
    fn new(graph: &'g Graph) -> Self {
        NodeTree(
            graph
                .nodes
                .iter()
                .map(|n| (n.id.as_str(), n.parent.as_deref()))
                .collect(),
        )
    }

    fn is_ancestor(&self, ancestor: &str, node: &str) -> bool {
        let mut current = self.0.get(node).copied().flatten();
        while let Some(id) = current {
            if id == ancestor {
                return true;
            }
            current = self.0.get(id).copied().flatten();
        }
        false
    }

    /// Whether the nodes are the same, or one is an ancestor of the other.
    fn related(&self, a: &str, b: &str) -> bool {
        a == b || self.is_ancestor(a, b) || self.is_ancestor(b, a)
    }
}
