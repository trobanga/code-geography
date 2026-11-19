//! Pass 2: the `imports`, `extends` and `implements` edges. The `calls` edges (pass 3,
//! `calls.rs`) are added to the same set at the end, so that the drop of related nodes
//! applies to them too.

use crate::bindings::{Bindings, Export, Import, Link, What};
use crate::config::Config;
use crate::declare::{Decl, container_id};
use crate::heritage;
use crate::layout::Source;
use crate::resolve::{Resolver, Target};
use crate::scope::{Kind, Scope, namespace_scopes};
use crate::{HeritageReport, ImportReport, NameReport};
use cg_core::{CallReport, EdgeKind, EdgeSet, Node, NodeKind};
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};

/// What pass 2 found.
pub struct Collected {
    pub edges: EdgeSet,
    pub imports: ImportReport,
    pub names: NameReport,
    pub heritage: HeritageReport,
    pub calls: CallReport,
    pub function_values: usize,
    pub jsx_elements: usize,
}

/// The files with what they declare, import and export.
pub(crate) struct Files<'a> {
    pub(crate) sources: &'a [Source<'a>],
    pub(crate) decls: &'a [Decl<'a>],
    pub(crate) resolver: Resolver<'a>,
    pub(crate) bindings: Vec<Bindings>,
    /// The types and functions at the top level of each file, by name: indices of `decls`.
    tops: Vec<HashMap<&'a str, Vec<usize>>>,
    /// The types of each file, in any namespace: indices of `decls`.
    types: Vec<Vec<usize>>,
    /// The declarations by the id of their parent: indices of `decls`.
    children: HashMap<&'a str, Vec<usize>>,
    /// The declaration of a syntax node, by file and byte range.
    pub(crate) node_at: HashMap<(usize, usize, usize), (usize, &'a str)>,
    /// The names of each file and namespace, by file and namespace path joined by `.`.
    pub(crate) scopes: HashMap<(usize, String), Scope<'a>>,
}

/// The files and names that a lookup has been through.
struct Reaching {
    visited: HashSet<(usize, String)>,
    /// Whether two stars that reach different declarations reach none.
    strict: bool,
}

impl Reaching {
    fn new(strict: bool) -> Self {
        Reaching {
            visited: HashSet::new(),
            strict,
        }
    }
}

/// The scope of each file and of each namespace in it, by file and namespace path.
fn name_scopes<'a>(sources: &'a [Source<'a>]) -> HashMap<(usize, String), Scope<'a>> {
    let mut scopes = HashMap::new();
    for (file, source) in sources.iter().enumerate() {
        let root = source.tree.root_node();
        let file_scope = Scope::of_statements(Kind::File, Vec::new(), root, source.text);
        scopes.insert((file, String::new()), file_scope);
        let mut namespaces = Vec::new();
        namespace_scopes(root, &[], source.text, &mut namespaces);
        for scope in namespaces {
            match scopes.entry((file, scope.path.join("."))) {
                Entry::Occupied(mut first) => first.get_mut().absorb(scope),
                Entry::Vacant(free) => {
                    free.insert(scope);
                }
            }
        }
    }
    scopes
}

impl<'a> Files<'a> {
    fn new(sources: &'a [Source<'a>], decls: &'a [Decl<'a>], config: &'a Config) -> Self {
        let mut tops: Vec<HashMap<&str, Vec<usize>>> = vec![HashMap::new(); sources.len()];
        let mut types: Vec<Vec<usize>> = vec![Vec::new(); sources.len()];
        let mut children: HashMap<&str, Vec<usize>> = HashMap::new();
        let mut node_at = HashMap::new();
        for (index, decl) in decls.iter().enumerate() {
            children
                .entry(decl.parent.as_str())
                .or_default()
                .push(index);
            let range = decl.node.byte_range();
            node_at.insert(
                (decl.source, range.start, range.end),
                (index, decl.node.kind()),
            );
            for node in &decl.also {
                let range = node.byte_range();
                let key = (decl.source, range.start, range.end);
                node_at.insert(key, (index, node.kind()));
            }
            if decl.kind == NodeKind::Type {
                types[decl.source].push(index);
            }
            let source = &sources[decl.source];
            if decl.namespace.is_empty()
                && decl.parent == container_id(source.module, &source.package)
            {
                tops[decl.source]
                    .entry(decl.name.as_str())
                    .or_default()
                    .push(index);
            }
        }
        Files {
            sources,
            decls,
            resolver: Resolver::new(sources, config),
            bindings: sources.iter().map(Bindings::of).collect(),
            tops,
            types,
            children,
            node_at,
            scopes: name_scopes(sources),
        }
    }

    /// The declarations whose parent is `parent` and whose name is `name`.
    pub(crate) fn named<'s>(
        &'s self,
        parent: &str,
        name: &'s str,
    ) -> impl Iterator<Item = usize> + 's {
        self.children
            .get(parent)
            .into_iter()
            .flatten()
            .copied()
            .filter(move |&i| self.decls[i].name == name)
    }

    /// The id of the node that holds the declarations of `file` in the namespace `path`.
    pub(crate) fn container(&self, file: usize, path: &[String]) -> String {
        let source = &self.sources[file];
        if path.is_empty() {
            container_id(source.module, &source.package)
        } else {
            format!(
                "pkg:{}:/{}#{}",
                source.module,
                source.package,
                path.join(".")
            )
        }
    }

    /// The types of `file` named `name` in the namespace path `namespace`.
    fn types_in(&self, file: usize, namespace: &[String], name: &str) -> Vec<usize> {
        self.types[file]
            .iter()
            .copied()
            .filter(|&i| self.decls[i].namespace == namespace && self.decls[i].name == name)
            .collect()
    }

    /// The package that holds the file.
    fn package(&self, file: usize) -> String {
        let source = &self.sources[file];
        container_id(source.module, &source.package)
    }

    /// The declarations that `name` reaches in `file`: what the file exports under that
    /// name, through the re-exports of the files between.
    pub(crate) fn lookup(&self, file: usize, name: &str) -> Vec<usize> {
        self.reach(file, name, &mut Reaching::new(false))
    }

    /// As `lookup`, but a name that two `export *` of a file reach with different
    /// declarations reaches nothing: a call must not pick one of them.
    pub(crate) fn lookup_unique(&self, file: usize, name: &str) -> Vec<usize> {
        self.reach(file, name, &mut Reaching::new(true))
    }

    fn reach(&self, file: usize, name: &str, seen: &mut Reaching) -> Vec<usize> {
        if !seen.visited.insert((file, name.to_string())) {
            return Vec::new();
        }
        match self.bindings[file].exports.get(name) {
            Some(Export::Local(local)) => match self.tops[file].get(local.as_str()) {
                Some(own) => own.clone(),
                None => self.reexported(file, local, seen),
            },
            Some(Export::From { specifier, name }) => {
                self.reach_through(file, specifier, name, seen)
            }
            None => self.through_stars(file, name, seen),
        }
    }

    /// `name` of the file that `specifier`, written in `file`, reaches.
    fn reach_through(
        &self,
        file: usize,
        specifier: &str,
        name: &str,
        seen: &mut Reaching,
    ) -> Vec<usize> {
        match self.resolver.resolve(file, specifier) {
            Target::Source(t) => self.reach(t, name, seen),
            _ => Vec::new(),
        }
    }

    /// A name that `file` imports and exports again, as `local`.
    fn reexported(&self, file: usize, local: &str, seen: &mut Reaching) -> Vec<usize> {
        let Some(import) = self.bindings[file].imports.get(local) else {
            return Vec::new();
        };
        let Target::Source(t) = self.resolver.resolve(file, &import.specifier) else {
            return Vec::new();
        };
        match &import.what {
            What::Name(n) => self.reach(t, n, seen),
            What::Default => self.reach(t, "default", seen),
            What::Namespace => Vec::new(),
        }
    }

    /// The first `export * from` of `file` that reaches `name`; a default export is not
    /// passed on by a star.
    fn through_stars(&self, file: usize, name: &str, seen: &mut Reaching) -> Vec<usize> {
        if name == "default" {
            return Vec::new();
        }
        let found = self.star_results(file, name, seen);
        match found.first() {
            Some(first) if found.iter().all(|f| f == first) => first.clone(),
            _ => Vec::new(),
        }
    }

    /// What each `export * from` of `file` reaches for `name`, the non-empty answers only;
    /// without `strict`, up to the first.
    fn star_results(&self, file: usize, name: &str, seen: &mut Reaching) -> Vec<Vec<usize>> {
        let mut found: Vec<Vec<usize>> = Vec::new();
        for specifier in &self.bindings[file].stars {
            if let Target::Source(t) = self.resolver.resolve(file, specifier) {
                let reached = self.reach(t, name, seen);
                if !reached.is_empty() {
                    found.push(reached);
                }
                if !found.is_empty() && !seen.strict {
                    break;
                }
            }
        }
        found
    }
}

impl Collected {
    /// An `imports` edge from the package of `file`, with a site in `file`.
    fn import(&mut self, files: &Files, file: usize, to: &str, line: u32) {
        self.edges.add(
            &files.package(file),
            to,
            EdgeKind::Imports,
            files.sources[file].path,
            line,
        );
    }

    /// The `import` and `export .. from` statements of `file`.
    fn links(&mut self, files: &Files, file: usize) {
        for link in &files.bindings[file].links {
            match files.resolver.resolve(file, &link.specifier) {
                Target::Source(t) => self.names_of(files, file, link, t),
                Target::Module(first) => {
                    // The names are in a file that the `package.json` does not prove.
                    self.imports.resolved += 1;
                    self.names.to_package += link.names.len();
                    let module = container_id(files.sources[first].module, "");
                    self.import(files, file, &module, link.line);
                }
                Target::External => self.imports.external += 1,
                Target::Unresolved => self.imports.unresolved += 1,
                Target::OtherFile => self.imports.other_files += 1,
            }
        }
    }

    /// The edges of the names of `link`, which reaches the file `t`.
    fn names_of(&mut self, files: &Files, file: usize, link: &Link, t: usize) {
        self.imports.resolved += 1;
        for (name, line) in &link.names {
            let reached = files.lookup(t, name);
            if reached.is_empty() {
                self.names.to_package += 1;
                self.import(files, file, &files.package(t), link.line);
            } else {
                self.names.to_declaration += 1;
            }
            for declaration in reached {
                self.import(files, file, &files.decls[declaration].id, *line);
            }
        }
        if link.whole {
            self.import(files, file, &files.package(t), link.line);
        }
    }
}

impl Collected {
    /// The `extends` and `implements` edges of the type `index` of `decls`.
    fn heritage(&mut self, files: &Files, index: usize) {
        let decl = &files.decls[index];
        let source = &files.sources[decl.source];
        for clause in heritage::clauses(decl.node, source.text) {
            let Some(name) = clause.name else {
                // An expression that is no name (`extends mixin(A)`).
                self.heritage.unresolved += 1;
                continue;
            };
            let at = Place {
                source: decl.source,
                namespace: &decl.namespace,
            };
            match files.type_target(&at, &name.path) {
                Reach::Types(reached) => {
                    self.heritage.resolved += 1;
                    for to in reached {
                        self.edges.add(
                            &decl.id,
                            &files.decls[to].id,
                            clause.kind,
                            source.path,
                            name.line,
                        );
                    }
                }
                Reach::External => self.heritage.external += 1,
                Reach::Unresolved => self.heritage.unresolved += 1,
            }
        }
    }
}

/// What a name of a type reaches.
pub(crate) enum Reach {
    /// Types of the repository: indices of `decls`.
    Types(Vec<usize>),
    /// A global, or a name of an external import.
    External,
    /// A name of the repository that reaches no type.
    Unresolved,
}

/// Where a name is written: a file and the namespaces around the name.
pub(crate) struct Place<'p> {
    pub(crate) source: usize,
    pub(crate) namespace: &'p [String],
}

impl Files<'_> {
    /// What the name `path`, written at `at`, reaches.
    pub(crate) fn type_target(&self, at: &Place, path: &[String]) -> Reach {
        match path {
            [name] => self.simple_name(at, name),
            [qualifier @ .., name] => self.qualified_name(at, qualifier, name),
            [] => Reach::Unresolved,
        }
    }

    /// `qualifier.name`: `x.B`, `a.b.C`.
    fn qualified_name(&self, at: &Place, qualifier: &[String], name: &str) -> Reach {
        let bindings = &self.bindings[at.source];
        let import = bindings.imports.get(&qualifier[0]);
        let target = import.map(|i| self.resolver.resolve(at.source, &i.specifier));
        if let (
            Some(Import {
                what: What::Namespace,
                ..
            }),
            Some(target),
        ) = (import, target)
            && let Some(reach) = self.through_namespace(qualifier, name, target)
        {
            return reach;
        }
        if let Some(found) = self.in_namespaces(at, qualifier, name) {
            return Reach::Types(found);
        }
        outside_namespaces(bindings, &qualifier[0], target)
    }

    /// The types `qualifier.name` of this file, in the namespaces around the declaration,
    /// innermost first.
    fn in_namespaces(&self, at: &Place, qualifier: &[String], name: &str) -> Option<Vec<usize>> {
        (0..=at.namespace.len()).rev().find_map(|depth| {
            let mut namespace = at.namespace[..depth].to_vec();
            namespace.extend_from_slice(qualifier);
            Some(self.types_in(at.source, &namespace, name)).filter(|found| !found.is_empty())
        })
    }

    /// `ns.name` where `ns` is a namespace import (`import * as ns`) that reaches `target`;
    /// `None` if the import does not settle the name.
    fn through_namespace(&self, qualifier: &[String], name: &str, target: Target) -> Option<Reach> {
        match target {
            Target::Source(t) if qualifier.len() == 1 => {
                Some(self.types_among(self.lookup(t, name)))
            }
            Target::Source(_) | Target::Module(_) => Some(Reach::Unresolved),
            Target::External => Some(Reach::External),
            Target::Unresolved | Target::OtherFile => None,
        }
    }

    /// `name`.
    fn simple_name(&self, at: &Place, name: &String) -> Reach {
        // The namespaces around the declaration, innermost first, then the top level.
        for depth in (0..=at.namespace.len()).rev() {
            let found = self.types_in(at.source, &at.namespace[..depth], name);
            if !found.is_empty() {
                return Reach::Types(found);
            }
        }
        let bindings = &self.bindings[at.source];
        let Some(import) = bindings.imports.get(name) else {
            // Declared here, but no type: a function, a `const`. Else a global.
            return if bindings.declared.contains(name) {
                Reach::Unresolved
            } else {
                Reach::External
            };
        };
        self.imported_name(at, import)
    }

    /// What the name that `import` binds reaches.
    fn imported_name(&self, at: &Place, import: &Import) -> Reach {
        match self.resolver.resolve(at.source, &import.specifier) {
            Target::Source(t) => {
                let reached = match &import.what {
                    What::Name(n) => self.lookup(t, n),
                    What::Default => self.lookup(t, "default"),
                    What::Namespace => Vec::new(),
                };
                self.types_among(reached)
            }
            Target::External => Reach::External,
            Target::Module(_) | Target::Unresolved | Target::OtherFile => Reach::Unresolved,
        }
    }

    /// The types among declarations: those, if there are some.
    fn types_among(&self, reached: Vec<usize>) -> Reach {
        let types: Vec<usize> = reached
            .into_iter()
            .filter(|&i| self.decls[i].kind == NodeKind::Type)
            .collect();
        if types.is_empty() {
            Reach::Unresolved
        } else {
            Reach::Types(types)
        }
    }
}

/// What a qualifier that is no namespace of the file reaches: `target` is what the import
/// of the qualifier reaches, if there is one.
fn outside_namespaces(bindings: &Bindings, qualifier: &str, target: Option<Target>) -> Reach {
    match target {
        Some(Target::External) => Reach::External,
        Some(_) => Reach::Unresolved,
        None if bindings.declared.contains(qualifier) => Reach::Unresolved,
        None => Reach::External,
    }
}

/// Drops the edges between a node and itself, its ancestor or its descendant.
pub fn drop_related(edges: &mut EdgeSet, nodes: &[Node]) {
    let parents: HashMap<&str, &str> = nodes
        .iter()
        .filter_map(|n| Some((n.id.as_str(), n.parent.as_deref()?)))
        .collect();
    let within = |inner: &str, outer: &str| {
        let mut at = Some(inner);
        while let Some(id) = at {
            if id == outer {
                return true;
            }
            at = parents.get(id).copied();
        }
        false
    };
    edges.retain(|from, to| !within(from, to) && !within(to, from));
}

pub fn collect<'a>(
    sources: &'a [Source<'a>],
    decls: &'a [Decl<'a>],
    config: &'a Config,
) -> Collected {
    let files = Files::new(sources, decls, config);
    let mut found = Collected {
        edges: EdgeSet::new(),
        imports: ImportReport::default(),
        names: NameReport::default(),
        heritage: HeritageReport::default(),
        calls: CallReport::default(),
        function_values: 0,
        jsx_elements: 0,
    };
    for file in 0..sources.len() {
        found.links(&files, file);
    }
    for (index, decl) in decls.iter().enumerate() {
        if decl.kind == NodeKind::Type {
            found.heritage(&files, index);
        }
    }
    crate::calls::collect(&files, &mut found);
    found
}
