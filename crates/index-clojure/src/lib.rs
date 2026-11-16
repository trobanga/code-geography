//! Clojure front end: turns the sources of a `deps.edn` project into a graph.

mod calls;
mod declare;
mod deps;
mod late;
mod locals;
mod ns;
mod syntax;

pub use cg_core::SourceFile;
use cg_core::{ClassTable, EdgeKind, EdgeSet, Graph, Node, NodeKind, PackageHolds, Span};
use deps::SourceDirs;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use tree_sitter::Parser;

/// What the indexer could not read or place.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Report {
    /// Source files below a `deps.edn` directory but outside all its source directories
    /// (`build.clj` scripts, `dev` and `test` directories that no alias names).
    pub files_outside_source_dirs: usize,
    /// Source files with no `deps.edn` above them.
    pub files_without_deps_edn: usize,
    /// Source files that are read and hold no `ns` form: no nodes.
    pub files_without_ns: usize,
    /// Files that name a namespace that an earlier file (in path order) names too: they
    /// give no nodes. Sorted.
    pub duplicate_namespaces: Vec<String>,
    /// The top-level forms that give no node, counted by their head symbol.
    pub forms_without_node: BTreeMap<String, usize>,
    /// The top-level forms that declare a name that an earlier form of the namespace declared
    /// (`defn` twice, `def` and `defn`, `defmethod` of the same multi and dispatch value
    /// twice, reader-conditional branches): no node, the first form gives it. By head.
    pub duplicate_declarations: BTreeMap<String, usize>,
    /// The nodes of kind value (`def`, `defonce` whose value is no literal `fn`).
    pub value_nodes: usize,
    /// The function nodes of a `defmethod`.
    pub method_nodes: usize,
    /// The function nodes of a method in `extend-protocol` and `extend-type`.
    pub extend_nodes: usize,
    /// The top-level `(extend T P {:m (fn ..)})` forms: their functions are not read.
    pub extend_forms: usize,
    /// How the symbols in the code of functions were resolved, counted per site
    /// (before the removal of duplicate edges). The same four classes as for Java and Rust.
    pub calls: cg_core::CallReport,
    /// Of the calls to a function: those where the function is named, not called
    /// (`(map a/f xs)`, `#'a/f`).
    pub function_values: usize,
    /// Of the unresolved calls: those whose namespace only `declare`s the name, which gives no
    /// node (a `def` gives a value node or a function node).
    pub var_without_node: usize,
    /// Calls to a function and references to a type or value in code that has no node (a
    /// duplicate `def`, `defmethod` or extend method; `defrecord` or `deftype` methods that
    /// no node holds; any other top-level form): no edge, not in `calls`. By the head of the
    /// top-level form: `def`, `defmethod`, `extend-protocol`, `extend-type`, `defrecord`,
    /// `deftype`, `other`.
    pub no_source_node: BTreeMap<String, usize>,
    /// The other unresolved calls: a repository namespace that does not declare the name
    /// (a macro of the repository can have made it). `namespace/name` to count.
    pub unresolved_names: BTreeMap<String, usize>,
    /// The sites of unqualified symbols that name a var of the repository (stage 2b).
    pub unqualified: Unqualified,
    /// The sites that name a class (`:import`, stage 3).
    pub classes: ClassRefs,
}

/// The sites in the code of functions that name a class: `(Foo. ..)`, `(new Foo ..)`,
/// `Foo/member`, `Foo` as a symbol, where `Foo` is a class of an `:import` clause or of the
/// `defrecord`, `deftype` or `definterface` of the file, or a full class name. The sites that
/// give a count are part of `Report::calls`, too (`to_type`, `external`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ClassRefs {
    /// Sites that name a type of the repository: Java or Clojure (an edge, unless it would
    /// join a node with itself or its ancestor).
    pub to_type: usize,
    /// Sites that name a class that is not in the repository (`java.util.Date`).
    pub external: usize,
    /// No edge: an occurrence of the short name of an imported class, in a top-level form
    /// where a local could bind this name (`locals`). Counts the binder and each other
    /// occurrence, so it is an upper bound of the lost references.
    pub maybe_local: usize,
}

/// The unqualified symbols in the code of functions that the namespace of the file or
/// a `:refer`, `:refer :all` or `:use` names, counted per site. Symbols that no table
/// declares (core functions, locals, interop, special forms) are not sites. The sites
/// that give an edge are part of `Report::calls` and of `Report::function_values`, too.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Unqualified {
    /// Sites that name a function (an edge, unless it would join a node with itself or
    /// its ancestor).
    pub to_function: usize,
    /// Of `to_function`: the function is named, not called.
    pub function_values: usize,
    /// Sites that name a protocol, an interface or a constructor of a type.
    pub to_type: usize,
    /// Sites that name a value node (`def`).
    pub to_value: usize,
    /// No edge: a call (the head of a list) whose name also occurs in the same top-level
    /// form at a place where a local could be bound (`locals`). The binder and other
    /// uses of the name are no sites.
    pub maybe_local: usize,
    /// No edge: the namespace of the file and a `:refer` (or two of these) both declare
    /// the name.
    pub ambiguous: usize,
}

/// Whether the indexer wants the file at `path` (relative to the root, `/` as separator):
/// a `.clj` or `.cljc` file or a `deps.edn`, not below a directory that
/// `cg_core::holds_no_sources`.
pub fn is_input(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    let source = [".clj", ".cljc"]
        .iter()
        .any(|ext| name.len() > ext.len() && name.ends_with(ext))
        // The Leiningen project file is Clojure code, but it names no namespace.
        && name != "project.clj";
    (name == "deps.edn" || source) && cg_core::outside_excluded_directories(path)
}

/// The `deps.edn` modules of a source set: what is needed to tell whether a path is test
/// code without reading the sources (the changed files of an overlay).
#[derive(Debug, Clone, Default)]
pub struct Layout {
    modules: Vec<(String, SourceDirs)>,
}

impl Layout {
    /// Reads the `deps.edn` files among `files`; other files are ignored.
    pub fn new(files: &[SourceFile]) -> Layout {
        let mut parser = clojure_parser();
        let modules = files
            .iter()
            .filter(|f| is_input(&f.path))
            .filter_map(|f| {
                let dir = match f.path.rsplit_once('/') {
                    Some((dir, "deps.edn")) => dir,
                    None if f.path == "deps.edn" => "",
                    _ => return None,
                };
                Some((dir.to_string(), SourceDirs::parse(&mut parser, &f.text)))
            })
            .collect();
        Layout { modules }
    }

    /// The directories of the `deps.edn` files, relative to the root (empty: the root).
    pub fn module_dirs(&self) -> impl Iterator<Item = &str> {
        self.modules.iter().map(|(dir, _)| dir.as_str())
    }

    /// Whether a file of any language, here a Java file, in the nearest module above `path`
    /// is test code by the source directories of its `deps.edn`: the longest one that
    /// contains it comes from an alias. A file outside every source directory is not test
    /// code. `None`: no `deps.edn` above the path.
    pub fn module_test_code(&self, path: &str) -> Option<bool> {
        let (dir, dirs) = self
            .modules
            .iter()
            .filter(|(dir, _)| in_directory(dir, path))
            .max_by_key(|(dir, _)| dir.len())?;
        Some(dirs.test_code(relative_to(dir, path)).unwrap_or(false))
    }

    /// Whether the Clojure file at `path` is test code by ADR 23: the longest source
    /// directory of the nearest `deps.edn` above it comes from an alias. `None` for a
    /// path that is no `.clj` or `.cljc` file, has no `deps.edn` above it, or lies in no
    /// source directory of that module.
    pub fn test_code(&self, path: &str) -> Option<bool> {
        if !(path.ends_with(".clj") || path.ends_with(".cljc")) {
            return None;
        }
        let (dir, dirs) = self
            .modules
            .iter()
            .filter(|(dir, _)| in_directory(dir, path))
            .max_by_key(|(dir, _)| dir.len())?;
        dirs.test_code(relative_to(dir, path))
    }
}

fn clojure_parser() -> Parser {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_clojure_orchard::LANGUAGE.into())
        .expect("the Clojure grammar matches the tree-sitter version");
    parser
}

/// Whether `path` is below the directory `dir` (empty: the root).
fn in_directory(dir: &str, path: &str) -> bool {
    dir.is_empty() || path.strip_prefix(dir).is_some_and(|r| r.starts_with('/'))
}

fn relative_to<'a>(dir: &str, path: &'a str) -> &'a str {
    path.strip_prefix(dir)
        .map_or(path, |rest| rest.trim_start_matches('/'))
}

/// Indexes sources that are given as data. The result does not depend on the order of
/// `files`. Files that `is_input` rejects are ignored.
pub fn index_sources(root_name: &str, files: &[SourceFile]) -> (Graph, Report) {
    let indexed = index_sources_with(root_name, files, &ClassTable::new());
    (indexed.graph, indexed.report)
}

/// What `index_sources_with` gives.
#[derive(Debug, Clone)]
pub struct Indexed {
    pub graph: Graph,
    pub report: Report,
    /// The namespaces that are package nodes by themselves, per module directory, with
    /// whether the file is test code. A caller that joins the graph with the packages of
    /// another language nests the packages of a module over the union of both.
    pub packages: PackageHolds,
}

/// Like `index_sources`, with what a caller needs to join the graph with those of other
/// front ends. `classes` are the classes that these front ends declare (Java types): a
/// class name in an `:import` clause or in code that is in `classes` is a reference to the
/// type node. The classes of `defrecord`, `deftype`, `definterface` and `defprotocol` of
/// the sources are known without it.
pub fn index_sources_with(
    root_name: &str,
    files: &[SourceFile],
    external_classes: &ClassTable,
) -> Indexed {
    let mut files: Vec<&SourceFile> = files.iter().filter(|f| is_input(&f.path)).collect();
    // The order of the components, as `Path` compares, not of the bytes.
    files.sort_by(|a, b| {
        a.path
            .split('/')
            .cmp(b.path.split('/'))
            .then_with(|| a.text.cmp(&b.text))
    });
    files.dedup_by(|a, b| a.path == b.path);

    let mut parser = clojure_parser();
    let mut graph = Graph::default();
    let mut module_dirs: Vec<&str> = Vec::new();
    let mut source_dirs: HashMap<&str, SourceDirs> = HashMap::new();
    for file in &files {
        let dir = match file.path.rsplit_once('/') {
            Some((dir, "deps.edn")) => dir,
            None if file.path == "deps.edn" => "",
            _ => continue,
        };
        module_dirs.push(dir);
        source_dirs.insert(dir, SourceDirs::parse(&mut parser, &file.text));
        graph.nodes.push(Node {
            id: format!("mod:{dir}"),
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

    let mut report = Report::default();
    let mut packages: PackageHolds = BTreeMap::new();
    let mut declared: Vec<Node> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut namespaces: BTreeSet<String> = BTreeSet::new();
    // For each namespace: the path of its file and the namespaces that it requires, each
    // with its line.
    let mut requires: Vec<Requires> = Vec::new();
    let mut package_ids: BTreeMap<String, String> = BTreeMap::new();
    let mut vars: HashMap<String, calls::Vars> = HashMap::new();
    let mut read: Vec<calls::File> = Vec::new();
    // For each file of `read`: its path, whether it is test code, its module.
    let mut read_info: Vec<(String, bool, String)> = Vec::new();
    let mut own_classes = ClassTable::new();
    for file in files.iter().filter(|f| !f.path.ends_with("deps.edn")) {
        let Some(module) = module_of(&module_dirs, &file.path) else {
            report.files_without_deps_edn += 1;
            continue;
        };
        let Some(is_test) = source_dirs[module].test_code(relative_to(module, &file.path)) else {
            report.files_outside_source_dirs += 1;
            continue;
        };
        let Some(tree) = parser.parse(&file.text, None) else {
            continue;
        };
        let source = file.text.as_str();
        let forms = syntax::flat_items(tree.root_node());
        let Some((ns_form, namespace)) = ns::namespace(&forms, source) else {
            report.files_without_ns += 1;
            continue;
        };
        if !namespaces.insert(namespace.clone()) {
            report.duplicate_namespaces.push(file.path.clone());
            continue;
        }
        packages.insert((module.to_string(), namespace.clone()), is_test);
        package_ids.insert(namespace.clone(), package_id(module, &namespace));
        requires.push(Requires {
            namespace: namespace.clone(),
            path: &file.path,
            required: ns::requires(ns_form, source),
        });
        let specs: Vec<ns::Spec> = ns::specs(ns_form, source)
            .into_iter()
            .filter(|s| !s.as_alias)
            .collect();
        let aliases: HashMap<String, String> = specs
            .iter()
            .filter_map(|s| Some((s.alias.clone()?, s.name.clone())))
            .collect();
        let mut refers: HashMap<String, Vec<String>> = HashMap::new();
        for spec in &specs {
            for name in &spec.refer {
                refers
                    .entry(name.clone())
                    .or_default()
                    .push(spec.name.clone());
            }
        }
        let refer_all: Vec<(String, Vec<String>)> = specs
            .iter()
            .filter(|s| s.refer_all)
            .map(|s| (s.name.clone(), s.except.clone()))
            .collect();
        let explicit_imports = ns::class_imports(ns_form, source);
        let mut class_imports: Vec<(String, String)> = explicit_imports
            .iter()
            .map(|(simple, class, _)| (simple.clone(), class.clone()))
            .collect();
        let ns_form = ns_form.id();
        let rest: Vec<_> = forms
            .iter()
            .copied()
            .filter(|f| f.id() != ns_form)
            .collect();
        vars.insert(namespace.clone(), calls::vars(&rest, source, &namespace));
        let mut owned: HashMap<usize, String> = HashMap::new();
        let found = declare::declarations(&rest, source, &namespace, &mut report);
        let late: Vec<late::Late> = found
            .late
            .iter()
            .map(|l| late::Late {
                forms: std::iter::once(l.decl.node.id())
                    .chain(l.decl.more_forms.iter().map(|f| f.id()))
                    .collect(),
                hash: own_hash(&l.decl, source),
                span: span_of(&l.decl),
                spec: l.spec.clone(),
            })
            .collect();
        let decls = found.decls;
        // The methods of a type that was declared before are skipped with it.
        let mut skipped: BTreeSet<String> = BTreeSet::new();
        for decl in decls {
            if decl.parent.as_ref().is_some_and(|p| skipped.contains(p)) {
                continue;
            }
            if !seen.insert(decl.id.clone()) {
                skipped.insert(decl.id);
                continue;
            }
            owned.insert(decl.node.id(), decl.id.clone());
            if decl.kind == NodeKind::Value {
                report.value_nodes += 1;
            }
            if decl.kind == NodeKind::Type {
                let class = class_name(&namespace, &decl.name);
                own_classes.insert(class.clone(), decl.id.clone());
                if decl.imports_class {
                    class_imports.push((decl.name.clone(), class));
                }
            }
            for form in &decl.more_forms {
                owned.insert(form.id(), decl.id.clone());
            }
            let (hash, span) = (own_hash(&decl, source), span_of(&decl));
            declared.push(Node {
                hash: Some(hash),
                id: decl.id,
                parent: Some(
                    decl.parent
                        .unwrap_or_else(|| package_id(module, &namespace)),
                ),
                kind: decl.kind,
                name: decl.name,
                file: Some(file.path.clone()),
                span: Some(span),
                test: is_test,
                change: None,
            });
        }
        read_info.push((file.path.clone(), is_test, module.to_string()));
        read.push(calls::File {
            late,
            path: &file.path,
            source,
            tree,
            ns_form,
            namespace: namespace.clone(),
            aliases,
            refers,
            refer_all,
            owned,
            imports: class_imports.into_iter().collect(),
            explicit_imports,
        });
    }
    for (module, nodes) in cg_core::marked_package_nodes(&packages) {
        for node in nodes {
            graph.nodes.push(Node {
                id: package_id(&module, &node.name),
                parent: Some(match &node.parent {
                    Some(parent) => package_id(&module, parent),
                    None => format!("mod:{module}"),
                }),
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
    graph.nodes.append(&mut declared);
    let mut classes = external_classes.clone();
    classes.extend(own_classes);
    for placed in late::place(&mut read, &vars, &classes, &mut seen, &mut report) {
        let (path, is_test, module) = &read_info[placed.file];
        graph.nodes.push(Node {
            id: placed.id,
            parent: Some(
                placed
                    .parent
                    .unwrap_or_else(|| package_id(module, &read[placed.file].namespace)),
            ),
            kind: NodeKind::Function,
            name: placed.name,
            file: Some(path.clone()),
            span: Some(placed.span),
            test: *is_test,
            hash: Some(placed.hash),
            change: None,
        });
    }
    let mut edges = imports(&package_ids, &requires);
    for file in &read {
        let from = &package_ids[&file.namespace];
        for (_, class, line) in &file.explicit_imports {
            if let Some(to) = classes.get(class) {
                edges.add(from, to, EdgeKind::Imports, file.path, *line);
            }
        }
    }
    calls::read(&read, &vars, &classes, &mut report, &mut edges);
    // No edge joins a node and its ancestor or descendant: the tree holds that relation.
    let tree = NodeTree::new(&graph);
    edges.retain(|from, to| !tree.related(from, to));
    graph.edges = edges.into_edges(&graph.nodes);
    Indexed {
        graph,
        report,
        packages,
    }
}

/// The binary name of the class that `defrecord` and its relatives make for `name` in
/// `namespace`: the names are munged (`x.y-z` gives `x.y_z`).
fn class_name(namespace: &str, name: &str) -> String {
    format!("{}.{}", munge(namespace), munge(name))
}

fn munge(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        match c {
            '-' => out.push('_'),
            '?' => out.push_str("_QMARK_"),
            '!' => out.push_str("_BANG_"),
            '*' => out.push_str("_STAR_"),
            '>' => out.push_str("_GT_"),
            '<' => out.push_str("_LT_"),
            '=' => out.push_str("_EQ_"),
            '+' => out.push_str("_PLUS_"),
            _ => out.push(c),
        }
    }
    out
}

fn package_id(module: &str, namespace: &str) -> String {
    format!("pkg:{module}:{namespace}")
}

/// The directory of the nearest `deps.edn` above the file.
fn module_of<'a>(module_dirs: &[&'a str], path: &str) -> Option<&'a str> {
    module_dirs
        .iter()
        .copied()
        .filter(|dir| in_directory(dir, path))
        .max_by_key(|dir| dir.len())
}

/// The hash of the own source text of a declaration: the text without the members that
/// are nodes themselves; for a method of several forms the text of all of them.
fn own_hash(decl: &declare::Decl, source: &str) -> String {
    let mut own = String::new();
    let mut position = decl.node.start_byte();
    for member in &decl.members {
        own.push_str(&source[position..member.start_byte()]);
        position = member.end_byte();
    }
    own.push_str(&source[position..decl.node.end_byte()]);
    for form in &decl.more_forms {
        own.push('\n');
        own.push_str(&source[form.byte_range()]);
    }
    cg_core::text_hash(&own)
}

/// The lines of a declaration: 1-based, inclusive. For a method of several forms that
/// follow one another from the first to the last.
fn span_of(decl: &declare::Decl) -> Span {
    let last = match decl.more_forms.last() {
        Some(last) if decl.adjacent => *last,
        _ => decl.node,
    };
    Span {
        start_line: decl.node.start_position().row as u32 + 1,
        end_line: last.end_position().row as u32 + 1,
    }
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

    /// Whether the nodes are the same, or one is an ancestor of the other: no edge joins
    /// them.
    fn related(&self, a: &str, b: &str) -> bool {
        a == b || self.is_ancestor(a, b) || self.is_ancestor(b, a)
    }
}

/// The `imports` edges from each namespace to the namespaces of the repository that it
/// requires.
fn imports(package_ids: &BTreeMap<String, String>, requires: &[Requires]) -> EdgeSet {
    let mut edges = EdgeSet::new();
    for Requires {
        namespace,
        path,
        required,
    } in requires
    {
        let from = &package_ids[namespace];
        for (to, line) in required
            .iter()
            .filter_map(|(name, line)| Some((package_ids.get(name)?, *line)))
        {
            edges.add(from, to, EdgeKind::Imports, path, line);
        }
    }
    edges
}

/// The namespaces that one namespace requires.
struct Requires<'f> {
    namespace: String,
    /// The file of the `ns` form: the file of the sites.
    path: &'f str,
    /// The namespaces as written, each with the line of its libspec.
    required: Vec<(String, u32)>,
}
