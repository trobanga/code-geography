//! The static types that the calls pass reads off the declarations: the written type of
//! a parameter, a field or a return value, as far as the repository tells.

use crate::declare::{Decl, generic_names};
use crate::edges::type_head;
use crate::model::Model;
use crate::modules::ModInfo;
use crate::syntax::{named_children, path_of, text};
use crate::world::{Ctx, Entity, Res, World};
use std::collections::HashMap;
use tree_sitter::Node as Ts;

/// The static type of an expression, as far as the repository tells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ty {
    /// A type that is declared in the repository: a struct, enum, union, alias or trait.
    Repo(usize),
    /// `Box<T>`, `Arc<T>`, `Rc<T>`: a method is looked up in `T` only if `T` declares it.
    Ptr(Box<Ty>),
    /// `Option<T>`, `Result<T, _>` or a future that yields `T`.
    Wrap(Wrapper, Box<Ty>),
    /// A value known only by traits of the repository: `dyn Tr`, `impl Tr`, `T: Tr`.
    Bounds(Vec<usize>),
    /// A type that is known and not declared in the repository.
    External,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wrapper {
    Option,
    Result,
    Future,
}

/// The names that the types and paths of a declaration are read against.
pub struct Cx {
    pub module: usize,
    pub generics: Vec<String>,
    /// The traits of the repository that each generic parameter is bound by.
    pub bounds: HashMap<String, Vec<usize>>,
    /// The types that the generic parameters of an alias stand for.
    pub subst: HashMap<String, Ty>,
    /// How many aliases were followed to get here.
    pub depth: u8,
    /// What `Self` means for paths. `None` outside an impl or trait, or for an external type.
    pub self_type: Option<Entity>,
    /// The type of `self`.
    pub self_ty: Ty,
}

impl Cx {
    pub fn ctx(&self) -> Ctx<'_> {
        Ctx {
            module: self.module,
            generics: &self.generics,
            self_type: self.self_type,
        }
    }
}

pub struct Types<'a, 'w, 't> {
    pub world: &'a World<'w, 't>,
    pub mods: &'a [ModInfo<'t>],
    pub decls: &'a [Decl<'t>],
    pub model: &'a Model,
}

impl<'t> Types<'_, '_, 't> {
    /// The names that the function `f` is read against: its impl or trait, its generics.
    pub fn fn_cx(&self, f: usize) -> Cx {
        let decl = &self.decls[f];
        let source = self.mods[decl.module].source;
        let holder = decl
            .item
            .parent()
            .and_then(|list| list.parent())
            .filter(|h| matches!(h.kind(), "impl_item" | "trait_item"));
        let mut cx = self.item_cx(decl.module, holder);
        cx.generics.extend(generic_names(decl.item, source));
        let mut bounds = std::mem::take(&mut cx.bounds);
        self.collect_bounds(&cx, decl.item, &mut bounds);
        cx.bounds = bounds;
        cx
    }

    /// The names that the body of an impl or a trait (`holder`), or of a mod, is read
    /// against.
    pub fn item_cx(&self, module: usize, holder: Option<Ts<'t>>) -> Cx {
        let source = self.mods[module].source;
        let mut generics = Vec::new();
        let mut self_type = None;
        if let Some(holder) = holder {
            generics = generic_names(holder, source);
            if holder.kind() == "trait_item" {
                self_type = self
                    .model
                    .decl_at
                    .get(&(module, holder.id()))
                    .map(|t| Entity::Type(*t));
            } else {
                let ctx = Ctx {
                    module,
                    generics: &generics,
                    self_type: None,
                };
                self_type = holder
                    .child_by_field_name("type")
                    .and_then(|t| type_head(self.world, &ctx, t, source))
                    .map(Entity::Type);
            }
        }
        let mut cx = Cx {
            module,
            generics,
            bounds: HashMap::new(),
            subst: HashMap::new(),
            depth: 0,
            self_type,
            self_ty: match (self_type, holder) {
                (Some(Entity::Type(t)), _) => Ty::Repo(t),
                (_, Some(_)) => Ty::External,
                _ => Ty::Unknown,
            },
        };
        if let Some(holder) = holder {
            let mut bounds = HashMap::new();
            self.collect_bounds(&cx, holder, &mut bounds);
            cx.bounds = bounds;
        }
        cx
    }

    /// The traits of the repository that the generic parameters of `item` are bound by,
    /// in the list of parameters and in the `where` clause.
    fn collect_bounds(&self, cx: &Cx, item: Ts<'t>, found: &mut HashMap<String, Vec<usize>>) {
        let source = self.mods[cx.module].source;
        let mut add = |name: &str, bounds: Ts<'t>| {
            let traits = self.traits_of(cx, bounds);
            if !traits.is_empty() {
                found.entry(name.to_string()).or_default().extend(traits);
            }
        };
        if let Some(list) = item.child_by_field_name("type_parameters") {
            for parameter in named_children(list) {
                if parameter.kind() == "type_parameter"
                    && let (Some(name), Some(bounds)) = (
                        parameter.child_by_field_name("name"),
                        parameter.child_by_field_name("bounds"),
                    )
                {
                    add(text(name, source), bounds);
                }
            }
        }
        for clause in named_children(item)
            .into_iter()
            .filter(|c| c.kind() == "where_clause")
        {
            for predicate in named_children(clause) {
                if let (Some(left), Some(bounds)) = (
                    predicate.child_by_field_name("left"),
                    predicate.child_by_field_name("bounds"),
                ) && left.kind() == "type_identifier"
                {
                    add(text(left, source), bounds);
                }
            }
        }
    }

    /// The traits of the repository that a bound, `dyn` or `impl` type names.
    fn traits_of(&self, cx: &Cx, node: Ts<'t>) -> Vec<usize> {
        let source = self.mods[cx.module].source;
        match node.kind() {
            "trait_bounds" | "bounded_type" => named_children(node)
                .into_iter()
                .flat_map(|child| self.traits_of(cx, child))
                .collect(),
            "dynamic_type" | "abstract_type" => node
                .child_by_field_name("trait")
                .map(|tr| self.traits_of(cx, tr))
                .unwrap_or_default(),
            _ => type_head(self.world, &cx.ctx(), node, source)
                .filter(|t| self.decls[*t].item.kind() == "trait_item")
                .into_iter()
                .collect(),
        }
    }

    /// The type that a type as written names.
    pub fn ty(&self, cx: &Cx, node: Ts<'t>) -> Ty {
        let source = self.mods[cx.module].source;
        match node.kind() {
            "reference_type" => node
                .child_by_field_name("type")
                .map_or(Ty::Unknown, |inner| self.ty(cx, inner)),
            "primitive_type" | "tuple_type" | "array_type" | "unit_type" | "function_type" => {
                Ty::External
            }
            "type_identifier" | "scoped_type_identifier" | "generic_type" => {
                let Some(path) = path_of(node, source) else {
                    return Ty::Unknown;
                };
                match self.world.resolve(&cx.ctx(), &path) {
                    Res::Item(Entity::Type(t), consumed) if consumed == path.len() => {
                        if self.decls[t].item.kind() == "type_item" {
                            self.alias(cx, t, node)
                        } else {
                            Ty::Repo(t)
                        }
                    }
                    Res::External => {
                        let first =
                            || Box::new(self.first_argument(cx, node).unwrap_or(Ty::Unknown));
                        match path.last().copied() {
                            Some("Box" | "Arc" | "Rc") => Ty::Ptr(first()),
                            Some("Option") => Ty::Wrap(Wrapper::Option, first()),
                            Some("Result") => Ty::Wrap(Wrapper::Result, first()),
                            _ => Ty::External,
                        }
                    }
                    Res::Hidden => match path.as_slice() {
                        [name] if cx.subst.contains_key(*name) => cx.subst[*name].clone(),
                        [name] if cx.bounds.contains_key(*name) => {
                            Ty::Bounds(cx.bounds[*name].clone())
                        }
                        _ => Ty::External,
                    },
                    _ => Ty::Unknown,
                }
            }
            "abstract_type" | "dynamic_type" | "bounded_type" => {
                if let Some(future) = self.future_output(cx, node) {
                    return future;
                }
                let traits = self.traits_of(cx, node);
                if traits.is_empty() {
                    Ty::External
                } else {
                    Ty::Bounds(traits)
                }
            }
            _ => Ty::Unknown,
        }
    }

    /// The type that the alias `alias`, used as `node`, stands for.
    fn alias(&self, cx: &Cx, alias: usize, node: Ts<'t>) -> Ty {
        let decl = &self.decls[alias];
        let source = self.mods[decl.module].source;
        let (Some(target), true) = (decl.item.child_by_field_name("type"), cx.depth < 8) else {
            return Ty::Unknown;
        };
        let names = generic_names(decl.item, source);
        let arguments: Vec<Ty> = node
            .child_by_field_name("type_arguments")
            .map(named_children)
            .unwrap_or_default()
            .into_iter()
            .filter(|a| !matches!(a.kind(), "lifetime" | "type_binding"))
            .map(|a| self.ty(cx, a))
            .collect();
        let inner = Cx {
            module: decl.module,
            generics: names.clone(),
            bounds: HashMap::new(),
            subst: names
                .into_iter()
                .zip(arguments.into_iter().chain(std::iter::repeat(Ty::Unknown)))
                .collect(),
            depth: cx.depth + 1,
            self_type: None,
            self_ty: Ty::Unknown,
        };
        self.ty(&inner, target)
    }

    /// The type that `t` names: the type itself, or the repository type that an alias
    /// stands for.
    pub fn unalias(&self, t: usize) -> Option<usize> {
        if self.decls[t].item.kind() != "type_item" {
            return Some(t);
        }
        let decl = &self.decls[t];
        let cx = self.item_cx(decl.module, None);
        match self.alias(&cx, t, decl.item) {
            Ty::Repo(target) => Some(target),
            _ => None,
        }
    }

    /// The first generic argument that is a type: `T` in `Option<T>`.
    fn first_argument(&self, cx: &Cx, generic: Ts<'t>) -> Option<Ty> {
        let arguments = generic.child_by_field_name("type_arguments")?;
        let first = named_children(arguments)
            .into_iter()
            .find(|a| !matches!(a.kind(), "lifetime" | "type_binding"))?;
        Some(self.ty(cx, first))
    }

    /// `impl Future<Output = T>` gives the future that yields `T`.
    fn future_output(&self, cx: &Cx, node: Ts<'t>) -> Option<Ty> {
        let source = self.mods[cx.module].source;
        let tr = node.child_by_field_name("trait")?;
        if tr.kind() != "generic_type" || path_of(tr, source)?.last() != Some(&"Future") {
            return None;
        }
        let arguments = tr.child_by_field_name("type_arguments")?;
        let output = named_children(arguments).into_iter().find(|a| {
            a.kind() == "type_binding"
                && a.child_by_field_name("name")
                    .is_some_and(|n| text(n, source) == "Output")
        })?;
        let ty = self.ty(cx, output.child_by_field_name("type")?);
        Some(Ty::Wrap(Wrapper::Future, Box::new(ty)))
    }

    pub fn is_trait(&self, t: usize) -> bool {
        self.decls[t].item.kind() == "trait_item"
    }

    /// The type that the function `f` returns, as written. `receiver` is the type that a
    /// function of a trait is called on: `Self` means that type.
    pub fn ret_ty(&self, f: usize, receiver: Option<usize>) -> Ty {
        let mut cx = self.fn_cx(f);
        if let (Some(receiver), Some(Entity::Type(owner))) = (receiver, cx.self_type)
            && self.decls[owner].item.kind() == "trait_item"
        {
            cx.self_type = Some(Entity::Type(receiver));
        }
        let returned = match self.decls[f].item.child_by_field_name("return_type") {
            Some(node) => self.ty(&cx, node),
            None => Ty::External,
        };
        if self.is_async(f) {
            Ty::Wrap(Wrapper::Future, Box::new(returned))
        } else {
            returned
        }
    }

    fn is_async(&self, f: usize) -> bool {
        let item = self.decls[f].item;
        let source = self.mods[self.decls[f].module].source;
        named_children(item)
            .into_iter()
            .any(|c| c.kind() == "function_modifiers" && text(c, source).contains("async"))
    }

    /// The type of the field `name` of the struct `owner`, as written.
    pub fn field_ty(&self, owner: usize, name: &str) -> Ty {
        let decl = &self.decls[owner];
        if decl.item.kind() != "struct_item" {
            return Ty::Unknown;
        }
        let source = self.mods[decl.module].source;
        let cx = Cx {
            module: decl.module,
            generics: generic_names(decl.item, source),
            bounds: HashMap::new(),
            subst: HashMap::new(),
            depth: 0,
            self_type: Some(Entity::Type(owner)),
            self_ty: Ty::Repo(owner),
        };
        let Some(body) = decl.item.child_by_field_name("body") else {
            return Ty::Unknown;
        };
        let node = match body.kind() {
            "field_declaration_list" => named_children(body)
                .into_iter()
                .filter(|f| f.kind() == "field_declaration")
                .find(|f| {
                    f.child_by_field_name("name")
                        .is_some_and(|n| text(n, source) == name)
                })
                .and_then(|f| f.child_by_field_name("type")),
            "ordered_field_declaration_list" => {
                let mut cursor = body.walk();
                let types: Vec<Ts> = body.children_by_field_name("type", &mut cursor).collect();
                name.parse::<usize>()
                    .ok()
                    .and_then(|position| types.get(position).copied())
            }
            _ => None,
        };
        node.map_or(Ty::Unknown, |node| self.ty(&cx, node))
    }
}
