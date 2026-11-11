//! The nesting of packages (ADR 18), shared by the front ends whose packages are
//! dot-separated names: Java packages and Clojure namespaces.

use std::collections::{BTreeMap, BTreeSet};

/// A package node of one module, see `package_nodes`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageNode {
    /// The full package name, also of a prefix that has no declarations of its own.
    pub name: String,
    /// The name that the node shows: relative to its parent package.
    pub display: String,
    /// The full name of the nearest package node whose name is a proper prefix.
    pub parent: Option<String>,
    pub test: bool,
}

/// The package nodes of each module (ADR 18), parents before children. `holds` has the
/// packages that are nodes by themselves (Java: those that hold a type; Clojure: those
/// with a file), with whether all their declarations are test code.
///
/// A package of `holds` is a node. So is each prefix of such a package, on `.`
/// boundaries, under which at least two different next segments occur, unless it is the
/// common prefix of all packages of the module. A prefix node is test code if all packages
/// below it are.
pub fn package_nodes<'a>(
    holds: &BTreeMap<(Option<&'a str>, String), bool>,
) -> Vec<(Option<&'a str>, Vec<PackageNode>)> {
    let mut modules: BTreeMap<Option<&str>, BTreeMap<&str, bool>> = BTreeMap::new();
    for ((module, name), test) in holds {
        modules.entry(*module).or_default().insert(name, *test);
    }
    let mut out = Vec::new();
    for (module, packages) in modules {
        // The default package has no name to nest by.
        let named: Vec<&str> = packages.keys().copied().filter(|n| !n.is_empty()).collect();
        let common = common_prefix(&named);
        let mut next: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        for name in &named {
            for (end, _) in name.match_indices('.') {
                let segments = name[end + 1..].split('.').next().unwrap_or_default();
                next.entry(&name[..end]).or_default().insert(segments);
            }
        }
        let mut nodes: BTreeMap<&str, bool> = packages.iter().map(|(n, t)| (*n, *t)).collect();
        for (prefix, following) in &next {
            if following.len() >= 2 && *prefix != common && !nodes.contains_key(prefix) {
                let below = named.iter().filter(|n| is_below(n, prefix));
                nodes.insert(prefix, below.clone().all(|n| packages[n]));
            }
        }
        let list = nodes
            .iter()
            .map(|(name, test)| {
                let parent = nodes
                    .keys()
                    .filter(|candidate| is_below(name, candidate))
                    .max_by_key(|candidate| candidate.len());
                PackageNode {
                    name: name.to_string(),
                    display: parent.map_or(*name, |p| &name[p.len() + 1..]).to_string(),
                    parent: parent.map(|p| p.to_string()),
                    test: *test && !packages.contains_key(name),
                }
            })
            .collect();
        out.push((module, list));
    }
    out
}

/// Whether `name` is a package below `prefix`, on `.` boundaries.
fn is_below(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix)
        .is_some_and(|rest| rest.starts_with('.'))
}

/// The longest common prefix of the names, on `.` boundaries.
fn common_prefix(names: &[&str]) -> String {
    let mut common: Vec<&str> = match names.first() {
        Some(first) => first.split('.').collect(),
        None => return String::new(),
    };
    for name in names {
        let shared = common
            .iter()
            .zip(name.split('.'))
            .take_while(|(a, b)| **a == *b)
            .count();
        common.truncate(shared);
    }
    common.join(".")
}
