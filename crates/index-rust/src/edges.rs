//! Pass 3: the edges between the declared nodes.

use crate::PathReport;
use crate::declare::{Decl, TYPE_ITEMS, container_id, generic_names};
use crate::modules::ModInfo;
use crate::syntax::{line_of, named_children, path_of, text};
use crate::world::{Ctx, Entity, Res, World, use_names, use_paths};
use cg_core::{EdgeKind, EdgeSet};
use std::collections::{HashMap, HashSet};
use tree_sitter::Node as Ts;

/// Items whose `name` field declares something instead of naming something.
const NAMING_ITEMS: [&str; 13] = [
    "struct_item",
    "enum_item",
    "union_item",
    "trait_item",
    "type_item",
    "associated_type",
    "const_item",
    "static_item",
    "type_parameter",
    "const_parameter",
    "mod_item",
    "enum_variant",
    "type_binding",
];

/// Items that hold no references of their own.
const SILENT: [&str; 9] = [
    "mod_item",
    "use_declaration",
    "macro_definition",
    "macro_invocation",
    "attribute_item",
    "inner_attribute_item",
    "extern_crate_declaration",
    "line_comment",
    "block_comment",
];

/// The edges that follow from the sources.
pub fn compute<'t>(
    world: &World<'_, 't>,
    packages: &[(&str, &str)],
    mods: &[ModInfo<'t>],
    decls: &[Decl<'t>],
    report: &mut PathReport,
) -> EdgeSet {
    let decl_at: HashMap<(usize, usize), usize> = decls
        .iter()
        .enumerate()
        .map(|(index, d)| ((d.module, d.item.id()), index))
        .collect();
    let mut edges = EdgeSet::new();
    for (index, m) in mods.iter().enumerate() {
        let (dir, crate_name) = packages[m.package];
        let container = container_id(dir, crate_name, m);
        let mut emit = Emitter {
            world,
            decls,
            module: index,
            source: m.source,
            file: m.file,
            edges: &mut edges,
            report: &mut *report,
        };
        for item in &m.items {
            let own = decl_at.get(&(index, item.id())).copied();
            match item.kind() {
                "use_declaration" => {
                    for (path, line) in use_paths(*item, m.source) {
                        let resolution = world.resolve_use(index, &path);
                        emit.report.count(resolution);
                        let Res::Item(entity, _) = resolution else {
                            continue;
                        };
                        let to = match entity {
                            Entity::Mod(target) => {
                                let (dir, crate_name) = packages[mods[target].package];
                                container_id(dir, crate_name, &mods[target])
                            }
                            Entity::Type(target) | Entity::Function(target) => {
                                decls[target].id.clone()
                            }
                            Entity::Other => continue,
                        };
                        emit.add(&container, &to, EdgeKind::Imports, line);
                    }
                }
                "trait_item" => {
                    let Some(own) = own else { continue };
                    let generics = generic_names(*item, m.source);
                    let members = trait_members(*item);
                    let mut site = Site::new(&decls[own].id, &generics, Some(Entity::Type(own)));
                    site.skip = members.iter().map(|n| n.id()).collect();
                    if let Some(bounds) = item.child_by_field_name("bounds") {
                        for bound in named_children(bounds) {
                            if let Some(target) = emit.type_head(&generics, None, bound) {
                                let line = line_of(head_path(bound));
                                emit.add(
                                    &decls[own].id,
                                    &decls[target].id,
                                    EdgeKind::Extends,
                                    line,
                                );
                                site.no_edge.insert(head_path(bound).id());
                            }
                        }
                    }
                    emit.references(site, *item);
                    for member in members {
                        let Some(member_decl) = decl_at.get(&(index, member.id())) else {
                            continue;
                        };
                        let mut all = generics.clone();
                        all.extend(generic_names(member, m.source));
                        let site =
                            Site::new(&decls[*member_decl].id, &all, Some(Entity::Type(own)));
                        emit.references(site, member);
                    }
                }
                kind if TYPE_ITEMS.contains(&kind) => {
                    let Some(own) = own else { continue };
                    let generics = generic_names(*item, m.source);
                    let site = Site::new(&decls[own].id, &generics, Some(Entity::Type(own)));
                    emit.references(site, *item);
                }
                "function_item" => {
                    let Some(own) = own else { continue };
                    let generics = generic_names(*item, m.source);
                    emit.references(Site::new(&decls[own].id, &generics, None), *item);
                }
                "impl_item" => {
                    let generics = generic_names(*item, m.source);
                    let self_node = item.child_by_field_name("type");
                    let trait_node = item.child_by_field_name("trait");
                    let self_decl = self_node.and_then(|n| emit.type_head(&generics, None, n));
                    let trait_decl = trait_node.and_then(|n| emit.type_head(&generics, None, n));
                    if let (Some(own), Some(tr), Some(trait_node)) =
                        (self_decl, trait_decl, trait_node)
                    {
                        let line = line_of(head_path(trait_node));
                        emit.add(&decls[own].id, &decls[tr].id, EdgeKind::Implements, line);
                    }
                    let from = match (self_decl, trait_decl) {
                        (Some(d), _) | (None, Some(d)) => decls[d].id.clone(),
                        (None, None) => container.clone(),
                    };
                    let self_type = self_decl.map(Entity::Type);
                    let members: Vec<Ts> = item
                        .child_by_field_name("body")
                        .map(named_children)
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|c| c.kind() == "function_item")
                        .collect();
                    let mut site = Site::new(&from, &generics, self_type);
                    site.skip = members.iter().map(|n| n.id()).collect();
                    if trait_decl.is_some()
                        && let Some(tr) = trait_node
                    {
                        site.no_edge.insert(head_path(tr).id());
                    }
                    emit.references(site, *item);
                    for member in members {
                        let Some(member_decl) = decl_at.get(&(index, member.id())) else {
                            continue;
                        };
                        let mut all = generics.clone();
                        all.extend(generic_names(member, m.source));
                        let site = Site::new(&decls[*member_decl].id, &all, self_type);
                        emit.references(site, member);
                    }
                }
                kind if SILENT.contains(&kind) => {}
                _ => emit.references(Site::new(&container, &[], None), *item),
            }
        }
    }
    edges
}

/// The functions that a trait declares, with and without a body.
fn trait_members(item: Ts) -> Vec<Ts> {
    item.child_by_field_name("body")
        .map(named_children)
        .unwrap_or_default()
        .into_iter()
        .filter(|c| matches!(c.kind(), "function_item" | "function_signature_item"))
        .collect()
}

/// The node of the name in a type as written: `Base` in `Base<T>`.
fn head_path(node: Ts) -> Ts {
    match node.kind() {
        "generic_type" => node.child_by_field_name("type").unwrap_or(node),
        _ => node,
    }
}

/// Where references are read: the declaration they belong to and the names in scope.
struct Site<'a> {
    from: &'a str,
    generics: &'a [String],
    self_type: Option<Entity>,
    /// Nodes that are not read at all: members that are declarations of their own.
    skip: HashSet<usize>,
    /// Name nodes that are counted but give no `type_ref`: the edge has another kind.
    no_edge: HashSet<usize>,
}

impl<'a> Site<'a> {
    fn new(from: &'a str, generics: &'a [String], self_type: Option<Entity>) -> Self {
        Site {
            from,
            generics,
            self_type,
            skip: HashSet::new(),
            no_edge: HashSet::new(),
        }
    }
}

struct Emitter<'e, 'w, 't> {
    world: &'e World<'w, 't>,
    decls: &'e [Decl<'t>],
    module: usize,
    source: &'t str,
    /// The file that `source` is the text of.
    file: &'t str,
    edges: &'e mut EdgeSet,
    report: &'e mut PathReport,
}

impl<'t> Emitter<'_, '_, 't> {
    /// An edge whose reference stands on `line` of the file that is being read.
    fn add(&mut self, from: &str, to: &str, kind: EdgeKind, line: u32) {
        self.edges.add(from, to, kind, self.file, line);
    }

    /// The type declaration that a type as written names, if it is in the repository.
    fn type_head(&self, generics: &[String], self_type: Option<Entity>, node: Ts) -> Option<usize> {
        let ctx = Ctx {
            module: self.module,
            generics,
            self_type,
        };
        type_head(self.world, &ctx, node, self.source)
    }

    /// Adds a `type_ref` from the declaration of `site` to each repository type that
    /// `node` names.
    fn references(&mut self, site: Site, node: Ts<'t>) {
        let mut hidden = site.generics.to_vec();
        locals(node, self.source, &mut hidden);
        let mut walker = Walker {
            ctx: Ctx {
                module: self.module,
                generics: &hidden,
                self_type: site.self_type,
            },
            world: self.world,
            source: self.source,
            skip: site.skip,
            no_edge: site.no_edge,
            found: Vec::new(),
            report: PathReport::default(),
        };
        walker.walk(node);
        let (found, counted) = (walker.found, walker.report);
        self.report.add(counted);
        for (target, line) in found {
            let to = self.decls[target].id.clone();
            self.add(site.from, &to, EdgeKind::TypeRef, line);
        }
    }
}

struct Walker<'c, 'w, 't> {
    ctx: Ctx<'c>,
    world: &'c World<'w, 't>,
    source: &'t str,
    skip: HashSet<usize>,
    /// Name nodes that are counted but give no `type_ref`: the edge has another kind.
    no_edge: HashSet<usize>,
    /// The types that were named, with the line of the name.
    found: Vec<(usize, u32)>,
    report: PathReport,
}

impl<'t> Walker<'_, '_, 't> {
    fn walk(&mut self, node: Ts<'t>) {
        if self.skip.contains(&node.id()) {
            return;
        }
        match node.kind() {
            "macro_invocation" => self.macro_arguments(node),
            kind if SILENT.contains(&kind) => {}
            "visibility_modifier" => {}
            "type_identifier" => self.reference(&[text(node, self.source)], node),
            "scoped_type_identifier" | "scoped_identifier" => match path_of(node, self.source) {
                Some(path) => {
                    self.reference(&path, node);
                    self.walk_arguments(node);
                }
                None => self.descend(node),
            },
            "generic_type" | "generic_type_with_turbofish" => self.descend(node),
            "call_expression" | "tuple_struct_pattern" => {
                let field = if node.kind() == "call_expression" {
                    "function"
                } else {
                    "type"
                };
                for child in named_children(node) {
                    if child.kind() == "identifier"
                        && node.child_by_field_name(field) == Some(child)
                    {
                        self.constructor(text(child, self.source), child);
                    } else {
                        self.walk(child);
                    }
                }
            }
            _ => self.descend(node),
        }
    }

    /// The arguments of a macro call are tokens, not a syntax tree. A run of names joined
    /// by `::` in them is read as a path, whatever the macro makes of it. Such paths
    /// are not counted in the report.
    fn macro_arguments(&mut self, call: Ts<'t>) {
        let mut tokens = Vec::new();
        for child in named_children(call) {
            if child.kind() == "token_tree" {
                flatten(child, self.source, &mut tokens);
            }
        }
        let mut at = 0;
        while at < tokens.len() {
            let is_word = |token: &str| {
                token
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_alphabetic() || c == '_')
            };
            let preceded = at > 0 && matches!(tokens[at - 1].0, "::" | "." | "$");
            if preceded || !is_word(tokens[at].0) {
                at += 1;
                continue;
            }
            let mut path = vec![tokens[at].0];
            let mut end = at + 1;
            while end + 1 < tokens.len() && tokens[end].0 == "::" && is_word(tokens[end + 1].0) {
                path.push(tokens[end + 1].0);
                end += 2;
            }
            let capital = path[0].chars().next().is_some_and(char::is_uppercase);
            if path.len() > 1 || capital {
                self.silent_reference(&path, tokens[at].1);
            }
            at = end;
        }
    }

    fn silent_reference(&mut self, path: &[&str], line: u32) {
        if let Res::Item(Entity::Type(target), _) = self.world.resolve(&self.ctx, path) {
            self.found.push((target, line));
        }
    }

    fn descend(&mut self, node: Ts<'t>) {
        let name = if NAMING_ITEMS.contains(&node.kind()) {
            node.child_by_field_name("name")
        } else {
            None
        };
        for child in named_children(node) {
            if Some(child) != name {
                self.walk(child);
            }
        }
    }

    /// The generic arguments inside a path: `Foo::<Bar>::new`.
    fn walk_arguments(&mut self, node: Ts<'t>) {
        for child in named_children(node) {
            match child.kind() {
                "type_arguments" => self.walk(child),
                "scoped_identifier"
                | "scoped_type_identifier"
                | "generic_type"
                | "generic_type_with_turbofish" => self.walk_arguments(child),
                _ => {}
            }
        }
    }

    /// A name that starts with a capital letter in the place of a function: the
    /// constructor of a tuple struct.
    fn constructor(&mut self, name: &str, node: Ts) {
        if name.chars().next().is_some_and(char::is_uppercase) {
            self.reference(&[name], node);
        }
    }

    fn reference(&mut self, path: &[&str], node: Ts) {
        if path == ["_"] {
            return;
        }
        let resolution = self.world.resolve(&self.ctx, path);
        self.report.count(resolution);
        if let Res::Item(Entity::Type(target), _) = resolution
            && !self.no_edge.contains(&node.id())
        {
            self.found.push((target, line_of(node)));
        }
    }
}

/// The leaf tokens of a token tree with their lines, in the order of the source.
fn flatten<'a>(node: Ts, source: &'a str, tokens: &mut Vec<(&'a str, u32)>) {
    if node.child_count() == 0 {
        tokens.push((text(node, source), line_of(node)));
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        flatten(child, source, tokens);
    }
}

/// The names that the items and `use` declarations inside `node` introduce. They hide
/// names of the same spelling for the rest of the declaration.
fn locals(node: Ts, source: &str, hidden: &mut Vec<String>) {
    for child in named_children(node) {
        match child.kind() {
            "struct_item" | "enum_item" | "union_item" | "trait_item" | "type_item"
            | "function_item" | "mod_item" => {
                if let Some(name) = child.child_by_field_name("name") {
                    hidden.push(text(name, source).to_string());
                }
            }
            "use_declaration" => hidden.extend(use_names(child, source)),
            _ => {}
        }
        locals(child, source, hidden);
    }
}

/// The type declaration that a type as written names, if it is in the repository.
/// References and generic arguments are looked through to the head.
pub fn type_head(world: &World, ctx: &Ctx, node: Ts, source: &str) -> Option<usize> {
    let node = if node.kind() == "reference_type" {
        node.child_by_field_name("type")?
    } else {
        node
    };
    let path = path_of(node, source)?;
    match world.resolve(ctx, &path) {
        Res::Item(Entity::Type(index), consumed) if consumed == path.len() => Some(index),
        _ => None,
    }
}

/// An edge between a node and itself, its ancestor or its descendant says nothing: the
/// tree already holds that relation.
pub fn drop_edges_to_ancestors(
    edges: &mut EdgeSet,
    decls: &[Decl],
    mods: &[ModInfo],
    packages: &[(&str, &str)],
) {
    let mut parents: HashMap<String, String> = decls
        .iter()
        .map(|d| (d.id.clone(), d.parent.clone()))
        .collect();
    for m in mods.iter().filter(|m| m.has_node) {
        let (dir, crate_name) = packages[m.package];
        let parent = match m.parent {
            Some(parent) => container_id(dir, crate_name, &mods[parent]),
            None => format!("mod:{dir}"),
        };
        parents.insert(container_id(dir, crate_name, m), parent);
    }
    let ancestor = |id: &str, of: &str| {
        let mut current = Some(of);
        while let Some(node) = current {
            if node == id {
                return true;
            }
            current = parents.get(node).map(String::as_str);
        }
        false
    };
    edges.retain(|from, to| !ancestor(to, from) && !ancestor(from, to));
}
