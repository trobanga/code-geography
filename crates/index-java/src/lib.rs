//! Java front end: reads a source tree and builds a `cg_core::Graph`.
//!
//! Pass 1 parses each file and declares the module, package, type and function nodes.
//! Pass 2 resolves the type names in each file against the declared types and
//! adds the edges. A generic type parameter hides a declared type of the same name.

mod calls;

pub use cg_core::SourceFile;
use cg_core::{Edge, EdgeKind, Graph, Node, NodeKind, Span};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io;
use std::path::Path;
use tree_sitter::{Node as Ts, Parser, Tree};

const TYPE_DECLARATIONS: [&str; 5] = [
    "class_declaration",
    "interface_declaration",
    "enum_declaration",
    "record_declaration",
    "annotation_type_declaration",
];

struct Unit {
    source: String,
    tree: Tree,
    package: String,
}

/// The names that a file can use without qualification.
struct Scope<'a> {
    declared: &'a HashSet<String>,
    package: &'a str,
    /// Simple name -> qualified name, from the single-type imports.
    explicit: HashMap<&'a str, &'a str>,
    /// Packages from the on-demand imports (`import p.*;`).
    on_demand: Vec<&'a str>,
    /// Qualified names of the types named by the static imports.
    static_holders: Vec<&'a str>,
    /// The static imports as (type, member); no member for `import static p.T.*;`.
    static_members: Vec<(&'a str, Option<&'a str>)>,
}

impl<'a> Scope<'a> {
    fn new(unit: &'a Unit, declared: &'a HashSet<String>) -> Self {
        let mut scope = Scope {
            declared,
            package: &unit.package,
            explicit: HashMap::new(),
            on_demand: Vec::new(),
            static_holders: Vec::new(),
            static_members: Vec::new(),
        };
        for import in children(unit.tree.root_node()).filter(|c| c.kind() == "import_declaration") {
            let is_static = (0..import.child_count())
                .filter_map(|i| import.child(i))
                .any(|c| c.kind() == "static");
            let Some(path) = children(import).find(|c| c.kind().ends_with("identifier")) else {
                continue;
            };
            let path = text(path, &unit.source);
            let wildcard = children(import).any(|c| c.kind() == "asterisk");
            if is_static {
                // The member is the last segment, except with `*`.
                let holder = if wildcard {
                    Some(path)
                } else {
                    path.rsplit_once('.').map(|(h, _)| h)
                };
                scope.static_holders.extend(holder);
                if wildcard {
                    scope.static_members.push((path, None));
                } else if let Some((holder, member)) = path.rsplit_once('.') {
                    scope.static_members.push((holder, Some(member)));
                }
            } else if wildcard {
                scope.on_demand.push(path);
            } else {
                let simple = path.rsplit('.').next().unwrap_or(path);
                scope.explicit.insert(simple, path);
            }
        }
        scope
    }

    /// Resolves a type name as it is written in the source to a declared type.
    /// `enclosing` holds the qualified names of the types around the name, outermost first.
    fn resolve(&self, name: &str, enclosing: &[String]) -> Option<String> {
        let name: String = name.split_whitespace().collect();
        match name.split_once('.') {
            None => self.resolve_simple(&name, enclosing),
            Some((head, rest)) => self
                .resolve_simple(head, enclosing)
                .map(|head| format!("{head}.{rest}"))
                .filter(|nested| self.declared.contains(nested))
                .or_else(|| self.resolve_qualified(&name)),
        }
    }

    /// Resolves a name with the full qualification, for example `a.Outer.Inner`.
    fn resolve_qualified(&self, name: &str) -> Option<String> {
        Some(type_id("", name)).filter(|id| self.declared.contains(id))
    }

    /// Order as in the Java language: member types of the enclosing types (innermost
    /// first), single-type import, same package, on-demand import.
    fn resolve_simple(&self, name: &str, enclosing: &[String]) -> Option<String> {
        let mut candidates = Vec::new();
        for outer in enclosing.iter().rev() {
            if outer.rsplit('.').next() == Some(name) {
                candidates.push(type_id("", outer));
            }
            candidates.push(type_id(outer, name));
        }
        candidates.extend(self.explicit.get(name).map(|path| type_id("", path)));
        candidates.push(type_id(self.package, name));
        candidates.extend(self.on_demand.iter().map(|p| type_id(p, name)));
        candidates.into_iter().find(|c| self.declared.contains(c))
    }
}

pub use cg_core::CallReport;

/// What the indexer could not resolve.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Report {
    pub calls: CallReport,
}

/// Whether the indexer wants the file at `path` (relative to the root, `/` as separator):
/// a `.java` file or a `pom.xml`, not below a directory that `cg_core::holds_no_sources`.
pub fn is_input(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    let wanted = name == "pom.xml" || (name.len() > ".java".len() && name.ends_with(".java"));
    wanted && cg_core::outside_excluded_directories(path)
}

pub fn index(root: &Path) -> io::Result<Graph> {
    index_with_report(root).map(|(graph, _)| graph)
}

/// Walks `root` with `cg_walk::read_sources`, keeps the files that `is_input` accepts and
/// indexes them with `index_sources`.
pub fn index_with_report(root: &Path) -> io::Result<(Graph, Report)> {
    let files = cg_walk::read_sources(root, is_input)?;
    Ok(index_sources(&file_name(root), &files))
}

/// Indexes sources that are given as data. The result does not depend on the order of
/// `files`. `root_name` names the module of a `pom.xml` in the root. Files that
/// `is_input` rejects are ignored.
pub fn index_sources(root_name: &str, files: &[SourceFile]) -> (Graph, Report) {
    let mut files: Vec<&SourceFile> = files.iter().filter(|f| is_input(&f.path)).collect();
    // The order of the components, as `Path` compares, not of the bytes.
    files.sort_by(|a, b| {
        a.path
            .split('/')
            .cmp(b.path.split('/'))
            .then_with(|| a.text.cmp(&b.text))
    });
    files.dedup_by(|a, b| a.path == b.path);
    let module_dirs: HashSet<&str> = files
        .iter()
        .filter_map(|f| match f.path.rsplit_once('/') {
            _ if f.path == "pom.xml" => Some(""),
            Some((dir, "pom.xml")) => Some(dir),
            _ => None,
        })
        .collect();

    let mut report = Report::default();
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_java::LANGUAGE.into())
        .expect("the Java grammar matches the tree-sitter version");

    let mut graph = Graph::default();
    let mut containers = BTreeSet::new();
    let mut units = Vec::new();
    let mut declared_nodes = Vec::new();
    // For each package that holds a type, in a module (`None`: no module): whether all
    // its types are test code.
    let mut package_holds: BTreeMap<(Option<&str>, String), bool> = BTreeMap::new();
    for file in files.iter().filter(|f| f.path.ends_with(".java")) {
        let source = file.text.clone();
        let Some(tree) = parser.parse(&source, None) else {
            continue;
        };
        let package = package_name(tree.root_node(), &source);
        let relative = file.path.as_str();
        let module = module_of(&module_dirs, relative);
        let module_id = module.map(|dir| format!("mod:{dir}"));
        if let (Some(dir), Some(id)) = (module, &module_id)
            && containers.insert(id.clone())
        {
            graph.nodes.push(Node {
                id: id.clone(),
                parent: None,
                kind: NodeKind::Module,
                name: dir
                    .rsplit('/')
                    .next()
                    .filter(|n| !n.is_empty())
                    .unwrap_or(root_name)
                    .to_string(),
                file: None,
                span: None,
                test: false,
                hash: None,
                change: None,
            });
        }
        let package_id = package_id(module, &package);

        let file = relative.to_string();
        let is_test = cg_core::in_test_sources(&file);
        let decls = declarations(tree.root_node(), &source, &package);
        if !decls.is_empty() {
            let all_test = package_holds
                .entry((module, package.clone()))
                .or_insert(true);
            *all_test &= is_test;
        }
        for decl in decls {
            declared_nodes.push(Node {
                hash: Some(own_hash(decl.node, &source)),
                id: decl.id,
                parent: Some(decl.parent.unwrap_or_else(|| package_id.clone())),
                kind: decl.kind,
                name: decl.name,
                file: Some(file.clone()),
                span: Some(span_of(decl.node)),
                test: is_test,
                change: None,
            });
        }
        units.push(Unit {
            source,
            tree,
            package,
        });
    }

    for (module, nodes) in cg_core::package_nodes(&package_holds) {
        let module_id = module.map(|dir| format!("mod:{dir}"));
        for node in nodes {
            graph.nodes.push(Node {
                id: package_id(module, &node.name),
                parent: node
                    .parent
                    .map(|parent| package_id(module, &parent))
                    .or_else(|| module_id.clone()),
                kind: NodeKind::Package,
                name: node.display,
                file: None,
                span: None,
                test: node.test,
                hash: None,
                change: None,
            });
        }
    }
    graph.nodes.append(&mut declared_nodes);

    let declared: HashSet<String> = graph
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Type)
        .map(|n| n.id.clone())
        .collect();
    let scopes: Vec<Scope> = units.iter().map(|u| Scope::new(u, &declared)).collect();
    let all_decls: Vec<Vec<Decl>> = units
        .iter()
        .map(|u| declarations(u.tree.root_node(), &u.source, &u.package))
        .collect();
    let model = calls::Model::build(&units, &scopes, &all_decls);
    let mut edges = BTreeSet::new();
    for ((unit, scope), decls) in units.iter().zip(&scopes).zip(all_decls) {
        let mut edge = |from: &str, to: Option<String>, kind| {
            if let Some(to) = to.filter(|to| to != from) {
                edges.insert(Edge {
                    from: from.to_string(),
                    to,
                    kind,
                    change: None,
                });
            }
        };
        let top_level = decls
            .iter()
            .filter(|d| d.kind == NodeKind::Type && d.parent.is_none());
        for top in top_level {
            for path in scope.explicit.values().chain(&scope.static_holders) {
                edge(&top.id, scope.resolve_qualified(path), EdgeKind::Imports);
            }
        }
        for decl in decls {
            let mut references = Vec::new();
            let resolve = |name: &str| {
                if decl.type_parameters.iter().any(|p| p == name) {
                    None
                } else {
                    scope.resolve(name, &decl.types)
                }
            };
            if decl.kind == NodeKind::Type {
                for (kind, name) in supertypes(decl.node, &unit.source) {
                    edge(&decl.id, resolve(name), kind);
                }
                collect_own_type_names(decl.node, &mut references);
            } else {
                collect_type_names(decl.node, &mut references);
            }
            for reference in references {
                // A function says nothing about its own type by naming it.
                let to = resolve(text(reference, &unit.source))
                    .filter(|to| decl.kind == NodeKind::Type || Some(to) != decl.parent.as_ref());
                edge(&decl.id, to, EdgeKind::TypeRef);
            }
            let mut found = Vec::new();
            calls::Walker::run(&model, unit, scope, &decl, &mut report.calls, &mut found);
            for (from, to) in found {
                edge(&from, Some(to), EdgeKind::Calls);
            }
        }
    }
    graph.edges = edges.into_iter().collect();
    (graph, report)
}

fn package_id(module: Option<&str>, package: &str) -> String {
    match module {
        Some(dir) => format!("pkg:{dir}:{package}"),
        None => format!("pkg:{package}"),
    }
}

/// The supertypes that a type declaration names, with the kind of the relation.
fn supertypes<'a>(declaration: Ts, source: &'a str) -> Vec<(EdgeKind, &'a str)> {
    let mut found = Vec::new();
    for clause in children(declaration) {
        let kind = match clause.kind() {
            "superclass" | "extends_interfaces" => EdgeKind::Extends,
            "super_interfaces" => EdgeKind::Implements,
            _ => continue,
        };
        let mut types = Vec::new();
        collect_type_heads(clause, &mut types);
        found.extend(types.into_iter().map(|t| (kind, text(t, source))));
    }
    found
}

/// Collects the type names below `node`, without the type arguments of generics.
fn collect_type_heads<'t>(node: Ts<'t>, found: &mut Vec<Ts<'t>>) {
    match node.kind() {
        "type_identifier" | "scoped_type_identifier" => found.push(node),
        "type_arguments" => {}
        _ => children(node).for_each(|c| collect_type_heads(c, found)),
    }
}

/// Collects the type names of a type declaration that belong to the type itself:
/// the header and the members that are not declarations of their own.
fn collect_own_type_names<'t>(declaration: Ts<'t>, found: &mut Vec<Ts<'t>>) {
    let body = declaration.child_by_field_name("body");
    for part in children(declaration).filter(|c| Some(*c) != body) {
        collect_type_names(part, found);
    }
    for member in members(declaration) {
        if !is_type_declaration(member) && !is_function_declaration(member) {
            collect_type_names(member, found);
        }
    }
}

/// Collects each type name below `node`, with the type arguments of generics.
fn collect_type_names<'t>(node: Ts<'t>, found: &mut Vec<Ts<'t>>) {
    match node.kind() {
        "type_identifier" | "scoped_type_identifier" => found.push(node),
        _ => children(node).for_each(|c| collect_type_names(c, found)),
    }
}

/// A type or function that becomes a node.
struct Decl<'t> {
    kind: NodeKind,
    id: String,
    name: String,
    node: Ts<'t>,
    /// The id of the enclosing type, `None` for a top-level type.
    parent: Option<String>,
    /// Qualified names of the types around the declaration, outermost first.
    /// A type is part of its own chain.
    types: Vec<String>,
    /// The generic type parameters in scope: those of the declaration itself and
    /// of every enclosing type. They hide declared types of the same name.
    type_parameters: Vec<String>,
}

/// All declarations of a file, an enclosing type before the declarations in it.
fn declarations<'t>(unit: Ts<'t>, source: &str, package: &str) -> Vec<Decl<'t>> {
    let mut found = Vec::new();
    for declaration in type_declarations(unit) {
        declare_type(
            declaration,
            source,
            package,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut found,
        );
    }
    found
}

/// The names that the `<...>` list of a type or method declares.
fn type_parameter_names(declaration: Ts, source: &str) -> Vec<String> {
    let Some(list) = declaration.child_by_field_name("type_parameters") else {
        return Vec::new();
    };
    children(list)
        .filter(|c| c.kind() == "type_parameter")
        .filter_map(|p| children(p).find(|c| c.kind() == "type_identifier"))
        .map(|name| text(name, source).to_string())
        .collect()
}

fn declare_type<'t>(
    declaration: Ts<'t>,
    source: &str,
    package: &str,
    chain: &mut Vec<String>,
    parameters: &mut Vec<String>,
    found: &mut Vec<Decl<'t>>,
) {
    let parameters_before = parameters.len();
    parameters.extend(type_parameter_names(declaration, source));
    let name = declared_name(declaration, source);
    let qualified = match chain.last() {
        Some(outer) => format!("{outer}.{name}"),
        None if package.is_empty() => name.to_string(),
        None => format!("{package}.{name}"),
    };
    let parent = chain.last().map(|outer| type_id("", outer));
    chain.push(qualified);
    found.push(Decl {
        kind: NodeKind::Type,
        id: type_id("", &chain[chain.len() - 1]),
        name: name.to_string(),
        node: declaration,
        parent,
        types: chain.clone(),
        type_parameters: parameters.clone(),
    });
    for member in members(declaration) {
        if is_type_declaration(member) {
            declare_type(member, source, package, chain, parameters, found);
        } else if is_function_declaration(member) {
            let owner = &chain[chain.len() - 1];
            let name = declared_name(member, source);
            let mut type_parameters = parameters.clone();
            type_parameters.extend(type_parameter_names(member, source));
            found.push(Decl {
                kind: NodeKind::Function,
                id: format!("fn:{owner}#{name}({})", parameter_types(member, source)),
                name: name.to_string(),
                node: member,
                parent: Some(type_id("", owner)),
                types: chain.clone(),
                type_parameters,
            });
        }
    }
    parameters.truncate(parameters_before);
    chain.pop();
}

/// The parameter types as written in the source, without whitespace, joined by `,`.
fn parameter_types(function: Ts, source: &str) -> String {
    let Some(parameters) = function.child_by_field_name("parameters") else {
        return String::new();
    };
    let types: Vec<String> = children(parameters)
        .filter_map(|parameter| {
            let ty = match parameter.kind() {
                "formal_parameter" => parameter.child_by_field_name("type")?,
                "spread_parameter" => children(parameter)
                    .find(|c| !matches!(c.kind(), "modifiers" | "variable_declarator"))?,
                _ => return None,
            };
            let written: String = text(ty, source).split_whitespace().collect();
            Some(if parameter.kind() == "spread_parameter" {
                format!("{written}...")
            } else {
                written
            })
        })
        .collect();
    types.join(",")
}

fn is_function_declaration(node: Ts) -> bool {
    matches!(
        node.kind(),
        "method_declaration" | "constructor_declaration" | "compact_constructor_declaration"
    ) && node.child_by_field_name("name").is_some()
}

/// The members of a type body. The constants and the members of an enum are siblings.
fn members(declaration: Ts) -> Vec<Ts> {
    let mut found = Vec::new();
    let Some(body) = declaration.child_by_field_name("body") else {
        return found;
    };
    for member in children(body) {
        if member.kind() == "enum_body_declarations" {
            found.extend(children(member));
        } else {
            found.push(member);
        }
    }
    found
}

fn is_type_declaration(node: Ts) -> bool {
    TYPE_DECLARATIONS.contains(&node.kind()) && node.child_by_field_name("name").is_some()
}

fn type_declarations(unit: Ts) -> impl Iterator<Item = Ts> {
    children(unit).filter(|c| is_type_declaration(*c))
}

fn declared_name<'a>(declaration: Ts, source: &'a str) -> &'a str {
    declaration
        .child_by_field_name("name")
        .map(|name| text(name, source))
        .unwrap_or_default()
}

/// The nearest directory above the file, inside the root, that has a `pom.xml`.
/// The root is the empty string.
fn module_of<'a>(module_dirs: &HashSet<&str>, relative: &'a str) -> Option<&'a str> {
    let mut dir = relative;
    while let Some((above, _)) = dir.rsplit_once('/') {
        dir = above;
        if module_dirs.contains(dir) {
            return Some(dir);
        }
    }
    module_dirs.contains("").then_some("")
}

fn package_name(unit: Ts, source: &str) -> String {
    children(unit)
        .find(|c| c.kind() == "package_declaration")
        .and_then(|p| children(p).find(|c| c.kind().ends_with("identifier")))
        .map(|name| text(name, source).to_string())
        .unwrap_or_default()
}

fn type_id(package: &str, name: &str) -> String {
    if package.is_empty() {
        format!("type:{name}")
    } else {
        format!("type:{package}.{name}")
    }
}

/// The hash of the own source text of a declaration: the text without the byte
/// ranges of the member functions and member types, which are nodes themselves.
fn own_hash(declaration: Ts, source: &str) -> String {
    let mut own = String::new();
    let mut position = declaration.start_byte();
    for member in members(declaration) {
        if is_type_declaration(member) || is_function_declaration(member) {
            own.push_str(&source[position..member.start_byte()]);
            position = member.end_byte();
        }
    }
    own.push_str(&source[position..declaration.end_byte()]);
    cg_core::text_hash(&own)
}

/// The lines of a declaration: 1-based, inclusive.
fn span_of(node: Ts) -> Span {
    Span {
        start_line: node.start_position().row as u32 + 1,
        end_line: node.end_position().row as u32 + 1,
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn children(node: Ts) -> impl Iterator<Item = Ts> {
    (0..node.named_child_count()).filter_map(move |i| node.named_child(i as u32))
}

fn text<'a>(node: Ts, source: &'a str) -> &'a str {
    &source[node.byte_range()]
}
