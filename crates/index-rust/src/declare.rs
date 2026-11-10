//! Pass 1: the type and function nodes of the mods.

use crate::edges::type_head;
use crate::modules::ModInfo;
use crate::syntax::{is_test_item, named_children, start_of, text};
use crate::world::{Ctx, World};
use cg_core::{NodeKind, Span};
use tree_sitter::Node as Ts;

/// The items that become type nodes.
pub const TYPE_ITEMS: [&str; 5] = [
    "struct_item",
    "enum_item",
    "union_item",
    "trait_item",
    "type_item",
];

/// A type or function that becomes a node.
pub struct Decl<'t> {
    pub id: String,
    pub kind: NodeKind,
    pub name: String,
    pub parent: String,
    /// The mod in whose source the declaration stands.
    pub module: usize,
    pub item: Ts<'t>,
    pub test: bool,
    pub hash: String,
    pub span: Span,
    /// A function of a trait or an impl, not a free function.
    pub is_member: bool,
}

/// The id of the node of a mod: the module node for the root without a node.
pub fn container_id(package_dir: &str, crate_name: &str, m: &ModInfo) -> String {
    if m.has_node {
        format!("pkg:{package_dir}:{}", qualified(crate_name, &m.path, None))
    } else {
        format!("mod:{package_dir}")
    }
}

/// `crate::a::b::name`.
pub fn qualified(crate_name: &str, path: &[String], name: Option<&str>) -> String {
    std::iter::once(crate_name)
        .chain(path.iter().map(String::as_str))
        .chain(name)
        .collect::<Vec<_>>()
        .join("::")
}

/// Declares the types, the free functions and the functions of traits.
pub fn declare<'t>(packages: &[(&str, &str)], mods: &[ModInfo<'t>]) -> Vec<Decl<'t>> {
    let mut found = Vec::new();
    for (index, m) in mods.iter().enumerate() {
        let (dir, crate_name) = packages[m.package];
        let container = container_id(dir, crate_name, m);
        for item in &m.items {
            let test = m.test || is_test_item(*item, m.source);
            match item.kind() {
                kind if TYPE_ITEMS.contains(&kind) => {
                    let Some(name) = item.child_by_field_name("name") else {
                        continue;
                    };
                    let name = text(name, m.source);
                    let id = format!("type:{}", qualified(crate_name, &m.path, Some(name)));
                    let members: Vec<Ts> = if kind == "trait_item" {
                        trait_functions(*item)
                    } else {
                        Vec::new()
                    };
                    found.push(decl(
                        NodeKind::Type,
                        id.clone(),
                        name,
                        container.clone(),
                        index,
                        *item,
                        m,
                        test,
                        &members,
                    ));
                    for member in members {
                        let Some(fn_name) = member.child_by_field_name("name") else {
                            continue;
                        };
                        let fn_name = text(fn_name, m.source);
                        let fn_id = format!("fn:{}#{fn_name}", &id["type:".len()..]);
                        let test = test || is_test_item(member, m.source);
                        let mut member_decl = decl(
                            NodeKind::Function,
                            fn_id,
                            fn_name,
                            id.clone(),
                            index,
                            member,
                            m,
                            test,
                            &[],
                        );
                        member_decl.is_member = true;
                        found.push(member_decl);
                    }
                }
                "function_item" => {
                    let Some(name) = item.child_by_field_name("name") else {
                        continue;
                    };
                    let name = text(name, m.source);
                    let id = format!("fn:{}", qualified(crate_name, &m.path, Some(name)));
                    found.push(decl(
                        NodeKind::Function,
                        id,
                        name,
                        container.clone(),
                        index,
                        *item,
                        m,
                        test,
                        &[],
                    ));
                }
                _ => {}
            }
        }
    }
    found
}

#[allow(clippy::too_many_arguments)]
fn decl<'t>(
    kind: NodeKind,
    id: String,
    name: &str,
    parent: String,
    module: usize,
    item: Ts<'t>,
    m: &ModInfo<'t>,
    test: bool,
    members: &[Ts<'t>],
) -> Decl<'t> {
    let (start_byte, start_row) = start_of(item);
    let mut own = String::new();
    let mut position = start_byte;
    for member in members {
        let member_start = start_of(*member).0;
        own.push_str(&m.source[position..member_start]);
        position = member.end_byte();
    }
    own.push_str(&m.source[position..item.end_byte()]);
    Decl {
        id,
        kind,
        name: name.to_string(),
        parent,
        module,
        item,
        test,
        is_member: false,
        hash: cg_core::text_hash(&own),
        span: Span {
            start_line: start_row as u32 + 1,
            end_line: item.end_position().row as u32 + 1,
        },
    }
}

/// The functions that a trait declares, with and without a body.
fn trait_functions(item: Ts) -> Vec<Ts> {
    item.child_by_field_name("body")
        .map(named_children)
        .unwrap_or_default()
        .into_iter()
        .filter(|c| matches!(c.kind(), "function_item" | "function_signature_item"))
        .collect()
}

/// The functions of the `impl` blocks, each a child of the type that the impl is for.
pub fn declare_impls<'t>(
    world: &World<'_, 't>,
    packages: &[(&str, &str)],
    mods: &[ModInfo<'t>],
) -> Vec<Decl<'t>> {
    let mut found = Vec::new();
    for (index, m) in mods.iter().enumerate() {
        let (dir, crate_name) = packages[m.package];
        for item in m.items.iter().filter(|i| i.kind() == "impl_item") {
            let Some(self_node) = item.child_by_field_name("type") else {
                continue;
            };
            let generics = generic_names(*item, m.source);
            let ctx = Ctx {
                module: index,
                generics: &generics,
                self_type: None,
            };
            let self_text = squash(text(self_node, m.source));
            let trait_text = item
                .child_by_field_name("trait")
                .map(|t| squash(text(t, m.source)));
            let self_decl = type_head(world, &ctx, self_node, m.source);
            let trait_decl = item
                .child_by_field_name("trait")
                .and_then(|t| type_head(world, &ctx, t, m.source));
            let (parent, owner, key) = match (self_decl, trait_decl) {
                (Some(own), _) => (
                    world.decl(own).id.clone(),
                    world.decl(own).id["type:".len()..].to_string(),
                    trait_text.as_ref().map(|t| format!("{t}::")),
                ),
                (None, Some(tr)) => (
                    world.decl(tr).id.clone(),
                    world.decl(tr).id["type:".len()..].to_string(),
                    Some(format!("for {self_text}::")),
                ),
                (None, None) => (
                    container_id(dir, crate_name, m),
                    qualified(crate_name, &m.path, None),
                    Some(match &trait_text {
                        Some(t) => format!("{t} for {self_text}::"),
                        None => format!("{self_text}::"),
                    }),
                ),
            };
            let test = m.test || is_test_item(*item, m.source);
            let Some(body) = item.child_by_field_name("body") else {
                continue;
            };
            for member in named_children(body)
                .into_iter()
                .filter(|c| c.kind() == "function_item")
            {
                let Some(name) = member.child_by_field_name("name") else {
                    continue;
                };
                let name = text(name, m.source);
                let id = format!("fn:{owner}#{}{name}", key.as_deref().unwrap_or(""));
                let test = test || is_test_item(member, m.source);
                let mut declared = decl(
                    NodeKind::Function,
                    id,
                    name,
                    parent.clone(),
                    index,
                    member,
                    m,
                    test,
                    &[],
                );
                declared.is_member = true;
                found.push(declared);
            }
        }
    }
    found
}

/// The names of the generic type and const parameters of an item.
pub fn generic_names(item: Ts, source: &str) -> Vec<String> {
    let Some(list) = item.child_by_field_name("type_parameters") else {
        return Vec::new();
    };
    named_children(list)
        .into_iter()
        .filter(|c| matches!(c.kind(), "type_parameter" | "const_parameter"))
        .filter_map(|p| p.child_by_field_name("name"))
        .map(|n| text(n, source).to_string())
        .collect()
}

/// A type as written, with a space only where two words meet.
fn squash(written: &str) -> String {
    let word = |c: char| c.is_alphanumeric() || c == '_' || c == '\'';
    let mut out = String::new();
    let mut pending_space = false;
    for c in written.chars() {
        if c.is_whitespace() {
            pending_space = true;
            continue;
        }
        if pending_space && out.chars().last().is_some_and(word) && word(c) {
            out.push(' ');
        }
        pending_space = false;
        out.push(c);
    }
    out
}

/// Keeps the first declaration of each id. Two declarations can share an id: variants
/// behind different `#[cfg(..)]`, or `impl W<u8>` and `impl W<u16>` with a function of
/// the same name.
pub fn dedup(decls: &mut Vec<Decl>) {
    let mut seen = std::collections::HashSet::new();
    decls.retain(|d| seen.insert(d.id.clone()));
}
