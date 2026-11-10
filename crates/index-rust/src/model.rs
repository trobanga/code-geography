//! What the declarations say about the functions of each type: the inherent impls, the
//! trait impls, the traits that a type implements, the supertraits of a trait.

use crate::declare::{Decl, generic_names};
use crate::edges::type_head;
use crate::modules::ModInfo;
use crate::syntax::named_children;
use crate::world::{Ctx, World};
use std::collections::HashMap;

#[derive(Default)]
struct TypeInfo {
    /// The functions in `impl T { }`, as indexes of the declarations.
    inherent: Vec<usize>,
    /// The functions in `impl Trait for T { }`.
    trait_impls: Vec<usize>,
    /// The traits of the repository that `T` implements.
    traits: Vec<usize>,
    /// For a trait: the functions that it declares.
    declared: Vec<usize>,
    /// For a trait: its supertraits in the repository.
    supers: Vec<usize>,
    /// For an enum: the names of its variants.
    variants: Vec<String>,
}

/// The outcome of looking a function up by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Found {
    None,
    One(usize),
    Several,
}

pub struct Model {
    types: HashMap<usize, TypeInfo>,
    /// The declaration of each function item: `(module, node id)` to the index.
    pub decl_at: HashMap<(usize, usize), usize>,
}

impl Model {
    pub fn build<'t>(world: &World<'_, 't>, mods: &[ModInfo<'t>], decls: &[Decl<'t>]) -> Model {
        let decl_at: HashMap<(usize, usize), usize> = decls
            .iter()
            .enumerate()
            .map(|(index, d)| ((d.module, d.item.id()), index))
            .collect();
        let mut types: HashMap<usize, TypeInfo> = HashMap::new();
        for (index, m) in mods.iter().enumerate() {
            for item in &m.items {
                match item.kind() {
                    "trait_item" => {
                        let Some(&own) = decl_at.get(&(index, item.id())) else {
                            continue;
                        };
                        let generics = generic_names(*item, m.source);
                        let ctx = Ctx {
                            module: index,
                            generics: &generics,
                            self_type: None,
                        };
                        let info = types.entry(own).or_default();
                        if let Some(bounds) = item.child_by_field_name("bounds") {
                            for bound in named_children(bounds) {
                                if let Some(sup) = type_head(world, &ctx, bound, m.source) {
                                    info.supers.push(sup);
                                }
                            }
                        }
                        info.declared = members(*item)
                            .into_iter()
                            .filter_map(|f| decl_at.get(&(index, f.id())).copied())
                            .collect();
                    }
                    "enum_item" => {
                        let Some(&own) = decl_at.get(&(index, item.id())) else {
                            continue;
                        };
                        let variants = item
                            .child_by_field_name("body")
                            .map(named_children)
                            .unwrap_or_default()
                            .into_iter()
                            .filter(|v| v.kind() == "enum_variant")
                            .filter_map(|v| v.child_by_field_name("name"))
                            .map(|n| m.source[n.byte_range()].to_string());
                        types.entry(own).or_default().variants.extend(variants);
                    }
                    "impl_item" => {
                        let Some(self_node) = item.child_by_field_name("type") else {
                            continue;
                        };
                        let generics = generic_names(*item, m.source);
                        let ctx = Ctx {
                            module: index,
                            generics: &generics,
                            self_type: None,
                        };
                        let Some(own) = type_head(world, &ctx, self_node, m.source) else {
                            continue;
                        };
                        let trait_decl = item
                            .child_by_field_name("trait")
                            .and_then(|t| type_head(world, &ctx, t, m.source));
                        let functions: Vec<usize> = members(*item)
                            .into_iter()
                            .filter_map(|f| decl_at.get(&(index, f.id())).copied())
                            .collect();
                        let info = types.entry(own).or_default();
                        if item.child_by_field_name("trait").is_none() {
                            info.inherent.extend(functions);
                        } else {
                            info.trait_impls.extend(functions);
                            info.traits.extend(trait_decl);
                        }
                    }
                    _ => {}
                }
            }
        }
        Model { types, decl_at }
    }

    /// Whether `owner` is an enum with a variant `name`.
    pub fn has_variant(&self, owner: usize, name: &str) -> bool {
        self.types
            .get(&owner)
            .is_some_and(|info| info.variants.iter().any(|v| v == name))
    }

    /// The function `name` of the type or trait `owner`: in its inherent impls, else in
    /// its trait impls, else among the default methods of the traits that it implements;
    /// for a trait, among its own functions and those of its supertraits.
    pub fn function(&self, decls: &[Decl], owner: usize, name: &str) -> Found {
        let named = |list: &[usize]| -> Vec<usize> {
            list.iter()
                .copied()
                .filter(|f| decls[*f].name == name)
                .collect()
        };
        let Some(info) = self.types.get(&owner) else {
            return Found::None;
        };
        let mut defaults: Vec<usize> = info
            .traits
            .iter()
            .flat_map(|t| self.trait_functions(*t))
            .filter(|f| decls[*f].name == name && has_body(&decls[*f]))
            .collect();
        defaults.sort_unstable();
        defaults.dedup();
        let stages: [Vec<usize>; 3] = [named(&info.inherent), named(&info.trait_impls), defaults];
        let trait_functions: Vec<usize> = self
            .trait_functions(owner)
            .into_iter()
            .filter(|f| decls[*f].name == name)
            .collect();
        for found in std::iter::once(trait_functions).chain(stages) {
            match found.as_slice() {
                [] => {}
                [one] => return Found::One(*one),
                _ => return Found::Several,
            }
        }
        Found::None
    }

    /// The functions that a trait and its supertraits declare.
    fn trait_functions(&self, tr: usize) -> Vec<usize> {
        let mut seen = vec![tr];
        let mut found = Vec::new();
        let mut at = 0;
        while at < seen.len() {
            if let Some(info) = self.types.get(&seen[at]) {
                found.extend(info.declared.iter().copied());
                for sup in &info.supers {
                    if !seen.contains(sup) {
                        seen.push(*sup);
                    }
                }
            }
            at += 1;
        }
        found
    }
}

fn has_body(decl: &Decl) -> bool {
    decl.item.child_by_field_name("body").is_some()
}

/// The functions that an impl or a trait declares, with and without a body.
fn members(item: tree_sitter::Node) -> Vec<tree_sitter::Node> {
    item.child_by_field_name("body")
        .map(named_children)
        .unwrap_or_default()
        .into_iter()
        .filter(|c| matches!(c.kind(), "function_item" | "function_signature_item"))
        .collect()
}
