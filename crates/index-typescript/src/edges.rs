//! Pass 2: the `imports`, `extends` and `implements` edges.

use crate::bindings::{Bindings, Export, Import, Link, What};
use crate::config::Config;
use crate::declare::{Decl, container_id};
use crate::heritage;
use crate::layout::Source;
use crate::resolve::{Resolver, Target};
use crate::{HeritageReport, ImportReport, NameReport};
use cg_core::{EdgeKind, EdgeSet, Node, NodeKind};
use std::collections::{HashMap, HashSet};

/// What pass 2 found.
pub struct Collected {
    pub edges: EdgeSet,
    pub imports: ImportReport,
    pub names: NameReport,
    pub heritage: HeritageReport,
}

/// The files with what they declare, import and export.
struct Files<'a, 'd, 't> {
    sources: &'a [Source<'a>],
    decls: &'d [Decl<'t>],
    resolver: Resolver<'a>,
    bindings: Vec<Bindings>,
    /// The types and functions at the top level of each file, by name: indices of `decls`.
    tops: Vec<HashMap<&'d str, Vec<usize>>>,
    /// The types of each file, in any namespace: indices of `decls`.
    types: Vec<Vec<usize>>,
}

impl<'a, 'd, 't> Files<'a, 'd, 't> {
    fn new(sources: &'a [Source<'a>], decls: &'d [Decl<'t>], config: &'a Config) -> Self {
        let mut tops: Vec<HashMap<&str, Vec<usize>>> = vec![HashMap::new(); sources.len()];
        let mut types: Vec<Vec<usize>> = vec![Vec::new(); sources.len()];
        for (index, decl) in decls.iter().enumerate() {
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
    fn lookup(&self, file: usize, name: &str) -> Vec<usize> {
        self.reach(file, name, &mut HashSet::new())
    }

    fn reach(&self, file: usize, name: &str, seen: &mut HashSet<(usize, String)>) -> Vec<usize> {
        if !seen.insert((file, name.to_string())) {
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
        seen: &mut HashSet<(usize, String)>,
    ) -> Vec<usize> {
        match self.resolver.resolve(file, specifier) {
            Target::Source(t) => self.reach(t, name, seen),
            _ => Vec::new(),
        }
    }

    /// A name that `file` imports and exports again, as `local`.
    fn reexported(
        &self,
        file: usize,
        local: &str,
        seen: &mut HashSet<(usize, String)>,
    ) -> Vec<usize> {
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
    fn through_stars(
        &self,
        file: usize,
        name: &str,
        seen: &mut HashSet<(usize, String)>,
    ) -> Vec<usize> {
        if name == "default" {
            return Vec::new();
        }
        for specifier in &self.bindings[file].stars {
            if let Target::Source(t) = self.resolver.resolve(file, specifier) {
                let reached = self.reach(t, name, seen);
                if !reached.is_empty() {
                    return reached;
                }
            }
        }
        Vec::new()
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
            match files.heritage_target(decl, &name.path) {
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

/// What a name in a clause reaches.
enum Reach {
    /// Types of the repository: indices of `decls`.
    Types(Vec<usize>),
    /// A global, or a name of an external import.
    External,
    /// A name of the repository that reaches no type.
    Unresolved,
}

impl Files<'_, '_, '_> {
    /// What the name `path`, written in the clause of `decl`, reaches.
    fn heritage_target(&self, decl: &Decl, path: &[String]) -> Reach {
        match path {
            [name] => self.simple_name(decl, name),
            [qualifier @ .., name] => self.qualified_name(decl, qualifier, name),
            [] => Reach::Unresolved,
        }
    }

    /// `qualifier.name`: `x.B`, `a.b.C`.
    fn qualified_name(&self, decl: &Decl, qualifier: &[String], name: &str) -> Reach {
        let bindings = &self.bindings[decl.source];
        let import = bindings.imports.get(&qualifier[0]);
        let target = import.map(|i| self.resolver.resolve(decl.source, &i.specifier));
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
        if let Some(found) = self.in_namespaces(decl, qualifier, name) {
            return Reach::Types(found);
        }
        outside_namespaces(bindings, &qualifier[0], target)
    }

    /// The types `qualifier.name` of this file, in the namespaces around the declaration,
    /// innermost first.
    fn in_namespaces(&self, decl: &Decl, qualifier: &[String], name: &str) -> Option<Vec<usize>> {
        (0..=decl.namespace.len()).rev().find_map(|depth| {
            let mut namespace = decl.namespace[..depth].to_vec();
            namespace.extend_from_slice(qualifier);
            Some(self.types_in(decl.source, &namespace, name)).filter(|found| !found.is_empty())
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
    fn simple_name(&self, decl: &Decl, name: &String) -> Reach {
        // The namespaces around the declaration, innermost first, then the top level.
        for depth in (0..=decl.namespace.len()).rev() {
            let found = self.types_in(decl.source, &decl.namespace[..depth], name);
            if !found.is_empty() {
                return Reach::Types(found);
            }
        }
        let bindings = &self.bindings[decl.source];
        let Some(import) = bindings.imports.get(name) else {
            // Declared here, but no type: a function, a `const`. Else a global.
            return if bindings.declared.contains(name) {
                Reach::Unresolved
            } else {
                Reach::External
            };
        };
        self.imported_name(decl, import)
    }

    /// What the name that `import` binds reaches.
    fn imported_name(&self, decl: &Decl, import: &Import) -> Reach {
        match self.resolver.resolve(decl.source, &import.specifier) {
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

pub fn collect<'t>(sources: &[Source], decls: &[Decl<'t>], config: &Config) -> Collected {
    let files = Files::new(sources, decls, config);
    let mut found = Collected {
        edges: EdgeSet::new(),
        imports: ImportReport::default(),
        names: NameReport::default(),
        heritage: HeritageReport::default(),
    };
    for file in 0..sources.len() {
        found.links(&files, file);
    }
    for (index, decl) in decls.iter().enumerate() {
        if decl.kind == NodeKind::Type {
            found.heritage(&files, index);
        }
    }
    found
}
