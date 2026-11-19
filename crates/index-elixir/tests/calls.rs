//! The call edges of the Elixir front end (ADR 0037).

use cg_core::{CallReport, EdgeKind, Graph};
use cg_index_elixir::{Report, SourceFile, UnresolvedCalls, index_sources};
use std::collections::BTreeMap;

const MIX: (&str, &str) = ("mix.exs", "def project, do: [app: :demo]\n");

fn run(files: &[(&str, &str)]) -> (Graph, Report) {
    let files: Vec<SourceFile> = files
        .iter()
        .map(|(path, text)| SourceFile {
            path: path.to_string(),
            text: text.to_string(),
        })
        .collect();
    index_sources("repo", &files)
}

fn run_lib(text: &str) -> (Graph, Report) {
    run(&[MIX, ("lib/a.ex", text)])
}

fn calls(g: &Graph) -> Vec<(&str, &str)> {
    let mut found: Vec<(&str, &str)> = g
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Calls)
        .map(|e| (e.from.as_str(), e.to.as_str()))
        .collect();
    found.sort();
    found
}

/// The lines of the sites of the edge `from` to `to`.
fn lines(g: &Graph, from: &str, to: &str) -> Vec<u32> {
    g.edge(from, to, EdgeKind::Calls)
        .unwrap_or_else(|| panic!("no edge {from} -> {to}; edges: {:?}", g.edges))
        .sites
        .iter()
        .flat_map(|s| s.lines.clone())
        .collect()
}

#[test]
fn a_remote_call_is_an_edge_from_the_calling_function_to_the_function_with_the_line_of_the_call() {
    let (g, report) = run_lib(
        "defmodule B do
  def g(x), do: x
end
defmodule A do
  def f(x) do
    y = 1
    B.g(x)
  end
end
",
    );

    assert_eq!(calls(&g), [("fn:A.f/1", "fn:B.g/1")]);
    assert_eq!(lines(&g, "fn:A.f/1", "fn:B.g/1"), [7]);
    assert_eq!(report.calls.to_function, 1);
    assert_eq!(report.calls.unresolved, 0);
}

#[test]
fn the_module_of_a_remote_call_is_expanded_by_the_aliases_and_module_is_the_own_module() {
    let (g, _) = run_lib(
        "defmodule X.B do
  def g, do: 1
end
defmodule A do
  alias X.B
  def f do
    B.g()
  end
  def h, do: __MODULE__.k()
  def k, do: 2
end
",
    );

    assert_eq!(
        calls(&g),
        [("fn:A.f/0", "fn:X.B.g/0"), ("fn:A.h/0", "fn:A.k/0")]
    );
}

#[test]
fn the_right_side_of_a_pipe_has_one_more_argument() {
    let (g, _) = run_lib(
        "defmodule B do
  def g(a, b), do: a + b
  def one(a), do: a
end
defmodule A do
  def f(x, y) do
    x |> B.g(y) |> B.one
  end
end
",
    );

    assert_eq!(
        calls(&g),
        [("fn:A.f/2", "fn:B.g/2"), ("fn:A.f/2", "fn:B.one/1")]
    );
}

#[test]
fn a_call_without_parentheses_and_trailing_keywords_count_their_arguments_as_written() {
    let (g, _) = run_lib(
        "defmodule B do
  def two(a, b), do: a + b
  def opts(a, o), do: {a, o}
end
defmodule A do
  def f do
    B.two 1, 2
    B.opts(1, a: 2, b: 3)
  end
end
",
    );

    assert_eq!(
        calls(&g),
        [("fn:A.f/0", "fn:B.opts/2"), ("fn:A.f/0", "fn:B.two/2")]
    );
}

#[test]
fn a_function_with_default_arguments_answers_every_arity_from_the_required_to_the_written() {
    let (g, _) = run_lib(
        "defmodule B do
  def g(a, b \\\\ 1, c \\\\ 2), do: a + b + c
  def h(a, b), do: a + b
  def h(a, b, c) when c > 1, do: a + b
end
defmodule A do
  def one, do: B.g(1)
  def two, do: B.g(1, 2)
  def three, do: B.g(1, 2, 3)
  def none, do: B.g()
  def four, do: B.g(1, 2, 3, 4)
  def h2, do: B.h(1, 2)
  def h3, do: B.h(1, 2, 3)
end
",
    );

    assert_eq!(
        calls(&g),
        [
            // Outside the arities of `g/3`: the call proves the module only.
            ("fn:A.four/0", "pkg::B"),
            ("fn:A.h2/0", "fn:B.h/2"),
            ("fn:A.h3/0", "fn:B.h/3"),
            ("fn:A.none/0", "pkg::B"),
            ("fn:A.one/0", "fn:B.g/3"),
            ("fn:A.three/0", "fn:B.g/3"),
            ("fn:A.two/0", "fn:B.g/3"),
        ]
    );
}

#[test]
fn a_local_call_reaches_a_function_of_the_module_also_a_private_one_and_a_recursion_has_no_edge() {
    let (g, report) = run_lib(
        "defmodule A do
  def f(x), do: g(x)
  defp g(x), do: h(x, 1)
  def h(x, d) do
    h(x - 1, d)
  end
  def p(x), do: x |> g
  def to_string(x), do: x
  def k(x), do: to_string(x)
  def k2(x), do: x |> to_string()
end
",
    );

    assert_eq!(
        calls(&g),
        [
            ("fn:A.f/1", "fn:A.g/1"),
            ("fn:A.g/1", "fn:A.h/2"),
            ("fn:A.k/1", "fn:A.to_string/1"),
            ("fn:A.k2/1", "fn:A.to_string/1"),
            ("fn:A.p/1", "fn:A.g/1"),
        ]
    );
    assert_eq!(lines(&g, "fn:A.f/1", "fn:A.g/1"), [2]);
    assert_eq!(report.calls.to_function, 6);
}

#[test]
fn a_capture_with_an_arity_is_a_function_value_and_a_capture_of_a_call_is_a_call() {
    let (g, report) = run_lib(
        "defmodule B do
  def g, do: 0
  def g(a, b), do: a + b
end
defmodule A do
  def remote, do: Enum.map([1], &B.g/2)
  def local, do: Enum.map([1], &h/1)
  def call, do: Enum.map([1], &B.g(&1, 3))
  def h(x), do: x
end
",
    );

    assert_eq!(
        calls(&g),
        [
            ("fn:A.call/0", "fn:B.g/2"),
            ("fn:A.local/0", "fn:A.h/1"),
            ("fn:A.remote/0", "fn:B.g/2"),
        ]
    );
    assert_eq!(report.calls.to_function, 3);
    assert_eq!(report.function_values, 2);
}

#[test]
fn a_remote_call_of_a_name_the_module_does_not_define_publicly_is_an_edge_to_its_package() {
    let (g, report) = run_lib(
        "defmodule B do
  def g(x), do: x
  defp p, do: 1
end
defmodule A do
  def missing, do: B.x()
  def arity, do: B.g()
  def private, do: B.p()
end
",
    );

    assert_eq!(
        calls(&g),
        [
            ("fn:A.arity/0", "pkg::B"),
            ("fn:A.missing/0", "pkg::B"),
            ("fn:A.private/0", "pkg::B"),
        ]
    );
    assert_eq!(lines(&g, "fn:A.missing/0", "pkg::B"), [6]);
    assert_eq!(report.calls.to_type, 3);
    assert_eq!(report.calls.to_function, 0);
    assert_eq!(
        report.undeclared_functions,
        BTreeMap::from([
            ("B.g/0".to_string(), 1),
            ("B.p/0".to_string(), 1),
            ("B.x/0".to_string(), 1),
        ])
    );
}

#[test]
fn calls_outside_the_repository_are_external_and_an_unknown_receiver_is_unresolved() {
    let (g, report) = run_lib(
        "defmodule A do
  def a, do: Enum.to_list(1)
  def b, do: :lists.reverse([])
  def c(x), do: if(x, do: to_string(x), else: apply(x, :f, []))
  def d(m), do: m.fun()
  def e(m), do: m.field
  def f(fun), do: fun.(1)
  def g(x), do: x
  def h, do: A.Missing.f()
end
",
    );

    assert_eq!(calls(&g), []);
    assert_eq!(
        report.calls,
        CallReport {
            external: 5,
            unresolved: 2,
            ..CallReport::default()
        }
    );
    assert_eq!(
        report.unresolved_calls,
        UnresolvedCalls {
            dynamic: 1,
            below_declared: 1,
            ..UnresolvedCalls::default()
        }
    );
}

#[test]
fn a_local_call_that_no_function_of_the_module_answers_reaches_the_function_that_an_import_brings()
{
    let (g, report) = run_lib(
        "defmodule B do
  def g(x), do: x
  def h(x), do: x
end
defmodule A do
  import B
  def f(x), do: g(x)
  def unknown(x), do: nobody(x)
end
",
    );

    assert_eq!(calls(&g), [("fn:A.f/1", "fn:B.g/1")]);
    assert_eq!(lines(&g, "fn:A.f/1", "fn:B.g/1"), [7]);
    assert_eq!(report.calls.to_function, 1);
    assert_eq!(report.calls.unresolved, 1);
    assert_eq!(report.unresolved_calls.local_unknown, 1);
}

#[test]
fn an_import_brings_what_only_and_except_let_through_by_name_arity_and_kind() {
    let (g, report) = run_lib(
        "defmodule B do
  def g(x), do: x
  def h(x), do: x
  def d(a, b \\\\ 1), do: a + b
  defmacro m(x), do: x
end
defmodule Only do
  import B, only: [g: 1, d: 1]
  def f(x), do: {g(x), h(x)}
  def one(x), do: d(x)
  def two(x), do: d(x, 1)
end
defmodule Except do
  import B, except: [g: 1]
  def f(x), do: {g(x), h(x)}
end
defmodule Fns do
  import B, only: :functions
  def f(x), do: {g(x), m(x)}
end
defmodule Macs do
  import B, only: :macros
  def f(x), do: {g(x), m(x)}
end
",
    );

    assert_eq!(
        calls(&g),
        [
            ("fn:Except.f/1", "fn:B.h/1"),
            ("fn:Fns.f/1", "fn:B.g/1"),
            ("fn:Macs.f/1", "fn:B.m/1"),
            ("fn:Only.f/1", "fn:B.g/1"),
            ("fn:Only.one/1", "fn:B.d/2"),
        ]
    );
    // The names that a filter does not let through, and `d/2` for `only: [d: 1]`.
    assert_eq!(report.unresolved_calls.local_unknown, 5);
}

#[test]
fn two_imports_that_answer_or_a_filter_that_is_not_a_literal_give_no_edge_and_an_import_is_lexical()
{
    let (g, report) = run_lib(
        "defmodule B do
  def g(x), do: x
end
defmodule C do
  def g(x), do: x
  def only_c(x), do: x
end
defmodule Two do
  import B
  import C
  def f(x), do: g(x)
  def c(x), do: only_c(x)
end
defmodule Same do
  import B
  import B
  def f(x), do: g(x)
end
defmodule Maybe do
  import B, only: @names
  def f(x), do: g(x)
  def none(x), do: nobody(x)
end
defmodule Outside do
  import Enum
  import B
  def f(x), do: g(x)
end
defmodule Block do
  def a(x) do
    import B
    g(x)
  end
  def b(x), do: g(x)
end
",
    );

    assert_eq!(
        calls(&g),
        [
            ("fn:Block.a/1", "fn:B.g/1"),
            ("fn:Outside.f/1", "fn:B.g/1"),
            ("fn:Same.f/1", "fn:B.g/1"),
            ("fn:Two.c/1", "fn:C.only_c/1"),
        ]
    );
    assert_eq!(
        report.unresolved_calls,
        UnresolvedCalls {
            ambiguous: 2,
            local_unknown: 2,
            ..UnresolvedCalls::default()
        }
    );
    assert_eq!(report.calls.unresolved, 4);
}

#[test]
fn a_call_outside_every_function_belongs_to_the_package_and_the_head_a_typespec_and_a_quote_make_no_call()
 {
    let (g, report) = run_lib(
        "defmodule B do
  def a, do: 1
  def d, do: 1
  def w(x), do: x
  def tst, do: 1
  def q, do: 1
end
defmodule A do
  @attr B.a()
  @spec f(String.t(), integer) :: B.t()
  @type ty :: B.t()
  @callback cb(B.t()) :: B.t()
  def f(x, y \\\\ B.d()) when B.w(x), do: x
  test \"t\" do
    B.tst()
  end
  def q, do: quote(do: B.q())
end
",
    );

    assert_eq!(
        calls(&g),
        [
            ("fn:A.f/2", "fn:B.d/0"),
            ("fn:A.f/2", "fn:B.w/1"),
            ("pkg::A", "fn:B.a/0"),
            ("pkg::A", "fn:B.tst/0"),
        ]
    );
    assert_eq!(lines(&g, "pkg::A", "fn:B.a/0"), [9]);
    // `test` is a macro of a dependency, `attr` an attribute.
    assert_eq!(
        report.calls,
        CallReport {
            to_function: 4,
            unresolved: 1,
            ..CallReport::default()
        }
    );
    assert_eq!(report.unresolved_calls.local_unknown, 1);
}

#[test]
fn a_call_at_the_top_level_of_a_file_has_no_edge_and_is_counted_apart() {
    let (g, report) = run(&[
        MIX,
        (
            "lib/a.ex",
            "defmodule B do\n  def g(x), do: x\nend\nB.g(1)\nEnum.to_list(1)\n",
        ),
    ]);

    assert_eq!(calls(&g), []);
    assert_eq!(report.calls, CallReport::default());
    assert_eq!(report.calls_outside_module, 2);
}

#[test]
fn a_defdelegate_is_a_call_of_the_function_it_names_at_the_line_of_the_module() {
    let (g, report) = run_lib(
        "defmodule B do
  def g(x), do: x
  def h(x), do: x
end
defmodule A do
  defdelegate g(a), to: B
  defdelegate k(a), to: B, as: :h
  defdelegate m(a), to: Elixir.B
end
",
    );

    assert_eq!(
        calls(&g),
        [
            ("fn:A.g/1", "fn:B.g/1"),
            ("fn:A.k/1", "fn:B.h/1"),
            ("fn:A.m/1", "pkg::B"),
        ]
    );
    assert_eq!(lines(&g, "fn:A.g/1", "fn:B.g/1"), [6]);
    assert_eq!(lines(&g, "fn:A.k/1", "fn:B.h/1"), [7]);
    assert_eq!(report.calls.to_function, 2);
    assert_eq!(report.calls.to_type, 1);
}

/// A module `Repo` of the repository, and a macro `M` whose `use` aliases `MyApp.Repo`.
const REPO_AND_USING: &str = "defmodule Repo do
  def all, do: []
end
defmodule M do
  defmacro __using__(_opts) do
    quote do
      alias MyApp.Repo
    end
  end
end
";

#[test]
fn a_use_of_a_module_of_the_repository_whose_quote_aliases_the_name_hides_the_module() {
    let (g, report) = run_lib(&format!(
        "{REPO_AND_USING}defmodule User do
  use M
  def f, do: Repo.all()
end
defmodule Other do
  def f, do: Repo.all()
end
"
    ));

    assert_eq!(calls(&g), [("fn:Other.f/0", "fn:Repo.all/0")]);
    assert_eq!(report.calls.to_function, 1);
    assert_eq!(report.calls.unresolved, 1);
    assert_eq!(report.unresolved_calls.use_alias, 1);
}

#[test]
fn an_alias_written_after_the_use_wins_and_one_written_before_it_does_not() {
    let (g, report) = run_lib(&format!(
        "{REPO_AND_USING}defmodule Other.Repo do
  def all, do: []
end
defmodule After do
  use M
  alias Other.Repo
  def f, do: Repo.all()
end
defmodule Before do
  alias Other.Repo
  use M
  def f, do: Repo.all()
end
"
    ));

    assert_eq!(calls(&g), [("fn:After.f/0", "fn:Other.Repo.all/0")]);
    assert_eq!(report.unresolved_calls.use_alias, 1);
}

#[test]
fn the_aliases_of_a_quote_are_found_in_any_function_of_the_macro_module_and_through_nested_uses() {
    let (g, report) = run_lib(
        "defmodule Repo do
  def all, do: []
end
defmodule Web do
  def controller do
    quote do
      alias MyApp.Repo
    end
  end
  defmacro __using__(which) when is_atom(which) do
    apply(__MODULE__, which, [])
  end
end
defmodule Inner do
  defmacro __using__(_) do
    quote do
      alias Some.Repo
    end
  end
end
defmodule Outer do
  defmacro __using__(_) do
    quote do
      use Inner
    end
  end
end
defmodule Plain do
  defmacro __using__(_) do
    quote do
      alias Some.Else
    end
  end
end
defmodule Loop do
  defmacro __using__(_) do
    quote do
      use Loop
    end
  end
end
defmodule UsesWeb do
  use Web, :controller
  def f, do: Repo.all()
end
defmodule UsesOuter do
  use Outer
  def f, do: Repo.all()
end
defmodule UsesPlain do
  use Plain
  def f, do: Repo.all()
end
defmodule UsesLoop do
  use Loop
  def f, do: Repo.all()
end
",
    );

    assert_eq!(
        calls(&g),
        [
            ("fn:UsesLoop.f/0", "fn:Repo.all/0"),
            ("fn:UsesPlain.f/0", "fn:Repo.all/0"),
        ]
    );
    assert_eq!(report.unresolved_calls.use_alias, 2);
}

#[test]
fn a_use_of_a_module_of_the_repository_without_a_written_using_can_make_any_alias() {
    let (g, report) = run_lib(
        "defmodule B do
  def g, do: 1
end
defmodule Generated do
end
defmodule Local do
  def g, do: 1
  defmacro __using__(_), do: quote(do: nil)
end
defmodule U do
  use Generated
  def f, do: B.g()
end
defmodule L do
  use Local
  def f, do: B.g()
end
",
    );

    assert_eq!(calls(&g), [("fn:L.f/0", "fn:B.g/0")]);
    assert_eq!(report.unresolved_calls.use_alias, 1);
}

#[test]
fn a_name_in_a_macro_argument_of_a_module_body_that_is_no_function_gives_no_edge() {
    let (g, report) = run_lib(
        "defmodule B do
  def g, do: 1
end
defmodule S do
  typedstruct do
    field :b, B.t()
    field :c, B.g()
  end
  def f, do: B.t()
end
",
    );

    assert_eq!(calls(&g), [("fn:S.f/0", "pkg::B"), ("pkg::S", "fn:B.g/0")]);
    assert_eq!(report.calls.to_type, 1);
    assert_eq!(report.unresolved_calls.macro_argument, 1);
}

#[test]
fn main_code_calls_no_module_of_test_code_and_test_code_calls_both() {
    let (g, report) = run(&[
        MIX,
        (
            "lib/a.ex",
            "defmodule A do
  def f(x), do: String.length(x)
  def g, do: Fixture.h()
end
",
        ),
        (
            "test/support/string.ex",
            "defmodule String do
  def length(_), do: 0
end
defmodule Fixture do
  def h, do: 1
end
",
        ),
        (
            "test/a_test.exs",
            "defmodule ATest do
  def t, do: A.g()
  def u, do: Fixture.h()
end
",
        ),
    ]);

    assert_eq!(
        calls(&g),
        [
            ("fn:ATest.t/0", "fn:A.g/0"),
            ("fn:ATest.u/0", "fn:Fixture.h/0")
        ]
    );
    assert_eq!(report.calls.external, 2);
}

#[test]
fn the_using_block_of_a_case_template_is_read_as_its_using() {
    let (g, report) = run_lib(
        "defmodule B do
  def g, do: 1
end
defmodule Server do
  def g, do: 1
end
defmodule MyCase do
  use ExUnit.CaseTemplate
  using do
    quote do
      alias Other.Server
    end
  end
end
defmodule T do
  use MyCase
  def f, do: B.g()
  def s, do: Server.g()
end
",
    );

    assert_eq!(calls(&g), [("fn:T.f/0", "fn:B.g/0")]);
    assert_eq!(report.unresolved_calls.use_alias, 1);
}

#[test]
fn a_use_of_a_module_outside_the_repository_can_only_alias_the_names_its_arguments_write() {
    let (g, report) = run_lib(
        "defmodule B do
  def g, do: 1
end
defmodule Repo do
  def all, do: []
end
defmodule U do
  use Lib, repo: X.Repo
  def f, do: B.g()
  def r, do: Repo.all()
end
defmodule W do
  alias B
  use Lib, repo: X.Repo
  def f, do: B.g()
end
defmodule V do
  def f, do: B.g()
end
",
    );

    assert_eq!(
        calls(&g),
        [
            ("fn:U.f/0", "fn:B.g/0"),
            ("fn:V.f/0", "fn:B.g/0"),
            ("fn:W.f/0", "fn:B.g/0"),
        ]
    );
    assert_eq!(report.unresolved_calls.use_alias, 1);
    // `U.f` stands on the assumption that `Lib` makes no alias `B`; `W.f` has its own alias.
    assert_eq!(report.behind_outside_use, 1);
}

#[test]
fn an_alias_in_a_quote_whose_name_is_not_written_can_be_any_name_and_a_use_reaches_nested_modules()
{
    let (g, report) = run_lib(&format!(
        "{REPO_AND_USING}defmodule B do
  def g, do: 1
end
defmodule Dyn do
  defmacro __using__(opts) do
    quote do
      alias unquote(opts[:repo])
    end
  end
end
defmodule Own do
  defmacro __using__(_) do
    quote do
      alias __MODULE__.X
    end
  end
end
defmodule Named do
  defmacro __using__(opts) do
    quote do
      alias unquote(opts[:repo]), as: Foo
    end
  end
end
defmodule UsesDyn do
  use Dyn, repo: R
  def f, do: B.g()
end
defmodule UsesOwn do
  use Own
  def f, do: B.g()
end
defmodule UsesNamed do
  use Named, repo: R
  def f, do: B.g()
  def foo, do: Foo.g()
end
defmodule Outer do
  use M
  defmodule Inner do
    def f, do: Repo.all()
  end
end
"
    ));

    assert_eq!(calls(&g), [("fn:UsesNamed.f/0", "fn:B.g/0")]);
    // `UsesDyn.f`, `UsesOwn.f`, `UsesNamed.foo` (the quote of `Named` aliases `Foo`) and
    // `Outer.Inner.f` (the `use` of `Outer` reaches the nested module). `UsesNamed.f` keeps
    // its edge: `Named` can alias `Foo` only.
    assert_eq!(report.unresolved_calls.use_alias, 4);
}

#[test]
fn a_use_does_not_change_what_a_local_call_reaches() {
    let (g, report) = run_lib(
        "defmodule M do
  defmacro __using__(_) do
    quote do
      def helper(x), do: x
      def other(x), do: x
    end
  end
end
defmodule U do
  use M
  def helper(x), do: x
  def f(x), do: {helper(x), other(x)}
end
",
    );

    assert_eq!(calls(&g), [("fn:U.f/1", "fn:U.helper/1")]);
    assert_eq!(report.unresolved_calls.local_unknown, 1);
    assert_eq!(report.unresolved_calls.use_alias, 0);
}

#[test]
fn the_calls_do_not_depend_on_the_order_of_the_files() {
    let files = [
        MIX,
        ("apps/x/mix.exs", "def project, do: [app: :x]\n"),
        (
            "apps/x/lib/x.ex",
            "defmodule X do\n  def f, do: Demo.Z.g()\nend\n",
        ),
        (
            "lib/a.ex",
            "defmodule A do\n  use Z.M\n  import Z.Imp\n  def f(x), do: {Repo.all(), g(x), Z.Imp.g(x) |> h()}\nend\n",
        ),
        (
            "lib/z.ex",
            "defmodule Repo do\n  def all, do: []\nend
defmodule Z.M do\n  defmacro __using__(_), do: quote(do: alias(Other.Repo))\nend
defmodule Z.Imp do\n  def g(x), do: x\n  def h(x), do: x\nend\n",
        ),
        (
            "lib/dup.ex",
            "defmodule A do\n  def f(x), do: Z.Imp.h(x)\nend\n",
        ),
        (
            "test/a_test.exs",
            "defmodule ATest do\n  def t, do: A.f(1)\nend\n",
        ),
    ];
    let mut reversed = files;
    reversed.reverse();

    let (graph, report) = run(&files);

    // `Repo.all()` is hidden by the `use`; `Demo.Z.g()` is no module of the repository.
    assert_eq!(
        calls(&graph),
        [
            ("fn:A.f/1", "fn:Z.Imp.g/1"),
            ("fn:A.f/1", "fn:Z.Imp.h/1"),
            ("fn:ATest.t/0", "fn:A.f/1"),
        ]
    );
    assert!(report.unresolved_calls.use_alias > 0);
    assert_eq!(run(&reversed), (graph, report));
}

#[test]
fn broken_sources_with_calls_never_panic() {
    for text in [
        "defmodule A do\n  def f, do: B.\nend\n",
        "defmodule A do\n  def f, do: &/\n  def g, do: &B.h/\n  def h, do: x |>\nend\n",
        "defmodule A do\n  use\n  import B, only: [\n  @\n  defdelegate\n  defdelegate f(, to:\nend\n",
        "defmodule A do\n  quote\n  quote do alias end\n  def f, do: quote(do: use(M))\nend\n",
        "defmodule A do\n  def f(a \\\\\\\\), do: a.()\n  def g, do: .x\nend\n",
        "A.f(\nB.g 1,\n&C.h/\nx |> y |> ",
        "defmodule A do\n  import B, only: [f: ]\n  import B, except: :x\n  f()\nend\n",
    ] {
        let (g, _) = run_lib(text);
        assert!(g.node("mod:").is_some());
        assert_eq!(calls(&g), [], "{text:?}");
    }
}

#[test]
fn the_condition_of_an_if_in_a_module_body_is_a_call_of_the_package_and_its_functions_keep_their_own()
 {
    let (g, report) = run_lib(
        "defmodule B do
  def on?, do: true
  def g, do: 1
end
defmodule A do
  if B.on?() do
    def f, do: B.g()
  end
end
",
    );

    assert_eq!(
        calls(&g),
        [("fn:A.f/0", "fn:B.g/0"), ("pkg::A", "fn:B.on?/0")]
    );
    // The `if` is a call of Elixir.
    assert_eq!(report.calls.to_function, 2);
    assert_eq!(report.calls.external, 1);
}

#[test]
fn a_defdelegate_to_a_name_that_a_use_can_alias_is_unresolved() {
    let (g, report) = run_lib(&format!(
        "{REPO_AND_USING}defmodule User do
  use M
  defdelegate all(), to: Repo
end
"
    ));

    assert_eq!(calls(&g), []);
    assert_eq!(report.unresolved_calls.use_alias, 1);
}

#[test]
fn the_modifiers_of_a_bitstring_segment_are_no_calls_but_its_value_is_read() {
    let (g, report) = run_lib(
        "defmodule B do
  def g(x), do: x
end
defmodule A do
  def size(b), do: byte_size(b)
  def parse(<<n::size(8), rest::binary-size(n), tail::unit(8)-integer>>), do: {n, rest, tail}
  def build(x), do: <<B.g(x)::size(8), x::binary-size(4)>>
end
",
    );

    assert_eq!(calls(&g), [("fn:A.build/1", "fn:B.g/1")]);
    assert_eq!(report.calls.to_function, 1);
    // `byte_size` only.
    assert_eq!(report.calls.external, 1);
    assert_eq!(report.calls.unresolved, 0);
}

#[test]
fn a_field_chain_is_no_call_but_a_call_on_a_variable_with_parentheses_is_dynamic() {
    let (g, report) = run_lib(
        "defmodule A do
  @attr %{}
  def a(socket), do: socket.assigns.user
  def b, do: @attr.field
  def c, do: Mod.make().field
  def d(m), do: m.fun()
end
",
    );

    assert_eq!(calls(&g), []);
    // `Mod.make()` is external; `m.fun()` is the only unresolved call.
    assert_eq!(
        report.calls,
        CallReport {
            external: 1,
            unresolved: 1,
            ..CallReport::default()
        }
    );
    assert_eq!(report.unresolved_calls.dynamic, 1);
}

#[test]
fn a_later_import_of_a_module_replaces_the_earlier_one() {
    let (g, report) = run_lib(
        "defmodule B do
  def f(x), do: x
  def g(x), do: x
end
defmodule A do
  import B
  import B, except: [f: 1]
  def a(x), do: f(x)
  def b(x), do: g(x)
end
",
    );

    assert_eq!(calls(&g), [("fn:A.b/1", "fn:B.g/1")]);
    assert_eq!(report.unresolved_calls.local_unknown, 1);
}

#[test]
fn a_use_of_an_outside_module_in_a_quote_can_alias_the_names_its_arguments_write() {
    let (g, report) = run_lib(
        "defmodule Repo do
  def all, do: []
end
defmodule B do
  def g, do: 1
end
defmodule M do
  defmacro __using__(_) do
    quote do
      use Lib, repo: X.Repo
    end
  end
end
defmodule U do
  use M
  def r, do: Repo.all()
  def b, do: B.g()
end
",
    );

    assert_eq!(calls(&g), [("fn:U.b/0", "fn:B.g/0")]);
    assert_eq!(report.unresolved_calls.use_alias, 1);
}

#[test]
fn a_function_of_the_module_beats_an_import_that_answers_the_same_name_and_arity() {
    let (g, report) = run_lib(
        "defmodule B do
  def g(x), do: x
end
defmodule A do
  import B
  def g(x), do: x
  def f(x), do: g(x)
end
",
    );

    assert_eq!(calls(&g), [("fn:A.f/1", "fn:A.g/1")]);
    assert_eq!(report.unresolved_calls.ambiguous, 0);
}

#[test]
fn a_remote_call_of_a_protocol_function_is_an_edge_to_its_signature() {
    let (g, _) = run_lib(
        "defprotocol Shape do
  def area(shape)
end
defmodule A do
  def f(s), do: Shape.area(s)
end
",
    );

    assert_eq!(calls(&g), [("fn:A.f/1", "fn:Shape.area/1")]);
}

#[test]
fn a_name_that_starts_with_elixir_is_not_hidden_by_a_use() {
    let (g, report) = run_lib(&format!(
        "{REPO_AND_USING}defmodule User do
  use M
  def hidden, do: Repo.all()
  def plain, do: Elixir.Repo.all()
end
"
    ));

    assert_eq!(calls(&g), [("fn:User.plain/0", "fn:Repo.all/0")]);
    assert_eq!(report.unresolved_calls.use_alias, 1);
}

#[test]
fn a_defmodule_and_a_require_with_as_in_a_quote_make_aliases_too() {
    let (g, report) = run_lib(
        "defmodule X do
  def f, do: 1
end
defmodule Y do
  def f, do: 1
end
defmodule Z do
  def f, do: 1
end
defmodule M do
  defmacro __using__(_) do
    quote do
      defmodule X.Inner do
      end
      require A.B, as: Y
    end
  end
end
defmodule U do
  use M
  def x, do: X.f()
  def y, do: Y.f()
  def z, do: Z.f()
end
",
    );

    assert_eq!(calls(&g), [("fn:U.z/0", "fn:Z.f/0")]);
    assert_eq!(report.unresolved_calls.use_alias, 2);
}

#[test]
fn a_parent_module_that_calls_its_child_gets_no_edge() {
    let (g, report) = run_lib(
        "defmodule A do
  defmodule B do
    def f, do: 1
  end
  @value B.f()
end
",
    );

    assert_eq!(calls(&g), []);
    assert_eq!(report.calls.to_function, 1);
}

#[test]
fn test_code_reaches_a_module_of_test_code_that_main_code_does_not() {
    let (g, report) = run(&[
        MIX,
        (
            "test/support/mock.ex",
            "defmodule String do\n  def mocked, do: 1\nend\n",
        ),
        (
            "lib/a.ex",
            "defmodule A do\n  def f, do: String.mocked()\nend\n",
        ),
        (
            "test/a_test.exs",
            "defmodule ATest do\n  def t, do: String.mocked()\nend\n",
        ),
    ]);

    // The mock is test code, so it is a gap that `ATest` reaches it where the real `String`
    // might be meant; main code never does.
    assert_eq!(calls(&g), [("fn:ATest.t/0", "fn:String.mocked/0")]);
    assert_eq!(report.calls.external, 1);
}

#[test]
fn a_remote_call_without_parentheses_that_is_not_piped_has_no_arguments() {
    let (g, _) = run_lib(
        "defmodule B do
  def zero, do: 1
  def one(x), do: x
end
defmodule A do
  def f, do: B.zero
  def g, do: B.one
end
",
    );

    assert_eq!(
        calls(&g),
        [("fn:A.f/0", "fn:B.zero/0"), ("fn:A.g/0", "pkg::B")]
    );
}
