//! Pass 2: the names of every mod and the resolution of paths against them. Only what
//! the declarations prove: a path that leaves the repository or cannot be followed gives
//! no item.

use crate::declare::Decl;
use crate::modules::ModInfo;
use crate::syntax::{named_children, path_of, text};
use cg_core::NodeKind;
use std::collections::{HashMap, HashSet};
use tree_sitter::Node as Ts;

/// What a path can name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entity {
    /// Index into the mods.
    Mod(usize),
    /// Index into the declarations: a struct, enum, union, trait or alias.
    Type(usize),
    /// Index into the declarations: a free function.
    Function(usize),
    /// A constant, a static, an enum variant: it exists, but is no node.
    Other,
}

/// The outcome of the resolution of one path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Res {
    /// The path reaches `Entity` after the given number of segments; the rest of the
    /// segments (associated items, variants) is not followed.
    Item(Entity, usize),
    /// The path leaves the repository (std, a dependency, the prelude).
    External,
    /// A generic parameter or a local item hides the name.
    Hidden,
    /// The path should name something in the repository, but cannot be followed.
    Unresolved,
}

/// The names that a path is resolved against.
pub struct Ctx<'c> {
    pub module: usize,
    pub generics: &'c [String],
    /// What `Self` means. `None` outside an impl or trait, or for an external type.
    pub self_type: Option<Entity>,
}

#[derive(Default)]
struct Scope {
    /// Mods and types.
    types: HashMap<String, Entity>,
    /// Functions.
    values: HashMap<String, Entity>,
    /// The names that `use` declarations bring in, with the path they were written with.
    uses: Vec<(String, Vec<String>)>,
    /// The paths of `use path::*`.
    globs: Vec<Vec<String>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Ns {
    Type,
    Value,
    Any,
}

pub struct World<'a, 't> {
    mods: &'a [ModInfo<'t>],
    decls: &'a [Decl<'t>],
    scopes: Vec<Scope>,
    /// The root mod of the lib of each package, by crate name.
    lib_roots: HashMap<&'a str, usize>,
    /// The names that the manifest of each package lets code use as crates.
    dependencies: &'a [&'a HashSet<String>],
}

const DEPTH: usize = 16;

impl<'a, 't> World<'a, 't> {
    /// `crate_names[p]` is the crate name of package `p`.
    pub fn new(
        crate_names: &'a [&'a str],
        dependencies: &'a [&'a HashSet<String>],
        mods: &'a [ModInfo<'t>],
        decls: &'a [Decl<'t>],
    ) -> Self {
        let mut scopes: Vec<Scope> = mods.iter().map(|_| Scope::default()).collect();
        for (index, m) in mods.iter().enumerate() {
            for (parent, name) in &m.declared_in {
                scopes[*parent]
                    .types
                    .insert(name.clone(), Entity::Mod(index));
            }
        }
        for (index, decl) in decls.iter().enumerate() {
            match (decl.kind, decl.is_member) {
                (NodeKind::Type, _) => {
                    scopes[decl.module]
                        .types
                        .insert(decl.name.clone(), Entity::Type(index));
                }
                (NodeKind::Function, false) => {
                    scopes[decl.module]
                        .values
                        .insert(decl.name.clone(), Entity::Function(index));
                }
                _ => {}
            }
        }
        for (index, m) in mods.iter().enumerate() {
            for item in &m.items {
                match item.kind() {
                    "use_declaration" => {
                        if let Some(argument) = item.child_by_field_name("argument") {
                            collect_uses(argument, m.source, &[], &mut scopes[index]);
                        }
                    }
                    // Items that exist but are no nodes.
                    "const_item" | "static_item" | "macro_definition" => {
                        if let Some(name) = item.child_by_field_name("name") {
                            scopes[index]
                                .values
                                .insert(text(name, m.source).to_string(), Entity::Other);
                        }
                    }
                    _ => {}
                }
            }
        }
        let lib_roots = mods
            .iter()
            .enumerate()
            .filter(|(_, m)| m.parent.is_none() && m.path.is_empty())
            .map(|(index, m)| (crate_names[m.package], index))
            .collect();
        World {
            mods,
            decls,
            scopes,
            dependencies,
            lib_roots,
        }
    }

    /// Resolves the path of a `use` declaration in `module`.
    pub fn resolve_use(&self, module: usize, path: &[String]) -> Res {
        let segments: Vec<&str> = path.iter().map(String::as_str).collect();
        if segments.is_empty() {
            return Res::Unresolved;
        }
        self.resolve_from(module, &segments, Ns::Any, DEPTH)
    }

    pub fn decl(&self, index: usize) -> &'a Decl<'t> {
        &self.decls[index]
    }

    /// Resolves a path as written in the source of `ctx.module`.
    pub fn resolve(&self, ctx: &Ctx, segments: &[&str]) -> Res {
        let Some(head) = segments.first() else {
            return Res::Unresolved;
        };
        if ctx.generics.iter().any(|g| g == head) {
            return Res::Hidden;
        }
        if *head == "Self" {
            return match ctx.self_type {
                Some(entity) => self.follow(entity, 1, segments, DEPTH),
                None => Res::External,
            };
        }
        self.resolve_from(ctx.module, segments, Ns::Any, DEPTH)
    }

    fn resolve_from(&self, module: usize, segments: &[&str], ns: Ns, depth: usize) -> Res {
        let head = segments[0];
        let last = segments.len() == 1;
        let head_ns = if last { ns } else { Ns::Type };
        let start = match head {
            "crate" => Some((Entity::Mod(self.root_of(module)), 1)),
            "self" => Some((Entity::Mod(module), 1)),
            "super" => Some((Entity::Mod(self.mods[module].outer().unwrap_or(module)), 1)),
            _ => None,
        };
        if let Some((entity, consumed)) = start {
            return self.follow_as(entity, consumed, segments, ns, depth);
        }
        match self.lookup(module, head, head_ns, depth) {
            Some(Res::Item(entity, _)) => self.follow_as(entity, 1, segments, ns, depth),
            Some(other) => other,
            None => match self.lib_roots.get(head.replace('-', "_").as_str()).copied() {
                Some(root) => self.follow_as(Entity::Mod(root), 1, segments, ns, depth),
                None => {
                    let package = self.mods[module].package;
                    if is_external_head(head) || self.dependencies[package].contains(head) {
                        Res::External
                    } else {
                        Res::Unresolved
                    }
                }
            },
        }
    }

    fn follow(&self, entity: Entity, consumed: usize, segments: &[&str], depth: usize) -> Res {
        self.follow_as(entity, consumed, segments, Ns::Any, depth)
    }

    /// Follows the segments from `consumed` on through mods.
    fn follow_as(
        &self,
        mut entity: Entity,
        mut consumed: usize,
        segments: &[&str],
        ns: Ns,
        depth: usize,
    ) -> Res {
        while consumed < segments.len() {
            let Entity::Mod(module) = entity else {
                break;
            };
            let segment = segments[consumed];
            entity = match segment {
                "super" => Entity::Mod(self.mods[module].outer().unwrap_or(module)),
                "self" => entity,
                _ => {
                    let want = if consumed + 1 == segments.len() {
                        ns
                    } else {
                        Ns::Type
                    };
                    match self.lookup(module, segment, want, depth) {
                        Some(Res::Item(found, _)) => found,
                        Some(other) => return other,
                        None => return Res::Unresolved,
                    }
                }
            };
            consumed += 1;
        }
        Res::Item(entity, consumed)
    }

    fn root_of(&self, mut module: usize) -> usize {
        while let Some(parent) = self.mods[module].outer() {
            module = parent;
        }
        module
    }

    /// What `name` means in `module`: its own items, then its `use` declarations, then the
    /// globs. `None` if the mod does not bind the name.
    fn lookup(&self, module: usize, name: &str, ns: Ns, depth: usize) -> Option<Res> {
        if depth == 0 {
            return None;
        }
        let scope = &self.scopes[module];
        if ns != Ns::Value
            && let Some(entity) = scope.types.get(name)
        {
            return Some(Res::Item(*entity, 1));
        }
        if ns != Ns::Type
            && let Some(entity) = scope.values.get(name)
        {
            return Some(Res::Item(*entity, 1));
        }
        let mut unfollowed = None;
        for (_, path) in scope.uses.iter().filter(|(bound, _)| bound == name) {
            let segments: Vec<&str> = path.iter().map(String::as_str).collect();
            match self.resolve_from(module, &segments, ns, depth - 1) {
                Res::Item(entity, consumed) if consumed == segments.len() => {
                    return Some(Res::Item(entity, 1));
                }
                // The path ends inside a type: an enum variant, an associated item.
                Res::Item(_, _) => return Some(Res::Item(Entity::Other, 1)),
                // Another `use` of the same name can still name it (other namespace).
                other => {
                    unfollowed.get_or_insert(other);
                }
            }
        }
        if unfollowed.is_some() {
            return unfollowed;
        }
        for path in &scope.globs {
            let segments: Vec<&str> = path.iter().map(String::as_str).collect();
            match self.resolve_from(module, &segments, Ns::Type, depth - 1) {
                Res::Item(Entity::Mod(target), consumed) if consumed == segments.len() => {
                    if let Some(found) = self.lookup(target, name, ns, depth - 1) {
                        return Some(found);
                    }
                }
                Res::External => return Some(Res::External),
                _ => {}
            }
        }
        None
    }
}

/// `std`, the prelude and the primitive types: names that no declaration of the
/// repository introduces, but that are known to be outside it.
fn is_external_head(name: &str) -> bool {
    const KNOWN: &[&str] = &[
        "std",
        "core",
        "alloc",
        "Option",
        "Some",
        "None",
        "Result",
        "Ok",
        "Err",
        "Vec",
        "String",
        "Box",
        "ToString",
        "ToOwned",
        "Clone",
        "Copy",
        "Send",
        "Sync",
        "Sized",
        "Unpin",
        "Drop",
        "Fn",
        "FnMut",
        "FnOnce",
        "Default",
        "Eq",
        "PartialEq",
        "Ord",
        "PartialOrd",
        "Iterator",
        "IntoIterator",
        "Extend",
        "FromIterator",
        "From",
        "Into",
        "TryFrom",
        "TryInto",
        "AsRef",
        "AsMut",
        "DoubleEndedIterator",
        "ExactSizeIterator",
        "drop",
        "bool",
        "char",
        "str",
        "u8",
        "u16",
        "u32",
        "u64",
        "u128",
        "usize",
        "i8",
        "i16",
        "i32",
        "i64",
        "i128",
        "isize",
        "f32",
        "f64",
    ];
    KNOWN.contains(&name)
}

/// The paths that a `use` declaration names, globs included.
pub fn use_paths(item: Ts, source: &str) -> Vec<Vec<String>> {
    let mut scope = Scope::default();
    if let Some(argument) = item.child_by_field_name("argument") {
        collect_uses(argument, source, &[], &mut scope);
    }
    scope
        .uses
        .into_iter()
        .map(|(_, path)| path)
        .chain(scope.globs)
        .collect()
}

/// The names that a `use` declaration binds (an alias, or the last segment).
pub fn use_names(item: Ts, source: &str) -> Vec<String> {
    let mut scope = Scope::default();
    if let Some(argument) = item.child_by_field_name("argument") {
        collect_uses(argument, source, &[], &mut scope);
    }
    scope.uses.into_iter().map(|(name, _)| name).collect()
}

/// Collects the names that a `use` argument brings in.
fn collect_uses(argument: Ts, source: &str, prefix: &[String], scope: &mut Scope) {
    let path_with = |node: Ts| -> Option<Vec<String>> {
        let mut path = prefix.to_vec();
        path.extend(path_of(node, source)?.into_iter().map(str::to_string));
        Some(path)
    };
    match argument.kind() {
        "identifier" | "scoped_identifier" | "crate" | "super" => {
            if let Some(path) = path_with(argument) {
                bind(scope, path, None);
            }
        }
        "self" => bind(scope, prefix.to_vec(), None),
        "use_as_clause" => {
            let alias = argument
                .child_by_field_name("alias")
                .map(|a| text(a, source).to_string());
            if let Some(path) = argument.child_by_field_name("path").and_then(path_with) {
                bind(scope, path, Some(alias.unwrap_or_default()));
            }
        }
        "use_wildcard" => {
            let path = match named_children(argument).first() {
                Some(node) => path_with(*node),
                None => Some(prefix.to_vec()),
            };
            scope.globs.extend(path);
        }
        "scoped_use_list" => {
            let Some(path) = argument.child_by_field_name("path").and_then(path_with) else {
                return;
            };
            if let Some(list) = argument.child_by_field_name("list") {
                collect_uses(list, source, &path, scope);
            }
        }
        "use_list" => {
            for child in named_children(argument) {
                collect_uses(child, source, prefix, scope);
            }
        }
        _ => {}
    }
}

fn bind(scope: &mut Scope, path: Vec<String>, alias: Option<String>) {
    let name = match alias {
        Some(alias) => alias,
        None => match path.last().map(String::as_str) {
            Some("self") | None => return,
            Some(last) => last.to_string(),
        },
    };
    if name != "_" && !name.is_empty() {
        scope.uses.push((name, path));
    }
}
