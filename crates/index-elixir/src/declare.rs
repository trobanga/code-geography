//! The walk over the syntax tree of one file: modules, functions and references.

use crate::Report;
use crate::calls::CallSite;
use crate::quote::{self, Effect};
use crate::scope::{Aliases, Filter, Name};
use crate::sites::{self, Read};
use crate::syntax::{
    arguments, blocks, call_head, default_count, descendants, end_line_of, forms, function_head,
    is_operator, line_of, options, pair, text,
};
use cg_core::{EdgeKind, Span};
use std::collections::{BTreeMap, BTreeSet};
use tree_sitter::Node as Ts;

/// A module that a form declares.
pub struct ModuleDecl {
    /// The full name.
    pub name: String,
    /// The directory of the Mix project of the file.
    pub dir: String,
    /// The file of the form.
    pub file: String,
    /// Whether the file is test code.
    pub test: bool,
    /// Set for the module of a `defimpl`.
    pub impl_of: Option<ImplOf>,
}

/// What a `defimpl` implements, for whom.
pub struct ImplOf {
    /// The full name of the protocol.
    pub protocol: String,
    /// The full name of the type.
    pub ty: String,
    /// The line of the protocol in the `defimpl`.
    pub line: u32,
}

/// A function of a module: all clauses of one name and arity.
pub struct FunctionDecl {
    pub module: String,
    pub name: String,
    pub arity: usize,
    pub file: String,
    pub span: Span,
    pub hash: String,
    pub test: bool,
    /// The number of parameters with a default (the largest of its clauses): the function
    /// answers the arities `arity - defaults` to `arity`.
    pub defaults: usize,
    /// Whether other modules can call it: `def`, `defmacro`, `defguard`, `defdelegate`.
    pub public: bool,
    /// Whether it is a macro: `defmacro`, `defguard`.
    pub macro_: bool,
}

impl FunctionDecl {
    /// Whether a call with `arity` arguments reaches the function.
    pub fn answers(&self, arity: usize) -> bool {
        arity <= self.arity && arity + self.defaults >= self.arity
    }
}

/// How a module names another one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefKind {
    Alias,
    Import,
    Require,
    Use,
    Behaviour,
    /// The protocol of a `defimpl`.
    Defimpl,
    /// A written type of a `defimpl`.
    For,
}

impl RefKind {
    /// The key of the kind in the report.
    pub fn name(self) -> &'static str {
        match self {
            RefKind::Alias => "alias",
            RefKind::Import => "import",
            RefKind::Require => "require",
            RefKind::Use => "use",
            RefKind::Behaviour => "behaviour",
            RefKind::Defimpl => "defimpl",
            RefKind::For => "for",
        }
    }

    /// The edge that a reference of this kind makes from the module that holds it. The
    /// edge of a `defimpl` starts at its type, not at the module that holds the form.
    pub fn edge(self) -> Option<EdgeKind> {
        match self {
            RefKind::Alias | RefKind::Import | RefKind::Require | RefKind::Use => {
                Some(EdgeKind::Imports)
            }
            RefKind::Behaviour => Some(EdgeKind::Implements),
            RefKind::Defimpl | RefKind::For => None,
        }
    }

    /// The directive that a call head names.
    pub fn directive(head: Option<&str>) -> Option<RefKind> {
        match head? {
            "alias" => Some(RefKind::Alias),
            "import" => Some(RefKind::Import),
            "require" => Some(RefKind::Require),
            "use" => Some(RefKind::Use),
            _ => None,
        }
    }
}

/// A place where a module names another one.
pub struct Reference {
    /// The module that holds the reference; `None` for a reference that is counted only.
    pub from: Option<String>,
    pub kind: RefKind,
    pub name: Name,
    pub file: String,
    /// 1-based.
    pub line: u32,
}

/// What the walk found, over all files.
#[derive(Default)]
pub struct Declared {
    pub modules: Vec<ModuleDecl>,
    pub functions: Vec<FunctionDecl>,
    pub references: Vec<Reference>,
    /// The calls that the bodies of modules make, resolved after all files are read.
    pub sites: Vec<CallSite>,
    /// The aliases that the `quote`s of each module can make.
    pub quotes: BTreeMap<String, Effect>,
    /// The modules whose body holds a `using` block (`ExUnit.CaseTemplate`): the block is
    /// their `__using__`.
    pub using_blocks: BTreeSet<String>,
    /// The full names of `modules`: a later form of the same name is a duplicate.
    names: BTreeSet<String>,
}

/// The file that is read.
pub struct File<'s> {
    pub source: &'s str,
    pub path: &'s str,
    pub dir: &'s str,
    pub test: bool,
}

/// The clauses of one function so far.
struct Clauses {
    name: String,
    arity: usize,
    /// The extent of each clause (with its attributes), in source order.
    extents: Vec<std::ops::Range<usize>>,
    first_line: u32,
    /// The last line of the last clause that follows the first with no clause of another
    /// function between.
    end_line: u32,
    /// Whether all clauses so far form one run.
    adjacent: bool,
    /// The largest number of defaults of a clause.
    defaults: usize,
    /// Whether the first clause is a public definition.
    public: bool,
    macro_: bool,
}

/// The body of the module that is read now.
struct Body {
    module: String,
    functions: Vec<Clauses>,
    /// The function (index in `functions`) of the last clause that was read.
    last: Option<usize>,
}

const FUNCTION_FORMS: [&str; 7] = [
    "def",
    "defp",
    "defmacro",
    "defmacrop",
    "defguard",
    "defguardp",
    "defdelegate",
];

/// The attributes that belong to the clause that follows them.
const BOUND_ATTRIBUTES: [&str; 4] = ["doc", "spec", "impl", "deprecated"];

/// The attributes that hold types: `String.t()` in them is no call.
const TYPESPEC_ATTRIBUTES: [&str; 6] = [
    "type",
    "typep",
    "opaque",
    "spec",
    "callback",
    "macrocallback",
];

/// The name of a module attribute form (`@doc "x"` gives `doc`).
fn attribute<'a>(form: Ts, source: &'a str) -> Option<&'a str> {
    if form.kind() != "unary_operator"
        || form
            .child_by_field_name("operator")
            .is_none_or(|op| text(op, source) != "@")
    {
        return None;
    }
    call_head(form.child_by_field_name("operand")?, source)
}

/// Whether `node` is `left |> right`.
fn is_pipe(node: Ts, source: &str) -> bool {
    node.child_by_field_name("operator")
        .is_some_and(|op| text(op, source) == "|>")
}

/// The `name: arity` pairs of a literal keyword list (`[f: 1, g: 2]`).
fn arities(list: Ts, source: &str) -> Option<Vec<(String, usize)>> {
    if list.kind() != "list" {
        return None;
    }
    let groups: Option<Vec<_>> = forms(list)
        .into_iter()
        .map(|keywords| keyword_arities(keywords, source))
        .collect();
    groups.map(|groups| groups.into_iter().flatten().collect())
}

fn keyword_arities(keywords: Ts, source: &str) -> Option<Vec<(String, usize)>> {
    if keywords.kind() != "keywords" {
        return None;
    }
    forms(keywords)
        .into_iter()
        .map(|entry| arity_entry(entry, source))
        .collect()
}

fn arity_entry(entry: Ts, source: &str) -> Option<(String, usize)> {
    let (name, value) = pair(entry, source)?;
    if value.kind() != "integer" {
        return None;
    }
    Some((name.to_string(), text(value, source).parse().ok()?))
}

/// What the `only:` and `except:` of an `import` let through.
fn import_filter(form: Ts, source: &str) -> Filter {
    let options = options(form, source);
    let find = |key: &str| options.iter().find(|(k, _)| *k == key).map(|(_, v)| *v);
    match (find("only"), find("except")) {
        (None, None) => Filter::All,
        (Some(only), None) => only_filter(only, source),
        (None, Some(except)) => arities(except, source).map_or(Filter::Unknown, Filter::Except),
        (Some(_), Some(_)) => Filter::Unknown,
    }
}

fn only_filter(only: Ts, source: &str) -> Filter {
    match text(only, source) {
        ":functions" => Filter::OnlyKind { macros: false },
        ":macros" => Filter::OnlyKind { macros: true },
        _ => arities(only, source).map_or(Filter::Unknown, Filter::Only),
    }
}

/// The alias that `as: Name` of a directive makes.
pub fn as_name<'a>(form: Ts, source: &'a str) -> Option<&'a str> {
    options(form, source)
        .into_iter()
        .find(|(key, value)| *key == "as" && value.kind() == "alias")
        .map(|(_, value)| text(value, source))
}

/// The last segments of the module names that the arguments of a `use` write, not counting
/// the first argument.
pub fn written_segments(form: Ts, source: &str) -> Vec<String> {
    arguments(form)
        .into_iter()
        .skip(1)
        .flat_map(descendants)
        .filter(|n| n.kind() == "alias")
        .filter_map(|n| text(n, source).rsplit('.').next().map(str::to_string))
        .collect()
}

/// The prefix and the entries of a group `A.{B, C}`.
pub fn group(argument: Ts) -> Option<(Ts, Ts)> {
    if argument.kind() != "dot" {
        return None;
    }
    let entries = argument.child_by_field_name("right")?;
    (entries.kind() == "tuple").then_some((argument.child_by_field_name("left")?, entries))
}

/// The modules that the first argument of a directive names, each with its line: one for
/// `A.B`, one per entry for `A.{B, C.D}`.
pub fn entries(
    argument: Ts,
    aliases: &Aliases,
    source: &str,
    current: Option<&str>,
) -> Vec<(Name, u32)> {
    let Some((left, entries)) = group(argument) else {
        return vec![(aliases.expand(argument, source, current), line_of(argument))];
    };
    let prefix = aliases.expand(left, source, current);
    forms(entries)
        .into_iter()
        .map(|entry| {
            let name = match &prefix {
                Name::Module(prefix) if entry.kind() == "alias" => {
                    Name::Module(format!("{prefix}.{}", text(entry, source)))
                }
                _ => Name::Dynamic,
            };
            (name, line_of(entry))
        })
        .collect()
}

pub struct Walk<'a, 's> {
    file: &'a File<'s>,
    out: &'a mut Declared,
    report: &'a mut Report,
    stack: Vec<Body>,
    /// The name and the arity of the function whose clause is read now.
    function: Option<(String, usize)>,
}

impl<'a, 's> Walk<'a, 's> {
    pub fn new(file: &'a File<'s>, out: &'a mut Declared, report: &'a mut Report) -> Self {
        Walk {
            file,
            out,
            report,
            stack: Vec::new(),
            function: None,
        }
    }

    /// Reads the forms at the top level of the file.
    pub fn read(&mut self, root: Ts) {
        let mut aliases = Aliases::default();
        self.level(&forms(root), &mut aliases);
    }

    /// Reads forms at a place of definitions: the top level of a file, the body of a module,
    /// a block of an `if` or `unless` there.
    fn level(&mut self, forms: &[Ts], aliases: &mut Aliases) {
        let source = self.file.source;
        // The first of the attributes directly above the form that is read next.
        let mut above: Option<Ts> = None;
        for &form in forms {
            let named = attribute(form, source);
            if named.is_some_and(|a| BOUND_ATTRIBUTES.contains(&a)) {
                above.get_or_insert(form);
                continue;
            }
            let above = above.take();
            if named == Some("behaviour") {
                self.behaviour(form, aliases);
                continue;
            }
            self.place_form(form, above, aliases);
        }
    }

    /// Reads one form at a place of definitions; `above` is the first attribute bound to it.
    fn place_form(&mut self, form: Ts, above: Option<Ts>, aliases: &mut Aliases) {
        match call_head(form, self.file.source) {
            Some(head @ ("defmodule" | "defprotocol")) => self.module_form(form, head, aliases),
            Some("defimpl") => self.impl_form(form, aliases),
            Some("if" | "unless") => self.conditional(form, aliases),
            Some(head) if FUNCTION_FORMS.contains(&head) && !self.stack.is_empty() => {
                self.function_clause(form, head, above, aliases);
            }
            head => self.other_form(form, head, aliases),
        }
    }

    /// An `if` or `unless` at a place of definitions: its blocks are places too.
    fn conditional(&mut self, form: Ts, aliases: &mut Aliases) {
        // The condition is read like any form of the body.
        self.call_site(form, aliases, false);
        for condition in arguments(form).into_iter().take(1) {
            self.scan(condition, aliases);
        }
        for block in blocks(form, self.file.source) {
            let mark = aliases.mark();
            self.level(&block, aliases);
            aliases.end_block(mark);
        }
    }

    /// A `def`, `defp`, `defmacro`, `defmacrop` or `defdelegate` in a module.
    fn function_clause(&mut self, form: Ts, head: &str, above: Option<Ts>, aliases: &mut Aliases) {
        self.function = self.function_form(form, head, above);
        if let Some((name, arity)) = self.function.clone().filter(|_| head == "defdelegate") {
            let current = self.current();
            let source = self.file.source;
            let read = sites::delegate(form, &name, arity, source, aliases, current.as_deref());
            self.record(read);
        }
        let mark = aliases.mark();
        self.scan_function(form, aliases);
        aliases.end_block(mark);
        self.function = None;
    }

    /// Any other form of a place of definitions.
    fn other_form(&mut self, form: Ts, head: Option<&str>, aliases: &mut Aliases) {
        if let (Some("using"), Some(module)) = (head, self.current()) {
            self.out.using_blocks.insert(module);
        }
        let directive = RefKind::directive(head).is_some();
        if form.kind() == "call" && !directive && !self.stack.is_empty() {
            self.count_without_node(head.unwrap_or("other"));
        }
        self.scan(form, aliases);
    }

    /// `@behaviour B` in the body of a module.
    fn behaviour(&mut self, form: Ts, aliases: &Aliases) {
        let Some(from) = self.current() else {
            return;
        };
        let Some(name) = form
            .child_by_field_name("operand")
            .and_then(|call| arguments(call).first().copied())
        else {
            return;
        };
        let module = aliases.expand(name, self.file.source, Some(&from));
        self.refer(Some(from), RefKind::Behaviour, module, line_of(name));
    }

    /// Reads what can stand anywhere in a module: the directives. A block ends the aliases
    /// that are made in it.
    fn scan(&mut self, node: Ts, aliases: &mut Aliases) {
        if !self.scan_special(node, aliases) {
            self.scan_children(node, aliases);
        }
    }

    /// Reads a node of a kind with a rule of its own; whether it is read whole (else its
    /// children are read, too).
    fn scan_special(&mut self, node: Ts, aliases: &mut Aliases) -> bool {
        match node.kind() {
            "call" => self.scan_call(node, aliases),
            "unary_operator" => self.scan_unary(node, aliases),
            "binary_operator" => self.scan_pipe(node, aliases),
            "bitstring" => {
                self.scan_bitstring(node, aliases);
                true
            }
            "do_block" => {
                self.scan_do_block(node, aliases);
                true
            }
            "stab_clause" => {
                let mark = aliases.mark();
                self.scan_children(node, aliases);
                aliases.end_block(mark);
                true
            }
            _ => false,
        }
    }

    /// A call node; whether it is read whole (else its children are read, too).
    fn scan_call(&mut self, node: Ts, aliases: &mut Aliases) -> bool {
        let head = call_head(node, self.file.source);
        // What a `quote` holds is code of another module; a nested module
        // that is not read at a place of modules is not read at all.
        if head == Some("quote") {
            if let Some(module) = self.current() {
                let effect = quote::read(node, self.file.source, aliases);
                self.out.quotes.entry(module).or_default().merge(effect);
            }
            return true;
        }
        if matches!(head, Some("defmodule" | "defprotocol" | "defimpl")) {
            return true;
        }
        if let Some(kind) = RefKind::directive(head) {
            self.directive(node, kind, aliases);
            return true;
        }
        self.call_site(node, aliases, false);
        false
    }

    /// A unary operator: a capture or an attribute; whether it is read whole.
    fn scan_unary(&mut self, node: Ts, aliases: &mut Aliases) -> bool {
        let current = self.current();
        let read = sites::capture(node, self.file.source, aliases, current.as_deref());
        if read.is_some() {
            self.record(read);
            return true;
        }
        // `@name value`: the value is read, the attribute is no call; a typespec
        // holds types.
        let Some(name) = attribute(node, self.file.source) else {
            return false;
        };
        if !TYPESPEC_ATTRIBUTES.contains(&name)
            && let Some(operand) = node.child_by_field_name("operand")
        {
            self.scan_children(operand, aliases);
        }
        true
    }

    /// `left |> right`; whether `node` is one (else nothing is read).
    fn scan_pipe(&mut self, node: Ts, aliases: &mut Aliases) -> bool {
        if !is_pipe(node, self.file.source) {
            return false;
        }
        let (Some(left), Some(right)) = (
            node.child_by_field_name("left"),
            node.child_by_field_name("right"),
        ) else {
            return true;
        };
        self.scan(left, aliases);
        if right.kind() == "call" {
            self.call_site(right, aliases, true);
            self.scan_children(right, aliases);
        } else {
            self.record(sites::piped_name(right, self.file.source, aliases));
            self.scan(right, aliases);
        }
        true
    }

    /// `<<value::modifiers, ..>>`: the value is read, the modifiers (`size(8)`,
    /// `binary-size(n)`, `unit(8)`) are no calls.
    fn scan_bitstring(&mut self, node: Ts, aliases: &mut Aliases) {
        let source = self.file.source;
        let mut cursor = node.walk();
        for segment in node.named_children(&mut cursor) {
            let typed = segment.kind() == "binary_operator" && is_operator(segment, source, "::");
            match segment.child_by_field_name("left") {
                Some(value) if typed => self.scan(value, aliases),
                _ => self.scan(segment, aliases),
            }
        }
    }

    fn scan_do_block(&mut self, node: Ts, aliases: &mut Aliases) {
        let mark = aliases.mark();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            // The `else` and similar blocks follow the forms of the `do` and start
            // with the aliases above it.
            if child.kind().ends_with("_block") {
                aliases.end_block(mark);
            }
            self.scan(child, aliases);
        }
        aliases.end_block(mark);
    }

    /// Reads a function form: its body, its guards and the default values of its head. The
    /// form and its head are no calls.
    fn scan_function(&mut self, form: Ts, aliases: &mut Aliases) {
        let mut cursor = form.walk();
        for child in form.named_children(&mut cursor) {
            if child.kind() != "arguments" {
                self.scan(child, aliases);
                continue;
            }
            for (index, argument) in forms(child).into_iter().enumerate() {
                if index == 0 {
                    self.scan_head(argument, aliases);
                } else {
                    self.scan(argument, aliases);
                }
            }
        }
    }

    /// The head of a function form: `f(a, b \\ 1)`, with guards `when g(a)`.
    fn scan_head(&mut self, head: Ts, aliases: &mut Aliases) {
        let guarded = head.kind() == "binary_operator"
            && head
                .child_by_field_name("operator")
                .is_some_and(|op| text(op, self.file.source) == "when");
        if !guarded {
            self.scan_children(head, aliases);
            return;
        }
        if let Some(left) = head.child_by_field_name("left") {
            self.scan_head(left, aliases);
        }
        if let Some(guard) = head.child_by_field_name("right") {
            self.scan(guard, aliases);
        }
    }

    /// Records the call that `node` makes, if it is one; `piped` for the right side of `|>`.
    fn call_site(&mut self, node: Ts, aliases: &Aliases, piped: bool) {
        let current = self.current();
        let read = sites::call(node, self.file.source, aliases, current.as_deref(), piped);
        self.record(read);
    }

    /// Records a call site of the module that is read now.
    fn record(&mut self, read: Option<Read>) {
        let Some(read) = read else {
            return;
        };
        let Some(module) = self.current() else {
            self.report.calls_outside_module += 1;
            return;
        };
        self.out.sites.push(CallSite {
            module,
            function: self.function.clone(),
            call: read.call,
            arity: read.arity,
            value: read.value,
            test: self.file.test,
            file: self.file.path.to_string(),
            line: read.line,
        });
    }

    fn scan_children(&mut self, node: Ts, aliases: &mut Aliases) {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            self.scan(child, aliases);
        }
    }

    /// `alias`, `import`, `require` and `use`: a reference of the module that holds it, and
    /// maybe an alias.
    fn directive(&mut self, form: Ts, kind: RefKind, aliases: &mut Aliases) {
        let source = self.file.source;
        let current = self.current();
        let Some(first) = arguments(form).first().copied() else {
            return;
        };
        let named = as_name(form, source);
        let grouped = group(first).is_some();
        for (name, line) in entries(first, aliases, source, current.as_deref()) {
            if let Name::Module(full) = &name {
                let short = alias_short(kind, named, grouped, full);
                if let Some(short) = short {
                    aliases.insert(short, full);
                }
                if kind == RefKind::Import {
                    aliases.insert_import(full, import_filter(form, source));
                }
            }
            if kind == RefKind::Use {
                aliases.insert_use(name.clone(), written_segments(form, source));
            }
            if current.is_some() {
                self.refer(current.clone(), kind, name, line);
            }
        }
    }

    /// The module that is read now.
    fn current(&self) -> Option<String> {
        self.stack.last().map(|b| b.module.clone())
    }

    fn count_without_node(&mut self, head: &str) {
        *self
            .report
            .forms_without_node
            .entry(head.to_string())
            .or_default() += 1;
    }

    /// The full name that a `defmodule` declares, and the alias that it makes for the
    /// module around it: `defmodule Inner` in `A` is `A.Inner`, and `Inner` stands for
    /// `A.Inner` in the rest of `A`.
    fn declared_name(
        &self,
        name: Ts,
        aliases: &Aliases,
    ) -> Option<(String, Option<(String, String)>)> {
        let source = self.file.source;
        let outer = self.current();
        let written = text(name, source);
        match &outer {
            Some(outer) if name.kind() == "alias" && !written.starts_with("Elixir.") => {
                let first = written.split('.').next()?;
                Some((
                    format!("{outer}.{written}"),
                    Some((first.to_string(), format!("{outer}.{first}"))),
                ))
            }
            _ => match aliases.expand(name, source, outer.as_deref()) {
                Name::Module(full) => Some((full, None)),
                _ => None,
            },
        }
    }

    /// Whether this form is the first that declares the module `full`. A later form is
    /// listed as a duplicate.
    fn claim(&mut self, full: &str, form: Ts) -> bool {
        let first = self.out.names.insert(full.to_string());
        if !first {
            self.report
                .duplicate_modules
                .push(format!("{}:{}", self.file.path, line_of(form)));
        }
        first
    }

    fn module_form(&mut self, form: Ts, keyword: &str, aliases: &mut Aliases) {
        let Some((full, alias)) = arguments(form)
            .first()
            .and_then(|name| self.declared_name(*name, aliases))
        else {
            self.count_without_node(keyword);
            return;
        };
        if !self.claim(&full, form) {
            return;
        }
        if let Some((short, long)) = alias {
            aliases.insert(&short, &long);
        }
        self.read_module(form, full, None, aliases);
    }

    /// `defimpl P, for: T` declares the module `P.T`, one per entry if `for:` is a list.
    fn impl_form(&mut self, form: Ts, aliases: &mut Aliases) {
        let source = self.file.source;
        let current = self.current();
        let Some(written) = arguments(form).first().copied() else {
            self.count_without_node("defimpl");
            return;
        };
        let protocol = aliases.expand(written, source, current.as_deref());
        let protocol_line = line_of(written);
        let types = impl_types(form, aliases, source, current);
        if types.is_empty() {
            self.refer(None, RefKind::Defimpl, protocol.clone(), protocol_line);
            self.count_without_node("defimpl");
        }
        for (ty, ty_line) in types {
            let module = impl_module(&protocol, &ty, protocol_line);
            if module
                .as_ref()
                .is_some_and(|(full, _)| !self.claim(full, form))
            {
                continue;
            }
            self.refer(None, RefKind::Defimpl, protocol.clone(), protocol_line);
            if let Some(line) = ty_line {
                self.refer(None, RefKind::For, ty, line);
            }
            match module {
                Some((full, of)) => self.read_module(form, full, Some(of), aliases),
                None => self.count_without_node("defimpl"),
            }
        }
    }

    fn refer(&mut self, from: Option<String>, kind: RefKind, name: Name, line: u32) {
        self.out.references.push(Reference {
            from,
            kind,
            name,
            file: self.file.path.to_string(),
            line,
        });
    }

    /// Declares the module `full` and reads the body of `form` as its body. The aliases
    /// that the body makes end with it.
    fn read_module(&mut self, form: Ts, full: String, of: Option<ImplOf>, aliases: &mut Aliases) {
        self.out.modules.push(ModuleDecl {
            name: full.clone(),
            dir: self.file.dir.to_string(),
            file: self.file.path.to_string(),
            test: self.file.test,
            impl_of: of,
        });
        self.stack.push(Body {
            module: full,
            functions: Vec::new(),
            last: None,
        });
        let body = blocks(form, self.file.source)
            .into_iter()
            .next()
            .unwrap_or_default();
        let mark = aliases.mark();
        self.level(&body, aliases);
        aliases.end_block(mark);
        let body = self.stack.pop().expect("the body was pushed above");
        self.flush(body);
    }

    /// One clause of a function; `above` is the first of the attributes that belong to it.
    fn function_form(
        &mut self,
        form: Ts,
        keyword: &str,
        above: Option<Ts>,
    ) -> Option<(String, usize)> {
        let head = arguments(form).first().copied();
        let Some((name, arity)) = head.and_then(|head| function_head(head, self.file.source))
        else {
            self.count_without_node(keyword);
            return None;
        };
        let defaults = head.map_or(0, |head| default_count(head, self.file.source));
        let start = above.unwrap_or(form);
        let body = self.stack.last_mut().expect("a module is open");
        let known = body
            .functions
            .iter()
            .position(|c| c.name == name && c.arity == arity);
        let index = known.unwrap_or(body.functions.len());
        if known.is_none() {
            body.functions.push(Clauses {
                name: name.to_string(),
                arity,
                extents: Vec::new(),
                first_line: line_of(start),
                end_line: 0,
                adjacent: true,
                defaults: 0,
                public: !keyword.ends_with('p'),
                macro_: matches!(keyword, "defmacro" | "defmacrop" | "defguard" | "defguardp"),
            });
        }
        // A clause after a clause of another function is not part of the span.
        let follows = known.is_none() || body.last == Some(index);
        body.last = Some(index);
        let clauses = &mut body.functions[index];
        clauses.extents.push(start.start_byte()..form.end_byte());
        clauses.adjacent &= follows;
        clauses.defaults = clauses.defaults.max(defaults);
        if clauses.adjacent {
            clauses.end_line = end_line_of(form);
        }
        Some((name.to_string(), arity))
    }

    fn flush(&mut self, body: Body) {
        for clauses in body.functions {
            let own = clauses
                .extents
                .iter()
                .map(|r| &self.file.source[r.clone()])
                .collect::<Vec<_>>()
                .join("\n");
            self.out.functions.push(FunctionDecl {
                module: body.module.clone(),
                name: clauses.name,
                arity: clauses.arity,
                file: self.file.path.to_string(),
                span: Span {
                    start_line: clauses.first_line,
                    end_line: clauses.end_line,
                },
                hash: cg_core::text_hash(&own),
                test: self.file.test,
                defaults: clauses.defaults,
                public: clauses.public,
                macro_: clauses.macro_,
            });
        }
    }
}

/// The types that a `defimpl` is for: the entries of `for:`, else the module around it.
fn impl_types(
    form: Ts,
    aliases: &Aliases,
    source: &str,
    current: Option<String>,
) -> Vec<(Name, Option<u32>)> {
    let Some((_, value)) = options(form, source)
        .into_iter()
        .find(|(key, _)| *key == "for")
    else {
        return current
            .into_iter()
            .map(|c| (Name::Module(c), None))
            .collect();
    };
    let entries = if value.kind() == "list" {
        forms(value)
    } else {
        vec![value]
    };
    entries
        .into_iter()
        .map(|t| {
            (
                aliases.expand(t, source, current.as_deref()),
                Some(line_of(t)),
            )
        })
        .collect()
}

/// The module `P.T` of an implementation, if both names are written modules.
fn impl_module(protocol: &Name, ty: &Name, line: u32) -> Option<(String, ImplOf)> {
    match (protocol, ty) {
        (Name::Module(p), Name::Module(t)) => Some((
            format!("{p}.{t}"),
            ImplOf {
                protocol: p.clone(),
                ty: t.clone(),
                line,
            },
        )),
        _ => None,
    }
}

/// The short name that a directive makes for the module `full`, if it makes one.
pub fn alias_short<'a>(
    kind: RefKind,
    named: Option<&'a str>,
    grouped: bool,
    full: &'a str,
) -> Option<&'a str> {
    match (kind, named) {
        (RefKind::Alias | RefKind::Require, Some(short)) if !grouped => Some(short),
        (RefKind::Alias, _) => full.rsplit('.').next(),
        _ => None,
    }
}
