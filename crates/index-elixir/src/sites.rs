//! Reading a call site from the syntax tree: what a call names, how many arguments it has,
//! and on which line. What the call resolves to is for `calls`.

use crate::calls::Call;
use crate::scope::{Aliases, Name, Use};
use crate::syntax::{arguments, is_operator, line_of, options, text};
use std::rc::Rc;
use tree_sitter::Node as Ts;

/// What a call site says.
pub struct Read {
    pub call: Call,
    pub arity: usize,
    /// A function that is named, not called (`&f/1`).
    pub value: bool,
    /// 1-based.
    pub line: u32,
}

/// The number of arguments of a call as written: a `do` block is one more.
fn arity(node: Ts) -> usize {
    let mut cursor = node.walk();
    let block = node
        .named_children(&mut cursor)
        .any(|c| c.kind() == "do_block");
    arguments(node).len() + usize::from(block)
}

/// The call that `node` makes, if it is a call with a written target. `piped` is set for the
/// right side of a `|>`, which has one more argument.
pub fn call(
    node: Ts,
    source: &str,
    aliases: &Aliases,
    current: Option<&str>,
    piped: bool,
) -> Option<Read> {
    let target = node.child_by_field_name("target")?;
    let arity = arity(node) + usize::from(piped);
    named(
        target,
        arity,
        false,
        arguments_of(node).is_some(),
        source,
        aliases,
        current,
    )
}

/// The call that `target` names: a bare name or `left.name`.
fn named(
    target: Ts,
    arity: usize,
    value: bool,
    arguments: bool,
    source: &str,
    aliases: &Aliases,
    current: Option<&str>,
) -> Option<Read> {
    if target.kind() == "identifier" {
        return Some(Read {
            call: Call::Local {
                name: text(target, source).to_string(),
                imports: aliases.imports(),
            },
            arity,
            value,
            line: line_of(target),
        });
    }
    let left = target.child_by_field_name("left")?;
    let name = target.child_by_field_name("right")?;
    let module = module_before_dot(left, arguments, source, aliases, current)?;
    Some(Read {
        call: module.call(text(name, source)),
        arity,
        value,
        line: line_of(name),
    })
}

/// The module that stands before the dot of a remote call. Something that is no written
/// module (a variable, a chain, an attribute, a result) is a dynamic module if the call has
/// arguments, else a field (`map.field`, `socket.assigns.user`, `@a.field`): no call.
fn module_before_dot(
    left: Ts,
    arguments: bool,
    source: &str,
    aliases: &Aliases,
    current: Option<&str>,
) -> Option<Module> {
    let module = Module::written(left, source, aliases, current);
    (arguments || module.name != Name::Dynamic).then_some(module)
}

/// The module that a remote call names, with what the `use` rule needs.
struct Module {
    name: Name,
    /// The first segment of the name as written, for an alias.
    first: String,
    /// Whether an alias expands the first segment.
    aliased: bool,
    /// The `use`s that can have made an alias of the first segment.
    uses: Vec<Rc<Use>>,
}

impl Module {
    fn of(name: Name) -> Module {
        Module {
            name,
            first: String::new(),
            aliased: false,
            uses: Vec::new(),
        }
    }

    /// The module that `node`, written in a call or after `to:`, stands for.
    fn written(node: Ts, source: &str, aliases: &Aliases, current: Option<&str>) -> Module {
        let name = aliases.expand(node, source, current);
        if node.kind() != "alias" {
            return Module::of(name);
        }
        let written = text(node, source);
        let (aliased, uses) = aliases.uses_after(written);
        Module {
            name,
            first: written.split('.').next().unwrap_or(written).to_string(),
            aliased,
            uses,
        }
    }

    fn call(self, function: &str) -> Call {
        Call::Remote {
            name: function.to_string(),
            module: self.name,
            first: self.first,
            aliased: self.aliased,
            uses: self.uses,
        }
    }
}

/// The call that `defdelegate name(..), to: M, as: :other` makes, at the line of `M`: the
/// function `other` (else `name`) of `M` with the arity of the head.
pub fn delegate(
    form: Ts,
    name: &str,
    arity: usize,
    source: &str,
    aliases: &Aliases,
    current: Option<&str>,
) -> Option<Read> {
    let options = options(form, source);
    let find = |key: &str| options.iter().find(|(k, _)| *k == key).map(|(_, v)| *v);
    let to = find("to")?;
    let function = match find("as") {
        None => Some(name),
        Some(other) if other.kind() == "atom" => text(other, source).strip_prefix(':'),
        Some(_) => None,
    };
    let call = match function {
        Some(function) => Module::written(to, source, aliases, current).call(function),
        None => Module::of(Name::Dynamic).call(name),
    };
    Some(Read {
        call,
        arity,
        value: false,
        line: line_of(to),
    })
}

/// The function value that a capture `&f/1` or `&Mod.f/2` names, if `node` is one.
pub fn capture(node: Ts, source: &str, aliases: &Aliases, current: Option<&str>) -> Option<Read> {
    let operand = slash_operand(node, source)?;
    let arity = integer(operand.child_by_field_name("right")?, source)?;
    let target = capture_target(operand.child_by_field_name("left")?)?;
    named(target, arity, true, true, source, aliases, current)
}

/// The `f/1` of a capture `&f/1`, if `node` is one.
fn slash_operand<'t>(node: Ts<'t>, source: &str) -> Option<Ts<'t>> {
    if node.kind() != "unary_operator" || !is_operator(node, source, "&") {
        return None;
    }
    let operand = node.child_by_field_name("operand")?;
    (operand.kind() == "binary_operator" && is_operator(operand, source, "/")).then_some(operand)
}

fn integer(node: Ts, source: &str) -> Option<usize> {
    if node.kind() != "integer" {
        return None;
    }
    text(node, source).parse().ok()
}

/// The name that the left side of a capture names: a bare name or `Mod.f` without arguments.
fn capture_target(left: Ts) -> Option<Ts> {
    match left.kind() {
        "identifier" => Some(left),
        "call" if arguments_of(left).is_none() => left.child_by_field_name("target"),
        _ => None,
    }
}

/// Whether the call has an `arguments` node: `Mod.f()` has one, `Mod.f` has none.
fn arguments_of(call: Ts) -> Option<Ts> {
    let mut cursor = call.walk();
    call.named_children(&mut cursor)
        .find(|c| c.kind() == "arguments")
}

/// The call that the right side of a `|>` makes if it is a bare name: `x |> f`.
pub fn piped_name(node: Ts, source: &str, aliases: &Aliases) -> Option<Read> {
    (node.kind() == "identifier").then(|| Read {
        call: Call::Local {
            name: text(node, source).to_string(),
            imports: aliases.imports(),
        },
        arity: 1,
        value: false,
        line: line_of(node),
    })
}
