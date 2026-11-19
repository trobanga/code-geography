//! Call sites: what the walk records, and how they resolve to edges (ADR 0037).

use crate::declare::{Declared, FunctionDecl};
use crate::edges::below_declared;
use crate::quote::Reach;
use crate::scope::{Import, Name, Use};
use crate::{Report, UnresolvedCalls, package_id};
use cg_core::{EdgeKind, EdgeSet};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

/// A call that the walk read, inside a module.
pub struct CallSite {
    /// The module around the call.
    pub module: String,
    /// The name and the arity of the function around the call, if any.
    pub function: Option<(String, usize)>,
    pub call: Call,
    /// The number of arguments as written, the right side of a pipe included.
    pub arity: usize,
    /// A function that is named, not called (`&f/1`).
    pub value: bool,
    /// Whether the call stands in test code: only test code reaches the modules of test
    /// code.
    pub test: bool,
    pub file: String,
    /// 1-based.
    pub line: u32,
}

/// What a call names.
pub enum Call {
    /// `Mod.fun(..)`.
    Remote {
        name: String,
        module: Name,
        /// The first segment of the module name as written.
        first: String,
        /// Whether an alias expands the first segment.
        aliased: bool,
        /// The `use`s that can have made an alias of the first segment.
        uses: Vec<Rc<Use>>,
    },
    /// `fun(..)`; the imports that hold at the call.
    Local {
        name: String,
        imports: Vec<Rc<Import>>,
    },
}

/// What a call site resolves to.
enum Outcome<'a> {
    Function(&'a FunctionDecl),
    /// The package of the module with this full name.
    Type(&'a str),
    External,
    Unresolved(Why),
}

/// Why a call site stays unresolved.
enum Why {
    UseAlias,
    Ambiguous,
    Dynamic,
    LocalUnknown,
    BelowDeclared,
    MacroArgument,
}

/// Resolves the call sites to edges, and counts how they resolve.
pub fn resolve(
    declared: &Declared,
    dirs: &BTreeMap<&str, &str>,
    edges: &mut EdgeSet,
    report: &mut Report,
) {
    let macros: HashSet<&str> = declared
        .functions
        .iter()
        .filter(|f| f.name == "__using__" && f.arity == 1)
        .map(|f| f.module.as_str())
        .chain(declared.using_blocks.iter().map(String::as_str))
        .collect();
    // Main code is compiled without test code: it reaches the modules of main code only.
    let main: HashSet<&str> = declared
        .modules
        .iter()
        .filter(|m| !m.test)
        .map(|m| m.name.as_str())
        .collect();
    let resolver = Resolver {
        functions: Functions::new(&declared.functions),
        dirs,
        reach: Reach::new(&declared.quotes, dirs, macros),
        main,
    };
    for site in &declared.sites {
        let outcome = resolver.outcome(site);
        let Some(to) = resolver.target(outcome, site, report) else {
            continue;
        };
        report.behind_outside_use += usize::from(resolver.behind_outside(site));
        let from = match &site.function {
            Some((name, arity)) => format!("fn:{}.{name}/{arity}", site.module),
            None => package_id(dirs[site.module.as_str()], &site.module),
        };
        edges.add(&from, &to, EdgeKind::Calls, &site.file, site.line);
    }
}

/// The functions of the repository by module and name.
struct Functions<'a>(HashMap<(&'a str, &'a str), Vec<&'a FunctionDecl>>);

impl<'a> Functions<'a> {
    fn new(functions: &'a [FunctionDecl]) -> Self {
        let mut by_name: HashMap<(&str, &str), Vec<&FunctionDecl>> = HashMap::new();
        for function in functions {
            by_name
                .entry((&function.module, &function.name))
                .or_default()
                .push(function);
        }
        Functions(by_name)
    }

    /// The function of `module` that a call with `arity` arguments reaches; only a public
    /// one if `public` is set.
    fn get(
        &self,
        module: &str,
        name: &str,
        arity: usize,
        public: bool,
    ) -> Option<&'a FunctionDecl> {
        self.0
            .get(&(module, name))?
            .iter()
            .copied()
            .find(|f| f.answers(arity) && (f.public || !public))
    }
}

struct Resolver<'a> {
    functions: Functions<'a>,
    dirs: &'a BTreeMap<&'a str, &'a str>,
    reach: Reach<'a>,
    main: HashSet<&'a str>,
}

impl Resolver<'_> {
    /// Whether the call site reaches the module: the repository declares it, and test code
    /// is the only one to reach modules of test code.
    fn known(&self, site: &CallSite, module: &str) -> bool {
        self.dirs.contains_key(module) && (site.test || self.main.contains(module))
    }

    fn outcome<'x>(&'x self, site: &'x CallSite) -> Outcome<'x> {
        match &site.call {
            Call::Remote { first, uses, .. }
                if uses.iter().any(|used| self.reach.may_alias(used, first)) =>
            {
                Outcome::Unresolved(Why::UseAlias)
            }
            Call::Remote { module, name, .. } => self.remote(site, module, name),
            Call::Local { name, imports } => self.local(site, name, imports),
        }
    }

    fn remote<'x>(&'x self, site: &'x CallSite, module: &'x Name, name: &str) -> Outcome<'x> {
        match module {
            Name::Module(full) if self.known(site, full) => self.declared_module(site, full, name),
            Name::Module(full) if below_declared(self.dirs, full) => {
                Outcome::Unresolved(Why::BelowDeclared)
            }
            Name::Module(_) | Name::Erlang => Outcome::External,
            Name::Dynamic => Outcome::Unresolved(Why::Dynamic),
        }
    }

    /// `full.name(..)` of a module that the site reaches.
    fn declared_module<'x>(&'x self, site: &CallSite, full: &'x str, name: &str) -> Outcome<'x> {
        match self.functions.get(full, name, site.arity, true) {
            Some(function) => Outcome::Function(function),
            None if site.function.is_none() => Outcome::Unresolved(Why::MacroArgument),
            None => Outcome::Type(full),
        }
    }

    fn local<'x>(&'x self, site: &CallSite, name: &str, imports: &[Rc<Import>]) -> Outcome<'x> {
        let own = self.functions.get(&site.module, name, site.arity, false);
        match own.map_or_else(
            || {
                imported(&self.functions, imports, name, site.arity, |module| {
                    self.known(site, module)
                })
            },
            Imported::Function,
        ) {
            Imported::Function(function) => Outcome::Function(function),
            Imported::Ambiguous => Outcome::Unresolved(Why::Ambiguous),
            Imported::Nothing if KERNEL.contains(&name) || name.starts_with("sigil_") => {
                Outcome::External
            }
            Imported::Nothing => Outcome::Unresolved(Why::LocalUnknown),
        }
    }

    /// The id of the node that the call goes to, and the counts of the report; `None` for a
    /// call that gives no edge.
    fn target(&self, outcome: Outcome, site: &CallSite, report: &mut Report) -> Option<String> {
        match outcome {
            Outcome::Function(function) => {
                report.calls.to_function += 1;
                report.function_values += usize::from(site.value);
                Some(format!(
                    "fn:{}.{}/{}",
                    function.module, function.name, function.arity
                ))
            }
            Outcome::Type(full) => {
                report.calls.to_type += 1;
                if let Call::Remote { name, .. } = &site.call {
                    let key = format!("{full}.{name}/{}", site.arity);
                    *report.undeclared_functions.entry(key).or_default() += 1;
                }
                Some(package_id(self.dirs[full], full))
            }
            Outcome::External => {
                report.calls.external += 1;
                None
            }
            Outcome::Unresolved(why) => {
                report.calls.unresolved += 1;
                count_unresolved(&mut report.unresolved_calls, &why);
                None
            }
        }
    }

    /// Whether the call goes behind a `use` of a module outside the repository, with no alias.
    fn behind_outside(&self, site: &CallSite) -> bool {
        let Call::Remote { aliased, uses, .. } = &site.call else {
            return false;
        };
        let outside = |used: &Use| match &used.module {
            Name::Module(module) => !self.dirs.contains_key(module.as_str()),
            Name::Erlang => true,
            Name::Dynamic => false,
        };
        !aliased && uses.iter().any(|u| outside(u))
    }
}

fn count_unresolved(counts: &mut UnresolvedCalls, why: &Why) {
    match why {
        Why::UseAlias => counts.use_alias += 1,
        Why::Ambiguous => counts.ambiguous += 1,
        Why::Dynamic => counts.dynamic += 1,
        Why::LocalUnknown => counts.local_unknown += 1,
        Why::BelowDeclared => counts.below_declared += 1,
        Why::MacroArgument => counts.macro_argument += 1,
    }
}

/// What the imports bring for a local call.
enum Imported<'a> {
    Nothing,
    Function(&'a FunctionDecl),
    /// Two functions, or a function that an import of unknown filter may bring: the code
    /// would not compile, or the filter is not read.
    Ambiguous,
}

fn imported<'a>(
    functions: &Functions<'a>,
    imports: &[Rc<Import>],
    name: &str,
    arity: usize,
    known: impl Fn(&str) -> bool,
) -> Imported<'a> {
    // Each function that an import of the repository has under the name, and whether the
    // import brings it: `None` if its filter is not read.
    let brought: Vec<(Option<bool>, &FunctionDecl)> = imports
        .iter()
        .filter(|i| known(&i.module))
        .filter_map(|import| {
            let function = functions.get(&import.module, name, arity, true)?;
            Some((import.filter.brings(name, arity, function.macro_), function))
        })
        .collect();
    let maybe = brought.iter().any(|(brings, _)| brings.is_none());
    let found = distinct(
        brought
            .iter()
            .filter(|(brings, _)| *brings == Some(true))
            .map(|(_, function)| *function),
    );
    match (found.as_slice(), maybe) {
        ([], false) => Imported::Nothing,
        ([function], false) => Imported::Function(function),
        _ => Imported::Ambiguous,
    }
}

/// The functions without repeats, by identity.
fn distinct<'a>(functions: impl Iterator<Item = &'a FunctionDecl>) -> Vec<&'a FunctionDecl> {
    let mut out: Vec<&FunctionDecl> = Vec::new();
    for function in functions {
        if !out.iter().any(|f| std::ptr::eq(*f, function)) {
            out.push(function);
        }
    }
    out
}

/// The names of `Kernel` and `Kernel.SpecialForms` that stand as `name(..)` (and the
/// `sigil_` functions): a local call of one of them is a call of Elixir itself, with any
/// arity.
#[rustfmt::skip]
const KERNEL: &[&str] = &[
    "__aliases__", "__block__", "__CALLER__", "__DIR__", "__ENV__", "__MODULE__",
    "__STACKTRACE__", "abs", "alias", "alias!", "and", "apply", "binary_part", "binary_slice",
    "binding", "bit_size", "byte_size", "case", "ceil", "cond", "dbg", "def", "defdelegate",
    "defexception", "defguard", "defguardp", "defimpl", "defmacro", "defmacrop", "defmodule",
    "defoverridable", "defp", "defprotocol", "defstruct", "destructure", "div", "elem", "exit",
    "floor", "fn", "for", "function_exported?", "get_and_update_in", "get_in", "hd", "if",
    "import", "in", "inspect", "is_atom", "is_binary", "is_bitstring", "is_boolean",
    "is_exception", "is_float", "is_function", "is_integer", "is_list", "is_map", "is_map_key",
    "is_nil", "is_non_struct_map", "is_number", "is_pid", "is_port", "is_reference",
    "is_struct", "is_tuple", "length", "macro_exported?", "make_ref", "map_size", "match?",
    "max", "min", "node", "not", "or", "pop_in", "put_elem", "put_in", "quote", "raise",
    "receive", "rem", "require", "reraise", "round", "self", "send", "spawn", "spawn_link",
    "spawn_monitor", "struct", "struct!", "super", "tap", "then", "throw", "tl", "to_charlist",
    "to_string", "to_timeout", "trunc", "try", "tuple_size", "unless", "unquote",
    "unquote_splicing", "update_in", "use", "var!", "with",
];
