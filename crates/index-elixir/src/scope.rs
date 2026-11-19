//! The lexical scope of a module body: the aliases, and how a written module name turns
//! into a full name. Free of anything about edges or reports.

use crate::syntax::text;
use std::rc::Rc;
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

/// Which functions of a module an `import` brings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Filter {
    /// No `only:` and no `except:`.
    All,
    /// `only: [f: 1]`.
    Only(Vec<(String, usize)>),
    /// `only: :functions` or `only: :macros`.
    OnlyKind { macros: bool },
    /// `except: [f: 1]`.
    Except(Vec<(String, usize)>),
    /// A filter that is not a literal: it can bring any function.
    Unknown,
}

impl Filter {
    /// Whether the import brings the function `name` that a call with `arity` arguments
    /// reaches; `macro_` for a macro.
    pub fn brings(&self, name: &str, arity: usize, macro_: bool) -> Option<bool> {
        let listed = |list: &[(String, usize)]| list.iter().any(|(n, a)| n == name && *a == arity);
        match self {
            Filter::All => Some(true),
            Filter::Only(list) => Some(listed(list)),
            Filter::OnlyKind { macros } => Some(*macros == macro_),
            Filter::Except(list) => Some(!listed(list)),
            Filter::Unknown => None,
        }
    }
}

/// An `import` of a module.
#[derive(Debug, PartialEq, Eq)]
pub struct Import {
    /// The full name of the module.
    pub module: String,
    pub filter: Filter,
}

/// A `use` of a module.
#[derive(Debug, PartialEq, Eq)]
pub struct Use {
    pub module: Name,
    /// The last segments of the module names that its arguments write (`use Lib, repo:
    /// MyApp.Repo` writes `Repo`), not counting the module itself.
    pub written: Vec<String>,
}

/// One thing that a directive made at the place: the entries keep their order, a later one
/// hides an earlier one of the same alias.
#[derive(Debug, Clone)]
enum Entry {
    Alias(String, String),
    Import(Rc<Import>),
    Use(Rc<Use>),
}

/// The aliases and imports that hold at one place, in the order in which they were made.
#[derive(Debug, Default, Clone)]
pub struct Aliases(Vec<Entry>);

impl Aliases {
    pub fn insert(&mut self, short: &str, full: &str) {
        self.0
            .push(Entry::Alias(short.to_string(), full.to_string()));
    }

    /// Adds an `import` of the module `full`.
    pub fn insert_import(&mut self, full: &str, filter: Filter) {
        self.0.push(Entry::Import(Rc::new(Import {
            module: full.to_string(),
            filter,
        })));
    }

    /// Adds a `use`.
    pub fn insert_use(&mut self, module: Name, written: Vec<String>) {
        self.0.push(Entry::Use(Rc::new(Use { module, written })));
    }

    /// The `use`s that can have made an alias of the first segment of the module name
    /// `written`: those after the alias that expands it, all of them if no alias does, none
    /// for a name that starts with `Elixir.`. Whether an alias expands the first segment.
    pub fn uses_after(&self, written: &str) -> (bool, Vec<Rc<Use>>) {
        let first = written.split('.').next().unwrap_or(written);
        if first == "Elixir" {
            return (false, Vec::new());
        }
        let alias = self
            .0
            .iter()
            .rposition(|e| matches!(e, Entry::Alias(short, _) if short == first));
        let after = alias.map_or(0, |index| index + 1);
        let uses = self.0[after..]
            .iter()
            .filter_map(|e| match e {
                Entry::Use(used) => Some(used.clone()),
                _ => None,
            })
            .collect();
        (alias.is_some(), uses)
    }

    /// The imports that hold at this place: a later `import` of a module replaces an
    /// earlier one of the same module.
    pub fn imports(&self) -> Vec<Rc<Import>> {
        let mut seen: Vec<&str> = Vec::new();
        let mut imports: Vec<Rc<Import>> = Vec::new();
        for entry in self.0.iter().rev() {
            if let Entry::Import(import) = entry
                && !seen.contains(&import.module.as_str())
            {
                seen.push(&import.module);
                imports.push(import.clone());
            }
        }
        imports.reverse();
        imports
    }

    /// The start of a block: what `end_block` needs to end the entries made in it.
    pub fn mark(&self) -> usize {
        self.0.len()
    }

    /// Ends the entries that were made after `mark`.
    pub fn end_block(&mut self, mark: usize) {
        self.0.truncate(mark);
    }

    fn get(&self, short: &str) -> Option<&str> {
        let found = self.0.iter().rev().find_map(|e| match e {
            Entry::Alias(name, full) if name == short => Some(full),
            _ => None,
        });
        found.map(String::as_str)
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
