use cg_core::{EdgeKind, Graph, NodeKind, Span};
use cg_index_java::{CallReport, SourceFile, index, index_sources, index_with_report, is_input};
use std::fs;
use tempfile::TempDir;

fn repo(files: &[(&str, &str)]) -> TempDir {
    let dir = TempDir::new().unwrap();
    for (path, content) in files {
        let full = dir.path().join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, content).unwrap();
    }
    dir
}

fn parent<'a>(g: &'a Graph, id: &str) -> &'a cg_core::Node {
    let node = g.node(id).unwrap_or_else(|| panic!("no node {id}"));
    g.node(node.parent.as_deref().expect("node has no parent"))
        .unwrap()
}

#[test]
fn a_class_gives_the_chain_package_type_with_file_and_span() {
    let dir = repo(&[(
        "src/main/java/a/b/Foo.java",
        "package a.b;\n\npublic class Foo {\n    int x;\n}\n",
    )]);

    let g = index(dir.path()).unwrap();

    let ty = g.node("type:a.b.Foo").expect("type node");
    assert_eq!((ty.kind, ty.name.as_str()), (NodeKind::Type, "Foo"));
    assert_eq!(ty.file.as_deref(), Some("src/main/java/a/b/Foo.java"));
    assert_eq!(
        ty.span,
        Some(Span {
            start_line: 3,
            end_line: 5
        })
    );
    assert!(!ty.test);
    let pkg = parent(&g, "type:a.b.Foo");
    assert_eq!((pkg.kind, pkg.name.as_str()), (NodeKind::Package, "a.b"));
    assert_eq!((pkg.file.as_ref(), pkg.span, pkg.test), (None, None, false));
    assert!(
        g.nodes
            .iter()
            .all(|n| n.id != "file:src/main/java/a/b/Foo.java")
    );
}

#[test]
fn a_type_below_a_src_directory_other_than_main_is_marked_as_test() {
    let dir = repo(&[
        ("m/src/main/java/a/Foo.java", "package a;\nclass Foo {}\n"),
        (
            "m/src/test/java/a/FooTest.java",
            "package a;\nclass FooTest {}\n",
        ),
        ("m/src/e2e/java/a/E2e.java", "package a;\nclass E2e {}\n"),
        ("m/src/it/java/a/Sys.java", "package a;\nclass Sys {}\n"),
        ("m/lib/a/NoSrc.java", "package a;\nclass NoSrc {}\n"),
        ("m/src/Direct.java", "package a;\nclass Direct {}\n"),
        ("m/srcx/test/a/Odd.java", "package a;\nclass Odd {}\n"),
    ]);

    let g = index(dir.path()).unwrap();

    for id in ["type:a.FooTest", "type:a.E2e", "type:a.Sys"] {
        assert!(g.node(id).unwrap().test, "{id}");
    }
    for id in ["type:a.Foo", "type:a.NoSrc", "type:a.Direct", "type:a.Odd"] {
        assert!(!g.node(id).unwrap().test, "{id}");
    }
}

#[test]
fn a_nested_type_is_a_type_node_below_the_enclosing_type() {
    let dir = repo(&[(
        "a/Outer.java",
        "package a;\n\
         public class Outer {\n\
         \x20   class Inner {\n\
         \x20       enum Deep { X; interface InEnum {} }\n\
         \x20   }\n\
         \x20   interface Face { record Rec(int x) {} }\n\
         \x20   @interface Ann { class InAnn {} }\n\
         }\n",
    )]);

    let g = index(dir.path()).unwrap();

    for (id, parent_id, name) in [
        ("type:a.Outer.Inner", "type:a.Outer", "Inner"),
        ("type:a.Outer.Inner.Deep", "type:a.Outer.Inner", "Deep"),
        (
            "type:a.Outer.Inner.Deep.InEnum",
            "type:a.Outer.Inner.Deep",
            "InEnum",
        ),
        ("type:a.Outer.Face", "type:a.Outer", "Face"),
        ("type:a.Outer.Face.Rec", "type:a.Outer.Face", "Rec"),
        ("type:a.Outer.Ann", "type:a.Outer", "Ann"),
        ("type:a.Outer.Ann.InAnn", "type:a.Outer.Ann", "InAnn"),
    ] {
        let node = g.node(id).unwrap_or_else(|| panic!("no node {id}"));
        assert_eq!(node.kind, NodeKind::Type, "{id}");
        assert_eq!(node.name, name, "{id}");
        assert_eq!(node.parent.as_deref(), Some(parent_id), "{id}");
        assert_eq!(node.file.as_deref(), Some("a/Outer.java"), "{id}");
    }
    assert_eq!(
        g.node("type:a.Outer.Inner.Deep").unwrap().span,
        Some(Span {
            start_line: 4,
            end_line: 4
        })
    );
}

#[test]
fn a_simple_name_resolves_to_a_nested_type_of_an_enclosing_type_first() {
    let dir = repo(&[
        ("a/Inner.java", "package a;\nclass Inner {}\n"),
        ("a/Mid.java", "package a;\nclass Mid {}\n"),
        ("x/Inner.java", "package x;\npublic class Inner {}\n"),
        (
            "a/Outer.java",
            "package a;\nimport x.Inner;\n\
             class Outer {\n\
             \x20   class Inner {}\n\
             \x20   class Mid {\n\
             \x20       class Deep { Inner i; Mid m; }\n\
             \x20       class Shadow { class Inner {} Inner own; }\n\
             \x20   }\n\
             }\n",
        ),
    ]);

    let g = index(dir.path()).unwrap();

    let deep = "type:a.Outer.Mid.Deep";
    assert!(g.has_edge(deep, "type:a.Outer.Inner", EdgeKind::TypeRef));
    assert!(!g.has_edge(deep, "type:a.Inner", EdgeKind::TypeRef));
    assert!(!g.has_edge(deep, "type:x.Inner", EdgeKind::TypeRef));
    assert!(
        g.has_edge(deep, "type:a.Outer.Mid", EdgeKind::TypeRef),
        "an enclosing type before the type of the same name in the package"
    );
    assert!(!g.has_edge(deep, "type:a.Mid", EdgeKind::TypeRef));
    assert!(g.has_edge(
        "type:a.Outer.Mid.Shadow",
        "type:a.Outer.Mid.Shadow.Inner",
        EdgeKind::TypeRef
    ));
}

#[test]
fn a_written_name_outer_dot_inner_and_an_import_of_a_nested_type_resolve() {
    let dir = repo(&[
        (
            "x/Outer.java",
            "package x;\npublic class Outer { public static class Inner {} }\n",
        ),
        (
            "a/Outer.java",
            "package a;\npublic class Outer { public class Inner {} }\n",
        ),
        (
            "a/ByHead.java",
            "package a;\nimport x.Outer;\nclass ByHead { Outer.Inner i; }\n",
        ),
        (
            "a/ByPackage.java",
            "package a;\nclass ByPackage { x.Outer.Inner i; java.util.Map.Entry<String, String> e; }\n",
        ),
        (
            "a/Plain.java",
            "package a;\nclass Plain { Outer.Inner i; }\n",
        ),
        (
            "b/Imported.java",
            "package b;\nimport x.Outer.Inner;\nclass Imported { Inner i; }\n",
        ),
        (
            "b/OnDemand.java",
            "package b;\nimport x.Outer.*;\nclass OnDemand { Inner i; }\n",
        ),
    ]);

    let g = index(dir.path()).unwrap();

    let by_head = g
        .edges
        .iter()
        .filter(|e| e.from == "type:a.ByHead" && e.kind == EdgeKind::TypeRef);
    assert_eq!(
        by_head.map(|e| e.to.as_str()).collect::<Vec<_>>(),
        ["type:x.Outer.Inner"],
        "`Outer` resolves through the import, then `Inner` is looked up in it"
    );
    assert!(g.has_edge("type:a.ByPackage", "type:x.Outer.Inner", EdgeKind::TypeRef));
    assert!(g.has_edge("type:a.Plain", "type:a.Outer.Inner", EdgeKind::TypeRef));
    assert!(g.has_edge("type:b.Imported", "type:x.Outer.Inner", EdgeKind::TypeRef));
    assert!(g.has_edge("type:b.OnDemand", "type:x.Outer.Inner", EdgeKind::TypeRef));
}

#[test]
fn methods_and_constructors_are_function_nodes_below_their_innermost_type() {
    let dir = repo(&[(
        "src/test/java/a/Foo.java",
        "package a;\n\
         class Foo {\n\
         \x20   Foo(int x) {}\n\
         \x20   void bar(int x, List<String> names) {\n\
         \x20       Runnable r = () -> {};\n\
         \x20       Object o = new Object() { void hidden() {} };\n\
         \x20   }\n\
         \x20   void bar(Map<String, int[]> m, final String... rest) {}\n\
         \x20   static class Inner { void in() {} }\n\
         \x20   enum E { A { void constant() {} }; void inEnum() {} }\n\
         \x20   record R(int x) { R {} }\n\
         \x20   interface I { void abstractOne(); }\n\
         }\n",
    )]);

    let g = index(dir.path()).unwrap();

    for (id, parent_id, name, lines) in [
        ("fn:a.Foo#Foo(int)", "type:a.Foo", "Foo", (3, 3)),
        (
            "fn:a.Foo#bar(int,List<String>)",
            "type:a.Foo",
            "bar",
            (4, 7),
        ),
        (
            "fn:a.Foo#bar(Map<String,int[]>,String...)",
            "type:a.Foo",
            "bar",
            (8, 8),
        ),
        ("fn:a.Foo.Inner#in()", "type:a.Foo.Inner", "in", (9, 9)),
        ("fn:a.Foo.E#inEnum()", "type:a.Foo.E", "inEnum", (10, 10)),
        ("fn:a.Foo.R#R()", "type:a.Foo.R", "R", (11, 11)),
        (
            "fn:a.Foo.I#abstractOne()",
            "type:a.Foo.I",
            "abstractOne",
            (12, 12),
        ),
    ] {
        let node = g.node(id).unwrap_or_else(|| panic!("no node {id}"));
        assert_eq!(node.kind, NodeKind::Function, "{id}");
        assert_eq!(node.name, name, "{id}");
        assert_eq!(node.parent.as_deref(), Some(parent_id), "{id}");
        assert_eq!(
            node.file.as_deref(),
            Some("src/test/java/a/Foo.java"),
            "{id}"
        );
        let (start_line, end_line) = lines;
        assert_eq!(
            node.span,
            Some(Span {
                start_line,
                end_line
            }),
            "{id}"
        );
        assert!(node.test, "{id}");
    }
    let functions = g
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Function)
        .count();
    assert_eq!(
        functions, 7,
        "no node for the anonymous classes and lambdas"
    );
}

fn edges_from<'a>(g: &'a Graph, from: &str) -> Vec<(&'a str, EdgeKind)> {
    g.edges
        .iter()
        .filter(|e| e.from == from)
        .map(|e| (e.to.as_str(), e.kind))
        .collect()
}

#[test]
fn a_type_parameter_hides_a_declared_type_of_the_same_name() {
    let dir = repo(&[
        ("a/T.java", "package a;\nclass T {}\n"),
        ("a/U.java", "package a;\nclass U {}\n"),
        (
            "a/Box.java",
            "package a;\n\
             class Box<T> {\n\
             \x20   T value;\n\
             \x20   T get() { return value; }\n\
             \x20   class Inner { T outer; }\n\
             \x20   <U> U pick(U x, T y) { return x; }\n\
             \x20   U plain;\n\
             \x20   static <T> T shadow(T x) { return x; }\n\
             }\n",
        ),
        (
            "a/Free.java",
            "package a;\nclass Free {\n    T used;\n    <T> T pick(T x) { return x; }\n    T after;\n}\n",
        ),
    ]);

    let g = index(dir.path()).unwrap();

    assert_eq!(
        edges_from(&g, "type:a.Box"),
        [("type:a.U", EdgeKind::TypeRef)]
    );
    assert_eq!(edges_from(&g, "fn:a.Box#get()"), []);
    assert_eq!(edges_from(&g, "type:a.Box.Inner"), []);
    assert_eq!(
        edges_from(&g, "fn:a.Box#pick(U,T)"),
        [],
        "the parameters of the method and of the enclosing type hide `U` and `T`"
    );
    assert_eq!(edges_from(&g, "fn:a.Box#shadow(T)"), []);
    assert_eq!(
        edges_from(&g, "type:a.Free"),
        [("type:a.T", EdgeKind::TypeRef)],
        "a `T` outside of any parameter scope still resolves"
    );
    assert_eq!(edges_from(&g, "fn:a.Free#pick(T)"), []);
}

#[test]
fn a_type_reference_gives_an_edge_from_the_innermost_declaration() {
    let dir = repo(&[
        ("a/P.java", "package a;\nclass P {}\n"),
        ("a/Ret.java", "package a;\nclass Ret {}\n"),
        (
            "a/Thrown.java",
            "package a;\nclass Thrown extends Exception {}\n",
        ),
        ("a/Made.java", "package a;\nclass Made {}\n"),
        ("a/Cast.java", "package a;\nclass Cast {}\n"),
        ("a/Arg.java", "package a;\nclass Arg {}\n"),
        ("a/Local.java", "package a;\nclass Local {}\n"),
        ("a/FieldT.java", "package a;\nclass FieldT {}\n"),
        ("a/Head.java", "package a;\nclass Head {}\n"),
        ("a/Base.java", "package a;\nclass Base {}\n"),
        ("a/Face.java", "package a;\ninterface Face {}\n"),
        ("a/InCtor.java", "package a;\nclass InCtor {}\n"),
        (
            "a/Foo.java",
            "package a;\n\
             class Foo<T extends Head> extends Base implements Face {\n\
             \x20   FieldT field;\n\
             \x20   Foo(InCtor c) {}\n\
             \x20   Ret bar(P p) throws Thrown {\n\
             \x20       Object o = new Made();\n\
             \x20       Cast c = (Cast) o;\n\
             \x20       java.util.List<Arg> l = null;\n\
             \x20       Local x = null;\n\
             \x20       return null;\n\
             \x20   }\n\
             }\n",
        ),
    ]);

    let g = index(dir.path()).unwrap();

    let mut from_function = edges_from(&g, "fn:a.Foo#bar(P)");
    from_function.retain(|(_, kind)| *kind == EdgeKind::TypeRef);
    from_function.sort();
    let expected_function: Vec<_> = ["Arg", "Cast", "Local", "Made", "P", "Ret", "Thrown"]
        .iter()
        .map(|name| (format!("type:a.{name}"), EdgeKind::TypeRef))
        .collect();
    assert_eq!(
        from_function
            .iter()
            .map(|(to, kind)| (to.to_string(), *kind))
            .collect::<Vec<_>>(),
        expected_function
    );
    assert_eq!(
        edges_from(&g, "fn:a.Foo#Foo(InCtor)"),
        [("type:a.InCtor", EdgeKind::TypeRef)]
    );

    let from_type = edges_from(&g, "type:a.Foo");
    for edge in [
        ("type:a.FieldT", EdgeKind::TypeRef),
        ("type:a.Head", EdgeKind::TypeRef),
        ("type:a.Base", EdgeKind::Extends),
        ("type:a.Face", EdgeKind::Implements),
    ] {
        assert!(from_type.contains(&edge), "{edge:?}");
    }
    for name in [
        "Arg", "Cast", "Local", "Made", "P", "Ret", "Thrown", "InCtor",
    ] {
        assert!(
            !from_type.contains(&(format!("type:a.{name}").as_str(), EdgeKind::TypeRef)),
            "{name} is used by a function only"
        );
    }
}

#[test]
fn an_edge_from_a_node_to_itself_or_from_a_function_to_its_own_type_is_dropped() {
    let dir = repo(&[(
        "a/Outer.java",
        "package a;\n\
             class Outer {\n\
             \x20   Outer self;\n\
             \x20   Outer make(Outer o) { return new Outer(); }\n\
             \x20   class Inner {\n\
             \x20       Outer up(Inner i) { return null; }\n\
             \x20   }\n\
             }\n",
    )]);

    let g = index(dir.path()).unwrap();

    assert_eq!(edges_from(&g, "type:a.Outer"), []);
    assert_eq!(edges_from(&g, "fn:a.Outer#make(Outer)"), []);
    assert_eq!(
        edges_from(&g, "fn:a.Outer.Inner#up(Inner)"),
        [("type:a.Outer", EdgeKind::TypeRef)],
        "to the outer type: not the own type of the function"
    );
    assert_eq!(edges_from(&g, "type:a.Outer.Inner"), []);
}

#[test]
fn a_single_type_import_gives_an_edge_from_each_top_level_type_of_the_file() {
    let dir = repo(&[
        ("x/Used.java", "package x;\npublic class Used {}\n"),
        ("x/Unused.java", "package x;\npublic class Unused {}\n"),
        (
            "x/Outer.java",
            "package x;\npublic class Outer { public static class Nest {} }\n",
        ),
        (
            "a/Two.java",
            "package a;\nimport x.Used;\nimport x.Outer.Nest;\nimport x.Missing;\nimport java.util.List;\n\
             class First { class Inside { void f() {} } void g() {} }\n\
             class Second {}\n",
        ),
    ]);

    let g = index(dir.path()).unwrap();

    for from in ["type:a.First", "type:a.Second"] {
        assert!(g.has_edge(from, "type:x.Used", EdgeKind::Imports), "{from}");
        assert!(
            g.has_edge(from, "type:x.Outer.Nest", EdgeKind::Imports),
            "{from}"
        );
    }
    let imports: Vec<_> = g
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Imports)
        .collect();
    assert_eq!(
        imports.len(),
        4,
        "no edge from nested types or functions, none for `Missing`"
    );
    assert!(g.nodes.iter().all(|n| !n.id.starts_with("file:")));
}

#[test]
fn a_static_import_gives_an_imports_edge_to_the_type_that_holds_the_member() {
    let dir = repo(&[
        (
            "x/Util.java",
            "package x;\npublic class Util { public static void foo() {} }\n",
        ),
        (
            "x/Outer.java",
            "package x;\npublic class Outer { public static class Helper { public static void h() {} } }\n",
        ),
        ("y/All.java", "package y;\npublic class All {}\n"),
        (
            "a/User.java",
            "package a;\n\
             import static x.Util.foo;\n\
             import static x.Outer.Helper.h;\n\
             import static y.All.*;\n\
             import static java.util.Objects.requireNonNull;\n\
             import static java.lang.Math.*;\n\
             class User { class Inner {} }\n\
             class Other {}\n",
        ),
    ]);

    let g = index(dir.path()).unwrap();

    for from in ["type:a.User", "type:a.Other"] {
        for to in ["type:x.Util", "type:x.Outer.Helper", "type:y.All"] {
            assert!(g.has_edge(from, to, EdgeKind::Imports), "{from} -> {to}");
        }
    }
    assert_eq!(
        g.edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Imports)
            .count(),
        6,
        "only from the top-level types, nothing for types outside the repository"
    );
}

#[test]
fn extends_and_implements_give_edges_to_types_in_other_files() {
    let dir = repo(&[
        ("src/a/Base.java", "package a;\npublic class Base {}\n"),
        (
            "src/a/Shape.java",
            "package a;\npublic interface Shape {}\n",
        ),
        (
            "src/a/Circle.java",
            "package a;\npublic class Circle extends Base implements Shape {}\n",
        ),
    ]);

    let g = index(dir.path()).unwrap();

    assert!(g.has_edge("type:a.Circle", "type:a.Base", EdgeKind::Extends));
    assert!(g.has_edge("type:a.Circle", "type:a.Shape", EdgeKind::Implements));
}

#[test]
fn explicit_import_wildcard_import_and_same_package_resolve_to_the_correct_type() {
    let dir = repo(&[
        ("x/Explicit.java", "package x;\npublic class Explicit {}\n"),
        ("z/Explicit.java", "package z;\npublic class Explicit {}\n"),
        ("y/Wild.java", "package y;\npublic class Wild {}\n"),
        ("a/Near.java", "package a;\nclass Near {}\n"),
        (
            "a/User.java",
            "package a;\nimport x.Explicit;\nimport y.*;\n\
             public class User { Explicit e; java.util.List<Wild> w; Near n; }\n",
        ),
    ]);

    let g = index(dir.path()).unwrap();

    assert!(g.has_edge("type:a.User", "type:x.Explicit", EdgeKind::TypeRef));
    assert!(g.has_edge("type:a.User", "type:y.Wild", EdgeKind::TypeRef));
    assert!(g.has_edge("type:a.User", "type:a.Near", EdgeKind::TypeRef));
    assert!(!g.has_edge("type:a.User", "type:z.Explicit", EdgeKind::TypeRef));
    assert!(g.has_edge("type:a.User", "type:x.Explicit", EdgeKind::Imports));
}

#[test]
fn a_reference_to_a_type_outside_the_repository_gives_no_edge() {
    let dir = repo(&[(
        "a/User.java",
        "package a;\nimport java.util.List;\nimport static java.util.Objects.requireNonNull;\n\
         public class User extends Thread implements Runnable { List<String> names; }\n",
    )]);

    let g = index(dir.path()).unwrap();

    assert!(g.node("type:a.User").is_some());
    assert_eq!(g.edges, vec![]);
}

#[test]
fn a_maven_module_is_the_parent_of_its_packages() {
    let class = "package a;\npublic class Foo {}\n";
    let dir = repo(&[
        ("pom.xml", "<project/>"),
        ("api/pom.xml", "<project/>"),
        ("api/src/main/java/a/Foo.java", class),
        ("util/pom.xml", "<project/>"),
        (
            "util/src/main/java/a/Bar.java",
            "package a;\npublic class Bar {}\n",
        ),
    ]);

    let g = index(dir.path()).unwrap();

    let api_package = parent(&g, "type:a.Foo");
    let util_package = parent(&g, "type:a.Bar");
    assert_ne!(
        api_package.id, util_package.id,
        "one package node for each module"
    );
    let api = parent(&g, &api_package.id);
    assert_eq!((api.kind, api.name.as_str()), (NodeKind::Module, "api"));
    assert_eq!(parent(&g, &util_package.id).name, "util");
}

#[test]
fn hidden_directories_and_build_output_are_not_indexed() {
    let class = "package a;\npublic class Foo {}\n";
    let dir = repo(&[
        ("src/a/Foo.java", class),
        (".worktrees/copy/src/a/Foo.java", class),
        ("target/generated-sources/a/Foo.java", class),
    ]);

    let g = index(dir.path()).unwrap();

    let types: Vec<_> = g
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Type)
        .collect();
    assert_eq!(types.len(), 1);
    assert_eq!(types[0].file.as_deref(), Some("src/a/Foo.java"));
}

fn hash_of(g: &Graph, id: &str) -> Option<String> {
    g.node(id)
        .unwrap_or_else(|| panic!("no node {id}"))
        .hash
        .clone()
}

const TWO_METHODS: &str =
    "package a;\nclass Foo {\n    int one() { return 1; }\n    int two() { return 2; }\n}\n";

#[test]
fn a_changed_method_body_changes_the_hash_of_that_function_only() {
    let before = index(repo(&[("a/Foo.java", TWO_METHODS)]).path()).unwrap();
    let changed = TWO_METHODS.replace("return 1;", "return 11;");
    let after = index(repo(&[("a/Foo.java", &changed)]).path()).unwrap();

    let one = "fn:a.Foo#one()";
    assert!(hash_of(&before, one).is_some());
    assert_ne!(hash_of(&before, one), hash_of(&after, one));
    assert_eq!(
        hash_of(&before, "fn:a.Foo#two()"),
        hash_of(&after, "fn:a.Foo#two()")
    );
    assert_eq!(
        hash_of(&before, "type:a.Foo"),
        hash_of(&after, "type:a.Foo")
    );
}

#[test]
fn a_new_field_changes_the_hash_of_the_type_and_not_of_its_methods() {
    let before = index(repo(&[("a/Foo.java", TWO_METHODS)]).path()).unwrap();
    let changed = TWO_METHODS.replace("class Foo {\n", "class Foo {\n    int field;\n");
    let after = index(repo(&[("a/Foo.java", &changed)]).path()).unwrap();

    assert_ne!(
        hash_of(&before, "type:a.Foo"),
        hash_of(&after, "type:a.Foo")
    );
    for method in ["fn:a.Foo#one()", "fn:a.Foo#two()"] {
        assert_eq!(hash_of(&before, method), hash_of(&after, method));
    }
}

#[test]
fn whitespace_and_position_do_not_change_any_hash() {
    let before = index(repo(&[("a/Foo.java", TWO_METHODS)]).path()).unwrap();
    let reformatted = "\n\n\npackage a;\n\n\nclass   Foo {\n\n        int one() {\n            return 1;\n        }\n\n\n\tint two() { return 2; }\n}\n";
    let after = index(repo(&[("a/Foo.java", reformatted)]).path()).unwrap();

    for id in ["type:a.Foo", "fn:a.Foo#one()", "fn:a.Foo#two()"] {
        assert_eq!(hash_of(&before, id), hash_of(&after, id), "{id}");
    }
    assert_eq!(
        before
            .node("type:a.Foo")
            .unwrap()
            .hash
            .as_deref()
            .map(str::len),
        Some(16)
    );
}

/// The targets of the `calls` edges that leave `from`.
fn calls_from<'a>(g: &'a Graph, from: &str) -> Vec<&'a str> {
    g.edges
        .iter()
        .filter(|e| e.from == from && e.kind == EdgeKind::Calls)
        .map(|e| e.to.as_str())
        .collect()
}

#[test]
fn new_t_calls_the_one_constructor_of_t_with_that_number_of_arguments() {
    let dir = repo(&[
        (
            "a/T.java",
            "package a;\nclass T {\n    T() {}\n    T(int x) {}\n}\n",
        ),
        (
            "a/User.java",
            "package a;\nclass User {\n    void one() { new T(1); }\n    void none() { new T(); }\n}\n",
        ),
    ]);

    let g = index(dir.path()).unwrap();

    assert_eq!(calls_from(&g, "fn:a.User#one()"), ["fn:a.T#T(int)"]);
    assert_eq!(calls_from(&g, "fn:a.User#none()"), ["fn:a.T#T()"]);
}

#[test]
fn new_t_calls_the_type_when_several_constructors_fit_or_none_is_declared() {
    let dir = repo(&[
        (
            "a/Two.java",
            "package a;\nclass Two {\n    Two(int x) {}\n    Two(String s) {}\n    Two(int... xs) {}\n}\n",
        ),
        ("a/Implicit.java", "package a;\nclass Implicit {}\n"),
        (
            "a/User.java",
            "package a;\n\
             import java.util.ArrayList;\n\
             class User<G> {\n\
             \x20   void overloads() { new Two(1); }\n\
             \x20   void varargs() { new Two(1, 2, 3); }\n\
             \x20   void none() { new Implicit(); }\n\
             \x20   void external() { new ArrayList<String>(); new java.io.File(\"x\"); new int[3]; }\n\
             \x20   <P> void parameter() { new G(); new P(); }\n\
             }\n",
        ),
    ]);

    let g = index(dir.path()).unwrap();

    assert_eq!(calls_from(&g, "fn:a.User#overloads()"), ["type:a.Two"]);
    assert_eq!(
        calls_from(&g, "fn:a.User#varargs()"),
        ["fn:a.Two#Two(int...)"]
    );
    assert_eq!(calls_from(&g, "fn:a.User#none()"), ["type:a.Implicit"]);
    assert!(calls_from(&g, "fn:a.User#external()").is_empty());
    assert!(calls_from(&g, "fn:a.User#parameter()").is_empty());
}

#[test]
fn a_call_in_a_field_initializer_or_an_initializer_block_comes_from_the_type() {
    let dir = repo(&[
        ("a/T.java", "package a;\nclass T {\n    T(int x) {}\n}\n"),
        ("a/U.java", "package a;\nclass U {\n    U(String s) {}\n}\n"),
        ("a/V.java", "package a;\nclass V {\n    V(long s) {}\n}\n"),
        (
            "a/Holder.java",
            "package a;\n\
             class Holder {\n\
             \x20   T field = new T(1);\n\
             \x20   static { new U(\"s\"); }\n\
             \x20   { new V(2L); }\n\
             \x20   class Inner {\n\
             \x20       T inner = new T(2);\n\
             \x20       void f() { new V(3L); }\n\
             \x20   }\n\
             }\n",
        ),
    ]);

    let g = index(dir.path()).unwrap();

    let mut own = calls_from(&g, "type:a.Holder");
    own.sort();
    assert_eq!(own, ["fn:a.T#T(int)", "fn:a.U#U(String)", "fn:a.V#V(long)"]);
    assert_eq!(calls_from(&g, "type:a.Holder.Inner"), ["fn:a.T#T(int)"]);
    assert_eq!(calls_from(&g, "fn:a.Holder.Inner#f()"), ["fn:a.V#V(long)"]);
}

#[test]
fn a_call_edge_follows_the_conventions_of_the_other_edges() {
    let dir = repo(&[
        ("a/T.java", "package a;\nclass T {\n    T(int x) {}\n}\n"),
        ("a/U.java", "package a;\nclass U {\n    U(int x) {}\n}\n"),
        (
            "a/Outer.java",
            "package a;\n\
             class Outer {\n\
             \x20   void self() { new Outer(); }\n\
             \x20   void twice() { new T(1); new T(2); }\n\
             \x20   void inLambda() { Runnable r = () -> new T(1); }\n\
             \x20   void inAnonymous() { new Object() { void g() { new U(1); } }; }\n\
             \x20   class Inner {\n\
             \x20       void up() { new Outer(); }\n\
             \x20   }\n\
             }\n",
        ),
        (
            "a/Rec.java",
            "package a;\nclass Rec {\n    Rec(int n) { new Rec(n - 1); }\n}\n",
        ),
    ]);

    let g = index(dir.path()).unwrap();

    assert!(
        calls_from(&g, "fn:a.Outer#self()").is_empty(),
        "a function does not call its own type"
    );
    assert_eq!(calls_from(&g, "fn:a.Outer#twice()"), ["fn:a.T#T(int)"]);
    assert_eq!(calls_from(&g, "fn:a.Outer#inLambda()"), ["fn:a.T#T(int)"]);
    assert_eq!(
        calls_from(&g, "fn:a.Outer#inAnonymous()"),
        ["fn:a.U#U(int)"]
    );
    assert_eq!(
        calls_from(&g, "fn:a.Outer.Inner#up()"),
        ["type:a.Outer"],
        "to the outer type: not the own type of the function"
    );
    assert!(
        calls_from(&g, "fn:a.Rec#Rec(int)").is_empty(),
        "a function that calls itself gives no edge"
    );
}

#[test]
fn t_dot_m_calls_the_method_of_the_type_that_fits_by_name_and_number_of_arguments() {
    let dir = repo(&[
        (
            "a/Util.java",
            "package a;\n\
             class Util {\n\
             \x20   static int one(int a) { return a; }\n\
             \x20   static int two(int a) { return a; }\n\
             \x20   static int two(String s) { return 0; }\n\
             \x20   static void zero() {}\n\
             \x20   static void many(String s, int... rest) {}\n\
             }\n",
        ),
        (
            "a/User.java",
            "package a;\n\
             class User {\n\
             \x20   void one() { Util.one(1); }\n\
             \x20   void overloaded() { Util.two(1); }\n\
             \x20   void zero() { Util.zero(); }\n\
             \x20   void varargs() { Util.many(\"s\"); Util.many(\"s\", 1, 2); }\n\
             \x20   void undeclared() { Util.missing(); Util.one(1, 2); }\n\
             \x20   void external() { Math.max(1, 2); java.util.List.of(); }\n\
             }\n",
        ),
    ]);

    let g = index(dir.path()).unwrap();

    assert_eq!(calls_from(&g, "fn:a.User#one()"), ["fn:a.Util#one(int)"]);
    assert_eq!(calls_from(&g, "fn:a.User#overloaded()"), ["type:a.Util"]);
    assert_eq!(calls_from(&g, "fn:a.User#zero()"), ["fn:a.Util#zero()"]);
    assert_eq!(
        calls_from(&g, "fn:a.User#varargs()"),
        ["fn:a.Util#many(String,int...)"]
    );
    assert_eq!(calls_from(&g, "fn:a.User#undeclared()"), ["type:a.Util"]);
    assert!(calls_from(&g, "fn:a.User#external()").is_empty());
}

#[test]
fn m_without_receiver_looks_in_the_enclosing_types_and_their_supertypes() {
    let dir = repo(&[
        (
            "a/Base.java",
            "package a;\nclass Base {\n    void inherited() {}\n    void shadowed() {}\n}\n",
        ),
        (
            "a/Mid.java",
            "package a;\nclass Mid extends Base {\n    void shadowed() {}\n}\n",
        ),
        (
            "a/Iface.java",
            "package a;\ninterface Iface {\n    default void viaInterface() {}\n}\n",
        ),
        (
            "a/Leaf.java",
            "package a;\n\
             class Leaf extends Mid implements Iface {\n\
             \x20   void own() {}\n\
             \x20   void f() { own(); inherited(); shadowed(); viaInterface(); }\n\
             \x20   void g() { absent(); String.valueOf(1); }\n\
             }\n",
        ),
        (
            "a/Outer.java",
            "package a;\n\
             class Outer {\n\
             \x20   void outerMethod() {}\n\
             \x20   void both() {}\n\
             \x20   class Inner {\n\
             \x20       void both() {}\n\
             \x20       void g() { outerMethod(); both(); }\n\
             \x20   }\n\
             }\n",
        ),
    ]);

    let g = index(dir.path()).unwrap();

    assert_eq!(
        calls_from(&g, "fn:a.Leaf#f()"),
        [
            "fn:a.Base#inherited()",
            "fn:a.Iface#viaInterface()",
            "fn:a.Leaf#own()",
            "fn:a.Mid#shadowed()",
        ]
    );
    assert!(calls_from(&g, "fn:a.Leaf#g()").is_empty());
    assert_eq!(
        calls_from(&g, "fn:a.Outer.Inner#g()"),
        ["fn:a.Outer#outerMethod()", "fn:a.Outer.Inner#both()"],
        "the innermost type that has the method wins"
    );
}

#[test]
fn this_dot_m_looks_in_the_innermost_type_and_its_supertypes() {
    let dir = repo(&[
        (
            "a/Base.java",
            "package a;\nclass Base {\n    void inherited() {}\n}\n",
        ),
        (
            "a/Outer.java",
            "package a;\n\
             class Outer extends Base {\n\
             \x20   void own() {}\n\
             \x20   void outerOnly() {}\n\
             \x20   void f() { this.own(); this.inherited(); }\n\
             \x20   class Inner {\n\
             \x20       void g() { this.outerOnly(); }\n\
             \x20   }\n\
             }\n",
        ),
    ]);

    let g = index(dir.path()).unwrap();

    assert_eq!(
        calls_from(&g, "fn:a.Outer#f()"),
        ["fn:a.Base#inherited()", "fn:a.Outer#own()"]
    );
    assert!(
        calls_from(&g, "fn:a.Outer.Inner#g()").is_empty(),
        "`this` is the inner object, which has no `outerOnly`: its own type, no edge"
    );
}

#[test]
fn m_without_receiver_falls_back_to_a_static_import() {
    let dir = repo(&[
        (
            "a/Util.java",
            "package a;\n\
             public class Util {\n\
             \x20   public static void helper() {}\n\
             \x20   public static void other() {}\n\
             \x20   public static void twice(int x) {}\n\
             \x20   public static void twice(String x) {}\n\
             }\n",
        ),
        (
            "b/User.java",
            "package b;\n\
             import static a.Util.helper;\n\
             import static a.Util.twice;\n\
             import static a.Util.*;\n\
             import static org.junit.Assert.assertEquals;\n\
             class User {\n\
             \x20   void helper() {}\n\
             \x20   void f() { helper(); other(); twice(1); assertEquals(1, 2); }\n\
             }\n",
        ),
    ]);

    let g = index(dir.path()).unwrap();

    assert_eq!(
        calls_from(&g, "fn:b.User#f()"),
        ["fn:a.Util#other()", "fn:b.User#helper()", "type:a.Util"],
        "a method of the type wins over the import; two overloads of the import give the type"
    );
}

#[test]
fn super_dot_m_looks_in_the_superclass_and_not_in_the_own_type() {
    let dir = repo(&[
        (
            "a/Base.java",
            "package a;\nclass Base {\n    void m() {}\n    void n(int a) {}\n}\n",
        ),
        (
            "a/Mid.java",
            "package a;\nclass Mid extends Base {\n    void inherited() {}\n}\n",
        ),
        (
            "a/Sub.java",
            "package a;\n\
             class Sub extends Mid {\n\
             \x20   void m() { super.m(); }\n\
             \x20   void h() { super.n(1); super.inherited(); }\n\
             \x20   void external() { super.toString(); }\n\
             }\n",
        ),
        (
            "a/Ext.java",
            "package a;\nclass Ext extends java.util.ArrayList<String> {\n    void m() { super.add(\"x\"); }\n}\n",
        ),
    ]);

    let g = index(dir.path()).unwrap();

    assert_eq!(calls_from(&g, "fn:a.Sub#m()"), ["fn:a.Base#m()"]);
    assert_eq!(
        calls_from(&g, "fn:a.Sub#h()"),
        ["fn:a.Base#n(int)", "fn:a.Mid#inherited()"]
    );
    assert!(calls_from(&g, "fn:a.Sub#external()").is_empty());
    assert!(calls_from(&g, "fn:a.Ext#m()").is_empty());
}

#[test]
fn x_dot_m_uses_the_declared_type_of_a_local_variable_or_a_parameter() {
    let dir = repo(&[
        (
            "a/A.java",
            "package a;\nclass A {\n    void a() {}\n    void shared() {}\n}\n",
        ),
        (
            "a/B.java",
            "package a;\nclass B {\n    void b() {}\n    void shared() {}\n}\n",
        ),
        (
            "a/User.java",
            "package a;\n\
             import java.util.List;\n\
             class User {\n\
             \x20   User(A fromConstructor) { fromConstructor.a(); }\n\
             \x20   void local() { A x = null; x.a(); }\n\
             \x20   void parameter(B p) { p.b(); }\n\
             \x20   void generic() { java.util.List<A> l = null; l.add(null); }\n\
             \x20   void innermost() {\n\
             \x20       A x = null;\n\
             \x20       { B x = null; x.b(); }\n\
             \x20       x.a();\n\
             \x20   }\n\
             \x20   void leaves() {\n\
             \x20       { B y = null; }\n\
             \x20       y.b();\n\
             \x20   }\n\
             \x20   void multiple() { A p = null, q = null; q.a(); }\n\
             \x20   void loop() { for (A i = null; i != null;) { i.a(); } }\n\
             }\n",
        ),
    ]);

    let g = index(dir.path()).unwrap();

    assert_eq!(calls_from(&g, "fn:a.User#User(A)"), ["fn:a.A#a()"]);
    let a = "fn:a.A#a()";
    assert_eq!(calls_from(&g, "fn:a.User#local()"), [a]);
    assert_eq!(calls_from(&g, "fn:a.User#parameter(B)"), ["fn:a.B#b()"]);
    assert!(calls_from(&g, "fn:a.User#generic()").is_empty());
    assert_eq!(
        calls_from(&g, "fn:a.User#innermost()"),
        [a, "fn:a.B#b()"],
        "the inner `x` hides the outer one only inside its block"
    );
    assert!(calls_from(&g, "fn:a.User#leaves()").is_empty());
    assert_eq!(calls_from(&g, "fn:a.User#multiple()"), [a]);
    assert_eq!(calls_from(&g, "fn:a.User#loop()"), [a]);
}

#[test]
fn x_dot_m_uses_the_type_of_a_catch_for_resource_and_lambda_parameter_and_of_a_pattern() {
    let dir = repo(&[
        (
            "a/A.java",
            "package a;\nclass A {\n    void a() {}\n    void shared() {}\n}\n",
        ),
        (
            "a/B.java",
            "package a;\nclass B {\n    void b() {}\n    void shared() {}\n}\n",
        ),
        (
            "a/User.java",
            "package a;\n\
             import java.util.List;\n\
             import java.util.function.Consumer;\n\
             class User {\n\
             \x20   void each(List<A> l) { for (A i : l) { i.a(); } }\n\
             \x20   void caught() { try { run(); } catch (B e) { e.b(); } }\n\
             \x20   void multi() { try { run(); } catch (A | B e) { e.shared(); } }\n\
             \x20   void resource() throws Exception { try (A r = null; A s = null) { r.a(); s.shared(); } }\n\
             \x20   void typed() { Consumer<A> c = (A p) -> p.a(); }\n\
             \x20   void untyped() { Consumer<A> c = p -> p.a(); }\n\
             \x20   void pattern(Object o) { if (o instanceof A x && x.a()) { x.shared(); } }\n\
             \x20   void switched(Object o) { switch (o) { case B y -> y.b(); default -> {} } }\n\
             \x20   void run() {}\n\
             }\n",
        ),
    ]);

    let g = index(dir.path()).unwrap();

    assert_eq!(calls_from(&g, "fn:a.User#each(List<A>)"), ["fn:a.A#a()"]);
    assert_eq!(
        calls_from(&g, "fn:a.User#caught()"),
        ["fn:a.B#b()", "fn:a.User#run()"]
    );
    assert_eq!(
        calls_from(&g, "fn:a.User#multi()"),
        ["fn:a.User#run()"],
        "the type of a multi-catch variable is not one type"
    );
    assert_eq!(
        calls_from(&g, "fn:a.User#resource()"),
        ["fn:a.A#a()", "fn:a.A#shared()"]
    );
    assert_eq!(calls_from(&g, "fn:a.User#typed()"), ["fn:a.A#a()"]);
    assert!(
        calls_from(&g, "fn:a.User#untyped()").is_empty(),
        "a lambda parameter without a written type is unresolved"
    );
    assert_eq!(
        calls_from(&g, "fn:a.User#pattern(Object)"),
        ["fn:a.A#a()", "fn:a.A#shared()"]
    );
    assert_eq!(calls_from(&g, "fn:a.User#switched(Object)"), ["fn:a.B#b()"]);
}

#[test]
fn var_has_the_type_of_a_new_expression_and_is_unresolved_otherwise() {
    let dir = repo(&[
        (
            "a/A.java",
            "package a;\nclass A {\n    A() {}\n    void a() {}\n    A self() { return this; }\n}\n",
        ),
        (
            "a/User.java",
            "package a;\n\
             class User {\n\
             \x20   void created() { var x = new A(); x.a(); }\n\
             \x20   void other(A p) { var y = p.self(); y.a(); }\n\
             \x20   void each(java.util.List<A> l) { for (var i : l) { i.a(); } }\n\
             }\n",
        ),
    ]);

    let g = index(dir.path()).unwrap();

    assert_eq!(
        calls_from(&g, "fn:a.User#created()"),
        ["fn:a.A#A()", "fn:a.A#a()"]
    );
    assert_eq!(
        calls_from(&g, "fn:a.User#other(A)"),
        ["fn:a.A#self()"],
        "`y` is `var` with an initializer that is not a `new` expression"
    );
    assert!(calls_from(&g, "fn:a.User#each(java.util.List<A>)").is_empty());
}

#[test]
fn x_dot_m_uses_the_type_of_a_field_of_an_enclosing_type_or_of_its_supertypes() {
    let dir = repo(&[
        ("a/A.java", "package a;\nclass A {\n    void a() {}\n}\n"),
        ("a/B.java", "package a;\nclass B {\n    void b() {}\n}\n"),
        (
            "a/Base.java",
            "package a;\nclass Base {\n    A inheritedField;\n    static A constant;\n}\n",
        ),
        (
            "a/User.java",
            "package a;\n\
             import java.util.function.Consumer;\n\
             class User extends Base {\n\
             \x20   A field;\n\
             \x20   B hidden;\n\
             \x20   void viaField() { field.a(); }\n\
             \x20   void viaInherited() { inheritedField.a(); constant.a(); }\n\
             \x20   void viaThis() { this.field.a(); }\n\
             \x20   void localWins() { A hidden = null; hidden.a(); }\n\
             \x20   void parameterWins(A hidden) { hidden.a(); }\n\
             \x20   void lambdaHides() { Consumer<B> c = field -> field.b(); }\n\
             \x20   void variableWinsOverType() { B A = null; A.b(); }\n\
             \x20   class Inner {\n\
             \x20       void g() { field.a(); }\n\
             \x20   }\n\
             }\n",
        ),
    ]);

    let g = index(dir.path()).unwrap();

    let a = ["fn:a.A#a()"];
    assert_eq!(calls_from(&g, "fn:a.User#viaField()"), a);
    assert_eq!(calls_from(&g, "fn:a.User#viaInherited()"), a);
    assert_eq!(calls_from(&g, "fn:a.User#viaThis()"), a);
    assert_eq!(calls_from(&g, "fn:a.User#localWins()"), a);
    assert_eq!(calls_from(&g, "fn:a.User#parameterWins(A)"), a);
    assert!(calls_from(&g, "fn:a.User#lambdaHides()").is_empty());
    assert_eq!(
        calls_from(&g, "fn:a.User#variableWinsOverType()"),
        ["fn:a.B#b()"]
    );
    assert_eq!(calls_from(&g, "fn:a.User.Inner#g()"), a);
}

#[test]
fn this_dot_f_dot_m_and_a_dot_f_dot_m_use_the_declared_type_of_the_field() {
    let dir = repo(&[
        (
            "a/Leaf.java",
            "package a;\nclass Leaf {\n    void leaf() {}\n    static Leaf INSTANCE;\n}\n",
        ),
        (
            "a/Mid.java",
            "package a;\nclass Mid {\n    Leaf leaf;\n    java.util.List<Leaf> list;\n    Leaf[] array;\n    class Nested { static void n() {} }\n}\n",
        ),
        (
            "a/Top.java",
            "package a;\n\
             class Top {\n\
             \x20   Mid mid;\n\
             \x20   void fromThis() { this.mid.leaf.leaf(); }\n\
             \x20   void fromVariable(Mid m) { m.leaf.leaf(); }\n\
             \x20   void fromStatic() { Leaf.INSTANCE.leaf(); a.Leaf.INSTANCE.leaf(); }\n\
             \x20   void fromNested() { Mid.Nested.n(); a.Mid.Nested.n(); }\n\
             \x20   void unresolved(Mid m) { m.list.get(0).leaf(); m.array.length(); m.absent.leaf(); }\n\
             }\n",
        ),
    ]);

    let g = index(dir.path()).unwrap();

    assert_eq!(calls_from(&g, "fn:a.Top#fromThis()"), ["fn:a.Leaf#leaf()"]);
    assert_eq!(
        calls_from(&g, "fn:a.Top#fromVariable(Mid)"),
        ["fn:a.Leaf#leaf()"]
    );
    assert_eq!(
        calls_from(&g, "fn:a.Top#fromStatic()"),
        ["fn:a.Leaf#leaf()"]
    );
    assert_eq!(
        calls_from(&g, "fn:a.Top#fromNested()"),
        ["fn:a.Mid.Nested#n()"]
    );
    assert!(calls_from(&g, "fn:a.Top#unresolved(Mid)").is_empty());
}

#[test]
fn a_dot_b_dot_c_follows_the_declared_return_type_of_a_unique_function() {
    let dir = repo(&[
        (
            "a/B.java",
            "package a;\nclass B {\n    void c() {}\n    B self() { return this; }\n}\n",
        ),
        (
            "a/A.java",
            "package a;\n\
             class A {\n\
             \x20   B b() { return null; }\n\
             \x20   String name() { return \"\"; }\n\
             \x20   <T> T generic() { return null; }\n\
             \x20   B over(int i) { return null; }\n\
             \x20   B over(String s) { return null; }\n\
             }\n",
        ),
        (
            "a/User.java",
            "package a;\n\
             class User {\n\
             \x20   void chain(A x) { x.b().c(); }\n\
             \x20   void longer(A x) { x.b().self().self().c(); }\n\
             \x20   void created() { new A().b().c(); }\n\
             \x20   void external(A x) { x.name().length(); }\n\
             \x20   void generic(A x) { x.generic().c(); }\n\
             \x20   void overloaded(A x) { x.over(1).c(); }\n\
             \x20   void parenthesized(A x) { ((A) null).b().c(); (x).b().c(); }\n\
             }\n",
        ),
    ]);

    let g = index(dir.path()).unwrap();

    let chain = ["fn:a.A#b()", "fn:a.B#c()"];
    assert_eq!(calls_from(&g, "fn:a.User#chain(A)"), chain);
    assert_eq!(
        calls_from(&g, "fn:a.User#longer(A)"),
        ["fn:a.A#b()", "fn:a.B#c()", "fn:a.B#self()"]
    );
    assert_eq!(
        calls_from(&g, "fn:a.User#created()"),
        ["fn:a.A#b()", "fn:a.B#c()", "type:a.A"]
    );
    assert_eq!(calls_from(&g, "fn:a.User#external(A)"), ["fn:a.A#name()"]);
    assert_eq!(calls_from(&g, "fn:a.User#generic(A)"), ["fn:a.A#generic()"]);
    assert_eq!(
        calls_from(&g, "fn:a.User#overloaded(A)"),
        ["type:a.A"],
        "several overloads fit: the return type is not known"
    );
    assert_eq!(calls_from(&g, "fn:a.User#parenthesized(A)"), chain);
}

#[test]
fn a_method_reference_is_a_call_with_an_unknown_number_of_arguments() {
    let dir = repo(&[
        (
            "a/Util.java",
            "package a;\n\
             class Util {\n\
             \x20   Util() {}\n\
             \x20   static void one(int a) {}\n\
             \x20   static void two(int a) {}\n\
             \x20   static void two(String s) {}\n\
             }\n",
        ),
        (
            "a/Plain.java",
            "package a;\nclass Plain {\n    void p() {}\n}\n",
        ),
        (
            "a/User.java",
            "package a;\n\
             class User extends Plain {\n\
             \x20   void own() {}\n\
             \x20   void unique() { Object o = Util::one; }\n\
             \x20   void overloaded() { Object o = Util::two; }\n\
             \x20   void constructor() { Object o = Util::new; }\n\
             \x20   void implicit() { Object o = Plain::new; }\n\
             \x20   void onThis() { Object o = this::own; }\n\
             \x20   void onVariable(Plain x) { Object o = x::p; }\n\
             \x20   void onSuper() { Object o = super::p; }\n\
             \x20   void external() { Object o = String::valueOf; Object n = java.util.ArrayList<String>::new; Object a = int[]::new; }\n\
             }\n",
        ),
    ]);

    let g = index(dir.path()).unwrap();

    assert_eq!(calls_from(&g, "fn:a.User#unique()"), ["fn:a.Util#one(int)"]);
    assert_eq!(calls_from(&g, "fn:a.User#overloaded()"), ["type:a.Util"]);
    assert_eq!(
        calls_from(&g, "fn:a.User#constructor()"),
        ["fn:a.Util#Util()"]
    );
    assert_eq!(calls_from(&g, "fn:a.User#implicit()"), ["type:a.Plain"]);
    assert_eq!(calls_from(&g, "fn:a.User#onThis()"), ["fn:a.User#own()"]);
    assert_eq!(
        calls_from(&g, "fn:a.User#onVariable(Plain)"),
        ["fn:a.Plain#p()"]
    );
    assert_eq!(calls_from(&g, "fn:a.User#onSuper()"), ["fn:a.Plain#p()"]);
    assert!(calls_from(&g, "fn:a.User#external()").is_empty());
}

#[test]
fn the_report_counts_the_call_sites_by_how_they_were_resolved() {
    let dir = repo(&[
        ("a/T.java", "package a;\nclass T {\n    void m() {}\n}\n"),
        (
            "a/Two.java",
            "package a;\nclass Two {\n    Two(int a) {}\n    Two(String s) {}\n}\n",
        ),
        (
            "a/User.java",
            "package a;\n\
             class User {\n\
             \x20   void f(T t) {\n\
             \x20       t.m();\n\
             \x20       t.m();\n\
             \x20       new Two(1);\n\
             \x20       new java.io.File(\"x\");\n\
             \x20       Math.max(1, 2);\n\
             \x20       unknown.call();\n\
             \x20   }\n\
             }\n",
        ),
    ]);

    let (g, report) = index_with_report(dir.path()).unwrap();

    assert_eq!(
        report.calls,
        CallReport {
            to_function: 2,
            to_type: 1,
            external: 2,
            unresolved: 1,
            to_value: 0,
        },
        "duplicates count: the sites are counted, not the edges"
    );
    assert_eq!(g, index(dir.path()).unwrap());
}

#[test]
fn a_call_on_a_literal_or_of_a_method_of_object_is_external() {
    let dir = repo(&[(
        "a/User.java",
        "package a;\n\
         class User {\n\
         \x20   void f() {\n\
         \x20       \"x\".length();\n\
         \x20       User.class.getName();\n\
         \x20       getClass();\n\
         \x20       hashCode();\n\
         \x20   }\n\
         }\n",
    )]);

    let (g, report) = index_with_report(dir.path()).unwrap();

    assert_eq!(
        report.calls,
        CallReport {
            to_function: 0,
            to_type: 0,
            external: 4,
            unresolved: 0,
            to_value: 0,
        }
    );
    assert!(calls_from(&g, "fn:a.User#f()").is_empty());
}

#[test]
fn a_member_of_an_anonymous_class_hides_the_method_of_the_enclosing_type() {
    let dir = repo(&[
        (
            "a/Base.java",
            "package a;\nclass Base {\n    void inherited() {}\n}\n",
        ),
        (
            "a/User.java",
            "package a;\n\
             class User {\n\
             \x20   void helper() {}\n\
             \x20   void clash() {}\n\
             \x20   void f() {\n\
             \x20       new Base() {\n\
             \x20           void own() {}\n\
             \x20           void clash() {}\n\
             \x20           void g() { helper(); clash(); own(); inherited(); this.helper(); }\n\
             \x20       };\n\
             \x20   }\n\
             }\n",
        ),
    ]);

    let g = index(dir.path()).unwrap();

    assert_eq!(
        calls_from(&g, "fn:a.User#f()"),
        ["fn:a.Base#inherited()", "fn:a.User#helper()", "type:a.Base"],
        "`clash()` and `own()` are the members of the anonymous class, `this.helper()` is not known"
    );
}

#[test]
fn this_and_super_with_arguments_call_a_constructor_of_the_own_type_or_the_superclass() {
    let dir = repo(&[
        (
            "a/Base.java",
            "package a;\nclass Base {\n    Base(int x) {}\n}\n",
        ),
        (
            "a/Sub.java",
            "package a;\n\
             class Sub extends Base {\n\
             \x20   Sub() { this(1); }\n\
             \x20   Sub(int x) { super(x); }\n\
             }\n",
        ),
    ]);

    let g = index(dir.path()).unwrap();

    assert_eq!(calls_from(&g, "fn:a.Sub#Sub()"), ["fn:a.Sub#Sub(int)"]);
    assert_eq!(calls_from(&g, "fn:a.Sub#Sub(int)"), ["fn:a.Base#Base(int)"]);
}

fn sources(files: &[(&str, &str)]) -> Vec<SourceFile> {
    files
        .iter()
        .map(|(path, text)| SourceFile {
            path: (*path).into(),
            text: (*text).into(),
        })
        .collect()
}

const TREE: [(&str, &str); 5] = [
    ("pom.xml", ""),
    ("api/pom.xml", ""),
    (
        "api/src/main/java/a/Foo.java",
        "package a;\nimport b.Bar;\npublic class Foo extends Bar { void f() { new Bar().g(); } }\n",
    ),
    (
        "util/src/main/java/b/Bar.java",
        "package b;\npublic class Bar { public void g() {} }\n",
    ),
    (
        "api/src/test/java/a/FooTest.java",
        "package a;\nclass FooTest { Foo foo; }\n",
    ),
];

#[test]
fn index_sources_gives_the_graph_that_index_gives_for_the_same_files_on_disk() {
    let dir = repo(&TREE);
    let root_name = dir
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();

    let from_disk = index_with_report(dir.path()).unwrap();
    let from_data = index_sources(&root_name, &sources(&TREE));

    assert_eq!(from_data, from_disk);
}

#[test]
fn is_input_takes_java_files_and_poms_outside_hidden_and_target_directories() {
    for wanted in [
        "pom.xml",
        "api/pom.xml",
        "A.java",
        "src/main/java/a/Foo.java",
        "target-x/A.java",
    ] {
        assert!(is_input(wanted), "{wanted}");
    }
    for rejected in [
        "",
        "README.md",
        "api/pom.xml.bak",
        "Foo.javax",
        ".java",
        "src/Foo.kt",
        ".git/A.java",
        "a/.hidden/B.java",
        "target/A.java",
        "api/target/classes/B.java",
        "api/target/pom.xml",
        ".mvn/pom.xml",
        "node_modules/x/A.java",
        "web/node_modules/x/pom.xml",
    ] {
        assert!(!is_input(rejected), "{rejected}");
    }
}

#[test]
fn index_sources_ignores_rejected_files_and_the_order_of_the_input() {
    let mut files = sources(&TREE);
    let expected = index_sources("r", &files);

    files.push(SourceFile {
        path: "api/target/classes/Gen.java".into(),
        text: "package gen;\nclass Gen {}\n".into(),
    });
    files.push(SourceFile {
        path: "README.md".into(),
        text: "class NotJava {}".into(),
    });
    files.reverse();
    let shuffled = index_sources("r", &files);
    files.rotate_left(3);

    assert_eq!(shuffled, expected);
    assert_eq!(index_sources("r", &files), expected);
    assert!(expected.0.node("type:gen.Gen").is_none());
}

#[test]
fn index_sources_sorts_paths_by_component_like_a_directory_walk() {
    // By bytes `a-b/` comes before `a/`, by component `a/` comes first. The order of
    // the nodes is the order of the files.
    let files = sources(&[
        ("a-b/X.java", "package q;\nclass Second {}\n"),
        ("a/X.java", "package p;\nclass First {}\n"),
    ]);

    let (graph, _) = index_sources("r", &files);

    let types: Vec<&str> = graph
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Type)
        .map(|n| n.id.as_str())
        .collect();
    assert_eq!(types, ["type:p.First", "type:q.Second"]);
}

#[test]
fn the_module_in_the_root_is_named_after_the_root() {
    let (graph, _) = index_sources(
        "my-repo",
        &sources(&[
            ("pom.xml", ""),
            ("src/main/java/a/A.java", "package a;\nclass A {}\n"),
        ]),
    );

    let module = graph.node("mod:").expect("module of the root");
    assert_eq!(
        (module.kind, module.name.as_str()),
        (NodeKind::Module, "my-repo")
    );
}

fn java_types(packages: &[&str]) -> TempDir {
    let files: Vec<(String, String)> = packages
        .iter()
        .map(|p| {
            let dir = p.replace('.', "/");
            (
                format!("src/main/java/{dir}/T.java"),
                format!("package {p};\nclass T {{}}\n"),
            )
        })
        .collect();
    let files: Vec<(&str, &str)> = files
        .iter()
        .map(|(p, c)| (p.as_str(), c.as_str()))
        .collect();
    repo(&[&[("pom.xml", "<project/>")], files.as_slice()].concat())
}

fn packages(g: &Graph) -> Vec<(String, Option<String>, String)> {
    g.nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Package)
        .map(|n| (n.id.clone(), n.parent.clone(), n.name.clone()))
        .collect()
}

fn package<'a>(g: &'a Graph, name: &str) -> (&'a str, &'a str) {
    let node = g
        .node(&format!("pkg::{name}"))
        .unwrap_or_else(|| panic!("no package {name}"));
    let parent = g.node(node.parent.as_deref().unwrap()).unwrap();
    (parent.id.as_str(), node.name.as_str())
}

#[test]
fn a_prefix_where_packages_branch_is_a_package_node_that_holds_them() {
    let dir = java_types(&["a.b.c.x", "a.b.c.y", "a.b.d.z"]);

    let g = index(dir.path()).unwrap();

    assert_eq!(package(&g, "a.b.c"), ("mod:", "a.b.c"));
    assert_eq!(package(&g, "a.b.c.x"), ("pkg::a.b.c", "x"));
    assert_eq!(package(&g, "a.b.c.y"), ("pkg::a.b.c", "y"));
    assert_eq!(package(&g, "a.b.d.z"), ("mod:", "a.b.d.z"));
    let ids: Vec<String> = packages(&g).into_iter().map(|p| p.0).collect();
    for absent in ["pkg::a", "pkg::a.b", "pkg::a.b.d"] {
        assert!(!ids.iter().any(|id| id == absent), "{absent}");
    }
}

#[test]
fn a_package_below_another_package_that_holds_types_is_its_child() {
    let dir = java_types(&["a.b", "a.b.c"]);

    let g = index(dir.path()).unwrap();

    assert_eq!(package(&g, "a.b"), ("mod:", "a.b"));
    assert_eq!(package(&g, "a.b.c"), ("pkg::a.b", "c"));
    assert_eq!(packages(&g).len(), 2);
}

#[test]
fn one_package_gives_one_node_without_a_chain_of_its_segments() {
    let dir = java_types(&["a.b.c"]);

    let g = index(dir.path()).unwrap();

    assert_eq!(package(&g, "a.b.c"), ("mod:", "a.b.c"));
    assert_eq!(packages(&g).len(), 1);
}

#[test]
fn the_common_prefix_of_all_packages_of_a_module_is_no_node_unless_it_holds_types() {
    let siblings = java_types(&["x.y.cda.impl", "x.y.cda.rest"]);
    let with_types = java_types(&["x.y.util", "x.y.util.fhir", "x.y.util.auth"]);

    let g = index(siblings.path()).unwrap();
    assert_eq!(package(&g, "x.y.cda.impl"), ("mod:", "x.y.cda.impl"));
    assert_eq!(package(&g, "x.y.cda.rest"), ("mod:", "x.y.cda.rest"));
    assert_eq!(packages(&g).len(), 2);

    let g = index(with_types.path()).unwrap();
    assert_eq!(package(&g, "x.y.util"), ("mod:", "x.y.util"));
    assert_eq!(package(&g, "x.y.util.fhir"), ("pkg::x.y.util", "fhir"));
    assert_eq!(package(&g, "x.y.util.auth"), ("pkg::x.y.util", "auth"));
}

#[test]
fn nesting_goes_through_several_levels_and_each_package_names_itself_relative_to_its_parent() {
    let dir = java_types(&["a.b", "a.b.c.d", "a.b.c.e", "a.b.f.g.h", "a.b.f.g.i"]);

    let g = index(dir.path()).unwrap();

    assert_eq!(package(&g, "a.b.c.d"), ("pkg::a.b.c", "d"));
    assert_eq!(package(&g, "a.b.c"), ("pkg::a.b", "c"));
    assert_eq!(package(&g, "a.b.f.g"), ("pkg::a.b", "f.g"));
    assert_eq!(package(&g, "a.b.f.g.h"), ("pkg::a.b.f.g", "h"));
    assert!(g.node("pkg::a.b.f").is_none());
}

#[test]
fn package_ids_keep_the_module_directory_and_the_full_name() {
    let dir = repo(&[
        ("m/pom.xml", "<project/>"),
        (
            "m/src/main/java/a/b/c/X.java",
            "package a.b.c;\nclass X {}\n",
        ),
        (
            "m/src/main/java/a/b/d/Y.java",
            "package a.b.d;\nclass Y {}\n",
        ),
        ("m/src/main/java/a/e/Z.java", "package a.e;\nclass Z {}\n"),
    ]);

    let g = index(dir.path()).unwrap();

    assert_eq!(parent(&g, "type:a.b.c.X").id, "pkg:m:a.b.c");
    assert_eq!(parent(&g, "pkg:m:a.b.c").id, "pkg:m:a.b");
    assert_eq!(parent(&g, "pkg:m:a.b").id, "mod:m");
    assert_eq!(g.node("pkg:m:a.b").unwrap().name, "a.b");
    assert_eq!(g.node("pkg:m:a.b.c").unwrap().name, "c");
}

#[test]
fn packages_of_different_modules_do_not_share_prefix_nodes() {
    let dir = repo(&[
        ("m1/pom.xml", "<project/>"),
        ("m1/src/main/java/a/x/X.java", "package a.x;\nclass X {}\n"),
        ("m1/src/main/java/a/y/Y.java", "package a.y;\nclass Y {}\n"),
        ("m2/pom.xml", "<project/>"),
        ("m2/src/main/java/a/z/Z.java", "package a.z;\nclass Z {}\n"),
    ]);

    let g = index(dir.path()).unwrap();

    assert_eq!(parent(&g, "pkg:m1:a.x").id, "mod:m1");
    assert_eq!(parent(&g, "pkg:m2:a.z").id, "mod:m2");
    assert!(g.node("pkg:m1:a").is_none() && g.node("pkg:m2:a").is_none());
}

#[test]
fn a_prefix_node_is_test_code_only_if_every_package_below_it_is() {
    let dir = repo(&[
        ("pom.xml", "<project/>"),
        ("src/main/java/a/p/x/X.java", "package a.p.x;\nclass X {}\n"),
        ("src/main/java/a/p/y/Y.java", "package a.p.y;\nclass Y {}\n"),
        ("src/test/java/a/q/x/T.java", "package a.q.x;\nclass T {}\n"),
        ("src/test/java/a/q/y/U.java", "package a.q.y;\nclass U {}\n"),
        ("src/main/java/a/r/x/V.java", "package a.r.x;\nclass V {}\n"),
        ("src/test/java/a/r/y/W.java", "package a.r.y;\nclass W {}\n"),
    ]);

    let g = index(dir.path()).unwrap();

    let prefix = |name: &str| g.node(&format!("pkg::{name}")).unwrap();
    assert!(!prefix("a.p").test);
    assert!(prefix("a.q").test);
    assert!(!prefix("a.r").test);
    for n in ["a.p", "a.q", "a.r"] {
        let node = prefix(n);
        assert_eq!(
            (node.file.as_ref(), node.span, node.hash.as_ref()),
            (None, None, None)
        );
    }
    assert!(g.without_tests().node("pkg::a.q").is_none());
    assert!(g.without_tests().node("pkg::a.r").is_some());
}

#[test]
fn a_file_without_a_type_gives_no_package_node_and_the_default_package_does_not_nest() {
    let dir = repo(&[
        ("pom.xml", "<project/>"),
        (
            "src/main/java/a/b/package-info.java",
            "/** Docs. */\npackage a.b;\n",
        ),
        ("src/main/java/a/c/C.java", "package a.c;\nclass C {}\n"),
        ("src/main/java/a/d/D.java", "package a.d;\nclass D {}\n"),
        ("src/main/java/Top.java", "class Top {}\n"),
    ]);

    let g = index(dir.path()).unwrap();

    assert!(g.node("pkg::a.b").is_none());
    assert_eq!(package(&g, "a.c"), ("mod:", "a.c"));
    assert_eq!(parent(&g, "type:Top").id, "pkg::");
    assert_eq!(g.node("pkg::").unwrap().parent.as_deref(), Some("mod:"));
}

#[test]
fn new_record_calls_the_compact_constructor_by_the_number_of_components() {
    let dir = repo(&[
        (
            "a/P.java",
            "package a;\nrecord P(String a, int b) { P { if (a == null) throw new IllegalStateException(); } }\n",
        ),
        ("a/Q.java", "package a;\nrecord Q(String a, int b) {}\n"),
        (
            "a/User.java",
            "package a;\n\
             class User {\n\
             \x20   void compact() { new P(\"x\", 1); }\n\
             \x20   void implicit() { new Q(\"x\", 1); }\n\
             }\n",
        ),
    ]);

    let g = index(dir.path()).unwrap();

    assert_eq!(calls_from(&g, "fn:a.User#compact()"), ["fn:a.P#P()"]);
    assert_eq!(calls_from(&g, "fn:a.User#implicit()"), ["type:a.Q"]);
}

#[test]
fn an_implicit_record_accessor_wins_over_an_inherited_method_of_the_same_name() {
    let dir = repo(&[
        (
            "a/Req.java",
            "package a;\ninterface Req { String domain(); String other(); }\n",
        ),
        (
            "a/FetchReq.java",
            "package a;\nrecord FetchReq(String domain, String other) implements Req {\n    public String other() { return other; }\n}\n",
        ),
        (
            "a/C.java",
            "package a;\n\
             class C {\n\
             \x20   void implicit(FetchReq r) { r.domain(); }\n\
             \x20   void written(FetchReq r) { r.other(); }\n\
             \x20   void viaInterface(Req r) { r.domain(); }\n\
             \x20   void withArgument(FetchReq r) { r.domain(1); }\n\
             }\n",
        ),
    ]);

    let g = index(dir.path()).unwrap();

    assert_eq!(
        calls_from(&g, "fn:a.C#implicit(FetchReq)"),
        ["type:a.FetchReq"]
    );
    assert_eq!(
        calls_from(&g, "fn:a.C#written(FetchReq)"),
        ["fn:a.FetchReq#other()"]
    );
    assert_eq!(
        calls_from(&g, "fn:a.C#viaInterface(Req)"),
        ["fn:a.Req#domain()"]
    );
    // Not an accessor: the arity differs, so the inherited search finds nothing.
    assert_eq!(
        calls_from(&g, "fn:a.C#withArgument(FetchReq)"),
        ["type:a.FetchReq"]
    );
}

#[test]
fn an_unqualified_call_in_a_record_reaches_its_implicit_accessor_as_the_type() {
    let dir = repo(&[
        (
            "a/Req.java",
            "package a;\ninterface Req { String domain(); }\n",
        ),
        (
            "a/Other.java",
            "package a;\nrecord Other(String domain) implements Req {\n    String twice() { return domain() + domain(); }\n    String viaThis() { return this.domain(); }\n}\n",
        ),
    ]);

    let g = index(dir.path()).unwrap();

    // Calls to the own type are dropped (ADR 0014): no edge to the interface method.
    assert!(calls_from(&g, "fn:a.Other#twice()").is_empty());
    assert!(calls_from(&g, "fn:a.Other#viaThis()").is_empty());
}
