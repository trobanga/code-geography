//! The lexical scope of a module body: the aliases, and how a written module name turns
//! into a full name. Free of anything about edges or reports.

use crate::syntax::text;
use tree_sitter::Node as Ts;

/// What a written module name stands for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Name {
    /// A module of Elixir, by its full name.
    Module(String),
    /// An Erlang module (`:lists`).
    Erlang,
    /// Not a written name (`unquote(m)`, a variable, `__MODULE__` outside a module).
    Dynamic,
}

/// The aliases that hold at one place, short name and full name, in the order in which
/// they were made: a later one hides an earlier one of the same name.
#[derive(Debug, Default)]
pub struct Aliases(Vec<(String, String)>);

impl Aliases {
    pub fn insert(&mut self, short: &str, full: &str) {
        self.0.push((short.to_string(), full.to_string()));
    }

    /// The start of a block: what `end_block` needs to end the aliases made in it.
    pub fn mark(&self) -> usize {
        self.0.len()
    }

    /// Ends the aliases that were made after `mark`.
    pub fn end_block(&mut self, mark: usize) {
        self.0.truncate(mark);
    }

    fn get(&self, short: &str) -> Option<&str> {
        let found = self.0.iter().rev().find(|(name, _)| name == short);
        found.map(|(_, full)| full.as_str())
    }

    /// The full name of a name as written (`A.B`): `Elixir.` is dropped, else the first
    /// segment is replaced if an alias holds for it.
    pub fn expand_written(&self, written: &str) -> String {
        let (first, rest) = match written.split_once('.') {
            Some((first, rest)) => (first, Some(rest)),
            None => (written, None),
        };
        match (first, rest) {
            ("Elixir", Some(rest)) => rest.to_string(),
            _ => match (self.get(first), rest) {
                (Some(full), Some(rest)) => format!("{full}.{rest}"),
                (Some(full), None) => full.to_string(),
                (None, _) => written.to_string(),
            },
        }
    }

    /// The module that the name `node` stands for at this place, in the module `current`.
    pub fn expand(&self, node: Ts, source: &str, current: Option<&str>) -> Name {
        match node.kind() {
            "alias" => Name::Module(self.expand_written(text(node, source))),
            "atom" => Name::Erlang,
            "identifier" if text(node, source) == "__MODULE__" => {
                current.map_or(Name::Dynamic, |m| Name::Module(m.to_string()))
            }
            "dot" => self.expand_module_dot(node, source, current),
            _ => Name::Dynamic,
        }
    }

    /// `__MODULE__.Child`, else dynamic.
    fn expand_module_dot(&self, node: Ts, source: &str, current: Option<&str>) -> Name {
        let (Some(left), Some(right)) = (
            node.child_by_field_name("left"),
            node.child_by_field_name("right"),
        ) else {
            return Name::Dynamic;
        };
        if left.kind() != "identifier"
            || text(left, source) != "__MODULE__"
            || right.kind() != "alias"
        {
            return Name::Dynamic;
        }
        current.map_or(Name::Dynamic, |m| {
            Name::Module(format!("{m}.{}", text(right, source)))
        })
    }
}
