//! What a `use` can add to the aliases of a module (ADR 0037): the `quote`s of the macro
//! that it runs. A `quote` is code of the module that uses it, so the walk does not read it
//! as code, but it keeps the names that the `quote` aliases.

use crate::declare::{RefKind, alias_short, as_name, entries, group, written_segments};
use crate::scope::{Aliases, Name, Use};
use crate::syntax::{arguments, call_head, descendants, text};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use tree_sitter::Node as Ts;

/// The aliases that the `quote`s of one module can make.
#[derive(Debug, Default)]
pub struct Effect {
    /// The short names of `alias` and `require .., as:` directives, and the first segment of
    /// the name of a `defmodule`.
    shorts: BTreeSet<String>,
    /// The modules that a `use` names, each with the last segments of the module names that
    /// the arguments of its `use`s write.
    uses: BTreeMap<String, BTreeSet<String>>,
    /// An alias whose name is not written: it can be any name.
    any: bool,
}

impl Effect {
    pub fn merge(&mut self, other: Effect) {
        self.shorts.extend(other.shorts);
        for (module, written) in other.uses {
            self.uses.entry(module).or_default().extend(written);
        }
        self.any |= other.any;
    }

    /// A `defmodule` of the `quote`: it makes the first segment of its name an alias.
    fn defmodule(&mut self, first: Ts, source: &str) {
        let written = text(first, source);
        let written = written.strip_prefix("Elixir.").unwrap_or(written);
        match written.split('.').next() {
            Some(short) if first.kind() == "alias" => {
                self.shorts.insert(short.to_string());
            }
            _ => self.any = true,
        }
    }

    /// A `use` of the module `name`; `written` are the last segments of the module names
    /// in its arguments.
    fn use_module(&mut self, name: Name, written: &[String]) {
        match name {
            Name::Module(full) => {
                self.uses
                    .entry(full)
                    .or_default()
                    .extend(written.iter().cloned());
            }
            Name::Dynamic => self.any = true,
            Name::Erlang => {}
        }
    }

    /// An `alias`, `require` or `import` of the module `name`; `local` holds the aliases
    /// that the `quote` has made so far.
    fn directive(
        &mut self,
        kind: RefKind,
        named: Option<&str>,
        grouped: bool,
        name: &Name,
        local: &mut Aliases,
    ) {
        match (short_name(kind, named, grouped, name), name) {
            (Some(short), Name::Module(full)) => {
                self.shorts.insert(short.to_string());
                local.insert(short, full);
            }
            (Some(short), _) => {
                self.shorts.insert(short.to_string());
            }
            (None, Name::Dynamic) if kind == RefKind::Alias => self.any = true,
            (None, _) => {}
        }
    }
}

/// The short name that a directive makes an alias of.
fn short_name<'a>(
    kind: RefKind,
    named: Option<&'a str>,
    grouped: bool,
    name: &'a Name,
) -> Option<&'a str> {
    match name {
        Name::Module(full) => alias_short(kind, named, grouped, full),
        _ => named.filter(|_| !grouped && kind != RefKind::Import),
    }
}

/// The aliases that the `quote` at `quote` can make; `aliases` hold where it stands.
pub fn read(quote: Ts, source: &str, aliases: &Aliases) -> Effect {
    let mut local = aliases.clone();
    let mut effect = Effect::default();
    for node in descendants(quote) {
        let head = call_head(node, source);
        let Some(first) = arguments(node).first().copied() else {
            continue;
        };
        if head == Some("defmodule") {
            effect.defmodule(first, source);
            continue;
        }
        let Some(kind) = RefKind::directive(head) else {
            continue;
        };
        let named = as_name(node, source);
        let grouped = group(first).is_some();
        let written = written_segments(node, source);
        for (name, _) in entries(first, &local, source, None) {
            if kind == RefKind::Use {
                effect.use_module(name, &written);
            } else {
                effect.directive(kind, named, grouped, &name, &mut local);
            }
        }
    }
    effect
}

/// Which aliases a `use` can make, for the modules that the repository declares.
pub struct Reach<'a> {
    effects: &'a BTreeMap<String, Effect>,
    /// The modules of the repository.
    modules: &'a BTreeMap<&'a str, &'a str>,
    /// The modules of the repository with a function `__using__/1`.
    macros: HashSet<&'a str>,
}

impl<'a> Reach<'a> {
    pub fn new(
        effects: &'a BTreeMap<String, Effect>,
        modules: &'a BTreeMap<&'a str, &'a str>,
        macros: HashSet<&'a str>,
    ) -> Self {
        Reach {
            effects,
            modules,
            macros,
        }
    }

    /// Whether the `use` can make an alias with the name `first`.
    pub fn may_alias(&self, used: &Use, first: &str) -> bool {
        match &used.module {
            Name::Dynamic => true,
            Name::Module(module) if self.modules.contains_key(module.as_str()) => {
                self.macro_may_alias(module, first, &mut HashSet::new())
            }
            // A module outside the repository knows the names of the repository only
            // through its arguments.
            Name::Module(_) | Name::Erlang => used.written.iter().any(|w| w == first),
        }
    }

    fn macro_may_alias(&self, module: &str, first: &str, visited: &mut HashSet<String>) -> bool {
        // A module that gets its `__using__` from a macro can make any alias.
        if !self.macros.contains(module) {
            return true;
        }
        if !visited.insert(module.to_string()) {
            return false;
        }
        let Some(effect) = self.effects.get(module) else {
            return false;
        };
        effect.any
            || effect.shorts.contains(first)
            || effect.uses.iter().any(|(used, written)| {
                if self.modules.contains_key(used.as_str()) {
                    self.macro_may_alias(used, first, visited)
                } else {
                    written.contains(first)
                }
            })
    }
}
