use cg_core::{EdgeKind, Graph, NodeKind, Span};
use cg_index_java::index;
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
