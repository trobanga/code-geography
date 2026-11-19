//! The `calls` edges and the call report (ADR 36).

use cg_core::{CallReport, EdgeKind, FileSites, Graph};
use cg_index_typescript::{Report, SourceFile, index_sources};

fn run(files: &[(&str, &str)]) -> (Graph, Report) {
    let mut all = vec![APP];
    all.extend(files);
    let files: Vec<SourceFile> = all
        .iter()
        .map(|(path, text)| SourceFile {
            path: path.to_string(),
            text: text.to_string(),
        })
        .collect();
    index_sources("repo", &files)
}

const APP: (&str, &str) = ("package.json", r#"{"name":"my-app"}"#);

type Sites<'a> = &'a [(Option<&'a str>, &'a [u32])];

/// The `calls` edges as `(from, to)`.
fn calls_of(g: &Graph) -> Vec<(&str, &str)> {
    g.edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Calls)
        .map(|e| (e.from.as_str(), e.to.as_str()))
        .collect()
}

fn assert_call(g: &Graph, from: &str, to: &str, sites: Sites) {
    let edge = g
        .edges
        .iter()
        .find(|e| e.kind == EdgeKind::Calls && e.from == from && e.to == to)
        .unwrap_or_else(|| panic!("no calls edge {from} -> {to}; edges: {:?}", calls_of(g)));
    let expected: Vec<FileSites> = sites
        .iter()
        .map(|(file, lines)| FileSites {
            file: file.map(str::to_string),
            lines: lines.to_vec(),
        })
        .collect();
    assert_eq!(edge.sites, expected, "sites of {from} -> {to}");
}

fn counts(to_function: usize, to_type: usize, external: usize, unresolved: usize) -> CallReport {
    CallReport {
        to_function,
        to_type,
        external,
        unresolved,
        to_value: 0,
    }
}

#[test]
fn a_function_calls_a_function_of_its_file_at_the_line_of_the_name() {
    let (g, report) = run(&[(
        "a.ts",
        "export function a() {\n  b();\n}\nfunction b() {}\n",
    )]);

    assert_eq!(calls_of(&g), vec![("fn::/a#a", "fn::/a#b")]);
    assert_call(&g, "fn::/a#a", "fn::/a#b", &[(None, &[2])]);
    assert_eq!(report.calls, counts(1, 0, 0, 0));
}

#[test]
fn a_call_of_a_global_is_external_and_makes_no_edge() {
    let (g, report) = run(&[(
        "a.ts",
        "export function a() {\n  setTimeout(() => {}, 1);\n  console.log('x');\n  JSON.stringify({});\n}\n",
    )]);

    assert_eq!(calls_of(&g), vec![]);
    assert_eq!(report.calls, counts(0, 0, 3, 0));
}

#[test]
fn a_call_of_an_imported_function_goes_to_it_through_re_exports_and_default_imports() {
    let cases = [
        "import { b } from './b';\nexport function a() {\n  b();\n}\n",
        "import { b as c } from './b';\nexport function a() {\n  c();\n}\n",
        "import { b } from './lib';\nexport function a() {\n  b();\n}\n",
        "import b from './b-default';\nexport function a() {\n  b();\n}\n",
    ];
    for text in cases {
        let (g, report) = run(&[
            ("a.ts", text),
            ("b.ts", "export function b() {}\n"),
            ("lib/index.ts", "export { b } from '../b';\n"),
            ("b-default.ts", "export default function b() {}\n"),
        ]);

        let to = if text.contains("b-default") {
            "fn::/b-default#b"
        } else {
            "fn::/b#b"
        };
        assert_eq!(calls_of(&g), vec![("fn::/a#a", to)], "{text}");
        assert_call(&g, "fn::/a#a", to, &[(None, &[3])]);
        assert_eq!(report.calls, counts(1, 0, 0, 0), "{text}");
    }
}

#[test]
fn a_call_of_an_import_that_reaches_no_function_or_leaves_the_repository() {
    let cases = [
        ("import { k } from './b';\nk();\n", (0, 0, 0, 1)),
        ("import { x } from './missing';\nx();\n", (0, 0, 0, 1)),
        ("import { x } from 'react';\nx();\n", (0, 0, 1, 0)),
        ("import { x } from './style.css';\nx();\n", (0, 0, 0, 1)),
    ];
    for (text, (function, to_type, external, unresolved)) in cases {
        let (g, report) = run(&[("a.ts", text), ("b.ts", "export const k = 1;\n")]);

        assert_eq!(calls_of(&g), vec![], "{text}");
        assert_eq!(
            report.calls,
            counts(function, to_type, external, unresolved),
            "{text}"
        );
    }
}

#[test]
fn a_name_that_a_function_binds_hides_the_function_of_the_file_in_the_whole_function() {
    let cases = [
        ("function a(b: () => void) {\n  b();\n}\n", 1),
        (
            "function a() {\n  if (x) {\n    const b = 1;\n  }\n  b();\n}\n",
            1,
        ),
        ("function a(b = 1) {\n  b();\n}\n", 1),
        ("function a({ b }: { b: () => void }) {\n  b();\n}\n", 1),
        ("function a([c, ...b]: any[]) {\n  b();\n}\n", 1),
        (
            "function a() {\n  const f = (b: () => void) => b();\n}\n",
            1,
        ),
        ("function a() {\n  const f = b => b();\n}\n", 1),
        ("function a() {\n  const { b } = x;\n  b();\n}\n", 1),
        (
            "function a() {\n  try {\n  } catch (b) {\n    b();\n  }\n}\n",
            1,
        ),
        (
            "function a() {\n  for (const b of x) {\n    b();\n  }\n}\n",
            1,
        ),
        (
            "function a() {\n  for (const b in x) {\n    b();\n  }\n}\n",
            1,
        ),
        ("function a() {\n  function b() {}\n  b();\n}\n", 1),
        ("function a() {\n  class b {}\n  b();\n}\n", 1),
        ("const a = function b() {\n  b();\n};\n", 1),
    ];
    for (text, unresolved) in cases {
        let (g, report) = run(&[("a.ts", &format!("{text}function b() {{}}\n"))]);

        assert_eq!(calls_of(&g), vec![], "{text}");
        assert_eq!(report.calls.unresolved, unresolved, "{text}");
        assert_eq!(report.calls.to_function, 0, "{text}");
    }
}

#[test]
fn a_call_in_a_function_that_is_no_node_counts_for_the_enclosing_node() {
    let cases = [
        "export function a() {\n  [1].forEach((x) => {\n    b();\n  });\n}\n",
        "export function a() {\n  function inner() {\n    b();\n  }\n}\n",
        "export function a() {\n  return {\n    m() {\n      b();\n    },\n  };\n}\n",
        "export function a() {\n  class Local {\n    m() {\n      b();\n    }\n  }\n}\n",
        "export const a = () => {\n  setTimeout(function () {\n    b();\n  });\n};\n",
    ];
    for text in cases {
        let (g, report) = run(&[("a.ts", &format!("{text}function b() {{}}\n"))]);

        assert_eq!(calls_of(&g), vec![("fn::/a#a", "fn::/a#b")], "{text}");
        assert_eq!(report.calls.to_function, 1, "{text}");
    }
}

#[test]
fn a_call_outside_every_node_is_an_edge_of_the_package_of_the_file() {
    let (g, report) = run(&[
        (
            "a.ts",
            "import { b } from './b';\nb();\nexport const C = memo(() => {\n  b();\n  own();\n});\nfunction own() {}\n",
        ),
        ("b.ts", "export function b() {}\n"),
    ]);

    assert_eq!(calls_of(&g), vec![("pkg::/a", "fn::/b#b")]);
    assert_call(&g, "pkg::/a", "fn::/b#b", &[(Some("a.ts"), &[2, 4])]);
    // `memo` is a global, `own()` is a call to a descendant of the package.
    assert_eq!(report.calls, counts(3, 0, 1, 0));
}

#[test]
fn a_call_at_the_top_of_an_index_file_is_an_edge_of_its_module() {
    let (g, _) = run(&[
        ("a/package.json", r#"{"name":"a-lib"}"#),
        ("a/index.ts", "import { b } from 'b-lib';\nb();\n"),
        ("b/package.json", r#"{"name":"b-lib"}"#),
        ("b/index.ts", "export function b() {}\n"),
    ]);

    assert_eq!(calls_of(&g), vec![("mod:a", "fn:b:/#b")]);
}

#[test]
fn a_function_in_a_let_or_var_is_no_proof_but_one_in_a_const_is() {
    let (g, report) = run(&[(
        "a.ts",
        "let l = () => {};\nvar v = function () {};\nconst c = () => {};\nexport function a() {\n  l();\n  v();\n  c();\n}\n",
    )]);

    assert_eq!(calls_of(&g), vec![("fn::/a#a", "fn::/a#c")]);
    assert_eq!(report.calls, counts(1, 0, 0, 2));
}

#[test]
fn a_member_of_a_namespace_import_is_looked_up_in_the_file_it_reaches() {
    let (g, report) = run(&[
        (
            "a.ts",
            "import * as ns from './b';\nimport * as fs from 'node:fs';\nimport * as lib from './lib';\nexport function a() {\n  ns.b();\n  ns.k();\n  fs.readFile();\n  lib.b();\n}\n",
        ),
        ("b.ts", "export function b() {}\nexport const k = 1;\n"),
        ("lib/index.ts", "export * from '../b';\n"),
    ]);

    assert_eq!(
        calls_of(&g),
        vec![("fn::/a#a", "fn::/b#b")],
        "{:?}",
        g.edges
    );
    assert_call(&g, "fn::/a#a", "fn::/b#b", &[(None, &[5, 8])]);
    assert_eq!(report.calls, counts(2, 0, 1, 1));
}

#[test]
fn this_is_the_class_in_methods_and_arrow_functions_but_not_in_a_function() {
    let (g, report) = run(&[(
        "a.ts",
        "export class T {\n  m() {\n    this.n();\n    const f = () => this.n();\n    function g() {\n      this.o();\n    }\n    const h = function () {\n      this.o();\n    };\n  }\n  n() {}\n  o() {}\n  p = () => this.n();\n  q = {\n    r() {\n      this.n();\n    },\n  };\n}\n",
    )]);

    assert_eq!(
        calls_of(&g),
        vec![("fn::/a#T.m", "fn::/a#T.n"), ("fn::/a#T.p", "fn::/a#T.n")]
    );
    assert_call(&g, "fn::/a#T.m", "fn::/a#T.n", &[(None, &[3, 4])]);
    assert_call(&g, "fn::/a#T.p", "fn::/a#T.n", &[(None, &[14])]);
    assert_eq!(report.calls, counts(3, 0, 0, 3));
}

const STORE: (&str, &str) = ("store.ts", "export class Store {\n  get() {}\n}\n");

#[test]
fn a_method_is_called_through_the_written_type_of_a_parameter() {
    let cases = [
        "export function a(s: Store) {\n  s.get();\n}\n",
        "export const a = (s: Store) => s.get();\n",
        "export function a(s?: Store) {\n  s?.get();\n}\n",
        "export function a(s: Store | undefined) {\n  s?.get();\n}\n",
        "export function a(s: null | Store | undefined) {\n  s!.get();\n}\n",
        "export function a(s: Store<number>) {\n  (s).get();\n}\n",
        "export function a(s: Store) {\n  s?.get?.();\n}\n",
    ];
    for text in cases {
        let (g, report) = run(&[
            (
                "a.ts",
                &format!("import {{ Store }} from './store';\n{text}"),
            ),
            STORE,
        ]);

        assert_eq!(
            calls_of(&g),
            vec![("fn::/a#a", "fn::/store#Store.get")],
            "{text}"
        );
        let line = if text.starts_with("export const") {
            2
        } else {
            3
        };
        assert_call(&g, "fn::/a#a", "fn::/store#Store.get", &[(None, &[line])]);
        assert_eq!(report.calls, counts(1, 0, 0, 0), "{text}");
    }
}

#[test]
fn a_call_through_an_interface_goes_to_its_signature() {
    let (g, _) = run(&[
        (
            "a.ts",
            "import { Store } from './store';\nexport function a(s: Store) {\n  s.get();\n}\n",
        ),
        ("store.ts", "export interface Store {\n  get(): void;\n}\n"),
    ]);

    assert_eq!(calls_of(&g), vec![("fn::/a#a", "fn::/store#Store.get")]);
}

#[test]
fn a_type_that_is_not_one_class_or_interface_of_the_repository_gives_no_edge() {
    let cases = [
        // A type parameter hides the type.
        (
            "export function a<Store>(s: Store) {\n  s.get();\n}\n",
            (0, 0, 0, 1),
        ),
        // Another union, an object type, a function type, an intersection.
        (
            "export function a(s: Store | Other) {\n  s.get();\n}\n",
            (0, 0, 0, 1),
        ),
        (
            "export function a(s: { get(): void }) {\n  s.get();\n}\n",
            (0, 0, 0, 1),
        ),
        (
            "export function a(s: () => void) {\n  s.get();\n}\n",
            (0, 0, 0, 1),
        ),
        (
            "export function a(s: Store & Other) {\n  s.get();\n}\n",
            (0, 0, 0, 1),
        ),
        // No type is written.
        ("export function a(s) {\n  s.get();\n}\n", (0, 0, 0, 1)),
        // A type of the language or of a library.
        (
            "export function a(s: string) {\n  s.trim();\n}\n",
            (0, 0, 1, 0),
        ),
        (
            "export function a(s: Store[]) {\n  s.map();\n}\n",
            (0, 0, 1, 0),
        ),
        (
            "export function a(s: Array<Store>) {\n  s.map();\n}\n",
            (0, 0, 1, 0),
        ),
        (
            "export function a(s: HTMLElement) {\n  s.focus();\n}\n",
            (0, 0, 1, 0),
        ),
        // A type alias and an enum have no member nodes.
        (
            "export function a(s: Alias) {\n  s.get();\n}\n",
            (0, 0, 0, 1),
        ),
        (
            "export function a(s: Color) {\n  s.get();\n}\n",
            (0, 0, 0, 1),
        ),
    ];
    for (text, (function, to_type, external, unresolved)) in cases {
        let source = format!(
            "import {{ Store }} from './store';\ntype Alias = Store;\nenum Color {{ Red }}\n{text}"
        );
        let (g, report) = run(&[("a.ts", &source), STORE]);

        assert_eq!(calls_of(&g), vec![], "{text}");
        assert_eq!(
            report.calls,
            counts(function, to_type, external, unresolved),
            "{text}"
        );
    }
}

#[test]
fn a_variable_has_the_type_that_is_written_or_that_new_makes() {
    let cases = [
        (
            "export function a() {\n  const x: Store = make();\n  x.get();\n}\n",
            true,
        ),
        (
            "export function a() {\n  let x: Store;\n  x.get();\n}\n",
            true,
        ),
        (
            "export function a() {\n  const x = new Store();\n  x.get();\n}\n",
            true,
        ),
        (
            "const x = new Store();\nexport function a() {\n  x.get();\n}\n",
            true,
        ),
        (
            "const x: Store = make();\nexport function a() {\n  x.get();\n}\n",
            true,
        ),
        (
            "export function a() {\n  const x = make();\n  x.get();\n}\n",
            false,
        ),
        (
            "export function a() {\n  let x = new Store();\n  x.get();\n}\n",
            false,
        ),
        (
            "export function a() {\n  const { x } = make();\n  x.get();\n}\n",
            false,
        ),
        (
            "export function a() {\n  if (c) {\n    const x: Store = 1;\n  } else {\n    const x = 2;\n  }\n  x.get();\n}\n",
            false,
        ),
    ];
    for (text, edge) in cases {
        let source = format!("import {{ Store }} from './store';\n{text}");
        let (g, _) = run(&[("a.ts", &source), STORE]);

        let reached = calls_of(&g).contains(&("fn::/a#a", "fn::/store#Store.get"));
        assert_eq!(reached, edge, "{text}");
    }
}

#[test]
fn a_method_is_called_through_a_field_with_a_type() {
    let source = "import { Store } from './store';\nexport interface P {\n  p: Store;\n}\nexport class A {\n  private s: Store;\n  t = new Store();\n  constructor(private readonly u: Store, v: Store, public w: Store) {}\n  m(a: P) {\n    this.s.get();\n    this.t.get();\n    this.u.get();\n    this.w.get();\n    this.v.get();\n    a.p.get();\n  }\n}\n";
    let (g, report) = run(&[("a.ts", source), STORE]);

    assert_call(
        &g,
        "fn::/a#A.m",
        "fn::/store#Store.get",
        &[(None, &[10, 11, 12, 13, 15])],
    );
    // `this.v` is no field.
    assert_eq!(report.calls.unresolved, 1);
    assert_eq!(report.calls.to_function, 5);
}

#[test]
fn a_type_parameter_of_a_class_hides_the_type_in_its_methods_and_fields() {
    let source = "import { Store } from './store';\nexport class A<Store> {\n  s: Store;\n  m(p: Store) {\n    p.get();\n    this.s.get();\n  }\n}\n";
    let (g, report) = run(&[("a.ts", source), STORE]);

    assert_eq!(calls_of(&g), vec![]);
    assert_eq!(report.calls, counts(0, 0, 0, 2));
}

#[test]
fn a_method_is_looked_up_in_the_supertypes_nearest_first() {
    let base = "export class A {\n  m() {}\n}\n";
    let source = "import { A } from './base';\nclass M extends A {\n  m() {}\n}\nexport class B extends A {\n  go() {\n    super.m();\n    this.m();\n  }\n}\nexport class C extends M {}\nexport function f(b: B, c: C) {\n  b.m();\n  c.m();\n}\n";
    let (g, report) = run(&[("a.ts", source), ("base.ts", base)]);

    assert_eq!(
        calls_of(&g),
        vec![
            ("fn::/a#B.go", "fn::/base#A.m"),
            ("fn::/a#f", "fn::/a#M.m"),
            ("fn::/a#f", "fn::/base#A.m"),
        ]
    );
    assert_call(&g, "fn::/a#B.go", "fn::/base#A.m", &[(None, &[7, 8])]);
    assert_call(&g, "fn::/a#f", "fn::/base#A.m", &[(None, &[13])]);
    assert_call(&g, "fn::/a#f", "fn::/a#M.m", &[(None, &[14])]);
    assert_eq!(report.calls, counts(4, 0, 0, 0));
}

#[test]
fn a_member_that_no_function_node_declares_is_external_or_a_call_to_the_type() {
    let source = "import * as React from 'react';\nexport interface I {\n  onChange: () => void;\n}\nexport interface I1 {\n  m(): void;\n}\nexport interface I2 {\n  m(): void;\n}\nexport interface I3 extends I1, I2 {}\nexport class C implements I1 {\n  m() {}\n}\nexport class W extends React.Component {\n  r() {\n    this.setState();\n  }\n}\nexport class P {\n  r() {\n    this.toString();\n  }\n}\nexport function f(i: I, j: I3, c: C) {\n  i.onChange();\n  j.m();\n}\n";
    let (g, report) = run(&[("a.ts", source)]);

    assert_eq!(
        calls_of(&g),
        vec![("fn::/a#f", "type::/a#I"), ("fn::/a#f", "type::/a#I3")]
    );
    assert_call(&g, "fn::/a#f", "type::/a#I", &[(None, &[26])]);
    assert_call(&g, "fn::/a#f", "type::/a#I3", &[(None, &[27])]);
    // `this.setState()` of an external superclass and `this.toString()`.
    assert_eq!(report.calls, counts(0, 2, 2, 0));
}

#[test]
fn a_static_method_is_called_through_the_class_of_the_file_an_import_or_a_namespace_import() {
    let store = ("store.ts", "export class Store {\n  static open() {}\n}\n");
    let cases = [
        "class Store {\n  static open() {}\n}\nexport function a() {\n  Store.open();\n}\n",
        "import { Store } from './store';\nexport function a() {\n  Store.open();\n}\n",
        "import * as s from './store';\nexport function a() {\n  s.Store.open();\n}\n",
        "import { Store } from './store';\nexport function a() {\n  (Store).open();\n}\n",
    ];
    for text in cases {
        let (g, report) = run(&[("a.ts", text), store]);

        let own = text.starts_with("class");
        let to = if own {
            "fn::/a#Store.open"
        } else {
            "fn::/store#Store.open"
        };
        assert_eq!(calls_of(&g), vec![("fn::/a#a", to)], "{text}");
        assert_eq!(report.calls, counts(1, 0, 0, 0), "{text}");
    }
}

#[test]
fn a_new_goes_to_the_constructor_else_to_the_type_and_super_to_that_of_the_parent() {
    let store = (
        "store.ts",
        "export class Store {\n  constructor() {}\n  get() {}\n}\n",
    );
    let plain = ("plain.ts", "export class Plain {}\n");
    let source = "import * as React from 'react';\nimport { Store } from './store';\nimport { Plain } from './plain';\nexport function a() {\n  new Store();\n  new Plain();\n  new Map();\n  new Store().get();\n}\nexport class B extends Store {\n  constructor() {\n    super();\n  }\n}\nexport class C extends Plain {\n  constructor() {\n    super();\n  }\n}\nexport class D extends React.Component {\n  constructor() {\n    super();\n  }\n}\n";
    let (g, report) = run(&[("a.ts", source), store, plain]);

    assert_eq!(
        calls_of(&g),
        vec![
            ("fn::/a#B.constructor", "fn::/store#Store.constructor"),
            ("fn::/a#C.constructor", "type::/plain#Plain"),
            ("fn::/a#a", "fn::/store#Store.constructor"),
            ("fn::/a#a", "fn::/store#Store.get"),
            ("fn::/a#a", "type::/plain#Plain"),
        ]
    );
    assert_call(
        &g,
        "fn::/a#a",
        "fn::/store#Store.constructor",
        &[(None, &[5, 8])],
    );
    assert_call(&g, "fn::/a#a", "type::/plain#Plain", &[(None, &[6])]);
    assert_call(
        &g,
        "fn::/a#B.constructor",
        "fn::/store#Store.constructor",
        &[(None, &[12])],
    );
    assert_eq!(report.calls, counts(4, 2, 2, 0));
}

#[test]
fn a_new_of_a_class_outside_the_repository_makes_a_variable_external() {
    let (g, report) = run(&[(
        "a.ts",
        "export function a() {\n  const m = new Map();\n  m.get(1);\n  const f = new Local();\n  f.x();\n}\n",
    )]);

    assert_eq!(calls_of(&g), vec![]);
    // `new Map()`, `m.get(1)` and the global `new Local()` and `f.x()`.
    assert_eq!(report.calls, counts(0, 0, 4, 0));
}

#[test]
fn a_jsx_element_with_a_capital_or_member_tag_is_a_call_of_the_component() {
    let source = "import { A, K } from './comp';\nimport * as ui from './ui';\nexport function Page() {\n  return (\n    <div>\n      <A />\n      <A>text</A>\n      <ui.B />\n      <K />\n      <Missing />\n    </div>\n  );\n}\n";
    let (g, report) = run(&[
        ("page.tsx", source),
        (
            "comp.tsx",
            "export function A() {\n  return null;\n}\nexport class K {}\n",
        ),
        ("ui.tsx", "export function B() {\n  return null;\n}\n"),
    ]);

    assert_eq!(
        calls_of(&g),
        vec![
            ("fn::/page#Page", "fn::/comp#A"),
            ("fn::/page#Page", "fn::/ui#B"),
            ("fn::/page#Page", "type::/comp#K"),
        ]
    );
    assert_call(&g, "fn::/page#Page", "fn::/comp#A", &[(None, &[6, 7])]);
    assert_call(&g, "fn::/page#Page", "fn::/ui#B", &[(None, &[8])]);
    assert_call(&g, "fn::/page#Page", "type::/comp#K", &[(None, &[9])]);
    assert_eq!(report.calls, counts(3, 1, 1, 0));
    assert_eq!(report.jsx_elements, 5);
}

#[test]
fn a_function_named_as_an_argument_or_a_jsx_attribute_is_a_call() {
    let source = "import { f } from './f';\nimport { Store } from './store';\nconst k = 1;\nfunction onSave() {}\nexport function a(s: Store, items: string[], other: any) {\n  items.map(f);\n  setTimeout(onSave, 1);\n  items.map(k);\n  items.map(s.get);\n  items.map(other);\n  return <button onClick={onSave} onBlur={k} />;\n}\n";
    let (g, report) = run(&[
        ("a.tsx", source),
        ("f.ts", "export function f() {}\n"),
        STORE,
    ]);

    assert_eq!(
        calls_of(&g),
        vec![
            ("fn::/a#a", "fn::/a#onSave"),
            ("fn::/a#a", "fn::/f#f"),
            ("fn::/a#a", "fn::/store#Store.get"),
        ]
    );
    assert_call(&g, "fn::/a#a", "fn::/a#onSave", &[(None, &[7, 11])]);
    assert_call(&g, "fn::/a#a", "fn::/f#f", &[(None, &[6])]);
    assert_call(&g, "fn::/a#a", "fn::/store#Store.get", &[(None, &[9])]);
    // Four `items.map(..)` and `setTimeout(..)` are external.
    assert_eq!(report.calls, counts(4, 0, 5, 0));
    assert_eq!(report.function_values, 4);
}

#[test]
fn a_call_in_a_field_initializer_is_a_call_of_the_type() {
    let (g, report) = run(&[
        (
            "a.ts",
            "import { b } from './b';\nexport class A {\n  x = b();\n  static y = b();\n  m() {\n    b();\n  }\n}\n",
        ),
        ("b.ts", "export function b() {}\n"),
    ]);

    assert_eq!(
        calls_of(&g),
        vec![("fn::/a#A.m", "fn::/b#b"), ("type::/a#A", "fn::/b#b")]
    );
    assert_call(&g, "type::/a#A", "fn::/b#b", &[(None, &[3, 4])]);
    assert_eq!(report.calls, counts(3, 0, 0, 0));
}

#[test]
fn a_name_in_a_namespace_is_looked_up_in_the_namespaces_around_then_in_the_file() {
    let source = "export namespace N {\n  export function f() {}\n  export function g() {\n    f();\n    N.f();\n  }\n  export namespace M {\n    export function h() {\n      f();\n      g();\n    }\n  }\n}\nnamespace P {\n  const x = top2();\n  const y = f();\n}\nexport function f() {}\nfunction top2() {}\nexport function top() {\n  N.f();\n  N.g();\n  N.M.h();\n  N.k();\n}\n";
    let (g, report) = run(&[("a.ts", source)]);

    assert_eq!(
        calls_of(&g),
        vec![
            ("fn::/a#N.M.h", "fn::/a#N.f"),
            ("fn::/a#N.M.h", "fn::/a#N.g"),
            ("fn::/a#N.g", "fn::/a#N.f"),
            ("fn::/a#top", "fn::/a#N.M.h"),
            ("fn::/a#top", "fn::/a#N.f"),
            ("fn::/a#top", "fn::/a#N.g"),
            ("pkg::/a#P", "fn::/a#f"),
            ("pkg::/a#P", "fn::/a#top2"),
        ]
    );
    assert_call(&g, "fn::/a#N.g", "fn::/a#N.f", &[(None, &[4, 5])]);
    assert_call(&g, "fn::/a#top", "fn::/a#N.f", &[(None, &[21])]);
    // `N.k()` names nothing of the namespace.
    assert_eq!(report.calls, counts(9, 0, 0, 1));
}

#[test]
fn a_dynamic_import_is_an_import_and_no_call_site() {
    let (g, report) = run(&[
        (
            "a.ts",
            "export async function a() {\n  const m = await import('./b');\n  return m;\n}\n",
        ),
        ("b.ts", "export const k = 1;\n"),
    ]);

    assert_eq!(calls_of(&g), vec![]);
    assert_eq!(report.calls, counts(0, 0, 0, 0));
}

#[test]
fn values_and_types_are_looked_up_apart() {
    let cases = [
        // A local type does not hide the function of that name.
        (
            "function a() {\n  interface b {}\n  type c = b;\n  b();\n}\n",
            true,
        ),
        // An assignment in a `for` declares nothing.
        ("function a() {\n  for (b in x) {}\n  b();\n}\n", true),
        // An enum is a value.
        ("function a() {\n  enum b { A }\n  b();\n}\n", false),
    ];
    for (text, edge) in cases {
        let (g, _) = run(&[("a.ts", &format!("{text}function b() {{}}\n"))]);

        let expected = if edge {
            vec![("fn::/a#a", "fn::/a#b")]
        } else {
            vec![]
        };
        assert_eq!(calls_of(&g), expected, "{text}");
    }
}

#[test]
fn a_local_type_hides_the_type_of_a_variable_but_not_a_value() {
    let source = "import { Store } from './store';\nexport function a(s: Store) {\n  type Store = string;\n  s.get();\n}\nexport function b(s: Store) {\n  s.get();\n}\n";
    let (g, _) = run(&[("a.ts", source), STORE]);

    assert_eq!(calls_of(&g), vec![("fn::/a#b", "fn::/store#Store.get")]);
}

#[test]
fn a_class_called_without_new_and_a_function_as_receiver() {
    let source = "import { b } from './b';\nclass K {}\nexport function a() {\n  K();\n  b.call(null);\n  b.bind(null);\n}\n";
    let (g, report) = run(&[("a.ts", source), ("b.ts", "export function b() {}\n")]);

    assert_eq!(calls_of(&g), vec![]);
    // `b.call` and `b.bind` are members of `Function`.
    assert_eq!(report.calls, counts(0, 0, 2, 1));
}

#[test]
fn this_and_super_are_unknown_where_the_class_is_no_node_or_extends_nothing() {
    let source = "export class A {\n  m() {\n    super.x();\n  }\n}\nexport class B extends mixin(A) {\n  m() {\n    super.x();\n    super();\n  }\n}\nexport const C = class {\n  m() {\n    this.n();\n  }\n  n() {}\n};\nexport default class {\n  m() {\n    this.n();\n  }\n  n() {}\n}\n";
    let (g, report) = run(&[("a.ts", source)]);

    // Only the unnamed default class is a node: `this.n()` in it.
    assert_eq!(calls_of(&g), vec![("fn::/a#default.m", "fn::/a#default.n")]);
    // `mixin(..)` is a global, the three `super` and `this.n()` of `C` are unknown.
    assert_eq!(report.calls, counts(1, 0, 1, 4));
}

#[test]
fn a_cycle_of_supertypes_ends() {
    let source = "export class A extends B {}\nexport class B extends A {}\nexport function f(a: A) {\n  a.x();\n}\n";
    let (g, report) = run(&[("a.ts", source)]);

    assert_eq!(calls_of(&g), vec![]);
    assert_eq!(report.calls, counts(0, 0, 1, 0));
}

#[test]
fn a_new_of_a_local_name_or_a_call_of_a_call_is_unresolved() {
    let source = "export function a(c, d) {\n  new c();\n  d()();\n}\n";
    let (g, report) = run(&[("a.ts", source)]);

    assert_eq!(calls_of(&g), vec![]);
    // `c` and `d` are parameters: `new c()`, `d()` and the call of its result.
    assert_eq!(report.calls, counts(0, 0, 0, 3));
}

#[test]
fn a_field_without_a_type_and_a_conflict_of_types_give_no_edge_and_equal_types_do() {
    let source = "import { Store } from './store';\nexport class A {\n  u = make();\n  m() {\n    this.u.get();\n    const x: Store = 1;\n    const x: Store = 2;\n    x.get();\n    const y: Store = 1;\n    const y: Other = 2;\n    y.get();\n  }\n}\n";
    let (g, report) = run(&[("a.ts", source), STORE]);

    assert_eq!(calls_of(&g), vec![("fn::/a#A.m", "fn::/store#Store.get")]);
    // `make()` is a global; `this.u.get()` and `y.get()` are unknown.
    assert_eq!(report.calls, counts(1, 0, 1, 2));
}

#[test]
fn a_type_in_a_namespace_import_is_a_type_of_the_repository() {
    let source =
        "import * as ns from './store';\nexport function a(s: ns.Store) {\n  s.get();\n}\n";
    let (g, _) = run(&[("a.ts", source), STORE]);

    assert_eq!(calls_of(&g), vec![("fn::/a#a", "fn::/store#Store.get")]);
}

#[test]
fn a_method_named_as_a_value_through_this_is_a_call() {
    let source =
        "export class A {\n  m(items: string[]) {\n    items.map(this.n);\n  }\n  n() {}\n}\n";
    let (g, report) = run(&[("a.ts", source)]);

    assert_eq!(calls_of(&g), vec![("fn::/a#A.m", "fn::/a#A.n")]);
    assert_eq!(report.function_values, 1);
}

#[test]
fn the_old_module_syntax_is_a_namespace_and_a_module_of_a_string_is_not() {
    let source = "module M {\n  export function h() {}\n  export function g() {\n    h();\n  }\n}\ndeclare module \"x\" {\n  export function y(): void;\n}\nexport function f() {\n  M.h();\n}\n";
    let (g, report) = run(&[("a.ts", source)]);

    assert_eq!(
        calls_of(&g),
        vec![("fn::/a#M.g", "fn::/a#M.h"), ("fn::/a#f", "fn::/a#M.h")]
    );
    assert_eq!(report.calls, counts(2, 0, 0, 0));
}

#[test]
fn overload_signatures_and_their_implementation_are_one_function() {
    let source = "function b(x: string): void;\nfunction b(x: number): void;\nfunction b(x: any) {}\nexport function a() {\n  b(1);\n}\n";
    let (g, _) = run(&[("a.ts", source)]);

    assert_eq!(calls_of(&g), vec![("fn::/a#a", "fn::/a#b")]);
}

#[test]
fn a_jsx_attribute_with_a_string_is_no_function_value() {
    let source = "export function Page() {\n  return <Missing label=\"x\" {...rest} />;\n}\n";
    let (g, report) = run(&[("page.tsx", source)]);

    assert_eq!(calls_of(&g), vec![]);
    assert_eq!(report.calls, counts(0, 0, 1, 0));
    assert_eq!(report.function_values, 0);
}

#[test]
fn a_receiver_of_type_any_or_unknown_is_unresolved() {
    let source =
        "export function a(x: any, y: unknown, z: string) {\n  x.m();\n  y.m();\n  z.trim();\n}\n";
    let (g, report) = run(&[("a.ts", source)]);

    assert_eq!(calls_of(&g), vec![]);
    assert_eq!(report.calls, counts(0, 0, 1, 2));
}

#[test]
fn a_member_that_no_type_declares_is_unresolved_if_a_supertype_is_not_resolved() {
    let source = "import { Base } from './missing';\ntype Alias = { m(): void };\nclass B extends Base {}\ninterface C extends Alias {}\nexport function a(b: B, c: C) {\n  b.m();\n  c.m();\n}\n";
    let (g, report) = run(&[("a.ts", source)]);

    assert_eq!(calls_of(&g), vec![]);
    assert_eq!(report.calls, counts(0, 0, 0, 2));
}

#[test]
fn a_static_block_is_a_scope_of_the_names_its_body_declares_and_belongs_to_the_class() {
    let hidden = "function helper() {}\nclass C {\n  static {\n    const helper = () => 1;\n    helper();\n  }\n}\n";
    let (g, report) = run(&[("a.ts", hidden)]);

    assert_eq!(calls_of(&g), vec![]);
    assert_eq!(report.calls, counts(0, 0, 0, 1));

    let open = "function helper() {}\nclass C {\n  static {\n    helper();\n  }\n}\n";
    let (g, _) = run(&[("a.ts", open)]);

    assert_eq!(calls_of(&g), vec![("type::/a#C", "fn::/a#helper")]);
}

#[test]
fn the_name_of_a_class_expression_is_bound_in_its_body() {
    let source = "import { B } from './b';\nexport const A = class B {\n  static make() {\n    return new B();\n  }\n};\n";
    let (g, report) = run(&[("a.ts", source), ("b.ts", "export function B() {}\n")]);

    assert_eq!(calls_of(&g), vec![]);
    assert_eq!(report.calls, counts(0, 0, 0, 1));
}

#[test]
fn two_bodies_of_one_namespace_give_no_edge_through_a_plain_name_of_either() {
    let cases = [
        // A name of the first body hides the function of the file in the second.
        "function f() {}\nnamespace A {\n  const f = 1;\n}\nnamespace A {\n  export function g() {\n    f();\n  }\n}\n",
        // A name of the second body is not visible in the first.
        "namespace A {\n  export function g() {\n    h();\n  }\n}\nnamespace A {\n  function h() {}\n}\n",
    ];
    for text in cases {
        let (g, report) = run(&[("a.ts", text)]);

        assert_eq!(calls_of(&g), vec![], "{text}");
        assert_eq!(report.calls, counts(0, 0, 0, 1), "{text}");
    }
}

#[test]
fn the_name_of_an_import_alias_is_bound_in_its_scope() {
    let source = "function x() {}\nnamespace A {\n  export function b() {}\n}\nnamespace N {\n  import x = A.b;\n  export function g() {\n    x();\n  }\n}\n";
    let (g, report) = run(&[("a.ts", source)]);

    assert_eq!(calls_of(&g), vec![]);
    assert_eq!(report.calls, counts(0, 0, 0, 1));
}

#[test]
fn the_type_of_a_variable_is_resolved_where_it_is_declared() {
    let cache = ("cache.ts", "export class Cache {\n  get() {}\n}\n");
    // A parameter of another function with the name of the class does not matter.
    let shadowed = "import { Store } from './store';\nimport { Cache } from './cache';\nconst store = new Store();\nexport function g(Store: Cache) {\n  store.get();\n}\n";
    let (g, _) = run(&[("a.ts", shadowed), STORE, cache]);

    assert_eq!(
        calls_of(&g),
        vec![
            ("fn::/a#g", "fn::/store#Store.get"),
            ("pkg::/a", "type::/store#Store")
        ]
    );

    // A class of a namespace with the name of the class of the file does not matter.
    let in_namespace = "import { Store } from './store';\nconst s = new Store();\nnamespace N {\n  class Store {\n    get() {}\n  }\n  export function g() {\n    s.get();\n  }\n}\n";
    let (g, _) = run(&[("a.ts", in_namespace), STORE]);

    assert_eq!(
        calls_of(&g),
        vec![
            ("fn::/a#N.g", "fn::/store#Store.get"),
            ("pkg::/a", "type::/store#Store")
        ]
    );
}

#[test]
fn the_type_of_a_variable_of_an_outer_function_is_resolved_outside_the_inner_one() {
    let cache = ("cache.ts", "export class Cache {\n  get() {}\n}\n");
    let source = "import { Store } from './store';\nimport { Cache } from './cache';\nexport function g() {\n  const s = new Store();\n  const inner = (Store: Cache) => s.get();\n}\n";
    let (g, _) = run(&[("a.ts", source), STORE, cache]);

    assert!(calls_of(&g).contains(&("fn::/a#g", "fn::/store#Store.get")));
    assert!(!calls_of(&g).contains(&("fn::/a#g", "fn::/cache#Cache.get")));
}

#[test]
fn a_member_that_shares_its_id_with_another_counts_its_calls_for_the_one_node() {
    let source = "import { other } from './b';\nexport class C {\n  get x() {\n    return 1;\n  }\n  set x(v) {\n    other(v);\n  }\n  m() {}\n  static m() {\n    other(1);\n  }\n  n() {\n    this.x;\n  }\n}\n";
    let (g, report) = run(&[("a.ts", source), ("b.ts", "export function other() {}\n")]);

    assert_eq!(
        calls_of(&g),
        vec![
            ("fn::/a#C.m", "fn::/b#other"),
            ("fn::/a#C.x", "fn::/b#other")
        ]
    );
    assert_eq!(report.calls, counts(2, 0, 0, 0));
}

#[test]
fn a_decorator_of_a_class_or_a_method_counts_for_the_type() {
    let deco = ("d.ts", "export function Injectable() {}\n");
    let cases = [
        (
            "import { Injectable } from './d';\n@Injectable()\nexport class A {}\n",
            "type::/a#A",
        ),
        (
            "import { Injectable } from './d';\n@Injectable()\nclass A {}\n",
            "type::/a#A",
        ),
        (
            "import { Injectable } from './d';\n@Injectable()\nexport default class {}\n",
            "type::/a#default",
        ),
        (
            "import { Injectable } from './d';\nexport class A {\n  @Injectable()\n  m() {}\n}\n",
            // In this grammar a decorator of a method is a child of the class body, beside
            // the method: it counts for the type.
            "type::/a#A",
        ),
    ];
    for (text, from) in cases {
        let (g, report) = run(&[("a.ts", text), deco]);

        assert_eq!(calls_of(&g), vec![(from, "fn::/d#Injectable")], "{text}");
        assert_eq!(report.calls, counts(1, 0, 0, 0), "{text}");
    }
}

#[test]
fn a_call_through_the_value_of_a_getter_is_unresolved_but_one_through_a_function_is_external() {
    let source = "import { Store } from './store';\nexport class A {\n  get store(): Store {\n    return make();\n  }\n  run() {\n    this.store.get();\n    this.run.bind(this);\n  }\n}\n";
    let (g, report) = run(&[("a.ts", source), STORE]);

    assert_eq!(calls_of(&g), vec![]);
    // `make()` and `this.run.bind(..)` are external, `this.store.get()` unresolved.
    assert_eq!(report.calls, counts(0, 0, 2, 1));
}

#[test]
fn the_type_of_a_field_is_resolved_in_the_file_that_declares_the_field() {
    let holder = (
        "holder.ts",
        "import { Store } from './store';\nexport class Holder {\n  s: Store;\n}\n",
    );
    let source = "import { Holder } from './holder';\nclass Store {\n  get() {}\n}\nexport function f(h: Holder) {\n  h.s.get();\n}\n";
    let (g, _) = run(&[("a.ts", source), holder, STORE]);

    assert_eq!(calls_of(&g), vec![("fn::/a#f", "fn::/store#Store.get")]);
}

#[test]
fn a_call_in_a_chain_over_several_lines_has_the_line_of_its_name() {
    let source =
        "import { Store } from './store';\nexport function a(s: Store) {\n  s\n    .get();\n}\n";
    let (g, _) = run(&[("a.ts", source), STORE]);

    assert_call(&g, "fn::/a#a", "fn::/store#Store.get", &[(None, &[4])]);
}

#[test]
fn a_tagged_template_is_a_call_of_its_tag() {
    let source = "import { sql } from './sql';\nimport styled from 'styled-components';\nexport function a() {\n  sql`select 1`;\n  styled.div`x`;\n}\n";
    let (g, report) = run(&[("a.ts", source), ("sql.ts", "export function sql() {}\n")]);

    assert_eq!(calls_of(&g), vec![("fn::/a#a", "fn::/sql#sql")]);
    assert_eq!(report.calls, counts(1, 0, 1, 0));
}

#[test]
fn a_new_of_a_function_node_is_a_call_of_the_function() {
    let (g, report) = run(&[(
        "a.ts",
        "function F() {}\nexport function a() {\n  new F();\n}\n",
    )]);

    assert_eq!(calls_of(&g), vec![("fn::/a#a", "fn::/a#F")]);
    assert_eq!(report.calls, counts(1, 0, 0, 0));
}

#[test]
fn a_call_to_itself_or_to_the_own_type_is_counted_and_makes_no_edge() {
    let source = "export function f() {\n  f();\n}\nexport class A {\n  static i = new A();\n}\n";
    let (g, report) = run(&[("a.ts", source)]);

    assert_eq!(calls_of(&g), vec![]);
    assert_eq!(report.calls, counts(1, 1, 0, 0));
}

#[test]
fn a_name_that_two_stars_reach_is_unknown() {
    let (g, report) = run(&[
        (
            "a.ts",
            "import { K } from './index';\nexport function a() {\n  new K();\n}\n",
        ),
        ("index.ts", "export * from './x';\nexport * from './y';\n"),
        ("x.ts", "export class K {}\n"),
        ("y.ts", "export class K {}\n"),
    ]);

    assert_eq!(calls_of(&g), vec![]);
    assert_eq!(report.calls, counts(0, 0, 0, 1));
}

#[test]
fn a_field_with_a_function_and_a_getter_are_targets_of_a_call_and_of_a_value() {
    let source = "import { use } from './use';\nexport class A {\n  h = () => {};\n  get size() {\n    return 1;\n  }\n  m() {\n    this.h();\n    use(this.size);\n  }\n}\n";
    let (g, report) = run(&[("a.ts", source), ("use.ts", "export function use() {}\n")]);

    assert_eq!(
        calls_of(&g),
        vec![
            ("fn::/a#A.m", "fn::/a#A.h"),
            ("fn::/a#A.m", "fn::/a#A.size"),
            ("fn::/a#A.m", "fn::/use#use"),
        ]
    );
    assert_eq!(report.calls, counts(3, 0, 0, 0));
    assert_eq!(report.function_values, 1);
}
