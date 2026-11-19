//! TypeScript front end: turns the sources of a repository with `package.json` files into
//! a graph (ADR 34).

pub use cg_core::SourceFile;
use cg_core::{Graph, Node, NodeKind, PackageHolds};
use serde::Serialize;
use std::collections::BTreeSet;
use tree_sitter::Parser;

mod bindings;
mod config;
mod declare;
mod edges;
mod heritage;
mod layout;
mod path;
mod resolve;
mod syntax;

use layout::Source;

/// What the indexer could not read or resolve. The call edges (CG-8) add their counts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Report {
    /// `.ts` and `.tsx` files with no `package.json` above them: not read.
    pub files_without_package_json: usize,
    /// `.ts` files that start with `<` or hold a NUL character (a Qt translation, a video
    /// stream): not read.
    pub files_not_typescript: usize,
    /// Files that are read and whose syntax tree has an error.
    pub files_with_syntax_errors: usize,
    /// How the imports were resolved: one count per `import` statement with a source, per
    /// `export .. from` and per `import('..')` with one string as argument.
    pub imports: ImportReport,
    /// The names (`{ A }`, a default import, `export { A } from`) of the imports that reach a
    /// file of the repository.
    pub names: NameReport,
    /// The names in `extends` and `implements` clauses.
    pub heritage: HeritageReport,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct ImportReport {
    /// The import reaches a file or a module of the repository.
    pub resolved: usize,
    /// The import leaves the repository: a package of `node_modules`, `node:fs`.
    pub external: usize,
    /// The import should reach the repository and no file that is read fits.
    pub unresolved: usize,
    /// A relative import of a file of another kind (`./a.css`, `./A.svelte`): not read.
    pub other_files: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct NameReport {
    /// The name reaches a type or a function.
    pub to_declaration: usize,
    /// The name reaches only the package of the file that the import names.
    pub to_package: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct HeritageReport {
    /// The name reaches a type of the repository: an edge.
    pub resolved: usize,
    /// A global of the language or of a library, or a name of an external import.
    pub external: usize,
    /// A name of the repository that reaches no type, or an expression.
    pub unresolved: usize,
}

/// Whether the indexer wants the file at `path` (relative to the root, `/` as separator):
/// a `.ts` or `.tsx` file, a `package.json`, a `tsconfig.json` or `tsconfig.*.json`, not
/// below a directory that `cg_core::holds_no_sources`.
pub fn is_input(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    let source = [".ts", ".tsx"]
        .iter()
        .any(|ext| name.len() > ext.len() && name.ends_with(ext));
    let config = name == "package.json"
        || name == "tsconfig.json"
        || (name.starts_with("tsconfig.") && name.ends_with(".json"));
    (source || config) && cg_core::outside_excluded_directories(path)
}

/// The test rule of ADR 34 for a path, whatever its module: for the changed files of an
/// overlay.
pub fn is_test_file(path: &str) -> bool {
    layout::is_test_file(path)
}

/// Indexes sources that are given as data. The result does not depend on the order of
/// `files`. Files that `is_input` rejects are ignored.
pub fn index_sources(root_name: &str, files: &[SourceFile]) -> (Graph, Report) {
    let files = sorted_inputs(files);
    let manifests = layout::manifests(files.iter().map(|f| (f.path.as_str(), f.text.as_str())));
    let mut report = Report::default();
    let sources = read_sources(&files, &manifests, &mut report);

    let mut graph = Graph::default();
    graph
        .nodes
        .extend(module_nodes(&sources, &manifests, root_name));
    graph.nodes.extend(package_nodes(&sources));
    let declared = declare::declare(&sources);
    graph
        .nodes
        .extend(declared.namespaces.iter().map(namespace_node));
    let decls = declared.decls;
    for kind in [NodeKind::Type, NodeKind::Function] {
        for decl in decls.iter().filter(|d| d.kind == kind) {
            graph.nodes.push(Node {
                id: decl.id.clone(),
                parent: Some(decl.parent.clone()),
                kind: decl.kind,
                name: decl.name.clone(),
                file: Some(sources[decl.source].path.to_string()),
                span: Some(decl.span),
                test: decl.test,
                hash: Some(decl.hash.clone()),
                change: None,
            });
        }
    }
    let config = config::Config::read(files.iter().map(|f| (f.path.as_str(), f.text.as_str())));
    let mut found = edges::collect(&sources, &decls, &config);
    edges::drop_related(&mut found.edges, &graph.nodes);
    report.imports = found.imports;
    report.names = found.names;
    report.heritage = found.heritage;
    graph.edges = found.edges.into_edges(&graph.nodes);
    (graph, report)
}

/// The files that `is_input` accepts, in the order of their path components, without
/// repeated paths.
fn sorted_inputs(files: &[SourceFile]) -> Vec<&SourceFile> {
    let mut files: Vec<&SourceFile> = files.iter().filter(|f| is_input(&f.path)).collect();
    // The order of the components, as `Path` compares, not of the bytes.
    files.sort_by(|a, b| {
        a.path
            .split('/')
            .cmp(b.path.split('/'))
            .then_with(|| a.text.cmp(&b.text))
    });
    files.dedup_by(|a, b| a.path == b.path);
    files
}

/// Parses the `.ts` and `.tsx` files that belong to a module; counts the others in `report`.
fn read_sources<'a>(
    files: &[&'a SourceFile],
    manifests: &'a layout::Manifests,
    report: &mut Report,
) -> Vec<Source<'a>> {
    let mut sources = Vec::new();
    for file in files.iter().filter(|f| layout::is_typescript_path(&f.path)) {
        if layout::is_not_typescript(&file.path, &file.text) {
            report.files_not_typescript += 1;
            continue;
        }
        let Some(module) = layout::module_dir(manifests, &file.path) else {
            report.files_without_package_json += 1;
            continue;
        };
        let Some(tree) = parse(file) else {
            continue;
        };
        if tree.root_node().has_error() {
            report.files_with_syntax_errors += 1;
        }
        sources.push(Source {
            path: &file.path,
            text: &file.text,
            tree,
            module,
            package: layout::package_path(&file.path, module),
            test: layout::is_test_file(&file.path),
        });
    }
    sources
}

/// The syntax tree of a file, in the grammar of its extension.
fn parse(file: &SourceFile) -> Option<tree_sitter::Tree> {
    let mut parser = Parser::new();
    let language = if file.path.ends_with(".tsx") {
        tree_sitter_typescript::LANGUAGE_TSX
    } else {
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT
    };
    parser
        .set_language(&language.into())
        .expect("the TypeScript grammar matches the tree-sitter version");
    parser.parse(&file.text, None)
}

/// A node for each module that holds a file.
fn module_nodes(sources: &[Source], manifests: &layout::Manifests, root_name: &str) -> Vec<Node> {
    let modules: BTreeSet<&str> = sources.iter().map(|s| s.module).collect();
    modules
        .into_iter()
        .map(|dir| Node {
            id: format!("mod:{dir}"),
            parent: None,
            kind: NodeKind::Module,
            name: layout::module_name(manifests, dir, root_name),
            file: None,
            span: None,
            test: false,
            hash: None,
            change: None,
        })
        .collect()
}

/// A node for each package of a file path, below its module or its parent package.
fn package_nodes(sources: &[Source]) -> Vec<Node> {
    // A package is test code if all files in it are.
    let mut holds = PackageHolds::new();
    for source in sources.iter().filter(|s| !s.package.is_empty()) {
        holds
            .entry((source.module.to_string(), source.package.clone()))
            .and_modify(|test| *test &= source.test)
            .or_insert(source.test);
    }
    let mut nodes = Vec::new();
    for (module, packages) in cg_core::marked_package_nodes_by(&holds, '/') {
        for package in packages {
            nodes.push(Node {
                id: declare::container_id(&module, &package.name),
                parent: Some(match &package.parent {
                    Some(parent) => declare::container_id(&module, parent),
                    None => format!("mod:{module}"),
                }),
                kind: NodeKind::Package,
                name: package.display,
                file: None,
                span: None,
                test: package.test,
                hash: None,
                change: None,
            });
        }
    }
    nodes
}

/// The node of a `namespace`.
fn namespace_node(namespace: &declare::Namespace) -> Node {
    Node {
        id: namespace.id.clone(),
        parent: Some(namespace.parent.clone()),
        kind: NodeKind::Package,
        name: namespace.name.clone(),
        file: None,
        span: None,
        test: namespace.test,
        hash: None,
        change: None,
    }
}
