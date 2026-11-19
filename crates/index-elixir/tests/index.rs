use cg_core::{EdgeKind, Graph, Node, NodeKind, Span};
use cg_index_elixir::{Layout, ModuleRefs, Report, SourceFile, index_sources, is_input};
use std::collections::BTreeMap;

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

fn index(files: &[(&str, &str)]) -> Graph {
    run(files).0
}

fn node<'a>(g: &'a Graph, id: &str) -> &'a Node {
    g.node(id)
        .unwrap_or_else(|| panic!("no node {id}; nodes: {:?}", ids(g)))
}

fn parent_of<'a>(g: &'a Graph, id: &str) -> &'a str {
    node(g, id).parent.as_deref().expect("node has no parent")
}

fn ids(g: &Graph) -> Vec<&str> {
    g.nodes.iter().map(|n| n.id.as_str()).collect()
}

#[test]
fn elixir_sources_mix_exs_and_test_scripts_are_inputs_but_not_below_hidden_or_build_directories() {
    assert!(is_input("lib/a.ex"));
    assert!(is_input("mix.exs"));
    assert!(is_input("apps/x/mix.exs"));
    assert!(is_input("test/a_test.exs"));
    assert!(is_input("apps/x/test/support/c.ex"));
    assert!(!is_input("config/config.exs"));
    assert!(!is_input("priv/repo/seeds.exs"));
    assert!(!is_input("lib/a.eex"));
    assert!(!is_input("lib/.ex"));
    assert!(!is_input("deps/x/lib/a.ex"));
    assert!(!is_input("_build/dev/a.ex"));
    assert!(!is_input(".elixir_ls/a.ex"));
    assert!(!is_input("node_modules/x/a.ex"));
}

#[test]
fn each_mix_exs_directory_is_a_module_named_by_app_the_last_segment_or_the_root_name() {
    let g = index(&[
        (
            "mix.exs",
            "def project do\n  [app: :my_app, version: \"1\"]\nend\n",
        ),
        ("apps/web/mix.exs", "defmodule Web.MixProject do\nend\n"),
        (
            "libs/db/impl/mix.exs",
            "def project, do: [version: \"1\", app: :db_impl]\n",
        ),
    ]);

    let root = node(&g, "mod:");
    assert_eq!(
        (root.kind, root.name.as_str()),
        (NodeKind::Module, "my_app")
    );
    assert_eq!(root.parent, None);
    assert_eq!(node(&g, "mod:apps/web").name, "web");
    assert_eq!(node(&g, "mod:libs/db/impl").name, "db_impl");
    assert_eq!(g.nodes.len(), 3);

    let g = index(&[("mix.exs", "")]);
    assert_eq!(node(&g, "mod:").name, "repo");
}

#[test]
fn an_umbrella_root_is_no_module_and_a_file_outside_its_apps_is_counted() {
    let (g, report) = run(&[
        (
            "mix.exs",
            "defmodule U.MixProject do\n  def project, do: [apps_path: \"apps\"]\nend\n",
        ),
        ("apps/a/mix.exs", "def project, do: [app: :a]\n"),
        ("apps/b/mix.exs", "def project, do: [app: :b]\n"),
        ("lib/stray.ex", "defmodule Stray do\nend\n"),
    ]);

    assert_eq!(ids(&g), ["mod:apps/a", "mod:apps/b"]);
    assert_eq!(report.files_without_project, 1);
}

const MIX: (&str, &str) = ("mix.exs", "def project, do: [app: :demo]\n");

#[test]
fn a_defmodule_is_a_package_and_def_a_function_with_file_span_and_hash() {
    let g = index(&[
        MIX,
        (
            "lib/foo/bar.ex",
            "defmodule Foo.Bar do\n  def baz(x) do\n    x\n  end\nend\n",
        ),
    ]);

    let pkg = node(&g, "pkg::Foo.Bar");
    assert_eq!(
        (pkg.kind, pkg.name.as_str()),
        (NodeKind::Package, "Foo.Bar")
    );
    assert_eq!(pkg.parent.as_deref(), Some("mod:"));
    assert_eq!(
        (pkg.file.clone(), pkg.span, pkg.hash.clone()),
        (None, None, None)
    );
    let f = node(&g, "fn:Foo.Bar.baz/1");
    assert_eq!((f.kind, f.name.as_str()), (NodeKind::Function, "baz/1"));
    assert_eq!(f.parent.as_deref(), Some("pkg::Foo.Bar"));
    assert_eq!(f.file.as_deref(), Some("lib/foo/bar.ex"));
    assert_eq!(
        f.span,
        Some(Span {
            start_line: 2,
            end_line: 4
        })
    );
    assert!(!f.test);
    // Whitespace does not change the hash; a change of the text does.
    let same = index(&[
        MIX,
        (
            "lib/foo/bar.ex",
            "defmodule Foo.Bar do\n def baz(x) do x end\nend\n",
        ),
    ]);
    let other = index(&[
        MIX,
        (
            "lib/foo/bar.ex",
            "defmodule Foo.Bar do\n def baz(x) do x + 1 end\nend\n",
        ),
    ]);
    assert!(f.hash.is_some());
    assert_eq!(node(&same, "fn:Foo.Bar.baz/1").hash, f.hash);
    assert_ne!(node(&other, "fn:Foo.Bar.baz/1").hash, f.hash);
    assert_eq!(g.nodes.len(), 3);
}

/// The ids of the nodes of one kind, sorted.
fn ids_of(g: &Graph, kind: NodeKind) -> Vec<&str> {
    let mut ids: Vec<&str> = g
        .nodes
        .iter()
        .filter(|n| n.kind == kind)
        .map(|n| n.id.as_str())
        .collect();
    ids.sort();
    ids
}

fn lib(text: &str) -> Graph {
    index(&[MIX, ("lib/a.ex", text)])
}

#[test]
fn function_forms_are_read_by_name_and_arity_as_written() {
    let g = lib("defmodule A do
  defp p(x), do: x
  def h, do: 1
  def gd(x) when is_integer(x), do: x
  def d(a, b \\\\ 1), do: a
  defmacro m(x), do: x
  defmacrop mp(x), do: x
  defguard is_pos(x) when x > 0
  defguardp is_neg(x) when x < 0
  defdelegate dl(x), to: B
end
");

    assert_eq!(
        ids_of(&g, NodeKind::Function),
        [
            "fn:A.d/2",
            "fn:A.dl/1",
            "fn:A.gd/1",
            "fn:A.h/0",
            "fn:A.is_neg/1",
            "fn:A.is_pos/1",
            "fn:A.m/1",
            "fn:A.mp/1",
            "fn:A.p/1",
        ]
    );
}

fn span(start_line: u32, end_line: u32) -> Option<Span> {
    Some(Span {
        start_line,
        end_line,
    })
}

#[test]
fn the_clauses_of_a_function_are_one_node_and_any_clause_changes_the_hash() {
    let text = "defmodule A do
  def f(0), do: :zero
  def f(1) do
    :one
  end
  def f(_), do: :many
end
";
    let g = lib(text);

    assert_eq!(ids_of(&g, NodeKind::Function), ["fn:A.f/1"]);
    assert_eq!(node(&g, "fn:A.f/1").span, span(2, 6));
    let changed = lib(&text.replace(":one", ":uno"));
    assert_ne!(node(&changed, "fn:A.f/1").hash, node(&g, "fn:A.f/1").hash);
    let last = lib(&text.replace(":many", ":lots"));
    assert_ne!(node(&last, "fn:A.f/1").hash, node(&g, "fn:A.f/1").hash);
}

#[test]
fn doc_spec_impl_and_deprecated_above_a_clause_belong_to_it_other_attributes_do_not() {
    let text = "defmodule A do
  @timeout 5
  # a comment between
  @doc \"the doc\"
  # another one
  @spec f(integer) :: integer
  @impl true
  @deprecated \"no\"
  def f(x), do: x
end
";
    let g = lib(text);

    assert_eq!(node(&g, "fn:A.f/1").span, span(4, 9));
    let attribute = lib(&text.replace("the doc", "other"));
    assert_ne!(node(&attribute, "fn:A.f/1").hash, node(&g, "fn:A.f/1").hash);
    let timeout = lib(&text.replace("@timeout 5", "@timeout 6"));
    assert_eq!(node(&timeout, "fn:A.f/1").hash, node(&g, "fn:A.f/1").hash);
}

#[test]
fn a_clause_after_a_clause_of_another_function_is_in_the_hash_but_not_in_the_span() {
    let text = "defmodule A do
  def f(0), do: 0
  @other 1
  def f(1), do: 1
  def g(x), do: x
  def f(2), do: 2
  def f(3), do: 3
end
";
    let g = lib(text);

    assert_eq!(node(&g, "fn:A.f/1").span, span(2, 4));
    assert_eq!(node(&g, "fn:A.g/1").span, span(5, 5));
    let far = lib(&text.replace("do: 3", "do: 4"));
    assert_ne!(node(&far, "fn:A.f/1").hash, node(&g, "fn:A.f/1").hash);
}

/// Counts of `forms_without_node` as `(head, count)` pairs.
fn forms(report: &Report) -> Vec<(&str, usize)> {
    report
        .forms_without_node
        .iter()
        .map(|(head, n)| (head.as_str(), *n))
        .collect()
}

fn run_lib(text: &str) -> (Graph, Report) {
    run(&[MIX, ("lib/a.ex", text)])
}

#[test]
fn operators_are_functions_by_their_symbol_and_a_name_that_is_not_written_gives_no_node() {
    let (g, report) = run_lib(
        "defmodule A do
  def a <> b, do: a
  def +a, do: a
  def unquote(name)(x), do: x
  def unquote(name)
  def f(x), do: x
end
",
    );

    assert_eq!(
        ids_of(&g, NodeKind::Function),
        ["fn:A.+/1", "fn:A.<>/2", "fn:A.f/1"]
    );
    assert_eq!(node(&g, "fn:A.<>/2").name, "<>/2");
    assert_eq!(forms(&report), [("def", 2)]);
}

#[test]
fn modules_nest_lexically_share_a_file_and_take_their_names_from_the_aliases_above() {
    let (g, report) = run_lib(
        "alias Foo.Bar
defmodule Bar.Baz do
end
defmodule A.B do
  defmodule Inner do
    def i, do: 1
  end
  defmodule __MODULE__.Other do
  end
  defmodule Elixir.Plain do
  end
  def f, do: 1
end
defmodule :atom do
end
defmodule unquote(x) do
end
",
    );

    assert_eq!(
        ids_of(&g, NodeKind::Package),
        [
            "pkg::A.B",
            "pkg::A.B.Inner",
            "pkg::A.B.Other",
            "pkg::Foo.Bar.Baz",
            "pkg::Plain",
        ]
    );
    assert_eq!(
        ids_of(&g, NodeKind::Function),
        ["fn:A.B.Inner.i/0", "fn:A.B.f/0"]
    );
    assert_eq!(
        node(&g, "fn:A.B.Inner.i/0").parent.as_deref(),
        Some("pkg::A.B.Inner")
    );
    assert_eq!(forms(&report), [("defmodule", 2)]);
}

#[test]
fn packages_nest_by_their_names_as_java_packages_do() {
    let g = index(&[
        MIX,
        ("lib/a.ex", "defmodule MyApp do\nend\n"),
        (
            "lib/b.ex",
            "defmodule MyApp.Accounts.User do\nend\ndefmodule MyApp.Accounts.Admin do\nend\n",
        ),
        ("lib/c.ex", "defmodule Single.Deep.Name do\nend\n"),
    ]);

    assert_eq!(parent_of(&g, "pkg::MyApp.Accounts"), "pkg::MyApp");
    assert_eq!(node(&g, "pkg::MyApp.Accounts").name, "Accounts");
    assert_eq!(
        parent_of(&g, "pkg::MyApp.Accounts.User"),
        "pkg::MyApp.Accounts"
    );
    assert_eq!(node(&g, "pkg::MyApp.Accounts.User").name, "User");
    assert_eq!(node(&g, "pkg::MyApp.Accounts.Admin").name, "Admin");
    assert_eq!(parent_of(&g, "pkg::MyApp"), "mod:");
    assert_eq!(parent_of(&g, "pkg::Single.Deep.Name"), "mod:");
    assert_eq!(node(&g, "pkg::Single.Deep.Name").name, "Single.Deep.Name");
}

#[test]
fn a_protocol_is_a_package_and_its_signatures_are_functions() {
    let g = lib("defprotocol Size do
  @doc \"The size\"
  def size(data)
  def count(data, opts)
end
");

    assert_eq!(ids_of(&g, NodeKind::Package), ["pkg::Size"]);
    assert_eq!(
        ids_of(&g, NodeKind::Function),
        ["fn:Size.count/2", "fn:Size.size/1"]
    );
    assert_eq!(node(&g, "fn:Size.size/1").span, span(2, 3));
}

#[test]
fn the_forms_of_a_module_body_that_give_no_node_are_counted_by_their_head() {
    let (_, report) = run_lib(
        "defmodule A do
  defstruct [:a]
  defstruct [:b]
  @type t :: %__MODULE__{}
  @moduledoc false
  alias B.C
  import X
  require Y
  use Z
  schema \"x\" do
    field :a
  end
  Foo.bar(1)
  x = 5
  defmodule Inner do
    defexception [:message]
  end
  def f, do: 1
end
defstruct [:top]
",
    );

    assert_eq!(
        forms(&report),
        [
            ("defexception", 1),
            ("defstruct", 2),
            ("other", 1),
            ("schema", 1)
        ]
    );
}

#[test]
fn definitions_are_read_in_if_and_unless_blocks_but_not_in_other_blocks_or_bodies() {
    let (g, report) = run_lib(
        "if Code.ensure_loaded?(Foo) do
  defmodule A.Opt do
  end
end
defmodule B do
  if true do
    def f(0), do: 0
  else
    def f(1), do: 1
  end
  unless x do
    def g, do: 1
  end
  for x <- list do
    def h, do: x
  end
  def k do
    def inner, do: 1
    defmodule Hidden do
    end
  end
  defmacro __using__(_) do
    quote do
      def injected, do: 1
      defmodule Quoted do
      end
    end
  end
end
",
    );

    assert_eq!(ids_of(&g, NodeKind::Package), ["pkg::A.Opt", "pkg::B"]);
    assert_eq!(
        ids_of(&g, NodeKind::Function),
        ["fn:B.__using__/1", "fn:B.f/1", "fn:B.g/0", "fn:B.k/0"]
    );
    assert_eq!(node(&g, "fn:B.f/1").span, span(7, 9));
    assert_eq!(forms(&report), [("for", 1)]);
}

#[test]
fn a_module_can_be_written_with_a_do_option() {
    let g = lib("defmodule A, do: (def f(x), do: x)\ndefmodule B, do: def(g, do: 1)\n");

    assert_eq!(ids_of(&g, NodeKind::Function), ["fn:A.f/1", "fn:B.g/0"]);
}

#[test]
fn source_directories_come_from_the_mix_exs_and_other_files_are_counted() {
    let (g, report) = run(&[
        ("p1/mix.exs", "def project, do: [app: :p1]\n"),
        ("p1/lib/a.ex", "defmodule P1.A do\nend\n"),
        ("p1/src/b.ex", "defmodule P1.B do\nend\n"),
        ("p1/test/c_test.exs", "defmodule P1.CTest do\nend\n"),
        ("p1/test/support/s.ex", "defmodule P1.S do\nend\n"),
        ("p1/test/s.exs", "defmodule P1.Script do\nend\n"),
        (
            "p2/mix.exs",
            "def project, do: [app: :p2, elixirc_paths: [\"src\"]]\n",
        ),
        ("p2/src/x.ex", "defmodule P2.X do\nend\n"),
        ("p2/lib/y.ex", "defmodule P2.Y do\nend\n"),
        ("p2/inner/mix.exs", "def project, do: [app: :inner]\n"),
        ("p2/inner/lib/i.ex", "defmodule Inner.I do\nend\n"),
        (
            "p3/mix.exs",
            "defmodule P3.MixProject do
  def project, do: [app: :p3, elixirc_paths: elixirc_paths(Mix.env())]
  defp elixirc_paths(:test), do: [\"lib\", \"test/support\"]
  defp elixirc_paths(_), do: [\"lib\", \"web\"]
end
",
        ),
        ("p3/web/w.ex", "defmodule P3.W do\nend\n"),
        ("p3/lib/l.ex", "defmodule P3.L do\nend\n"),
        ("stray/s.ex", "defmodule Stray do\nend\n"),
    ]);

    assert_eq!(
        ids_of(&g, NodeKind::Package),
        [
            "pkg:p1:P1.A",
            "pkg:p1:P1.CTest",
            "pkg:p1:P1.S",
            "pkg:p1:P1.Script",
            "pkg:p2/inner:Inner.I",
            "pkg:p2:P2.X",
            "pkg:p3:P3.L",
            "pkg:p3:P3.W",
        ]
    );
    assert_eq!(report.files_outside_source_dirs, 2);
    assert_eq!(report.files_without_project, 1);
    assert_eq!(forms(&report), []);
}

#[test]
fn code_below_the_test_directory_is_test_code() {
    let g = index(&[
        MIX,
        ("lib/a.ex", "defmodule A do\n  def f, do: 1\nend\n"),
        (
            "test/support/helper.ex",
            "defmodule Helper do\n  def g, do: 1\nend\n",
        ),
        ("test/a_test.exs", "defmodule ATest do\nend\n"),
    ]);

    assert!(!node(&g, "pkg::A").test);
    assert!(!node(&g, "fn:A.f/0").test);
    assert!(node(&g, "pkg::Helper").test);
    assert!(node(&g, "fn:Helper.g/0").test);
    assert!(node(&g, "pkg::ATest").test);
}

#[test]
fn a_project_below_the_test_directory_of_another_project_is_test_code() {
    let fixture = |path: &str, text: &str| SourceFile {
        path: path.to_string(),
        text: text.to_string(),
    };
    let files = [
        fixture("mix.exs", "def project, do: [app: :demo]\n"),
        fixture(
            "test/fixtures/umbrella/mix.exs",
            "def project, do: [apps_path: \"apps\"]\n",
        ),
        fixture(
            "test/fixtures/umbrella/apps/one/mix.exs",
            "def project, do: [app: :one]\n",
        ),
        fixture(
            "test/fixtures/umbrella/apps/one/lib/one.ex",
            "defmodule One do\n  def f, do: 1\nend\n",
        ),
    ];

    let (g, _) = index_sources("repo", &files);

    assert!(!node(&g, "mod:").test);
    assert!(node(&g, "mod:test/fixtures/umbrella/apps/one").test);
    assert!(node(&g, "pkg:test/fixtures/umbrella/apps/one:One").test);
    assert!(node(&g, "fn:One.f/0").test);
    assert_eq!(
        Layout::new(&files).test_code("test/fixtures/umbrella/apps/one/lib/one.ex"),
        Some(true)
    );
}

#[test]
fn a_module_that_an_earlier_form_declares_is_skipped_with_everything_in_it() {
    let (g, report) = run(&[
        MIX,
        (
            "lib/b.ex",
            "\ndefmodule A do\n  def g, do: 1\n  defmodule Nested do\n  end\nend\ndefmodule C do\nend\ndefmodule C do\nend\n",
        ),
        ("lib/a.ex", "defmodule A do\n  def f, do: 1\nend\n"),
    ]);

    assert_eq!(ids_of(&g, NodeKind::Function), ["fn:A.f/0"]);
    assert_eq!(ids_of(&g, NodeKind::Package), ["pkg::A", "pkg::C"]);
    assert_eq!(report.duplicate_modules, ["lib/b.ex:2", "lib/b.ex:9"]);
}

fn edges_of(g: &Graph, kind: EdgeKind) -> Vec<(&str, &str)> {
    g.edges
        .iter()
        .filter(|e| e.kind == kind)
        .map(|e| (e.from.as_str(), e.to.as_str()))
        .collect()
}

fn imports(g: &Graph) -> Vec<(&str, &str)> {
    edges_of(g, EdgeKind::Imports)
}

/// The sites of the one edge `from` to `to` of `kind`: file and lines. A package has no
/// file, so each site names its file.
fn sites(g: &Graph, from: &str, to: &str, kind: EdgeKind) -> Vec<(Option<String>, Vec<u32>)> {
    g.edge(from, to, kind)
        .unwrap_or_else(|| panic!("no edge {from} -> {to} {kind:?}; edges: {:?}", g.edges))
        .sites
        .iter()
        .map(|s| (s.file.clone(), s.lines.clone()))
        .collect()
}

fn in_file(file: &str, lines: &[u32]) -> Vec<(Option<String>, Vec<u32>)> {
    vec![(Some(file.to_string()), lines.to_vec())]
}

#[test]
fn alias_import_require_and_use_make_imports_edges_to_modules_of_the_repository() {
    let g = index(&[
        MIX,
        (
            "lib/a.ex",
            "defmodule A do
  alias B.One
  import B.Two
  require B.Three
  use B.Four
  alias Ext.Thing
  import Ext.Other
end
",
        ),
        (
            "lib/b.ex",
            "defmodule B.One do
end
defmodule B.Two do
end
defmodule B.Three do
end
defmodule B.Four do
end
",
        ),
    ]);

    assert_eq!(
        imports(&g),
        [
            ("pkg::A", "pkg::B.Four"),
            ("pkg::A", "pkg::B.One"),
            ("pkg::A", "pkg::B.Three"),
            ("pkg::A", "pkg::B.Two"),
        ]
    );
    assert_eq!(
        sites(&g, "pkg::A", "pkg::B.One", EdgeKind::Imports),
        in_file("lib/a.ex", &[2])
    );
    assert_eq!(
        sites(&g, "pkg::A", "pkg::B.Four", EdgeKind::Imports),
        in_file("lib/a.ex", &[5])
    );
}

/// The modules `T.B`, `T.B.D`, `T.C.D`, `T.B.C`, `T.E`, `T.F`, `T.B.S`, that the tests
/// below name.
const TARGETS: (&str, &str) = (
    "lib/targets.ex",
    "defmodule T.B do\nend\ndefmodule T.B.D do\nend\ndefmodule T.C.D do\nend\n\
     defmodule T.B.C do\nend\ndefmodule T.E do\nend\ndefmodule T.F do\nend\n\
     defmodule T.B.S do\nend\n",
);

/// The targets of the `imports` edges of one package with the lines of their sites.
fn imported<'a>(g: &'a Graph, from: &str) -> Vec<(&'a str, Vec<u32>)> {
    g.edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Imports && e.from == from)
        .map(|e| {
            let lines = e.sites.iter().flat_map(|s| s.lines.clone()).collect();
            (e.to.as_str(), lines)
        })
        .collect()
}

#[test]
fn aliases_with_as_and_groups_expand_the_names_after_them() {
    let g = index(&[
        MIX,
        TARGETS,
        (
            "lib/uses.ex",
            "defmodule Use1 do
  alias T.B, as: C
  alias C.D
end
defmodule Use2 do
  alias T.{
    E,
    C.D
  }
end
defmodule Use3 do
  alias T.B
  import B.C
end
defmodule Use4 do
  use T.{E, F}
end
defmodule Use5 do
  require T.B, as: R
  alias R.S
end
defmodule Use6 do
  alias Elixir.T.E
  import E
end
",
        ),
    ]);

    assert_eq!(
        imported(&g, "pkg::Use1"),
        [("pkg::T.B", vec![2]), ("pkg::T.B.D", vec![3])]
    );
    assert_eq!(
        imported(&g, "pkg::Use2"),
        [("pkg::T.C.D", vec![8]), ("pkg::T.E", vec![7])]
    );
    assert_eq!(
        imported(&g, "pkg::Use3"),
        [("pkg::T.B", vec![12]), ("pkg::T.B.C", vec![13])]
    );
    assert_eq!(
        imported(&g, "pkg::Use4"),
        [("pkg::T.E", vec![16]), ("pkg::T.F", vec![16])]
    );
    assert_eq!(
        imported(&g, "pkg::Use5"),
        [("pkg::T.B", vec![19]), ("pkg::T.B.S", vec![20])]
    );
    assert_eq!(imported(&g, "pkg::Use6"), [("pkg::T.E", vec![23, 24])]);
}

#[test]
fn an_alias_holds_to_the_end_of_its_block_and_a_nested_module_inherits_the_aliases_above() {
    let g = index(&[
        MIX,
        TARGETS,
        ("lib/plain.ex", "defmodule B.C do\nend\n"),
        (
            "lib/scopes.ex",
            "defmodule S1 do
  def f do
    alias T.B
    B.run()
  end
  import B.C
end
defmodule S2 do
  alias T.B
  defmodule Inner do
    import B.C
  end
end
defmodule S3 do
  defmodule Inner do
    defmodule Deep do
    end
  end
  defmodule User do
    alias Inner.Deep
  end
end
defmodule S4 do
  if true do
    alias T.B
    import B.C
  end
  import B.C
  fun = fn -> alias T.E end
  import E
end
",
        ),
    ]);

    assert_eq!(
        imported(&g, "pkg::S1"),
        [("pkg::B.C", vec![6]), ("pkg::T.B", vec![3])]
    );
    assert_eq!(imported(&g, "pkg::S2"), [("pkg::T.B", vec![9])]);
    assert_eq!(imported(&g, "pkg::S2.Inner"), [("pkg::T.B.C", vec![11])]);
    assert_eq!(
        imported(&g, "pkg::S3.User"),
        [("pkg::S3.Inner.Deep", vec![20])]
    );
    assert_eq!(
        imported(&g, "pkg::S4"),
        [
            ("pkg::B.C", vec![28]),
            ("pkg::T.B", vec![25]),
            ("pkg::T.B.C", vec![26]),
            // The alias of the `fn` clause ends with it: `import E` names no module.
            ("pkg::T.E", vec![29]),
        ]
    );
}

#[test]
fn an_alias_of_a_do_block_does_not_hold_in_its_else_block() {
    let g = index(&[
        MIX,
        TARGETS,
        ("lib/plain.ex", "defmodule B.C do\nend\n"),
        (
            "lib/branches.ex",
            "defmodule S5 do
  def f(x) do
    if x do
      alias T.B
    else
      import B.C
    end
  end
end
",
        ),
    ]);

    assert_eq!(
        imported(&g, "pkg::S5"),
        [("pkg::B.C", vec![6]), ("pkg::T.B", vec![4])]
    );
}

#[test]
fn a_directive_in_a_quote_is_no_reference_and_makes_no_alias() {
    let (g, report) = run(&[
        MIX,
        TARGETS,
        ("lib/plain.ex", "defmodule B.C do\nend\n"),
        (
            "lib/q.ex",
            "defmodule Q do
  defmacro __using__(_) do
    quote do
      alias T.B
      import T.E
      use T.F
    end
  end
  quote do: alias(T.E)
  import B.C
end
",
        ),
    ]);

    assert_eq!(imported(&g, "pkg::Q"), [("pkg::B.C", vec![10])]);
    assert_eq!(report.references["import"].resolved, 1);
    assert!(!report.references.contains_key("use"));
}

#[test]
fn no_edge_joins_a_module_with_itself_its_ancestor_or_descendant_but_modules_of_two_projects_are_joined()
 {
    let (g, report) = run(&[
        MIX,
        (
            "lib/p.ex",
            "defmodule P.Q do
  alias __MODULE__
  alias __MODULE__.Inner
  defmodule Inner do
  end
end
defmodule P do
  alias P.Q
end
",
        ),
    ]);

    assert_eq!(g.edges, []);
    assert_eq!(report.references["alias"].resolved, 3);

    let g = index(&[
        ("mix.exs", "def project, do: [apps_path: \"apps\"]\n"),
        ("apps/a/mix.exs", "def project, do: [app: :a]\n"),
        ("apps/b/mix.exs", "def project, do: [app: :b]\n"),
        (
            "apps/a/lib/web.ex",
            "defmodule Web do\n  alias __MODULE__.Helper\nend\n",
        ),
        ("apps/b/lib/helper.ex", "defmodule Web.Helper do\nend\n"),
    ]);

    assert_eq!(imports(&g), [("pkg:apps/a:Web", "pkg:apps/b:Web.Helper")]);
}

fn implements(g: &Graph) -> Vec<(&str, &str)> {
    edges_of(g, EdgeKind::Implements)
}

#[test]
fn a_behaviour_attribute_makes_an_implements_edge_to_the_behaviour() {
    let (g, report) = run(&[
        MIX,
        (
            "lib/b.ex",
            "defmodule Store do\n  @callback get(term) :: term\nend\n",
        ),
        (
            "lib/s.ex",
            "defmodule Memory do
  @behaviour Store
  @behaviour GenServer
  def f do
    @behaviour Store
  end
end
",
        ),
    ]);

    assert_eq!(implements(&g), [("pkg::Memory", "pkg::Store")]);
    assert_eq!(
        sites(&g, "pkg::Memory", "pkg::Store", EdgeKind::Implements),
        in_file("lib/s.ex", &[2])
    );
    assert_eq!(
        report.references["behaviour"],
        ModuleRefs {
            resolved: 1,
            external: 1,
            ..ModuleRefs::default()
        }
    );
}

#[test]
fn a_defimpl_is_a_package_below_its_type_and_makes_an_implements_edge_from_the_type() {
    let g = index(&[
        MIX,
        (
            "lib/user.ex",
            "defmodule MyApp.User do\n  defstruct [:name]\nend\n",
        ),
        (
            "lib/printer.ex",
            "defprotocol MyApp.Printer do\n  def print(x)\nend\n",
        ),
        (
            "lib/impl.ex",
            "\ndefimpl MyApp.Printer, for: MyApp.User do\n  def print(u), do: u.name\nend\n",
        ),
    ]);

    let package = node(&g, "pkg::MyApp.Printer.MyApp.User");
    assert_eq!(package.kind, NodeKind::Package);
    assert_eq!(package.parent.as_deref(), Some("pkg::MyApp.User"));
    assert_eq!(package.name, "MyApp.Printer");
    let print = node(&g, "fn:MyApp.Printer.MyApp.User.print/1");
    assert_eq!(
        print.parent.as_deref(),
        Some("pkg::MyApp.Printer.MyApp.User")
    );
    assert_eq!(implements(&g), [("pkg::MyApp.User", "pkg::MyApp.Printer")]);
    assert_eq!(
        sites(
            &g,
            "pkg::MyApp.User",
            "pkg::MyApp.Printer",
            EdgeKind::Implements
        ),
        in_file("lib/impl.ex", &[2])
    );
}

#[test]
fn a_defimpl_with_a_list_inside_a_module_or_for_the_module_gives_one_module_per_type() {
    let (g, report) = run(&[
        MIX,
        (
            "lib/a.ex",
            "defprotocol Fmt do
  def fmt(x)
end
defmodule Left do
end
defmodule Right do
end
defimpl Fmt, for: [Left, Right] do
  def fmt(x), do: x
end
defmodule Num do
  defimpl Fmt do
    def fmt(x), do: x
  end
end
defmodule Two do
  defimpl Fmt, for: __MODULE__ do
    def fmt(x), do: x
  end
end
defimpl Fmt do
end
",
        ),
    ]);

    for (module, ty) in [
        ("Fmt.Left", "Left"),
        ("Fmt.Right", "Right"),
        ("Fmt.Num", "Num"),
        ("Fmt.Two", "Two"),
    ] {
        let package = node(&g, &format!("pkg::{module}"));
        assert_eq!(
            package.parent.as_deref(),
            Some(format!("pkg::{ty}").as_str())
        );
        assert_eq!(package.name, "Fmt");
        assert_eq!(
            node(&g, &format!("fn:{module}.fmt/1")).parent.as_deref(),
            Some(format!("pkg::{module}").as_str())
        );
    }
    assert_eq!(
        implements(&g),
        [
            ("pkg::Left", "pkg::Fmt"),
            ("pkg::Num", "pkg::Fmt"),
            ("pkg::Right", "pkg::Fmt"),
            ("pkg::Two", "pkg::Fmt"),
        ]
    );
    assert_eq!(forms(&report), [("defimpl", 1)]);
}

#[test]
fn a_defimpl_for_a_type_outside_the_project_nests_by_name_and_makes_an_edge_only_to_known_modules()
{
    let (g, report) = run(&[
        ("mix.exs", "def project, do: [apps_path: \"apps\"]\n"),
        ("apps/a/mix.exs", "def project, do: [app: :a]\n"),
        ("apps/b/mix.exs", "def project, do: [app: :b]\n"),
        ("apps/a/lib/thing.ex", "defmodule Thing do\nend\n"),
        (
            "apps/b/lib/fmt.ex",
            "defprotocol Fmt do
  def fmt(x)
end
defimpl Fmt, for: Thing do
end
defimpl Fmt, for: Atom do
end
defimpl Jason.Encoder, for: Decimal do
end
defmodule User do
  alias Jason
end
",
        ),
    ]);

    // A type of another project: no place below it, an edge all the same.
    assert_eq!(parent_of(&g, "pkg:apps/b:Fmt.Thing"), "pkg:apps/b:Fmt");
    assert_eq!(node(&g, "pkg:apps/b:Fmt.Thing").name, "Thing");
    assert_eq!(implements(&g), [("pkg:apps/a:Thing", "pkg:apps/b:Fmt")]);
    // A type outside the repository.
    assert_eq!(parent_of(&g, "pkg:apps/b:Fmt.Atom"), "pkg:apps/b:Fmt");
    assert_eq!(node(&g, "pkg:apps/b:Fmt.Atom").name, "Atom");
    // A protocol and a type outside the repository: a package of its own name.
    assert_eq!(
        parent_of(&g, "pkg:apps/b:Jason.Encoder.Decimal"),
        "mod:apps/b"
    );
    assert_eq!(
        node(&g, "pkg:apps/b:Jason.Encoder.Decimal").name,
        "Jason.Encoder.Decimal"
    );
    assert_eq!(
        report.references["defimpl"],
        ModuleRefs {
            resolved: 2,
            external: 1,
            ..ModuleRefs::default()
        }
    );
    assert_eq!(
        report.references["for"],
        ModuleRefs {
            resolved: 1,
            external: 2,
            ..ModuleRefs::default()
        }
    );
    // The module of a `defimpl` for an outside protocol does not make `Jason` a name
    // that the repository should declare.
    assert_eq!(report.references["alias"].external, 1);
    assert_eq!(report.unresolved_names.len(), 0);
}

#[test]
fn the_report_counts_each_reference_as_resolved_external_unresolved_or_dynamic() {
    let (_, report) = run_lib(
        "defmodule Known do
end
defmodule Missing.Other do
end
defmodule Ref do
  use Known
  alias Known.Gone
  import Known.Gone.Deep
  require Missing.Thing
  import :lists
  import Elixir.Enum
  import unquote(m)
end
",
    );

    assert_eq!(
        report.references["use"],
        ModuleRefs {
            resolved: 1,
            ..ModuleRefs::default()
        }
    );
    assert_eq!(
        report.references["alias"],
        ModuleRefs {
            unresolved: 1,
            ..ModuleRefs::default()
        }
    );
    assert_eq!(
        report.references["import"],
        ModuleRefs {
            external: 2,
            unresolved: 1,
            dynamic: 1,
            ..ModuleRefs::default()
        }
    );
    // `Missing.Other` is declared, `Missing` is not: `Missing.Thing` can be of a dependency.
    assert_eq!(
        report.references["require"],
        ModuleRefs {
            external: 1,
            ..ModuleRefs::default()
        }
    );
    assert_eq!(
        report.unresolved_names,
        BTreeMap::from([
            ("Known.Gone".to_string(), 1),
            ("Known.Gone.Deep".to_string(), 1)
        ])
    );
}

#[test]
fn the_result_does_not_depend_on_the_order_of_the_files() {
    let files = [
        MIX,
        TARGETS,
        ("apps/x/mix.exs", "def project, do: [app: :x]\n"),
        (
            "apps/x/lib/x.ex",
            "defmodule X do\n  alias T.B\n  def f, do: 1\nend\n",
        ),
        (
            "lib/a.ex",
            "defmodule A do\n  @behaviour T.E\n  import T.F\nend\ndefmodule A do\nend\n",
        ),
        (
            "lib/b.ex",
            "defmodule A do\nend\ndefimpl T.F, for: A do\nend\n",
        ),
        ("test/a_test.exs", "defmodule ATest do\n  alias A\nend\n"),
    ];
    let mut reversed = files;
    reversed.reverse();

    let (graph, report) = run(&files);

    assert!(!graph.edges.is_empty());
    assert_eq!(report.duplicate_modules.len(), 2);
    assert_eq!(run(&reversed), (graph, report));
}

#[test]
fn broken_sources_give_what_can_be_read_and_never_panic() {
    for text in [
        "defmodule",
        "defmodule do\nend\n",
        "defmodule A do\n  def\n  def (\n  alias\n  import A.{\n  @behaviour\n  @behaviour 5\nend\n",
        "defmodule A do\n  defimpl\n  defimpl A, for: []\n  defimpl 5, for: 6 do\n  end\nend\n",
        "defimpl",
        "defmodule A do\n  if\n  unless do\n  end\n  def f(",
        "alias A.{",
        "\u{0}\u{1}%{",
    ] {
        let (g, _) = run_lib(text);
        assert!(g.node("mod:").is_some());
    }
    let (g, _) = run_lib("defmodule A do\n  def f, do: 1\nend\ndefmodule B do\n  def\n");
    assert!(g.node("fn:A.f/0").is_some());
}

#[test]
fn the_layout_tells_test_code_from_the_mix_projects_without_reading_the_sources() {
    let source = |path: &str, text: &str| SourceFile {
        path: path.to_string(),
        text: text.to_string(),
    };
    let layout = Layout::new(&[
        source("mix.exs", "def project, do: [apps_path: \"apps\"]\n"),
        source("apps/a/mix.exs", "def project, do: [app: :a]\n"),
        source(
            "apps/b/mix.exs",
            "def project, do: [app: :b, elixirc_paths: [\"src\"]]\n",
        ),
        source("apps/a/lib/a.ex", "this text is never read"),
    ]);

    assert_eq!(layout.test_code("apps/a/test/a_test.exs"), Some(true));
    assert_eq!(layout.test_code("apps/a/test/support/h.ex"), Some(true));
    assert_eq!(layout.test_code("apps/a/lib/a.ex"), Some(false));
    assert_eq!(layout.test_code("apps/a/lib/src/x/a.ex"), Some(false));
    assert_eq!(layout.test_code("apps/b/src/b.ex"), Some(false));
    // Outside the source directories, outside a project, or not Elixir.
    assert_eq!(layout.test_code("apps/b/lib/b.ex"), None);
    assert_eq!(layout.test_code("apps/a/lib/a.rs"), None);
    assert_eq!(layout.test_code("apps/a/config/config.exs"), None);
    assert_eq!(layout.test_code("lib/stray.ex"), None);
    assert_eq!(layout.test_code("test/stray_test.exs"), None);
    assert_eq!(Layout::default().test_code("lib/a.ex"), None);
}

#[test]
fn modules_below_a_defimpl_that_stands_in_its_type_stand_in_it_and_each_id_is_one_node() {
    let g = lib("defmodule T do
end
defimpl Fmt, for: T do
  defmodule A do
  end
  defmodule B do
  end
end
");

    let mut unique = ids(&g);
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), g.nodes.len(), "{:?}", ids(&g));
    assert_eq!(parent_of(&g, "pkg::Fmt.T"), "pkg::T");
    assert_eq!(parent_of(&g, "pkg::Fmt.T.A"), "pkg::Fmt.T");
    assert_eq!(parent_of(&g, "pkg::Fmt.T.B"), "pkg::Fmt.T");
    assert_eq!(node(&g, "pkg::Fmt.T.A").name, "A");
}

#[test]
fn a_module_name_is_unique_in_the_repository_also_over_two_projects() {
    let (g, report) = run(&[
        ("mix.exs", "def project, do: [apps_path: \"apps\"]\n"),
        ("apps/a/mix.exs", "def project, do: [app: :a]\n"),
        ("apps/b/mix.exs", "def project, do: [app: :b]\n"),
        (
            "apps/a/lib/s.ex",
            "defmodule Shared do\n  def f, do: 1\nend\n",
        ),
        (
            "apps/b/lib/s.ex",
            "defmodule Shared do\n  def f, do: 2\n  def g, do: 2\nend\n",
        ),
    ]);

    assert_eq!(parent_of(&g, "fn:Shared.f/0"), "pkg:apps/a:Shared");
    assert_eq!(
        ids_of(&g, NodeKind::Package),
        ["pkg:apps/a:Shared"],
        "{:?}",
        ids(&g)
    );
    assert_eq!(ids_of(&g, NodeKind::Function), ["fn:Shared.f/0"]);
    assert_eq!(report.duplicate_modules, ["apps/b/lib/s.ex:1"]);
}

#[test]
fn a_defimpl_of_an_outside_protocol_stands_in_its_type_without_an_edge() {
    let (g, report) = run_lib(
        "defmodule User do
end
defimpl Jason.Encoder, for: User do
  def encode(u, o), do: u
end
",
    );

    let node = node(&g, "pkg::Jason.Encoder.User");
    assert_eq!(node.parent.as_deref(), Some("pkg::User"));
    assert_eq!(node.name, "Jason.Encoder");
    assert_eq!(
        parent_of(&g, "fn:Jason.Encoder.User.encode/2"),
        "pkg::Jason.Encoder.User"
    );
    assert_eq!(implements(&g), []);
    assert_eq!(
        report.references["defimpl"],
        ModuleRefs {
            external: 1,
            ..ModuleRefs::default()
        }
    );
    assert_eq!(
        report.references["for"],
        ModuleRefs {
            resolved: 1,
            ..ModuleRefs::default()
        }
    );
}

#[test]
fn an_app_that_is_no_atom_gives_no_name_and_the_first_app_wins() {
    let g = index(&[
        (
            "apps/web/mix.exs",
            "@app :x\ndef project, do: [app: @app, releases: [app: :late]]\n",
        ),
        (
            "apps/two/mix.exs",
            "def project, do: [app: :first, other: [app: :second]]\n",
        ),
    ]);

    assert_eq!(node(&g, "mod:apps/web").name, "web");
    assert_eq!(node(&g, "mod:apps/two").name, "first");
}

#[test]
fn a_defimpl_in_a_defimpl_stands_in_the_module_of_the_outer_one() {
    let g = lib("defprotocol P do
end
defprotocol Q do
end
defmodule T do
end
defimpl P, for: T do
  defimpl Q do
  end
end
");

    assert_eq!(parent_of(&g, "pkg::P.T"), "pkg::T");
    assert_eq!(parent_of(&g, "pkg::Q.P.T"), "pkg::P.T");
    assert_eq!(node(&g, "pkg::Q.P.T").name, "Q");
    // The edge of the inner one would join `P.T` with `Q`, which no ancestor relation
    // forbids; the edge of the outer one joins `T` with `P`.
    assert_eq!(
        implements(&g),
        [("pkg::P.T", "pkg::Q"), ("pkg::T", "pkg::P")]
    );
}

#[test]
fn a_nested_defmodule_of_several_segments_makes_an_alias_of_its_first_segment() {
    let g = lib("defmodule A do
  defmodule B.C do
  end
  import B.C
  defmodule User do
    import B.C
  end
end
defmodule X do
  alias A.B
  import B.C
end
");

    assert_eq!(parent_of(&g, "pkg::A.B.C"), "pkg::A");
    assert_eq!(imported(&g, "pkg::A.User"), [("pkg::A.B.C", vec![6])]);
    assert_eq!(imported(&g, "pkg::X"), [("pkg::A.B.C", vec![11])]);
}

#[test]
fn a_prefix_package_is_test_code_if_all_below_it_is() {
    let g = index(&[
        MIX,
        ("lib/u.ex", "defmodule App.Acc.User do\nend\n"),
        ("lib/a.ex", "defmodule App.Acc.Admin do\nend\n"),
        ("test/support/h.ex", "defmodule App.Support.H do\nend\n"),
        ("test/support/i.ex", "defmodule App.Support.I do\nend\n"),
    ]);

    assert!(node(&g, "pkg::App.Support").test);
    assert!(node(&g, "pkg::App.Support.H").test);
    assert!(!node(&g, "pkg::App.Acc").test);
}

#[test]
fn only_alias_and_require_with_as_make_an_alias_and_a_later_alias_hides_an_earlier_one() {
    let g = index(&[
        MIX,
        TARGETS,
        (
            "lib/plain.ex",
            "defmodule B.C do\nend\ndefmodule X.C do\nend\n",
        ),
        (
            "lib/m.ex",
            "defmodule M1 do
  import T.B
  use T.B
  require T.B
  import B.C
end
defmodule M2 do
  use T.B, as: X
  import X.C
end
defmodule M3 do
  alias T.B
  alias B.C, as: B
  import B
end
",
        ),
    ]);

    assert_eq!(
        imported(&g, "pkg::M1"),
        [("pkg::B.C", vec![5]), ("pkg::T.B", vec![2, 3, 4])]
    );
    assert_eq!(
        imported(&g, "pkg::M2"),
        [("pkg::T.B", vec![8]), ("pkg::X.C", vec![9])]
    );
    // `B` stands for `T.B`, then for `T.B.C`: the `import B` names the second.
    assert_eq!(
        imported(&g, "pkg::M3"),
        [("pkg::T.B", vec![12]), ("pkg::T.B.C", vec![13, 14])]
    );
}

#[test]
fn elixirc_paths_are_cleaned_and_only_plain_strings_count() {
    let files = |mix: &'static str| {
        [
            ("mix.exs", mix),
            ("src/s.ex", "defmodule InSrc do\nend\n"),
            ("lib/l.ex", "defmodule InLib do\nend\n"),
            ("top.ex", "defmodule AtTop do\nend\n"),
        ]
    };
    let packages = |mix: &'static str| {
        let (g, report) = run(&files(mix));
        let ids: Vec<String> = ids_of(&g, NodeKind::Package)
            .into_iter()
            .map(str::to_string)
            .collect();
        (ids, report.files_outside_source_dirs)
    };

    assert_eq!(
        packages("def project, do: [elixirc_paths: [\"./src/\"]]\n"),
        (vec!["pkg::InSrc".to_string()], 2)
    );
    assert_eq!(packages("def project, do: [elixirc_paths: [\".\"]]\n").1, 0);
    // No string literal: as if it were `lib`.
    assert_eq!(
        packages("def project, do: [elixirc_paths: paths()]\n"),
        (vec!["pkg::InLib".to_string()], 2)
    );
    assert_eq!(
        packages("def project, do: [elixirc_paths: [\"#{dir()}/src\"]]\n"),
        (vec!["pkg::InLib".to_string()], 2)
    );
}

#[test]
fn a_mix_exs_that_does_not_parse_is_still_a_project_with_lib() {
    let g = index(&[
        ("apps/x/mix.exs", "defmodule ( do\n  [app: \n"),
        ("apps/x/lib/a.ex", "defmodule A do\nend\n"),
    ]);

    assert_eq!(node(&g, "mod:apps/x").name, "x");
    assert_eq!(parent_of(&g, "pkg:apps/x:A"), "mod:apps/x");
}

#[test]
fn empty_parentheses_and_none_are_the_same_function() {
    let g = lib("defmodule A do\n  def f(), do: 1\n  def f, do: 2\nend\n");

    assert_eq!(ids_of(&g, NodeKind::Function), ["fn:A.f/0"]);
    assert_eq!(node(&g, "fn:A.f/0").span, span(2, 3));
}

#[test]
fn a_defimpl_for_a_type_that_is_not_written_or_twice_for_one_type_gives_no_second_node() {
    let (g, report) = run_lib(
        "defmodule T do
end
defimpl Fmt, for: unquote(t) do
  def f(x), do: x
end
defimpl Fmt, for: T do
end
defimpl Fmt, for: T do
  def g(x), do: x
end
",
    );

    assert_eq!(ids_of(&g, NodeKind::Package), ["pkg::Fmt.T", "pkg::T"]);
    assert_eq!(ids_of(&g, NodeKind::Function), [] as [&str; 0]);
    assert_eq!(forms(&report), [("defimpl", 1)]);
    assert_eq!(report.duplicate_modules, ["lib/a.ex:8"]);
    assert_eq!(report.references["for"].dynamic, 1);
}

#[test]
fn text_that_is_not_ascii_and_windows_line_ends_keep_spans_and_hashes() {
    let unix =
        "defmodule A do\n  @doc \"größer → ∞\"\n  def f(x), do: \"ü\" <> x\n  def g, do: 1\nend\n";
    let windows = unix.replace('\n', "\r\n");
    let g = lib(unix);
    let w = index(&[MIX, ("lib/a.ex", &windows)]);

    assert_eq!(node(&g, "fn:A.f/1").span, span(2, 3));
    assert_eq!(node(&g, "fn:A.g/0").span, span(4, 4));
    assert_eq!(node(&w, "fn:A.f/1").span, span(2, 3));
    assert_eq!(node(&w, "fn:A.f/1").hash, node(&g, "fn:A.f/1").hash);
    assert_ne!(node(&g, "fn:A.f/1").hash, node(&g, "fn:A.g/0").hash);
}

#[test]
fn a_script_outside_a_test_directory_and_a_hidden_script_there_are_inputs_by_their_place() {
    assert!(!is_input("lib/a.exs"));
    assert!(!is_input(".formatter.exs"));
    assert!(!is_input("priv/repo/migrations/1_create.exs"));
    assert!(is_input("test/test_helper.exs"));
}

#[test]
fn one_module_below_a_defimpl_that_stands_in_its_type_stands_in_it_too() {
    let g = lib("defmodule T do
end
defmodule Other do
end
defimpl Fmt, for: T do
  defmodule Only.Deep do
  end
end
");

    assert_eq!(parent_of(&g, "pkg::Fmt.T"), "pkg::T");
    assert_eq!(parent_of(&g, "pkg::Fmt.T.Only.Deep"), "pkg::Fmt.T");
    assert_eq!(node(&g, "pkg::Fmt.T.Only.Deep").name, "Only.Deep");
}

#[test]
fn main_code_declares_a_module_before_test_code_of_an_earlier_path() {
    let (g, report) = run(&[
        ("mix.exs", "def project, do: [apps_path: \"apps\"]\n"),
        ("apps/a/mix.exs", "def project, do: [app: :a]\n"),
        ("apps/b/mix.exs", "def project, do: [app: :b]\n"),
        (
            "apps/a/test/support/foo.ex",
            "defmodule Foo do\n  def fake, do: 1\nend\n",
        ),
        (
            "apps/b/lib/foo.ex",
            "defmodule Foo do\n  def real, do: 1\nend\n",
        ),
    ]);

    assert_eq!(ids_of(&g, NodeKind::Package), ["pkg:apps/b:Foo"]);
    assert_eq!(ids_of(&g, NodeKind::Function), ["fn:Foo.real/0"]);
    assert!(!node(&g, "pkg:apps/b:Foo").test);
    assert_eq!(report.duplicate_modules, ["apps/a/test/support/foo.ex:1"]);
}
