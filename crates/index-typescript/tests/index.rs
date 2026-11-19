use cg_core::{Edge, EdgeKind, FileSites, Graph, Node, NodeKind, Span};
use cg_index_typescript::{
    HeritageReport, ImportReport, Report, SourceFile, index_sources, is_input, is_test_file,
};

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

const APP: (&str, &str) = ("package.json", r#"{"name":"my-app"}"#);

fn node<'a>(g: &'a Graph, id: &str) -> &'a Node {
    g.node(id)
        .unwrap_or_else(|| panic!("no node {id}; nodes: {:?}", ids(g)))
}

fn ids(g: &Graph) -> Vec<&str> {
    g.nodes.iter().map(|n| n.id.as_str()).collect()
}

fn parent_of<'a>(g: &'a Graph, id: &str) -> &'a str {
    node(g, id).parent.as_deref().expect("node has no parent")
}

#[test]
fn is_input_wants_typescript_and_configuration_files_outside_excluded_directories() {
    for path in [
        "a.ts",
        "src/a.tsx",
        "a/b.d.ts",
        "package.json",
        "x/package.json",
        "tsconfig.json",
        "tsconfig.app.json",
        "a/tsconfig.base.json",
    ] {
        assert!(is_input(path), "{path} should be input");
    }
    for path in [
        ".ts",
        "a.js",
        "a.mts",
        "a.jsx",
        "node_modules/x/index.ts",
        ".cache/x.ts",
        "package-lock.json",
        "tsconfig.txt",
        "jsconfig.json",
        "README.md",
    ] {
        assert!(!is_input(path), "{path} should not be input");
    }
}

#[test]
fn a_package_json_with_a_class_gives_module_package_and_type() {
    let g = index(&[APP, ("a.ts", "// top\nexport class Foo {\n  x = 1;\n}\n")]);

    let module = node(&g, "mod:");
    assert_eq!(
        (module.kind, module.name.as_str()),
        (NodeKind::Module, "my-app")
    );
    assert_eq!(module.parent, None);
    let package = node(&g, "pkg::/a");
    assert_eq!(
        (package.kind, package.name.as_str()),
        (NodeKind::Package, "a")
    );
    assert_eq!(package.parent.as_deref(), Some("mod:"));
    assert_eq!((package.file.as_deref(), package.span), (None, None));
    let ty = node(&g, "type::/a#Foo");
    assert_eq!((ty.kind, ty.name.as_str()), (NodeKind::Type, "Foo"));
    assert_eq!(ty.parent.as_deref(), Some("pkg::/a"));
    assert_eq!(ty.file.as_deref(), Some("a.ts"));
    assert_eq!(
        ty.span,
        Some(Span {
            start_line: 2,
            end_line: 4
        })
    );
    assert!(!ty.test);
    assert!(ty.hash.is_some());
    assert_eq!(g.nodes.len(), 3);
}

#[test]
fn a_module_is_named_by_its_package_json_else_by_its_directory_else_by_the_root() {
    let g = index(&[
        ("package.json", "{}"),
        ("r.ts", "export class R {}"),
        ("packages/core/package.json", "{}"),
        ("packages/core/c.ts", "export class C {}"),
        ("packages/bad/package.json", "{ not json"),
        ("packages/bad/b.ts", "export class B {}"),
    ]);

    assert_eq!(node(&g, "mod:").name, "repo");
    assert_eq!(node(&g, "mod:packages/core").name, "core");
    assert_eq!(node(&g, "mod:packages/bad").name, "bad");

    let g = index(&[
        ("package.json", "{ not json"),
        ("r.ts", "export class R {}"),
    ]);
    assert_eq!(node(&g, "mod:").name, "repo");
}

#[test]
fn a_file_belongs_to_the_nearest_package_json_and_modules_do_not_nest() {
    let g = index(&[
        APP,
        ("packages/a/package.json", "{}"),
        ("packages/a/x.ts", "export class X {}"),
        ("y.ts", "export class Y {}"),
    ]);

    assert_eq!(parent_of(&g, "pkg:packages/a:/x"), "mod:packages/a");
    assert_eq!(parent_of(&g, "type:packages/a:/x#X"), "pkg:packages/a:/x");
    assert_eq!(parent_of(&g, "pkg::/y"), "mod:");
    assert_eq!(node(&g, "mod:packages/a").parent, None);
    assert_eq!(node(&g, "mod:").parent, None);
}

#[test]
fn a_package_json_that_is_the_nearest_of_no_file_gives_no_module() {
    let g = index(&[
        APP,
        ("packages/a/package.json", "{}"),
        ("packages/a/x.ts", "export class X {}"),
    ]);
    assert!(g.node("mod:").is_none(), "{:?}", ids(&g));
    assert!(g.node("mod:packages/a").is_some());

    assert!(index(&[APP]).nodes.is_empty());
}

#[test]
fn a_file_without_a_package_json_above_it_is_not_read_and_counted() {
    let (g, report) = run(&[
        ("app/package.json", "{}"),
        ("app/a.ts", "export class A {}"),
        ("scripts/x.ts", "export class S {}"),
    ]);

    assert_eq!(report.files_without_package_json, 1);
    assert!(g.node("type::/scripts/x#S").is_none());
    assert!(g.node("pkg::/scripts/x").is_none());
    assert_eq!(ids(&g), ["mod:app", "pkg:app:/a", "type:app:/a#A"]);
}

#[test]
fn a_ts_file_that_is_xml_or_binary_is_not_read_and_counted() {
    let (g, report) = run(&[
        APP,
        ("tr.ts", "\u{feff}  \n<?xml version=\"1.0\"?>\n<TS></TS>"),
        ("video.ts", "G@\0\0\u{1}export class V {}"),
        ("a.ts", "export class A {}"),
    ]);

    assert_eq!(report.files_not_typescript, 2);
    assert_eq!(report.files_without_package_json, 0);
    assert_eq!(ids(&g), ["mod:", "pkg::/a", "type::/a#A"]);
}

#[test]
fn directories_nest_by_the_rule_of_adr_18_with_a_slash() {
    let g = index(&[
        APP,
        ("src/lib/forge.ts", "export class F {}"),
        ("src/lib/cover.ts", "export class C {}"),
        ("src/main.ts", "export class M {}"),
        ("vite.config.ts", "export class V {}"),
    ]);

    for (id, name, parent) in [
        ("pkg::/vite.config", "vite.config", "mod:"),
        ("pkg::/src", "src", "mod:"),
        ("pkg::/src/main", "main", "pkg::/src"),
        ("pkg::/src/lib", "lib", "pkg::/src"),
        ("pkg::/src/lib/forge", "forge", "pkg::/src/lib"),
    ] {
        let package = node(&g, id);
        assert_eq!(package.kind, NodeKind::Package, "{id}");
        assert_eq!(package.name, name, "{id}");
        assert_eq!(parent_of(&g, id), parent, "{id}");
    }
}

#[test]
fn the_common_directory_and_a_directory_with_one_child_are_no_nodes() {
    let g = index(&[
        APP,
        ("src/lib/a.ts", "export class A {}"),
        ("src/lib/b.ts", "export class B {}"),
        ("src/main.ts", "export class M {}"),
        ("src/only/deep/x.ts", "export class X {}"),
    ]);

    assert!(g.node("pkg::/src").is_none());
    assert!(g.node("pkg::/src/only").is_none());
    assert_eq!(node(&g, "pkg::/src/lib").name, "src/lib");
    assert_eq!(parent_of(&g, "pkg::/src/lib"), "mod:");
    assert_eq!(node(&g, "pkg::/src/lib/a").name, "a");
    assert_eq!(parent_of(&g, "pkg::/src/lib/a"), "pkg::/src/lib");
    assert_eq!(node(&g, "pkg::/src/only/deep/x").name, "src/only/deep/x");
    assert_eq!(parent_of(&g, "pkg::/src/only/deep/x"), "mod:");
}

#[test]
fn an_index_file_is_its_directory() {
    let g = index(&[
        APP,
        ("src/a/index.ts", "export class A {}"),
        ("src/a/b.ts", "export class B {}"),
        ("src/c.ts", "export class C {}"),
    ]);
    assert_eq!(parent_of(&g, "type::/src/a#A"), "pkg::/src/a");
    assert_eq!(parent_of(&g, "pkg::/src/a/b"), "pkg::/src/a");
    assert!(g.node("pkg::/src/a/index").is_none());

    let g = index(&[
        APP,
        ("x.ts", "export class X {}"),
        ("x/y.ts", "export class Y {}"),
    ]);
    assert_eq!(parent_of(&g, "type::/x#X"), "pkg::/x");
    assert_eq!(parent_of(&g, "pkg::/x/y"), "pkg::/x");
    assert_eq!(parent_of(&g, "pkg::/x"), "mod:");

    for index_file in ["index.ts", "index.tsx"] {
        let g = index(&[APP, (index_file, "export class Top {}")]);
        assert_eq!(parent_of(&g, "type::/#Top"), "mod:", "{index_file}");
        assert!(g.node("pkg::/").is_none());
        assert_eq!(g.nodes.len(), 2);
    }
}

#[test]
fn classes_interfaces_aliases_and_enums_are_types_but_enum_members_and_fields_are_not() {
    let g = index(&[
        APP,
        (
            "a.ts",
            "class C1 { f = 1 }\n\
             export abstract class C2 {}\n\
             interface I1 { a: number }\n\
             export interface I2 {}\n\
             type T1 = string;\n\
             export type T2<X> = X[];\n\
             enum E1 { A, B }\n\
             export const enum E2 { A }\n\
             declare class D1 {}\n\
             export declare class D2 {}\n\
             declare enum D3 { A }\n",
        ),
    ]);

    for name in [
        "C1", "C2", "I1", "I2", "T1", "T2", "E1", "E2", "D1", "D2", "D3",
    ] {
        let ty = node(&g, &format!("type::/a#{name}"));
        assert_eq!(ty.kind, NodeKind::Type, "{name}");
        assert_eq!(ty.name, name);
        assert_eq!(ty.parent.as_deref(), Some("pkg::/a"));
    }
    // module, package and the eleven types
    assert_eq!(g.nodes.len(), 13, "{:?}", ids(&g));
}

#[test]
fn functions_are_declarations_and_constants_with_a_function_value_but_nothing_inside() {
    let g = index(&[
        APP,
        (
            "a.ts",
            "function f() {}\n\
             export async function af() {}\n\
             function* gen() {}\n\
             declare function df(): void;\n\
             const h = () => 1;\n\
             const k = function () {};\n\
             let l = () => 1;\n\
             export const typed: Fn = (x) => x;\n\
             const v = 1;\n\
             const m = memo(() => 1);\n\
             const o = { f() {} };\n\
             function outer() {\n  function inner() {}\n  const arrow = () => 1;\n}\n",
        ),
    ]);

    for name in ["f", "af", "gen", "df", "h", "k", "l", "typed", "outer"] {
        let function = node(&g, &format!("fn::/a#{name}"));
        assert_eq!(function.kind, NodeKind::Function, "{name}");
        assert_eq!(function.name, name);
        assert_eq!(function.parent.as_deref(), Some("pkg::/a"));
        assert_eq!(function.file.as_deref(), Some("a.ts"));
        assert!(function.hash.is_some());
    }
    // module, package and the nine functions
    assert_eq!(g.nodes.len(), 11, "{:?}", ids(&g));
}

#[test]
fn a_declarator_among_several_has_the_span_of_the_declarator_alone() {
    let g = index(&[
        APP,
        (
            "a.ts",
            "export const h = () => {\n  return 1;\n},\n  v = 2;\n\nconst single = () => {\n  return 2;\n};\n",
        ),
    ]);

    assert_eq!(
        node(&g, "fn::/a#h").span,
        Some(Span {
            start_line: 1,
            end_line: 3
        })
    );
    assert_eq!(
        node(&g, "fn::/a#single").span,
        Some(Span {
            start_line: 6,
            end_line: 8
        })
    );
}

#[test]
fn methods_and_function_fields_of_a_class_are_functions_below_the_type() {
    let g = index(&[
        APP,
        (
            "a.ts",
            "export abstract class Foo {\n\
               method() {}\n\
               constructor() {}\n\
               get x() { return 1; }\n\
               static s() {}\n\
               abstract m(): void;\n\
               #p() {}\n\
               'quoted'() {}\n\
               field = () => 1;\n\
               fexpr = function () {};\n\
               [Symbol.iterator]() {}\n\
               n = 1;\n\
             }\n",
        ),
    ]);

    for name in [
        "method",
        "constructor",
        "x",
        "s",
        "m",
        "#p",
        "quoted",
        "field",
        "fexpr",
    ] {
        let function = node(&g, &format!("fn::/a#Foo.{name}"));
        assert_eq!(function.kind, NodeKind::Function, "{name}");
        assert_eq!(function.name, name);
        assert_eq!(function.parent.as_deref(), Some("type::/a#Foo"));
        assert_eq!(function.file.as_deref(), Some("a.ts"));
    }
    // module, package, the type and the nine members
    assert_eq!(g.nodes.len(), 12, "{:?}", ids(&g));
}

#[test]
fn the_method_signatures_of_an_interface_are_functions_but_function_typed_properties_are_not() {
    let g = index(&[
        APP,
        ("a.ts", "interface I {\n  m(): void;\n  p: () => void;\n}\n"),
    ]);

    assert_eq!(parent_of(&g, "fn::/a#I.m"), "type::/a#I");
    assert_eq!(g.nodes.len(), 4, "{:?}", ids(&g));
}

fn hash_of(g: &Graph, id: &str) -> String {
    node(g, id).hash.clone().expect("node has a hash")
}

#[test]
fn the_hash_of_a_type_does_not_hold_the_text_of_its_members() {
    let class = |method_body: &str, field: &str| {
        format!("export class A {{\n  f = {field};\n  m() {{ {method_body} }}\n}}\n")
    };
    let before = index(&[APP, ("a.ts", &class("return 1;", "1"))]);
    let body_changed = index(&[APP, ("a.ts", &class("return 2;", "1"))]);
    let field_changed = index(&[APP, ("a.ts", &class("return 1;", "2"))]);

    assert_eq!(
        hash_of(&before, "type::/a#A"),
        hash_of(&body_changed, "type::/a#A")
    );
    assert_ne!(
        hash_of(&before, "fn::/a#A.m"),
        hash_of(&body_changed, "fn::/a#A.m")
    );
    assert_ne!(
        hash_of(&before, "type::/a#A"),
        hash_of(&field_changed, "type::/a#A")
    );
    assert_eq!(
        hash_of(&before, "fn::/a#A.m"),
        hash_of(&field_changed, "fn::/a#A.m")
    );
}

#[test]
fn a_span_starts_at_the_first_decorator_or_at_the_export_keyword() {
    let g = index(&[
        APP,
        (
            "a.ts",
            "// c\n@Dec()\nexport class A {}\n\nexport class B {}\n",
        ),
        ("b.ts", "// c\n@Dec()\nclass A {}\n"),
    ]);

    let start = |id: &str| node(&g, id).span.expect("span").start_line;
    assert_eq!(start("type::/a#A"), 2);
    assert_eq!(start("type::/a#B"), 5);
    assert_eq!(start("type::/b#A"), 2);
}

#[test]
fn one_id_gives_one_node_and_a_body_replaces_an_overload_signature() {
    let g = index(&[
        APP,
        (
            "a.ts",
            "function over(a: string): void;\n\
             function over(a: number): void;\n\
             function over(a: any) {}\n\
             class C {\n\
               m(a: string): void;\n\
               m(a: number): void;\n\
               m(a: any) {}\n\
               get x() { return 1; }\n\
               set x(v) {}\n\
             }\n\
             interface I { a(): void }\n\
             interface I { b(): void }\n",
        ),
    ]);

    let start = |id: &str| node(&g, id).span.expect("span").start_line;
    assert_eq!(start("fn::/a#over"), 3);
    assert_eq!(start("fn::/a#C.m"), 7);
    assert_eq!(start("fn::/a#C.x"), 8);
    assert_eq!(start("type::/a#I"), 11);
    assert_eq!(parent_of(&g, "fn::/a#I.a"), "type::/a#I");
    assert_eq!(parent_of(&g, "fn::/a#I.b"), "type::/a#I");
    // module, package, C, I, over, C.m, C.x, I.a, I.b
    assert_eq!(g.nodes.len(), 9, "{:?}", ids(&g));
}

#[test]
fn a_default_export_is_a_node_by_its_name_else_by_default() {
    // (text of the file, ids of the nodes below the package)
    let cases: [(&str, &[&str]); 7] = [
        ("export default class Foo {}", &["type::/d#Foo"]),
        ("export default function foo() {}", &["fn::/d#foo"]),
        ("export default function () {}", &["fn::/d#default"]),
        ("export default () => 1;", &["fn::/d#default"]),
        (
            "export default class {\n  m() {}\n}",
            &["type::/d#default", "fn::/d#default.m"],
        ),
        ("export default 1;", &[]),
        ("const foo = 1;\nexport default foo;", &[]),
    ];
    for (text, expected) in cases {
        let g = index(&[APP, ("d.ts", text)]);
        let declared: Vec<&str> = ids(&g)
            .into_iter()
            .filter(|id| id.starts_with("type:") || id.starts_with("fn:"))
            .collect();
        assert_eq!(declared, expected, "{text}");
        for id in expected {
            let n = node(&g, id);
            let name = id.rsplit(['#', '.']).next().unwrap();
            assert_eq!(n.name, name, "{id}");
        }
    }
}

#[test]
fn a_namespace_is_a_package_below_the_package_of_its_file() {
    let g = index(&[
        APP,
        (
            "a.ts",
            "export namespace N {\n\
               export class A { m() {} }\n\
               export function f() {}\n\
               namespace Inner { export interface I {} }\n\
             }\n",
        ),
    ]);

    let n = node(&g, "pkg::/a#N");
    assert_eq!((n.kind, n.name.as_str()), (NodeKind::Package, "N"));
    assert_eq!(n.parent.as_deref(), Some("pkg::/a"));
    assert_eq!((n.file.as_deref(), n.span), (None, None));
    assert_eq!(parent_of(&g, "type::/a#N.A"), "pkg::/a#N");
    assert_eq!(parent_of(&g, "fn::/a#N.A.m"), "type::/a#N.A");
    assert_eq!(parent_of(&g, "fn::/a#N.f"), "pkg::/a#N");
    let inner = node(&g, "pkg::/a#N.Inner");
    assert_eq!(
        (inner.kind, inner.name.as_str()),
        (NodeKind::Package, "Inner")
    );
    assert_eq!(inner.parent.as_deref(), Some("pkg::/a#N"));
    assert_eq!(parent_of(&g, "type::/a#N.Inner.I"), "pkg::/a#N.Inner");
    assert_eq!(node(&g, "type::/a#N.A").name, "A");
    // module, package, N, Inner, A, I, A.m, f
    assert_eq!(g.nodes.len(), 8, "{:?}", ids(&g));
}

#[test]
fn a_dotted_namespace_gives_a_package_for_each_part() {
    let g = index(&[APP, ("a.ts", "namespace A.B { class C {} }\n")]);

    assert_eq!(parent_of(&g, "pkg::/a#A"), "pkg::/a");
    let b = node(&g, "pkg::/a#A.B");
    assert_eq!((b.kind, b.name.as_str()), (NodeKind::Package, "B"));
    assert_eq!(b.parent.as_deref(), Some("pkg::/a#A"));
    assert_eq!(parent_of(&g, "type::/a#A.B.C"), "pkg::/a#A.B");
}

#[test]
fn ambient_and_old_style_namespaces_count_but_modules_of_other_files_do_not() {
    let g = index(&[
        APP,
        (
            "a.ts",
            "declare namespace D { function f(): void; }\n\
             module Old { export class K {} }\n\
             declare module \"ext\" { export class Q {} }\n\
             declare global { interface W {} }\n\
             namespace Twice { export class X {} }\n\
             namespace Twice { export class Y {} }\n",
        ),
    ]);

    assert_eq!(parent_of(&g, "fn::/a#D.f"), "pkg::/a#D");
    assert_eq!(parent_of(&g, "type::/a#Old.K"), "pkg::/a#Old");
    assert_eq!(parent_of(&g, "type::/a#Twice.X"), "pkg::/a#Twice");
    assert_eq!(parent_of(&g, "type::/a#Twice.Y"), "pkg::/a#Twice");
    // module, package, D, Old, Twice, f, K, X, Y: nothing for "ext" and for `global`
    assert_eq!(g.nodes.len(), 9, "{:?}", ids(&g));
}

#[test]
fn tsx_is_parsed_with_jsx_and_ts_with_type_assertions() {
    let (g, report) = run(&[
        APP,
        (
            "app.tsx",
            "export function App() { return <div className=\"a\">{1}</div>; }\n\
             export const B = () => <App />;\n",
        ),
        (
            "g.ts",
            "export const f = <T>(x: T) => x;\n\
             export class After {}\n\
             const y = <number>(1 as unknown);\n",
        ),
    ]);

    assert_eq!(report.files_with_syntax_errors, 0);
    assert_eq!(node(&g, "fn::/app#App").kind, NodeKind::Function);
    assert_eq!(node(&g, "fn::/app#B").kind, NodeKind::Function);
    assert_eq!(node(&g, "fn::/g#f").kind, NodeKind::Function);
    assert_eq!(node(&g, "type::/g#After").kind, NodeKind::Type);
}

#[test]
fn a_file_with_a_syntax_error_is_counted_and_still_gives_its_nodes() {
    let (g, report) = run(&[
        APP,
        ("bad.ts", "export class A {}\nfunction (((\n"),
        ("good.ts", "export class G {}\n"),
    ]);

    assert_eq!(report.files_with_syntax_errors, 1);
    assert!(g.node("type::/bad#A").is_some());
}

const TESTS: [&str; 6] = [
    "src/a.test.ts",
    "src/b.spec.tsx",
    "src/__tests__/c.ts",
    "__mocks__/d.ts",
    "src/test.ts",
    "src/spec.tsx",
];
const MAIN: [&str; 5] = [
    "src/e.ts",
    "src/test/helper.ts",
    "src/latest.ts",
    "src/contest.ts",
    "src/a.test.util.ts",
];

fn with_declarations(paths: &[&str]) -> Graph {
    let mut files = vec![APP];
    files.extend(
        paths
            .iter()
            .map(|p| (*p, "export class T {}\nexport function f() {}\n")),
    );
    index(&files)
}

#[test]
fn the_nodes_of_test_files_carry_the_test_marker() {
    let g = with_declarations(&TESTS);
    let not_module = g.nodes.iter().filter(|n| n.kind != NodeKind::Module);
    assert_eq!(not_module.clone().count(), 6 * 3 + 1, "{:?}", ids(&g));
    for n in not_module {
        assert!(n.test, "{} should be test code", n.id);
    }

    let g = with_declarations(&MAIN);
    for n in g.nodes.iter().filter(|n| n.kind != NodeKind::Module) {
        assert!(!n.test, "{} should not be test code", n.id);
    }

    for path in TESTS {
        assert!(is_test_file(path), "{path}");
    }
    for path in MAIN {
        assert!(!is_test_file(path), "{path}");
    }
}

#[test]
fn a_directory_is_test_code_if_all_packages_below_it_are() {
    let g = index(&[
        APP,
        ("src/__tests__/x.ts", "export class X {}"),
        ("src/__tests__/y.ts", "export class Y {}"),
        ("src/main.ts", "export class M {}"),
        ("src/lib/a.ts", "export class A {}"),
        ("src/lib/b.ts", "export class B {}"),
    ]);

    assert!(node(&g, "pkg::/src/__tests__").test);
    assert!(node(&g, "pkg::/src/__tests__/x").test);
    assert!(!node(&g, "pkg::/src/lib").test);
    assert!(!node(&g, "pkg::/src/main").test);
}

#[test]
fn two_files_of_one_package_that_declare_one_name_give_one_node_in_either_order() {
    let a = ("a.ts", "export class X {}\n");
    let index_file = ("a/index.ts", "// the other\nexport class X {}\n");

    let forward = run(&[APP, a, index_file]);
    let backward = run(&[index_file, a, APP]);

    assert_eq!(forward, backward);
    let count = forward
        .0
        .nodes
        .iter()
        .filter(|n| n.id == "type::/a#X")
        .count();
    assert_eq!(count, 1);
}

#[test]
fn the_result_does_not_depend_on_the_order_of_the_files_and_a_path_given_twice_is_read_once() {
    let files = [
        APP,
        ("packages/a/package.json", r#"{"name":"@x/a"}"#),
        ("packages/a/src/x.ts", "export class X { m() {} }"),
        ("packages/a/src/y.ts", "export function y() {}"),
        ("src/main.ts", "export class Main {}"),
        ("src/lib/z.ts", "export const z = () => 1;"),
    ];
    let mut reversed = files;
    reversed.reverse();
    let twice: Vec<(&str, &str)> = files.iter().chain(files.iter()).copied().collect();

    let expected = run(&files);
    assert_eq!(run(&reversed), expected);
    assert_eq!(run(&twice), expected);
    assert!(expected.0.nodes.len() > 8);
}

// ---- edges (part 2): imports, extends, implements ----

/// The sites of an edge: `(file, lines)` per entry.
type Sites<'a> = &'a [(Option<&'a str>, &'a [u32])];

fn find_edge<'a>(g: &'a Graph, kind: EdgeKind, from: &str, to: &str) -> Option<&'a Edge> {
    g.edges
        .iter()
        .find(|e| e.kind == kind && e.from == from && e.to == to)
}

fn assert_edge(g: &Graph, kind: EdgeKind, from: &str, to: &str, sites: Sites) {
    let edge = find_edge(g, kind, from, to).unwrap_or_else(|| {
        panic!(
            "no {kind:?} edge {from} -> {to}; edges: {:?}",
            g.edges
                .iter()
                .map(|e| (e.kind, e.from.as_str(), e.to.as_str()))
                .collect::<Vec<_>>()
        )
    });
    let expected: Vec<FileSites> = sites
        .iter()
        .map(|(file, lines)| FileSites {
            file: file.map(str::to_string),
            lines: lines.to_vec(),
        })
        .collect();
    assert_eq!(edge.sites, expected, "sites of {from} -> {to}");
}

fn assert_import(g: &Graph, from: &str, to: &str, sites: Sites) {
    assert_edge(g, EdgeKind::Imports, from, to, sites);
}

#[test]
fn a_named_import_of_a_class_is_an_imports_edge_to_the_type_at_the_line_of_the_name() {
    let (g, report) = run(&[
        APP,
        ("a.ts", "import { B } from './b';\n"),
        ("b.ts", "export class B {}\n"),
    ]);

    assert_eq!(g.edges.len(), 1, "{:?}", g.edges);
    assert_import(&g, "pkg::/a", "type::/b#B", &[(Some("a.ts"), &[1])]);
    assert_eq!(report.imports.resolved, 1);
    assert_eq!(report.names.to_declaration, 1);
}

#[test]
fn a_named_import_of_a_function_goes_to_the_function_and_each_name_has_its_own_line() {
    let g = index(&[
        APP,
        (
            "a.ts",
            "import {\n  f,\n  g,\n} from './b';\nimport { f as h } from './b';\n",
        ),
        (
            "b.ts",
            "export function f() {}\nexport const g = () => 1;\n",
        ),
    ]);

    assert_eq!(g.edges.len(), 2, "{:?}", g.edges);
    assert_import(&g, "pkg::/a", "fn::/b#f", &[(Some("a.ts"), &[2, 5])]);
    assert_import(&g, "pkg::/a", "fn::/b#g", &[(Some("a.ts"), &[3])]);
}

#[test]
fn a_name_that_is_no_node_goes_to_the_package_at_the_line_of_the_string() {
    let (g, report) = run(&[
        APP,
        ("a.ts", "import {\n  X,\n  Y,\n} from './b';\n"),
        ("b.ts", "export const X = 1;\nexport const Y = 2;\n"),
    ]);

    assert_eq!(g.edges.len(), 1, "{:?}", g.edges);
    assert_import(&g, "pkg::/a", "pkg::/b", &[(Some("a.ts"), &[4])]);
    assert_eq!(report.names.to_package, 2);
    assert_eq!(report.names.to_declaration, 0);
}

#[test]
fn a_namespace_import_a_side_effect_import_and_an_export_star_go_to_the_package() {
    for text in [
        "\nimport * as b from './b';\n",
        "\nimport './b';\n",
        "\nexport * from './b';\n",
        "\nexport * as ns from './b';\n",
    ] {
        let (g, report) = run(&[APP, ("a.ts", text), ("b.ts", "export class B {}\n")]);

        assert_eq!(g.edges.len(), 1, "{text}: {:?}", g.edges);
        assert_import(&g, "pkg::/a", "pkg::/b", &[(Some("a.ts"), &[2])]);
        assert_eq!(report.imports.resolved, 1, "{text}");
        assert_eq!(report.names, Default::default(), "{text}");
    }
}

#[test]
fn a_default_import_reaches_the_declaration_that_the_default_export_names() {
    let cases = [
        ("export default class Foo {}\n", "type::/foo#Foo"),
        ("class Foo {}\nexport default Foo;\n", "type::/foo#Foo"),
        (
            "class Foo {}\nexport { Foo as default };\n",
            "type::/foo#Foo",
        ),
        ("function foo() {}\nexport default foo;\n", "fn::/foo#foo"),
        ("export default function () {}\n", "fn::/foo#default"),
        ("export default class {}\n", "type::/foo#default"),
    ];
    for (text, to) in cases {
        let (g, report) = run(&[
            APP,
            ("a.ts", "import Foo from './foo';\n"),
            ("foo.ts", text),
        ]);

        assert_eq!(g.edges.len(), 1, "{text}: {:?}", g.edges);
        assert_import(&g, "pkg::/a", to, &[(Some("a.ts"), &[1])]);
        assert_eq!(report.names.to_declaration, 1, "{text}");
    }

    let (g, report) = run(&[
        APP,
        ("a.ts", "import Foo from './foo';\n"),
        ("foo.ts", "export default 42;\n"),
    ]);
    assert_eq!(g.edges.len(), 1, "{:?}", g.edges);
    assert_import(&g, "pkg::/a", "pkg::/foo", &[(Some("a.ts"), &[1])]);
    assert_eq!(report.names.to_package, 1);
}

#[test]
fn a_renamed_import_and_a_type_only_import_reach_the_same_type() {
    for text in [
        "import { B as C } from './b';\n",
        "import type { B } from './b';\n",
        "import { type B } from './b';\n",
        "import { type B as C } from './b';\n",
    ] {
        let g = index(&[APP, ("a.ts", text), ("b.ts", "export class B {}\n")]);

        assert_eq!(g.edges.len(), 1, "{text}: {:?}", g.edges);
        assert_import(&g, "pkg::/a", "type::/b#B", &[(Some("a.ts"), &[1])]);
    }
}

#[test]
fn a_relative_path_finds_the_file_by_the_candidates_of_the_adr() {
    // (file that imports, specifier, file that is found, a file that must lose, edge target)
    let cases: [(&str, &str, &str, Option<&str>, &str); 11] = [
        ("a.ts", "./b", "b.tsx", None, "type::/b#B"),
        ("a.ts", "./b", "b.ts", None, "type::/b#B"),
        ("a.ts", "./b.js", "b.ts", None, "type::/b#B"),
        ("a.ts", "./b.jsx", "b.tsx", None, "type::/b#B"),
        ("a.ts", "./b.ts", "b.ts", None, "type::/b#B"),
        ("a.ts", "./c", "c/index.ts", None, "type::/c#B"),
        ("a.ts", "./d.js", "d.d.ts", None, "type::/d.d#B"),
        ("a.ts", "./b", "b.ts", Some("b/index.ts"), "type::/b#B"),
        ("x/y/z.ts", "..", "x/index.ts", None, "type::/x#B"),
        ("x/a.ts", ".", "x/index.ts", None, "type::/x#B"),
        ("x/y.ts", "../w", "w.ts", None, "type::/w#B"),
    ];
    for (importer, specifier, found, loser, to) in cases {
        let text = format!("import {{ B }} from '{specifier}';\n");
        let mut files = vec![
            APP,
            (importer, text.as_str()),
            (found, "export class B {}\n"),
        ];
        if let Some(loser) = loser {
            files.push((loser, "export class Other {}\n"));
        }
        let g = index(&files);

        let from = format!("pkg::/{}", importer.strip_suffix(".ts").unwrap());
        assert_import(&g, &from, to, &[(Some(importer), &[1])]);
    }
}

#[test]
fn a_relative_import_can_reach_a_file_of_another_module() {
    let g = index(&[
        ("packages/a/package.json", r#"{"name":"a"}"#),
        ("packages/b/package.json", r#"{"name":"b"}"#),
        ("packages/a/x.ts", "import { Y } from '../b/y';\n"),
        ("packages/b/y.ts", "export class Y {}\n"),
    ]);

    assert_eq!(g.edges.len(), 1, "{:?}", g.edges);
    assert_import(
        &g,
        "pkg:packages/a:/x",
        "type:packages/b:/y#Y",
        &[(Some("packages/a/x.ts"), &[1])],
    );
}

#[test]
fn a_relative_import_without_a_file_is_unresolved_and_one_of_another_kind_is_counted_apart() {
    let (g, report) = run(&[
        APP,
        (
            "a.ts",
            "import { A } from './missing';\nimport './gen.js';\nimport { B } from '../../x';\n",
        ),
        (
            "b.ts",
            "import './style.css';\nimport './App.svelte';\nimport d from './data.json';\n",
        ),
    ]);

    assert_eq!(g.edges.len(), 0, "{:?}", g.edges);
    assert_eq!(
        report.imports,
        ImportReport {
            resolved: 0,
            external: 0,
            unresolved: 3,
            other_files: 3,
        }
    );
}

#[test]
fn a_package_of_node_modules_is_external_and_an_unresolved_alias_is_unresolved() {
    let (g, report) = run(&[
        APP,
        (
            "a.ts",
            "import React from 'react';\nimport { x } from '@scope/pkg/sub';\nimport fs from 'node:fs';\n",
        ),
        (
            "b.ts",
            "import { a } from '@/x';\nimport { b } from '~/x';\nimport { c } from '#x';\nimport { d } from '$lib/x';\n",
        ),
    ]);

    assert_eq!(g.edges.len(), 0, "{:?}", g.edges);
    assert_eq!(
        report.imports,
        ImportReport {
            resolved: 0,
            external: 3,
            unresolved: 4,
            other_files: 0,
        }
    );
}

#[test]
fn a_re_export_gives_an_edge_from_the_importer_and_from_the_re_exporter_to_the_type() {
    let g = index(&[
        APP,
        ("api.ts", "export { A } from './impl';\n"),
        ("impl.ts", "export class A {}\n"),
        ("user.ts", "import { A } from './api';\n"),
    ]);

    assert_eq!(g.edges.len(), 2, "{:?}", g.edges);
    assert_import(
        &g,
        "pkg::/user",
        "type::/impl#A",
        &[(Some("user.ts"), &[1])],
    );
    assert_import(&g, "pkg::/api", "type::/impl#A", &[(Some("api.ts"), &[1])]);
}

#[test]
fn a_name_is_followed_through_renamed_re_exports_stars_and_chains() {
    // (text of api.ts, text of user.ts, edge target of user.ts)
    let cases = [
        (
            "export { A as B } from './impl';\n",
            "import { B } from './api';\n",
            "type::/impl#A",
        ),
        (
            "export * from './impl';\n",
            "import { A } from './api';\n",
            "type::/impl#A",
        ),
        (
            "export { A } from './mid';\n",
            "import { A } from './api';\n",
            "type::/impl#A",
        ),
        (
            "export * from './mid';\n",
            "import { A } from './api';\n",
            "type::/impl#A",
        ),
        (
            "export { default } from './impl2';\n",
            "import D from './api';\n",
            "type::/impl2#D",
        ),
        (
            "export { default as X } from './impl2';\n",
            "import { X } from './api';\n",
            "type::/impl2#D",
        ),
    ];
    for (api, user, to) in cases {
        let g = index(&[
            APP,
            ("api.ts", api),
            ("mid.ts", "export * from './impl';\n"),
            ("impl.ts", "export class A {}\n"),
            ("impl2.ts", "export default class D {}\n"),
            ("user.ts", user),
        ]);

        assert_import(&g, "pkg::/user", to, &[(Some("user.ts"), &[1])]);
        assert!(
            find_edge(&g, EdgeKind::Imports, "pkg::/user", "pkg::/api").is_none(),
            "{api}"
        );
    }
}

#[test]
fn an_export_type_star_that_the_grammar_does_not_know_is_read_as_export_star() {
    let (g, report) = run(&[
        APP,
        ("api.ts", "export type * from './impl';\n"),
        ("impl.ts", "export class A {}\n"),
        ("user.ts", "import { A } from './api';\n"),
    ]);

    assert_eq!(report.files_with_syntax_errors, 1);
    assert_eq!(g.edges.len(), 2, "{:?}", g.edges);
    assert_import(
        &g,
        "pkg::/user",
        "type::/impl#A",
        &[(Some("user.ts"), &[1])],
    );
    assert_import(&g, "pkg::/api", "pkg::/impl", &[(Some("api.ts"), &[1])]);
}

#[test]
fn a_cycle_of_re_exports_ends() {
    let (g, report) = run(&[
        APP,
        ("a.ts", "export * from './b';\n"),
        ("b.ts", "export * from './a';\n"),
        ("c.ts", "import { X } from './a';\n"),
        ("d.ts", "export { Y } from './e';\n"),
        ("e.ts", "export { Y } from './d';\n"),
    ]);

    assert_import(&g, "pkg::/c", "pkg::/a", &[(Some("c.ts"), &[1])]);
    assert_eq!(report.names.to_package, 3);
}

#[test]
fn a_name_that_a_file_imports_and_exports_again_is_followed_to_the_declaration() {
    let cases = [
        (
            "import { A } from './impl';\nexport { A };\n",
            "import { A } from './api';\n",
        ),
        (
            "import { A } from './impl';\nexport { A as Z };\n",
            "import { Z as A } from './api';\n",
        ),
        (
            "import A from './impl2';\nexport default A;\n",
            "import A from './api';\n",
        ),
    ];
    for (api, user) in cases {
        let g = index(&[
            APP,
            ("api.ts", api),
            ("impl.ts", "export class A {}\n"),
            ("impl2.ts", "export default class A {}\n"),
            ("user.ts", user),
        ]);

        let to = if api.contains("impl2") {
            "type::/impl2#A"
        } else {
            "type::/impl#A"
        };
        assert_import(&g, "pkg::/user", to, &[(Some("user.ts"), &[1])]);
    }
}

#[test]
fn a_name_that_the_file_declares_but_does_not_export_is_not_reached() {
    let (g, report) = run(&[
        APP,
        ("a.ts", "import { B } from './b';\n"),
        ("b.ts", "class B {}\n"),
    ]);

    assert_eq!(g.edges.len(), 1, "{:?}", g.edges);
    assert_import(&g, "pkg::/a", "pkg::/b", &[(Some("a.ts"), &[1])]);
    assert_eq!(report.names.to_package, 1);
}

#[test]
fn a_type_and_a_function_of_one_name_are_both_reached() {
    let (g, report) = run(&[
        APP,
        ("a.ts", "import { Foo } from './b';\n"),
        (
            "b.ts",
            "export type Foo = { a: number };\nexport const Foo = () => 1;\n",
        ),
    ]);

    assert_eq!(g.edges.len(), 2, "{:?}", g.edges);
    assert_import(&g, "pkg::/a", "type::/b#Foo", &[(Some("a.ts"), &[1])]);
    assert_import(&g, "pkg::/a", "fn::/b#Foo", &[(Some("a.ts"), &[1])]);
    assert_eq!(report.names.to_declaration, 1);
}

#[test]
fn no_edge_goes_to_a_node_itself_its_ancestor_or_its_descendant_and_the_counts_stay() {
    let cases = [
        // The package of `lib/index.ts` is the parent of `pkg::/lib/a`.
        (
            vec![
                ("lib/index.ts", "export { A } from './a';\n"),
                ("lib/a.ts", "export class A {}\n"),
            ],
            1,
        ),
        // The module node holds everything.
        (
            vec![
                (
                    "index.ts",
                    "import { A } from './src/a';\nimport './src/a';\n",
                ),
                ("src/a.ts", "export class A {}\n"),
            ],
            2,
        ),
        (
            vec![("a.ts", "import { A } from './a';\nexport class A {}\n")],
            1,
        ),
    ];
    for (files, resolved) in cases {
        let mut all = vec![APP];
        all.extend(files);
        let (g, report) = run(&all);

        assert_eq!(g.edges, vec![], "{all:?}");
        assert_eq!(report.imports.resolved, resolved, "{all:?}");
    }
}

#[test]
fn a_dynamic_import_with_a_string_is_an_edge_to_the_package_and_other_arguments_are_not_read() {
    let (g, report) = run(&[
        APP,
        (
            "a.ts",
            "export async function load() {\n  const m = await import(\n    './lazy'\n  );\n  return m;\n}\n",
        ),
        ("lazy.ts", "export class L {}\n"),
        (
            "b.ts",
            "const name = 'x';\nimport(name);\nimport(`./${name}`);\nimport('./x', { with: {} });\n",
        ),
    ]);

    assert_eq!(g.edges.len(), 1, "{:?}", g.edges);
    assert_import(&g, "pkg::/a", "pkg::/lazy", &[(Some("a.ts"), &[3])]);
    assert_eq!(
        report.imports,
        ImportReport {
            resolved: 1,
            ..Default::default()
        }
    );
}

fn assert_extends(g: &Graph, from: &str, to: &str, sites: Sites) {
    assert_edge(g, EdgeKind::Extends, from, to, sites);
}

fn assert_implements(g: &Graph, from: &str, to: &str, sites: Sites) {
    assert_edge(g, EdgeKind::Implements, from, to, sites);
}

#[test]
fn a_class_extends_a_class_of_its_file_at_the_line_of_the_name() {
    let (g, report) = run(&[APP, ("a.ts", "class A {}\nexport class B extends A {}\n")]);

    assert_eq!(g.edges.len(), 1, "{:?}", g.edges);
    assert_extends(&g, "type::/a#B", "type::/a#A", &[(None, &[2])]);
    assert_eq!(
        report.heritage,
        HeritageReport {
            resolved: 1,
            ..Default::default()
        }
    );
}

#[test]
fn a_class_extends_a_type_through_imports_re_exports_and_type_arguments() {
    let cases = [
        "import { Base } from './base';\nexport class C extends Base {}\n",
        "import Base from './base-default';\nexport class C extends Base {}\n",
        "import { Base as B } from './base';\nexport class C extends B {}\n",
        "import { Base } from './api';\nexport class C extends Base {}\n",
        "import { Base } from './base';\nexport class C extends Base<string> {}\n",
        "import type { Base } from './base';\nexport class C extends Base {}\n",
    ];
    for text in cases {
        let (g, report) = run(&[
            APP,
            ("c.ts", text),
            ("base.ts", "export class Base {}\n"),
            ("base-default.ts", "export default class Base {}\n"),
            ("api.ts", "export { Base } from './base';\n"),
        ]);

        let to = if text.contains("base-default") {
            "type::/base-default#Base"
        } else {
            "type::/base#Base"
        };
        assert_extends(&g, "type::/c#C", to, &[(None, &[2])]);
        assert_eq!(report.heritage.resolved, 1, "{text}");
    }
}

#[test]
fn a_name_of_a_namespace_import_is_resolved_in_the_file_that_the_import_names() {
    let (g, report) = run(&[
        APP,
        (
            "c.ts",
            "import * as ns from './b';\nexport class C extends ns.Base implements ns.I<string> {}\n",
        ),
        ("b.ts", "export class Base {}\nexport interface I {}\n"),
    ]);

    assert_eq!(g.edges.len(), 3, "{:?}", g.edges);
    assert_extends(&g, "type::/c#C", "type::/b#Base", &[(None, &[2])]);
    assert_implements(&g, "type::/c#C", "type::/b#I", &[(None, &[2])]);
    assert_eq!(
        report.heritage,
        HeritageReport {
            resolved: 2,
            ..Default::default()
        }
    );
}

#[test]
fn an_interface_extends_each_type_of_its_clause_and_a_class_implements_each_interface() {
    let g = index(&[
        APP,
        (
            "a.ts",
            "interface P {}\ninterface Q {}\nclass K {}\n\
             export interface R extends P,\n  Q {}\n\
             export class C extends K implements P,\n  Q {}\n",
        ),
    ]);

    assert_eq!(g.edges.len(), 5, "{:?}", g.edges);
    assert_extends(&g, "type::/a#R", "type::/a#P", &[(None, &[4])]);
    assert_extends(&g, "type::/a#R", "type::/a#Q", &[(None, &[5])]);
    assert_extends(&g, "type::/a#C", "type::/a#K", &[(None, &[6])]);
    assert_implements(&g, "type::/a#C", "type::/a#P", &[(None, &[6])]);
    assert_implements(&g, "type::/a#C", "type::/a#Q", &[(None, &[7])]);
}

#[test]
fn globals_and_external_names_are_external_and_names_of_the_repository_that_reach_no_type_are_not()
{
    let external = HeritageReport {
        external: 1,
        ..Default::default()
    };
    let unresolved = HeritageReport {
        unresolved: 1,
        ..Default::default()
    };
    let cases = [
        ("class A extends Error {}\n", external),
        (
            "import { Component } from 'react';\nclass A extends Component {}\n",
            external,
        ),
        (
            "import * as React from 'react';\nclass A extends React.Component {}\n",
            external,
        ),
        ("class A extends window.Base {}\n", external),
        ("class A extends mixin(B) {}\n", unresolved),
        (
            "import { X } from './b';\nclass A extends X {}\n",
            unresolved,
        ),
        (
            "import { f } from './b';\nclass A extends f {}\n",
            unresolved,
        ),
        ("function F() {}\nclass A extends F {}\n", unresolved),
        ("const K = 1;\nclass A extends K {}\n", unresolved),
        (
            "import { Y } from './missing';\nclass A extends Y {}\n",
            unresolved,
        ),
    ];
    for (text, expected) in cases {
        let (g, report) = run(&[
            APP,
            ("a.ts", text),
            ("b.ts", "export const X = 1;\nexport function f() {}\n"),
        ]);

        assert!(
            g.edges.iter().all(|e| e.kind == EdgeKind::Imports),
            "{text}: {:?}",
            g.edges
        );
        assert_eq!(report.heritage, expected, "{text}");
    }
}

#[test]
fn names_are_resolved_in_the_namespaces_around_the_declaration_innermost_first() {
    let cases = [
        (
            "namespace N {\n  export class A {}\n  export class B extends A {}\n}\n",
            "type::/a#N.B",
            "type::/a#N.A",
        ),
        (
            "namespace N {\n  export class A {}\n}\nclass C extends N.A {}\n",
            "type::/a#C",
            "type::/a#N.A",
        ),
        (
            "namespace N.M {\n  export class A {}\n}\nclass C extends N.M.A {}\n",
            "type::/a#C",
            "type::/a#N.M.A",
        ),
        (
            "namespace N {\n  export class A {}\n  export class B extends N.A {}\n}\n",
            "type::/a#N.B",
            "type::/a#N.A",
        ),
        (
            "class A {}\nnamespace N {\n  export class B extends A {}\n}\n",
            "type::/a#N.B",
            "type::/a#A",
        ),
        (
            "class A {}\nnamespace N {\n  export class A {}\n  export class B extends A {}\n}\n",
            "type::/a#N.B",
            "type::/a#N.A",
        ),
    ];
    for (text, from, to) in cases {
        let (g, report) = run(&[APP, ("a.ts", text)]);

        let line = text.lines().position(|l| l.contains("extends")).unwrap() as u32 + 1;
        assert_extends(&g, from, to, &[(None, &[line])]);
        assert_eq!(report.heritage.resolved, 1, "{text}");
    }
}

#[test]
fn an_unnamed_default_class_gives_its_extends_edge() {
    let g = index(&[
        APP,
        ("base.ts", "export class Base {}\n"),
        (
            "a.ts",
            "import { Base } from './base';\nexport default class extends Base {}\n",
        ),
    ]);

    assert_extends(&g, "type::/a#default", "type::/base#Base", &[(None, &[2])]);
}

#[test]
fn test_files_keep_their_edges() {
    let g = index(&[
        APP,
        ("a.ts", "export class A {}\n"),
        (
            "a.test.ts",
            "import { A } from './a';\nclass T extends A {}\n",
        ),
    ]);

    assert_import(
        &g,
        "pkg::/a.test",
        "type::/a#A",
        &[(Some("a.test.ts"), &[1])],
    );
    assert_extends(&g, "type::/a.test#T", "type::/a#A", &[(None, &[2])]);
    assert!(node(&g, "pkg::/a.test").test);
    assert!(!node(&g, "pkg::/a").test);
}

#[test]
fn edges_and_report_do_not_depend_on_the_order_of_the_files() {
    let files = [
        APP,
        ("packages/a/package.json", r#"{"name":"@x/a"}"#),
        (
            "packages/a/src/x.ts",
            "import { Y } from '../../b/y';\nexport class X extends Y {}\n",
        ),
        ("packages/b/package.json", r#"{"name":"@x/b"}"#),
        (
            "packages/b/y.ts",
            "export * from './z';\nexport default class Y2 {}\n",
        ),
        (
            "packages/b/z.ts",
            "export class Y {}\nexport interface I {}\n",
        ),
        (
            "src/api.ts",
            "export { A as B } from './impl';\nexport * from './impl2';\n",
        ),
        ("src/impl.ts", "export class A {}\n"),
        (
            "src/impl2.ts",
            "export class C extends Error {}\nexport const k = 1;\n",
        ),
        (
            "src/user.ts",
            "import { B, C, k } from './api';\nimport * as ns from './impl';\nimport('./impl2');\nclass U extends B implements ns.A {}\nclass V extends mixin(C) {}\n",
        ),
        ("src/cyc1.ts", "export * from './cyc2';\n"),
        (
            "src/cyc2.ts",
            "export * from './cyc1';\nimport { Q } from './cyc1';\n",
        ),
    ];
    let mut reversed = files;
    reversed.reverse();
    let mut rotated = files;
    rotated.rotate_left(5);

    let expected = run(&files);
    assert_eq!(run(&reversed), expected);
    assert_eq!(run(&rotated), expected);
    assert!(expected.0.edges.len() > 8, "{:?}", expected.0.edges);
    assert!(expected.1.names.to_declaration > 2);
    assert!(expected.1.heritage.resolved > 1);
}

// ---- edges (part 3): tsconfig and the packages of the repository ----

#[test]
fn a_paths_pattern_with_a_star_in_a_tsconfig_with_comments_and_trailing_commas_resolves() {
    let (g, report) = run(&[
        APP,
        (
            "tsconfig.json",
            "{ // c\n \"compilerOptions\": { /* x */ \"paths\": { \"@/*\": [\"./src/*\"], \"http://a\": [\"./src/b\"], }, }, }",
        ),
        ("src/a.ts", "import { B } from '@/b';\n"),
        ("src/b.ts", "export class B {}\n"),
    ]);

    assert_eq!(g.edges.len(), 1, "{:?}", g.edges);
    assert_import(
        &g,
        "pkg::/src/a",
        "type::/src/b#B",
        &[(Some("src/a.ts"), &[1])],
    );
    assert_eq!(report.imports.resolved, 1);
}

#[test]
fn a_tsconfig_with_a_bom_and_a_comment_after_a_trailing_comma_is_read() {
    let (g, _) = run(&[
        ("package.json", "\u{feff}{\"name\":\"my-app\"}"),
        (
            "tsconfig.json",
            "\u{feff}{ \"compilerOptions\": { \"paths\": { \"@/*\": [\"./src/*\"] },\n // \"types\": []\n /* x */ } }",
        ),
        ("src/a.ts", "import { B } from '@/b';\n"),
        ("src/b.ts", "export class B {}\n"),
    ]);

    assert_eq!(node(&g, "mod:").name, "my-app");
    assert_eq!(g.edges.len(), 1, "{:?}", g.edges);
    assert_import(
        &g,
        "pkg::/src/a",
        "type::/src/b#B",
        &[(Some("src/a.ts"), &[1])],
    );
}

#[test]
fn a_pattern_that_finds_no_file_leaves_the_name_to_the_packages_and_a_lone_star_claims_nothing() {
    let (g, report) = run(&[
        APP,
        (
            "tsconfig.json",
            r#"{"compilerOptions":{"paths":{"*":["./types/*"],"@acme/*":["./nowhere/*"]}}}"#,
        ),
        (
            "src/a.ts",
            "import React from 'react';\nimport { C } from '@acme/core';\n",
        ),
        ("packages/core/package.json", r#"{"name":"@acme/core"}"#),
        ("packages/core/index.ts", "export class C {}\n"),
    ]);

    assert_eq!(g.edges.len(), 1, "{:?}", g.edges);
    assert_import(
        &g,
        "pkg::/src/a",
        "type:packages/core:/#C",
        &[(Some("src/a.ts"), &[2])],
    );
    assert_eq!(
        report.imports,
        ImportReport {
            resolved: 1,
            external: 1,
            ..Default::default()
        }
    );
}

#[test]
fn a_broken_string_as_a_member_name_and_a_comment_in_a_clause_do_no_harm() {
    let (g, report) = run(&[
        APP,
        ("bad.ts", "class A { \""),
        ("bad2.ts", "class A { \"\u{e9}\n"),
        (
            "c.ts",
            "interface I {}\nclass C implements /* the one */ I {}\n",
        ),
    ]);

    assert_implements(&g, "type::/c#C", "type::/c#I", &[(None, &[2])]);
    assert_eq!(
        report.heritage,
        HeritageReport {
            resolved: 1,
            ..Default::default()
        }
    );
}

#[test]
fn a_pattern_without_a_star_matches_only_the_equal_string_and_a_directory_target_finds_its_index() {
    let (g, report) = run(&[
        APP,
        (
            "tsconfig.json",
            r#"{"compilerOptions": {"paths": {"@lib": ["./src/lib"]}}}"#,
        ),
        (
            "src/a.ts",
            "import { L } from '@lib';\nimport { X } from '@lib/x';\n",
        ),
        ("src/lib/index.ts", "export class L {}\n"),
    ]);

    assert_eq!(g.edges.len(), 1, "{:?}", g.edges);
    assert_import(
        &g,
        "pkg::/src/a",
        "type::/src/lib#L",
        &[(Some("src/a.ts"), &[1])],
    );
    assert_eq!((report.imports.resolved, report.imports.external), (1, 1));
}

#[test]
fn a_base_url_makes_the_targets_relative_to_it_and_resolves_bare_specifiers_below_it() {
    let (g, report) = run(&[
        APP,
        (
            "tsconfig.json",
            r#"{"compilerOptions": {"baseUrl": "./src", "paths": {"@/*": ["*"]}}}"#,
        ),
        (
            "src/a.ts",
            "import { B } from '@/b';\nimport { X } from 'utils/x';\nimport React from 'react';\n",
        ),
        ("src/b.ts", "export class B {}\n"),
        ("src/utils/x.ts", "export class X {}\n"),
    ]);

    assert_eq!(g.edges.len(), 2, "{:?}", g.edges);
    assert_import(
        &g,
        "pkg::/src/a",
        "type::/src/b#B",
        &[(Some("src/a.ts"), &[1])],
    );
    assert_import(
        &g,
        "pkg::/src/a",
        "type::/src/utils/x#X",
        &[(Some("src/a.ts"), &[2])],
    );
    assert_eq!((report.imports.resolved, report.imports.external), (2, 1));
}

#[test]
fn the_pattern_with_the_longest_prefix_wins_and_the_targets_are_tried_in_order() {
    let g = index(&[
        APP,
        (
            "tsconfig.json",
            r#"{"compilerOptions": {"paths": {
                "@a/*": ["./x/*"], "@a/b/*": ["./y/*"],
                "@/*": ["./gen/*", "./src/*"]
            }}}"#,
        ),
        (
            "m.ts",
            "import '@a/b/c';\nimport { B } from '@/b';\nimport '@a/b';\n",
        ),
        ("x/b/c.ts", "export class Wrong {}\n"),
        ("y/c.ts", "export class C {}\n"),
        ("x/b.ts", "export class XB {}\n"),
        ("src/b.ts", "export class B {}\n"),
    ]);

    assert_eq!(g.edges.len(), 3, "{:?}", g.edges);
    assert_import(&g, "pkg::/m", "pkg::/y/c", &[(Some("m.ts"), &[1])]);
    assert_import(&g, "pkg::/m", "type::/src/b#B", &[(Some("m.ts"), &[2])]);
    assert_import(&g, "pkg::/m", "pkg::/x/b", &[(Some("m.ts"), &[3])]);
}

#[test]
fn a_pattern_that_matches_and_finds_no_file_is_unresolved_not_external() {
    let (g, report) = run(&[
        APP,
        (
            "tsconfig.json",
            r#"{"compilerOptions": {"paths": {"@x/*": ["./src/*"]}}}"#,
        ),
        ("a.ts", "import { M } from '@x/missing';\n"),
    ]);

    assert_eq!(g.edges.len(), 0, "{:?}", g.edges);
    assert_eq!(
        report.imports,
        ImportReport {
            resolved: 0,
            external: 0,
            unresolved: 1,
            other_files: 0,
        }
    );
}

#[test]
fn the_nearest_tsconfig_json_decides_and_a_file_without_a_claim_is_unresolved_for_an_alias() {
    let (g, report) = run(&[
        APP,
        (
            "tsconfig.json",
            r#"{"compilerOptions": {"paths": {"@/*": ["./src/*"]}}}"#,
        ),
        ("packages/a/tsconfig.json", "{}"),
        ("packages/a/x.ts", "import '@/b';\n"),
        ("src/a.ts", "import '@/b';\n"),
        ("src/b.ts", "export class B {}\n"),
    ]);

    assert_eq!(g.edges.len(), 1, "{:?}", g.edges);
    assert_import(
        &g,
        "pkg::/src/a",
        "pkg::/src/b",
        &[(Some("src/a.ts"), &[1])],
    );
    assert_eq!((report.imports.resolved, report.imports.unresolved), (1, 1));
}

#[test]
fn paths_come_from_the_extended_file_relative_to_the_file_that_holds_them() {
    let (g, _) = run(&[
        APP,
        (
            "packages/a/tsconfig.json",
            r#"{"extends": "../tsconfig.base.json"}"#,
        ),
        (
            "packages/tsconfig.base.json",
            r#"{"compilerOptions": {"paths": {"@x/common": ["./common/src/index.ts"]}}}"#,
        ),
        ("packages/a/x.ts", "import { C } from '@x/common';\n"),
        ("packages/common/src/index.ts", "export class C {}\n"),
    ]);

    assert_eq!(g.edges.len(), 1, "{:?}", g.edges);
    assert_import(
        &g,
        "pkg::/packages/a/x",
        "type::/packages/common/src#C",
        &[(Some("packages/a/x.ts"), &[1])],
    );
}

#[test]
fn an_extends_without_the_json_extension_names_the_file_too() {
    let g = index(&[
        APP,
        (
            "packages/a/tsconfig.json",
            r#"{"extends": "../tsconfig.base"}"#,
        ),
        (
            "packages/tsconfig.base.json",
            r#"{"compilerOptions": {"paths": {"@x/common": ["./common.ts"]}}}"#,
        ),
        ("packages/a/x.ts", "import { C } from '@x/common';\n"),
        ("packages/common.ts", "export class C {}\n"),
    ]);

    assert_import(
        &g,
        "pkg::/packages/a/x",
        "type::/packages/common#C",
        &[(Some("packages/a/x.ts"), &[1])],
    );
}

#[test]
fn in_an_array_of_extends_the_later_entry_wins() {
    let g = index(&[
        APP,
        (
            "tsconfig.json",
            r#"{"extends": ["./tsconfig.one.json", "./tsconfig.two.json", "@tsconfig/node20/tsconfig.json"]}"#,
        ),
        (
            "tsconfig.one.json",
            r#"{"compilerOptions": {"paths": {"@x": ["./one.ts"]}}}"#,
        ),
        (
            "tsconfig.two.json",
            r#"{"compilerOptions": {"paths": {"@x": ["./two.ts"]}}}"#,
        ),
        ("m.ts", "import { T } from '@x';\n"),
        ("one.ts", "export class O {}\n"),
        ("two.ts", "export class T {}\n"),
    ]);

    assert_eq!(g.edges.len(), 1, "{:?}", g.edges);
    assert_import(&g, "pkg::/m", "type::/two#T", &[(Some("m.ts"), &[1])]);
}

#[test]
fn own_paths_replace_the_inherited_ones_and_an_extends_of_a_package_is_ignored() {
    let g = index(&[
        APP,
        (
            "tsconfig.json",
            r#"{"extends": ["@tsconfig/node20/tsconfig.json", "./tsconfig.base.json"],
                "compilerOptions": {"paths": {"@own": ["./own.ts"]}}}"#,
        ),
        (
            "tsconfig.base.json",
            r#"{"compilerOptions": {"paths": {"@own": ["./inherited.ts"], "@other": ["./inherited.ts"]}}}"#,
        ),
        (
            "m.ts",
            "import { O } from '@own';\nimport { I } from '@other';\n",
        ),
        ("own.ts", "export class O {}\n"),
        ("inherited.ts", "export class I {}\n"),
    ]);

    assert_eq!(g.edges.len(), 1, "{:?}", g.edges);
    assert_import(&g, "pkg::/m", "type::/own#O", &[(Some("m.ts"), &[1])]);
}

#[test]
fn a_cycle_of_extends_ends() {
    let g = index(&[
        APP,
        ("tsconfig.json", r#"{"extends": "./tsconfig.a.json"}"#),
        ("tsconfig.a.json", r#"{"extends": "./tsconfig.b.json"}"#),
        (
            "tsconfig.b.json",
            r#"{"extends": "./tsconfig.a.json", "compilerOptions": {"paths": {"@x": ["./x.ts"]}}}"#,
        ),
        ("m.ts", "import { X } from '@x';\nimport 'nothing';\n"),
        ("x.ts", "export class X {}\n"),
    ]);

    assert_eq!(g.edges.len(), 1, "{:?}", g.edges);
    assert_import(&g, "pkg::/m", "type::/x#X", &[(Some("m.ts"), &[1])]);
}

#[test]
fn an_inherited_base_url_is_relative_to_the_file_that_sets_it() {
    let g = index(&[
        APP,
        (
            "packages/a/tsconfig.json",
            r#"{"extends": "../tsconfig.base.json", "compilerOptions": {"paths": {"@c/*": ["common/*"]}}}"#,
        ),
        (
            "packages/tsconfig.base.json",
            r#"{"compilerOptions": {"baseUrl": "."}}"#,
        ),
        (
            "packages/a/x.ts",
            "import { C } from 'common/c';\nimport { D } from '@c/d';\n",
        ),
        ("packages/common/c.ts", "export class C {}\n"),
        ("packages/common/d.ts", "export class D {}\n"),
    ]);

    assert_eq!(g.edges.len(), 2, "{:?}", g.edges);
    assert_import(
        &g,
        "pkg::/packages/a/x",
        "type::/packages/common/c#C",
        &[(Some("packages/a/x.ts"), &[1])],
    );
    assert_import(
        &g,
        "pkg::/packages/a/x",
        "type::/packages/common/d#D",
        &[(Some("packages/a/x.ts"), &[2])],
    );
}

#[test]
fn the_references_of_the_nearest_tsconfig_json_add_their_configurations() {
    let g = index(&[
        APP,
        (
            "tsconfig.json",
            r#"{"files": [], "references": [{"path": "./tsconfig.app.json"}, {"path": "./tsconfig.node.json"}]}"#,
        ),
        (
            "tsconfig.app.json",
            r#"{"compilerOptions": {"paths": {"@/*": ["./src/*"]}}}"#,
        ),
        ("tsconfig.node.json", "{}"),
        ("src/a.ts", "import { B } from '@/b';\n"),
        ("src/b.ts", "export class B {}\n"),
    ]);

    assert_eq!(g.edges.len(), 1, "{:?}", g.edges);
    assert_import(
        &g,
        "pkg::/src/a",
        "type::/src/b#B",
        &[(Some("src/a.ts"), &[1])],
    );
}

#[test]
fn a_reference_to_a_directory_names_its_tsconfig_json_and_references_go_one_level_only() {
    let g = index(&[
        APP,
        ("tsconfig.json", r#"{"references": [{"path": "./sub"}]}"#),
        (
            "sub/tsconfig.json",
            r#"{"references": [{"path": "../deep"}], "compilerOptions": {"paths": {"@s": ["./s.ts"]}}}"#,
        ),
        (
            "deep/tsconfig.json",
            r#"{"compilerOptions": {"paths": {"@d": ["./d.ts"]}}}"#,
        ),
        ("a.ts", "import { S } from '@s';\nimport { D } from '@d';\n"),
        ("sub/s.ts", "export class S {}\n"),
        ("deep/d.ts", "export class D {}\n"),
    ]);

    assert_eq!(g.edges.len(), 1, "{:?}", g.edges);
    assert_import(&g, "pkg::/a", "type::/sub/s#S", &[(Some("a.ts"), &[1])]);
}

#[test]
fn referenced_configurations_that_give_different_files_make_it_unresolved_and_the_same_file_resolved()
 {
    let (g, report) = run(&[
        APP,
        (
            "tsconfig.json",
            r#"{"references": [{"path": "./tsconfig.one.json"}, {"path": "./tsconfig.two.json"}]}"#,
        ),
        (
            "tsconfig.one.json",
            r#"{"compilerOptions": {"paths": {"@x": ["./one.ts"], "@same": ["./same.ts"], "@miss": ["./one.ts"]}}}"#,
        ),
        (
            "tsconfig.two.json",
            r#"{"compilerOptions": {"paths": {"@x": ["./two.ts"], "@same": ["./same.ts"], "@miss": ["./gone.ts"]}}}"#,
        ),
        (
            "m.ts",
            "import '@x';\nimport { S } from '@same';\nimport '@miss';\n",
        ),
        ("one.ts", ""),
        ("two.ts", ""),
        ("same.ts", "export class S {}\n"),
    ]);

    assert_eq!(g.edges.len(), 1, "{:?}", g.edges);
    assert_import(&g, "pkg::/m", "type::/same#S", &[(Some("m.ts"), &[2])]);
    assert_eq!((report.imports.resolved, report.imports.unresolved), (1, 2));
}

#[test]
fn a_name_that_an_alias_import_brings_is_followed_in_extends_and_in_re_exports() {
    let (g, report) = run(&[
        APP,
        (
            "tsconfig.json",
            r#"{"compilerOptions": {"paths": {"@/*": ["./src/*"]}}}"#,
        ),
        (
            "src/a.ts",
            "import { Base } from '@/base';\nexport class A extends Base {}\n",
        ),
        ("src/base.ts", "export class Base {}\n"),
        ("src/api.ts", "export * from '@/impl';\n"),
        ("src/impl.ts", "export class Impl {}\n"),
        ("src/user.ts", "import { Impl } from './api';\n"),
    ]);

    assert_extends(
        &g,
        "type::/src/a#A",
        "type::/src/base#Base",
        &[(None, &[2])],
    );
    assert_import(
        &g,
        "pkg::/src/user",
        "type::/src/impl#Impl",
        &[(Some("src/user.ts"), &[1])],
    );
    assert_eq!(report.heritage.resolved, 1);
}

#[test]
fn a_package_of_the_repository_with_a_provable_entry_goes_to_the_declaration() {
    let (g, report) = run(&[
        (
            "packages/core/package.json",
            r#"{"name":"@acme/core","main":"./src/index.ts"}"#,
        ),
        ("packages/core/src/index.ts", "export class A {}\n"),
        ("packages/app/package.json", r#"{"name":"@acme/app"}"#),
        ("packages/app/x.ts", "import { A } from '@acme/core';\n"),
    ]);

    assert_eq!(g.edges.len(), 1, "{:?}", g.edges);
    assert_import(
        &g,
        "pkg:packages/app:/x",
        "type:packages/core:/src#A",
        &[(Some("packages/app/x.ts"), &[1])],
    );
    assert_eq!(report.imports.resolved, 1);
}

/// A package `@acme/core` with the given `package.json` text and the given files below
/// `packages/core`, and `packages/app/x.ts` that imports `specifier` from it.
fn with_core(
    manifest: &str,
    core_files: &[(&'static str, &'static str)],
    import: &str,
) -> (Graph, Report) {
    let manifest = format!(r#"{{"name":"@acme/core"{manifest}}}"#);
    let import = format!("{import}\n");
    let mut files: Vec<(&str, &str)> = vec![
        ("packages/app/package.json", r#"{"name":"@acme/app"}"#),
        ("packages/app/x.ts", &import),
        ("packages/core/package.json", &manifest),
    ];
    files.extend_from_slice(core_files);
    run(&files)
}

const CORE_INDEX: (&str, &str) = ("packages/core/src/index.ts", "export class A {}\n");

#[test]
fn the_entry_can_come_from_exports_in_each_of_its_forms() {
    for (exports, note) in [
        (r#","exports":"./src/index.ts""#, "a string"),
        (r#","exports":{".":"./src/index.ts"}"#, "the dot key"),
        (
            r#","exports":{".":{"types":"./dist/index.d.ts","import":"./src/index.ts"}}"#,
            "conditions below the dot key",
        ),
        (
            r#","exports":{"import":"./src/index.ts","require":"./dist/index.cjs"}"#,
            "conditions only",
        ),
        (r#","exports":["./dist/x.js","./src/index.ts"]"#, "an array"),
    ] {
        let (g, report) = with_core(exports, &[CORE_INDEX], "import { A } from '@acme/core';");
        assert_eq!(g.edges.len(), 1, "{note}: {:?}", g.edges);
        assert_import(
            &g,
            "pkg:packages/app:/x",
            "type:packages/core:/src#A",
            &[(Some("packages/app/x.ts"), &[1])],
        );
        assert_eq!(report.imports.resolved, 1, "{note}");
    }
}

#[test]
fn a_package_json_without_an_entry_names_the_index_of_its_directory() {
    let (g, _) = with_core(
        "",
        &[("packages/core/index.ts", "export class A {}\n")],
        "import { A } from '@acme/core';",
    );

    assert_eq!(g.edges.len(), 1, "{:?}", g.edges);
    assert_import(
        &g,
        "pkg:packages/app:/x",
        "type:packages/core:/#A",
        &[(Some("packages/app/x.ts"), &[1])],
    );
}

#[test]
fn a_path_below_a_package_without_exports_is_the_path_in_its_directory() {
    let (g, _) = with_core(
        r#","main":"./src/index.ts""#,
        &[
            CORE_INDEX,
            ("packages/core/src/util.ts", "export class U {}\n"),
        ],
        "import { U } from '@acme/core/src/util';",
    );

    assert_eq!(g.edges.len(), 1, "{:?}", g.edges);
    assert_import(
        &g,
        "pkg:packages/app:/x",
        "type:packages/core:/src/util#U",
        &[(Some("packages/app/x.ts"), &[1])],
    );
}

#[test]
fn a_path_below_a_package_with_exports_is_what_the_matching_entries_name() {
    let util = ("packages/core/src/util.ts", "export class U {}\n");
    for (exports, note) in [
        (r#","exports":{"./util":"./src/util.ts"}"#, "the equal key"),
        (
            r#","exports":{".":"./src/index.ts","./*":{"types":"./dist/*.d.ts","default":"./src/*.ts"}}"#,
            "a key with a star",
        ),
        (
            r#","exports":{"./*":"./src/index.ts","./ut*":"./src/ut*.ts"}"#,
            "the longest prefix",
        ),
    ] {
        let (g, _) = with_core(
            exports,
            &[CORE_INDEX, util],
            "import { U } from '@acme/core/util';",
        );
        assert_eq!(g.edges.len(), 1, "{note}: {:?}", g.edges);
        assert_import(
            &g,
            "pkg:packages/app:/x",
            "type:packages/core:/src/util#U",
            &[(Some("packages/app/x.ts"), &[1])],
        );
    }
}

#[test]
fn a_package_whose_package_json_proves_no_file_is_an_edge_to_its_module() {
    for (manifest, note) in [
        (r#","main":"./dist/index.js""#, "a file that is not read"),
        (
            r#","types":"./src/index.ts","main":"./src/other.ts""#,
            "two different files",
        ),
        (r#","exports":{"./x":"./src/index.ts"}"#, "no dot key"),
    ] {
        let (g, report) = with_core(
            manifest,
            &[
                CORE_INDEX,
                ("packages/core/src/other.ts", "export class A {}\n"),
            ],
            "import {\n  A } from\n  '@acme/core';",
        );
        assert_eq!(g.edges.len(), 1, "{note}: {:?}", g.edges);
        assert_import(
            &g,
            "pkg:packages/app:/x",
            "mod:packages/core",
            &[(Some("packages/app/x.ts"), &[3])],
        );
        assert_eq!(
            (report.imports.resolved, report.names.to_package),
            (1, 1),
            "{note}"
        );
    }
}

#[test]
fn an_entry_that_is_read_counts_beside_entries_that_are_not() {
    for field in ["types", "typings", "module"] {
        let manifest = format!(r#","main":"./dist/index.js","{field}":"./src/index.ts""#);
        let (g, _) = with_core(&manifest, &[CORE_INDEX], "import { A } from '@acme/core';");
        assert_eq!(g.edges.len(), 1, "{field}: {:?}", g.edges);
        assert_import(
            &g,
            "pkg:packages/app:/x",
            "type:packages/core:/src#A",
            &[(Some("packages/app/x.ts"), &[1])],
        );
    }
}

#[test]
fn a_path_that_no_key_of_exports_matches_is_an_edge_to_the_module() {
    let (g, report) = with_core(
        r#","exports":{".":"./src/index.ts"}"#,
        &[
            CORE_INDEX,
            ("packages/core/src/util.ts", "export class U {}\n"),
        ],
        "import { U } from '@acme/core/src/util';",
    );

    assert_eq!(g.edges.len(), 1, "{:?}", g.edges);
    assert_import(
        &g,
        "pkg:packages/app:/x",
        "mod:packages/core",
        &[(Some("packages/app/x.ts"), &[1])],
    );
    assert_eq!((report.imports.resolved, report.names.to_package), (1, 1));
}

#[test]
fn a_package_of_the_repository_without_a_module_is_unresolved() {
    let (g, report) = with_core("", &[], "import { A } from '@acme/core/x';");

    assert_eq!(g.edges, vec![]);
    assert_eq!(
        report.imports,
        ImportReport {
            unresolved: 1,
            ..Default::default()
        }
    );
}

#[test]
fn a_name_that_two_packages_of_the_repository_have_is_unresolved() {
    let (g, report) = run(&[
        ("a/package.json", r#"{"name":"twin"}"#),
        ("a/index.ts", "export class A {}\n"),
        ("b/package.json", r#"{"name":"twin"}"#),
        ("b/index.ts", "import { A } from 'twin';\n"),
    ]);

    assert_eq!(g.edges, vec![]);
    assert_eq!(report.imports.unresolved, 1);
}
