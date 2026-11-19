//! The nesting of packages (ADR 18), shared by the front ends whose packages are
//! dot-separated names: Java packages, Clojure namespaces and Elixir modules.

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
    package_nodes_by(holds, '.')
}

/// `package_nodes` with `separator` in the place of `.`.
fn package_nodes_by<'a>(
    holds: &BTreeMap<(Option<&'a str>, String), bool>,
    separator: char,
) -> Vec<(Option<&'a str>, Vec<PackageNode>)> {
    let mut modules: BTreeMap<Option<&str>, BTreeMap<&str, bool>> = BTreeMap::new();
    for ((module, name), test) in holds {
        modules.entry(*module).or_default().insert(name, *test);
    }
    modules
        .into_iter()
        .map(|(module, packages)| (module, module_package_nodes(&packages, separator)))
        .collect()
}

/// The package nodes of one module: `packages` with their prefixes that branch.
fn module_package_nodes(packages: &BTreeMap<&str, bool>, separator: char) -> Vec<PackageNode> {
    // The default package has no name to nest by.
    let named: Vec<&str> = packages.keys().copied().filter(|n| !n.is_empty()).collect();
    let common = common_prefix(&named, separator);
    let mut nodes: BTreeMap<&str, bool> = packages.iter().map(|(n, t)| (*n, *t)).collect();
    for (prefix, following) in &following_segments(&named, separator) {
        if following.len() >= 2 && *prefix != common && !nodes.contains_key(prefix) {
            let below = named.iter().filter(|n| is_below(n, prefix, separator));
            nodes.insert(prefix, below.clone().all(|n| packages[n]));
        }
    }
    nodes
        .iter()
        .map(|(name, test)| {
            let parent = nodes
                .keys()
                .filter(|candidate| is_below(name, candidate, separator))
                .max_by_key(|candidate| candidate.len());
            PackageNode {
                name: name.to_string(),
                display: parent
                    .map_or(*name, |p| &name[p.len() + separator.len_utf8()..])
                    .to_string(),
                parent: parent.map(|p| p.to_string()),
                test: *test && !packages.contains_key(name),
            }
        })
        .collect()
}

/// For each proper prefix of a name, on `separator` boundaries, the segments that follow it.
fn following_segments<'n>(
    named: &[&'n str],
    separator: char,
) -> BTreeMap<&'n str, BTreeSet<&'n str>> {
    let mut next: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for name in named {
        for (end, _) in name.match_indices(separator) {
            let segments = name[end + separator.len_utf8()..]
                .split(separator)
                .next()
                .unwrap_or_default();
            next.entry(&name[..end]).or_default().insert(segments);
        }
    }
    next
}

/// The packages that are nodes by themselves, per module (the key is the directory of the
/// module) and name, with whether everything in them is test code.
pub type PackageHolds = BTreeMap<(String, String), bool>;

/// The classes that a front end declares, for another front end that names them: the
/// binary class name (`a.b.Outer$Inner`) to the id of the type node.
pub type ClassTable = BTreeMap<String, String>;

/// Like `package_nodes`, with the `test` flag of the Clojure front end (ADR 23; the
/// Elixir front end uses it too, ADR 35) for each
/// node, also for a package that holds something: it is test code if all it holds is, and
/// so are all packages below it. A package of test code that holds no node (a file of
/// `deftest` forms) is hidden with the tests this way, unless a package of main code is
/// below it.
pub fn marked_package_nodes(holds: &PackageHolds) -> Vec<(String, Vec<PackageNode>)> {
    marked_package_nodes_by(holds, '.')
}

/// `marked_package_nodes` with `separator` in the place of `.`: for names such as
/// `src/lib/forge.test`, whose dots are no separators.
pub fn marked_package_nodes_by(
    holds: &PackageHolds,
    separator: char,
) -> Vec<(String, Vec<PackageNode>)> {
    let by_module: BTreeMap<(Option<&str>, String), bool> = holds
        .iter()
        .map(|((module, name), test)| ((Some(module.as_str()), name.clone()), *test))
        .collect();
    package_nodes_by(&by_module, separator)
        .into_iter()
        .map(|(module, mut nodes)| {
            let module = module.expect("every package has a module");
            let is_test = |n: &PackageNode| {
                holds
                    .get(&(module.to_string(), n.name.clone()))
                    .copied()
                    .unwrap_or(n.test)
            };
            let flags: Vec<bool> = nodes
                .iter()
                .map(|node| {
                    is_test(node)
                        && nodes
                            .iter()
                            .filter(|n| is_below(&n.name, &node.name, separator))
                            .all(is_test)
                })
                .collect();
            for (node, test) in nodes.iter_mut().zip(flags) {
                node.test = test;
            }
            (module.to_string(), nodes)
        })
        .collect()
}

/// Whether `name` is a package below `prefix`, on `separator` boundaries.
fn is_below(name: &str, prefix: &str, separator: char) -> bool {
    name.strip_prefix(prefix)
        .is_some_and(|rest| rest.starts_with(separator))
}

/// The longest common prefix of the names, on `separator` boundaries.
fn common_prefix(names: &[&str], separator: char) -> String {
    let mut common: Vec<&str> = match names.first() {
        Some(first) => first.split(separator).collect(),
        None => return String::new(),
    };
    for name in names {
        let shared = common
            .iter()
            .zip(name.split(separator))
            .take_while(|(a, b)| **a == *b)
            .count();
        common.truncate(shared);
    }
    common.join(&separator.to_string())
}
