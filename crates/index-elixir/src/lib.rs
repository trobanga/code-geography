//! Elixir front end: turns the sources of Mix projects into a graph.

mod calls;
mod declare;
mod edges;
mod mix;
mod quote;
mod scope;
mod sites;
mod syntax;

pub use cg_core::SourceFile;
use cg_core::{Graph, Node, NodeKind, PackageHolds};
use declare::{Declared, File, ImplOf, ModuleDecl, Walk};
use serde::Serialize;
use std::collections::BTreeMap;
use tree_sitter::Parser;

/// What the indexer could not read or place.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Report {
    /// Input files (not `mix.exs`) with no `mix.exs` above them, or whose nearest `mix.exs`
    /// is an umbrella root: not read.
    pub files_without_project: usize,
    /// Input files (not `mix.exs`) of a project that are not read: outside its source
    /// directories.
    pub files_outside_source_dirs: usize,
    /// The places (`path:line`) of the forms that declare a module that an earlier form
    /// declares: they give nothing. Sorted.
    pub duplicate_modules: Vec<String>,
    /// The forms in a module body that give no node, by their head (`defstruct`, `schema`,
    /// `test`, `def`, ...).
    pub forms_without_node: BTreeMap<String, usize>,
    /// By kind of reference: `alias`, `import`, `require`, `use`, `behaviour`, `defimpl`
    /// (the protocol), `for` (a written type of a `defimpl`).
    pub references: BTreeMap<String, ModuleRefs>,
    /// The unresolved module names, with how often.
    pub unresolved_names: BTreeMap<String, usize>,
    /// How the call sites resolved (ADR 0037), counted per call site before the edges are
    /// joined or dropped. `to_value` stays 0.
    pub calls: cg_core::CallReport,
    /// Of the calls to a function: those where the function is named, not called (`&f/1`).
    pub function_values: usize,
    /// Of the calls that give an edge: those of a module name that no alias expands, after a
    /// `use` of a module outside the repository. They stand on the assumption that the
    /// module makes no alias of the name (ADR 0037).
    pub behind_outside_use: usize,
    /// Call sites outside every module (the top level of a file): not in `calls`, and no
    /// edge.
    pub calls_outside_module: usize,
    /// Of the unresolved calls: why.
    pub unresolved_calls: UnresolvedCalls,
    /// The calls that reach the package of a module because it has no public function of
    /// that name and arity (`Mod.fun/n`), with how often.
    pub undeclared_functions: BTreeMap<String, usize>,
}

/// Why call sites stayed unresolved (ADR 0037); the sum is `Report::calls.unresolved`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct UnresolvedCalls {
    /// A `use` can make an alias of the first segment of the module name.
    pub use_alias: usize,
    /// Two imports, or an import that is not a literal list, can answer the name.
    pub ambiguous: usize,
    /// The module is not a written name: `mod.fun()`, `unquote(m).fun()`.
    pub dynamic: usize,
    /// A local call that nothing of the repository and no `Kernel` name answers.
    pub local_unknown: usize,
    /// The module is not declared, but a module above it is.
    pub below_declared: usize,
    /// A call in a module body, outside every function, of a name that the module of the
    /// repository does not declare: the argument of a macro, which may be a type
    /// (`field :b, B.t()`) or code that the macro does not run.
    pub macro_argument: usize,
}

/// How the references of one kind resolved, counted per reference.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct ModuleRefs {
    /// The repository declares the module.
    pub resolved: usize,
    /// The module is not in the repository (a dependency, Elixir itself, an Erlang module).
    pub external: usize,
    /// The repository declares no such module, but a module above it: `MyApp.Missing`
    /// beside a module `MyApp`.
    pub unresolved: usize,
    /// Not a written name: `import unquote(m)`.
    pub dynamic: usize,
}

/// Whether the indexer wants the file at `path` (relative to the root, `/` as separator):
/// an `.ex` file, a `mix.exs`, or an `.exs` file below a directory `test`; not below a
/// directory `deps` or `_build`, nor below one that `cg_core::holds_no_sources`.
pub fn is_input(path: &str) -> bool {
    let mut parts = path.split('/').rev();
    let name = parts.next().unwrap_or(path);
    let dirs: Vec<&str> = parts.collect();
    let source = name.len() > ".ex".len() && name.ends_with(".ex");
    let script = name.len() > ".exs".len() && name.ends_with(".exs");
    let wanted = source || name == "mix.exs" || (script && dirs.contains(&"test"));
    wanted
        && cg_core::outside_excluded_directories(path)
        && !dirs.iter().any(|d| matches!(*d, "deps" | "_build"))
}

/// The Mix projects of a source set: what is needed to tell whether a path is test code
/// without reading the sources (the changed files of an overlay).
#[derive(Debug, Clone, Default)]
pub struct Layout {
    projects: mix::Projects,
}

impl Layout {
    /// Reads the `mix.exs` files among `files`; other files are ignored.
    pub fn new(files: &[SourceFile]) -> Layout {
        let inputs = files.iter().filter(|f| is_input(&f.path));
        Layout {
            projects: mix::Projects::new(&mut elixir_parser(), inputs),
        }
    }

    /// Whether the file at `path` is test code: `Some(true)` for an `.ex` or `.exs` file
    /// below the directory `test` of its project, and for an `.ex` file of a project that
    /// is itself below the directory `test` of a `mix.exs` further up (a fixture).
    /// `Some(false)` for any other `.ex` file in a source directory of its project. `None`
    /// for any other path, a path with no project (or an umbrella root) above it included.
    pub fn test_code(&self, path: &str) -> Option<bool> {
        if !(path.ends_with(".ex") || path.ends_with(".exs")) {
            return None;
        }
        let project = self.projects.project_of(path)?;
        self.projects.place(project, path).test()
    }
}

fn elixir_parser() -> Parser {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_elixir::LANGUAGE.into())
        .expect("the Elixir grammar matches the tree-sitter version");
    parser
}

/// Indexes sources that are given as data. The result does not depend on the order of
/// `files`. Files that `is_input` rejects are ignored.
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

    let mut parser = elixir_parser();
    let mut graph = Graph::default();
    let mut report = Report::default();
    let projects = mix::Projects::new(&mut parser, files.iter().copied());
    for project in projects.all().filter(|p| !p.umbrella) {
        let name = project
            .app
            .clone()
            .or_else(|| {
                project
                    .dir
                    .rsplit('/')
                    .next()
                    .filter(|n| !n.is_empty())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| root_name.to_string());
        graph.nodes.push(Node {
            id: format!("mod:{}", project.dir),
            parent: None,
            kind: NodeKind::Module,
            name,
            file: None,
            span: None,
            test: projects.below_test(&project.dir),
            hash: None,
            change: None,
        });
    }
    let mut read: Vec<File> = Vec::new();
    for file in files.iter().filter(|f| mix::mix_dir(&f.path).is_none()) {
        let Some(project) = projects.project_of(&file.path) else {
            report.files_without_project += 1;
            continue;
        };
        let Some(test) = projects.place(project, &file.path).test() else {
            report.files_outside_source_dirs += 1;
            continue;
        };
        read.push(File {
            source: &file.text,
            path: &file.path,
            dir: &project.dir,
            test,
        });
    }
    // Main code first, so that a module of test code (a fixture) never takes the name of
    // a module of main code; in each of the two the order of the paths.
    read.sort_by_key(|file| file.test);
    let mut declared = Declared::default();
    for file in &read {
        if let Some(tree) = parser.parse(file.source, None) {
            Walk::new(file, &mut declared, &mut report).read(tree.root_node());
        }
    }
    report.duplicate_modules.sort();
    let dirs: BTreeMap<&str, &str> = declared
        .modules
        .iter()
        .map(|m| (m.name.as_str(), m.dir.as_str()))
        .collect();
    graph.nodes.extend(package_nodes(&declared.modules, &dirs));
    for function in &declared.functions {
        let parent = package_id(dirs[function.module.as_str()], &function.module);
        graph.nodes.push(Node {
            id: format!(
                "fn:{}.{}/{}",
                function.module, function.name, function.arity
            ),
            parent: Some(parent),
            kind: NodeKind::Function,
            name: format!("{}/{}", function.name, function.arity),
            file: Some(function.file.clone()),
            span: Some(function.span),
            test: function.test,
            hash: Some(function.hash.clone()),
            change: None,
        });
    }
    edges::count(&declared, &dirs, &mut report);
    let edges = edges::edges(&graph, &declared, &dirs, &mut report);
    graph.edges = edges.into_edges(&graph.nodes);
    (graph, report)
}

/// The package nodes of the modules (ADR 18), parents before children. The module of a
/// `defimpl` whose type is a module of the same project is a child of the package of the
/// type, named by the protocol; any other module nests by its name.
fn package_nodes(modules: &[ModuleDecl], dirs: &BTreeMap<&str, &str>) -> Vec<Node> {
    /// What `module` implements, if it stands in its type.
    fn placed<'m>(module: &'m ModuleDecl, dirs: &BTreeMap<&str, &str>) -> Option<&'m ImplOf> {
        let of = module.impl_of.as_ref()?;
        (dirs.get(of.ty.as_str()) == Some(&module.dir.as_str())).then_some(of)
    }
    let holds: PackageHolds = modules
        .iter()
        .filter(|m| placed(m, dirs).is_none())
        .map(|m| ((m.dir.clone(), m.name.clone()), m.test))
        .collect();
    let mut nodes = Vec::new();
    let mut package = |id: String, parent: String, name: String, test: bool| {
        nodes.push(Node {
            id,
            parent: Some(parent),
            kind: NodeKind::Package,
            name,
            file: None,
            span: None,
            test,
            hash: None,
            change: None,
        });
    };
    // The type of a placed module can be a placed module itself. Its name is shorter
    // (`P.T` holds `T`), so the order of the lengths puts each parent first.
    let mut in_types: Vec<_> = modules
        .iter()
        .filter_map(|m| Some((m, placed(m, dirs)?)))
        .collect();
    in_types.sort_by_key(|(m, _)| (m.name.len(), m.name.as_str()));
    for (dir, packages) in cg_core::marked_package_nodes(&holds) {
        for node in packages {
            // Two modules below a placed module make its name a prefix node: the placed
            // module is that node, and they stand in it.
            let is_placed = |(m, _): &(&ModuleDecl, &ImplOf)| m.dir == dir && m.name == node.name;
            if in_types.iter().any(is_placed) {
                continue;
            }
            // A module below a placed module stands in it, also when it is the only one
            // and the name of the placed module is no prefix node.
            let below = |name: &str| {
                let rest = node.name.strip_prefix(name);
                rest.is_some_and(|rest| rest.starts_with('.'))
            };
            let around = in_types
                .iter()
                .filter(|(m, _)| m.dir == dir)
                .map(|(m, _)| m.name.as_str())
                .filter(|name| below(name))
                .max_by_key(|name| name.len());
            let nearest = node.parent.as_ref().map_or(0, |parent| parent.len());
            let (parent, display) = match (around, &node.parent) {
                (Some(around), _) if around.len() > nearest => (
                    package_id(&dir, around),
                    node.name[around.len() + 1..].to_string(),
                ),
                (_, Some(parent)) => (package_id(&dir, parent), node.display),
                (_, None) => (format!("mod:{dir}"), node.display),
            };
            package(package_id(&dir, &node.name), parent, display, node.test);
        }
    }
    for (module, of) in in_types {
        package(
            package_id(&module.dir, &module.name),
            package_id(&module.dir, &of.ty),
            of.protocol.clone(),
            module.test,
        );
    }
    nodes
}

fn package_id(dir: &str, module: &str) -> String {
    format!("pkg:{dir}:{module}")
}
