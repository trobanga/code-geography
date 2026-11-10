use cg_core::{CallReport, EdgeKind, Graph, Node, NodeKind, Span};
use cg_index_rust::{PathReport, Report, SourceFile, index_sources, is_input};

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

const APP: (&str, &str) = (
    "Cargo.toml",
    "[package]\nname = \"my-app\"\nversion = \"0.1.0\"\n",
);

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
fn a_cargo_package_with_a_struct_gives_module_and_type_with_file_and_span() {
    let g = index(&[
        APP,
        ("src/lib.rs", "// top\npub struct Foo {\n    x: u8,\n}\n"),
    ]);

    let module = node(&g, "mod:");
    assert_eq!(
        (module.kind, module.name.as_str()),
        (NodeKind::Module, "my-app")
    );
    assert_eq!(module.parent, None);
    let ty = node(&g, "type:my_app::Foo");
    assert_eq!((ty.kind, ty.name.as_str()), (NodeKind::Type, "Foo"));
    assert_eq!(ty.parent.as_deref(), Some("mod:"));
    assert_eq!(ty.file.as_deref(), Some("src/lib.rs"));
    assert_eq!(
        ty.span,
        Some(Span {
            start_line: 2,
            end_line: 4
        })
    );
    assert!(!ty.test);
    assert_eq!(g.nodes.len(), 2);
}

#[test]
fn struct_enum_union_trait_and_alias_are_types_but_variants_and_fields_are_not() {
    let g = index(&[
        APP,
        (
            "src/lib.rs",
            "pub struct S { a: u8 }\n\
             pub enum E { A, B { x: u8 } }\n\
             pub union U { a: u8, b: u16 }\n\
             pub trait T { const N: usize; }\n\
             pub type Alias = S;\n\
             pub const C: u8 = 1;\n",
        ),
    ]);

    for name in ["S", "E", "U", "T", "Alias"] {
        let n = node(&g, &format!("type:my_app::{name}"));
        assert_eq!((n.kind, n.name.as_str()), (NodeKind::Type, name));
    }
    assert_eq!(g.nodes.len(), 6, "{:?}", ids(&g));
}

#[test]
fn mods_declared_with_mod_x_are_nested_packages_and_the_crate_root_has_no_node() {
    let g = index(&[
        APP,
        ("src/lib.rs", "pub mod a;\nmod b;\npub struct Top;\n"),
        ("src/a.rs", "pub struct A;\npub mod inner;\n"),
        ("src/a/inner.rs", "pub struct I;\n"),
        ("src/b/mod.rs", "pub struct B;\n"),
    ]);

    let a = node(&g, "pkg::my_app::a");
    assert_eq!((a.kind, a.name.as_str()), (NodeKind::Package, "a"));
    assert_eq!((a.file.as_ref(), a.span, a.test), (None, None, false));
    assert_eq!(parent_of(&g, "pkg::my_app::a"), "mod:");
    assert_eq!(parent_of(&g, "pkg::my_app::b"), "mod:");
    assert_eq!(parent_of(&g, "pkg::my_app::a::inner"), "pkg::my_app::a");
    assert_eq!(parent_of(&g, "type:my_app::Top"), "mod:");
    assert_eq!(parent_of(&g, "type:my_app::a::A"), "pkg::my_app::a");
    let inner = node(&g, "type:my_app::a::inner::I");
    assert_eq!(inner.parent.as_deref(), Some("pkg::my_app::a::inner"));
    assert_eq!(inner.file.as_deref(), Some("src/a/inner.rs"));
    assert_eq!(
        node(&g, "type:my_app::b::B").file.as_deref(),
        Some("src/b/mod.rs")
    );
    assert_eq!(g.nodes.len(), 1 + 3 + 4, "{:?}", ids(&g));
}

#[test]
fn an_inline_mod_is_a_package_and_one_file_can_hold_several_of_them() {
    let g = index(&[
        APP,
        (
            "src/lib.rs",
            "mod one {\n    pub struct A;\n    pub mod deep {\n        pub struct D;\n    }\n}\nmod two {\n    pub mod file;\n}\n",
        ),
        ("src/two/file.rs", "pub struct F;\n"),
    ]);

    assert_eq!(parent_of(&g, "pkg::my_app::one"), "mod:");
    assert_eq!(parent_of(&g, "pkg::my_app::one::deep"), "pkg::my_app::one");
    assert_eq!(parent_of(&g, "pkg::my_app::two::file"), "pkg::my_app::two");
    let d = node(&g, "type:my_app::one::deep::D");
    assert_eq!(d.parent.as_deref(), Some("pkg::my_app::one::deep"));
    assert_eq!(d.file.as_deref(), Some("src/lib.rs"));
    assert_eq!(
        d.span,
        Some(Span {
            start_line: 4,
            end_line: 4
        })
    );
    assert_eq!(
        node(&g, "type:my_app::two::file::F").file.as_deref(),
        Some("src/two/file.rs")
    );
}

#[test]
fn files_that_no_mod_reaches_and_path_attributes_are_reported_not_indexed() {
    let (g, report) = run(&[
        APP,
        ("src/lib.rs", "mod a;\n#[path = \"other.rs\"]\nmod b;\n"),
        ("src/a.rs", "pub struct A;\n"),
        ("src/other.rs", "pub struct Other;\n"),
        ("src/orphan.rs", "pub struct Orphan;\n"),
        ("build.rs", "fn main() {}\n"),
    ]);

    assert!(g.node("type:my_app::a::A").is_some());
    assert_eq!(
        g.nodes.iter().filter(|n| n.kind == NodeKind::Type).count(),
        1
    );
    assert_eq!(report.unreached_files, ["src/orphan.rs", "src/other.rs"]);
    assert_eq!(report.path_attributes, ["src/lib.rs:2"]);
}

#[test]
fn without_a_lib_the_main_is_the_root_of_the_package() {
    let g = index(&[
        APP,
        ("src/main.rs", "mod util;\nstruct M;\n"),
        ("src/util.rs", "struct U;\n"),
    ]);

    assert_eq!(parent_of(&g, "type:my_app::M"), "mod:");
    assert_eq!(parent_of(&g, "type:my_app::util::U"), "pkg::my_app::util");
}

#[test]
fn every_other_target_is_a_package_below_the_module_and_the_test_targets_are_test_code() {
    let g = index(&[
        APP,
        ("src/lib.rs", "pub struct L;\n"),
        ("src/main.rs", "mod m;\nstruct M;\n"),
        ("src/m.rs", "struct Mm;\n"),
        ("src/bin/tool.rs", "struct T;\n"),
        ("src/bin/multi/main.rs", "struct Multi;\n"),
        ("tests/it.rs", "mod common;\nstruct I;\n"),
        ("tests/common/mod.rs", "struct Common;\n"),
        ("benches/b.rs", "struct B;\n"),
        ("examples/e.rs", "struct E;\n"),
        ("build.rs", "struct Build;\n"),
    ]);

    assert_eq!(parent_of(&g, "type:my_app::L"), "mod:");
    for (package, name, ty, test) in [
        ("main", "main", "main::M", false),
        ("bin/tool", "bin/tool", "bin/tool::T", false),
        ("bin/multi", "bin/multi", "bin/multi::Multi", false),
        ("tests/it", "tests/it", "tests/it::I", true),
        ("benches/b", "benches/b", "benches/b::B", true),
        ("examples/e", "examples/e", "examples/e::E", true),
    ] {
        let pkg = node(&g, &format!("pkg::my_app::{package}"));
        assert_eq!(
            (pkg.kind, pkg.name.as_str(), pkg.test),
            (NodeKind::Package, name, test),
            "{package}"
        );
        assert_eq!(pkg.parent.as_deref(), Some("mod:"));
        let ty = node(&g, &format!("type:my_app::{ty}"));
        assert_eq!(
            (ty.parent.as_deref(), ty.test),
            (Some(pkg.id.as_str()), test),
            "{ty:?}"
        );
    }
    assert_eq!(
        parent_of(&g, "type:my_app::main::m::Mm"),
        "pkg::my_app::main::m"
    );
    let common = node(&g, "type:my_app::tests/it::common::Common");
    assert!(common.test);
    assert!(node(&g, "pkg::my_app::tests/it::common").test);
    assert!(g.nodes.iter().all(|n| n.name != "Build"));
}

#[test]
fn is_input_wants_rs_files_and_cargo_manifests_outside_hidden_target_and_node_modules() {
    for path in [
        "Cargo.toml",
        "a/Cargo.toml",
        "src/lib.rs",
        "crates/x/src/a/b.rs",
    ] {
        assert!(is_input(path), "{path}");
    }
    for path in [
        "Cargo.lock",
        "src/lib.rs.bak",
        "a/Cargo.toml.orig",
        ".rs",
        "src/Lib.java",
        "target/debug/build/x.rs",
        "a/target/Cargo.toml",
        ".git/x.rs",
        "web/node_modules/pkg/Cargo.toml",
        "",
    ] {
        assert!(!is_input(path), "{path}");
    }
}

#[test]
fn files_that_is_input_rejects_are_ignored_and_the_order_of_the_files_does_not_matter() {
    let files = [
        APP,
        ("src/lib.rs", "mod a;\npub struct Top;\n"),
        ("src/a.rs", "pub struct A;\n"),
        ("target/debug/x.rs", "pub struct Copy;\n"),
    ];
    let mut reversed = files;
    reversed.reverse();

    let g = run(&files);
    assert!(g.0.nodes.iter().all(|n| n.name != "Copy"));
    assert_eq!(g.1.unreached_files, Vec::<String>::new());
    assert_eq!(g, run(&reversed));
}

#[test]
fn a_free_fn_is_a_child_of_its_mod_and_a_trait_fn_is_a_child_of_the_trait() {
    let g = index(&[
        APP,
        (
            "src/lib.rs",
            "pub mod util {\n    pub fn helper() {\n        fn inner() {}\n        let c = || 1;\n    }\n}\n\
             pub fn run() {}\n\
             pub trait T {\n    fn required(&self);\n    fn provided(&self) {}\n}\n",
        ),
    ]);

    let helper = node(&g, "fn:my_app::util::helper");
    assert_eq!(
        (helper.kind, helper.name.as_str()),
        (NodeKind::Function, "helper")
    );
    assert_eq!(helper.parent.as_deref(), Some("pkg::my_app::util"));
    assert_eq!(helper.file.as_deref(), Some("src/lib.rs"));
    assert_eq!(
        helper.span,
        Some(Span {
            start_line: 2,
            end_line: 5
        })
    );
    assert_eq!(parent_of(&g, "fn:my_app::run"), "mod:");
    assert_eq!(parent_of(&g, "fn:my_app::T#required"), "type:my_app::T");
    assert_eq!(parent_of(&g, "fn:my_app::T#provided"), "type:my_app::T");
    assert_eq!(
        g.nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Function)
            .count(),
        4,
        "{:?}",
        ids(&g)
    );
}

#[test]
fn everything_below_cfg_test_is_test_code() {
    let g = index(&[
        APP,
        (
            "src/lib.rs",
            "pub struct Real;\n\
             #[cfg(test)]\nmod tests {\n    struct Fixture;\n    #[test]\n    fn it_works() {}\n}\n\
             #[cfg(test)]\nstruct OnlyTest;\n\
             #[cfg(not(test))]\nstruct NotTest;\n\
             #[cfg(all(test, unix))]\nfn unix_test() {}\n\
             #[test]\nfn loose_test() {}\n",
        ),
    ]);

    for id in [
        "pkg::my_app::tests",
        "type:my_app::tests::Fixture",
        "fn:my_app::tests::it_works",
        "type:my_app::OnlyTest",
        "fn:my_app::unix_test",
        "fn:my_app::loose_test",
    ] {
        assert!(node(&g, id).test, "{id}");
    }
    for id in ["type:my_app::Real", "type:my_app::NotTest"] {
        assert!(!node(&g, id).test, "{id}");
    }
}

#[test]
fn span_and_hash_include_the_attributes_and_the_hash_ignores_whitespace_and_trait_members() {
    let base = |source: &str| index(&[APP, ("src/lib.rs", source)]);
    let before = base(
        "// c\n#[derive(Debug)]\npub struct S { a: u8 }\npub trait T {\n    fn f(&self);\n}\n",
    );
    let spaced = base(
        "#[derive(Debug)]\npub   struct S {\n a: u8 }\npub trait T {\n    fn f(&self, x: u8);\n}\n",
    );
    let changed = base(
        "#[derive(Debug, Clone)]\npub struct S { a: u8 }\npub trait T {\n    fn f(&self);\n}\n",
    );

    let s = node(&before, "type:my_app::S");
    assert_eq!(
        s.span,
        Some(Span {
            start_line: 2,
            end_line: 3
        })
    );
    assert_eq!(s.hash.as_ref().map(String::len), Some(16));
    assert_eq!(s.hash, node(&spaced, "type:my_app::S").hash);
    assert_ne!(s.hash, node(&changed, "type:my_app::S").hash);
    // The own text of the trait does not hold its functions, which are nodes themselves.
    assert_eq!(
        node(&before, "type:my_app::T").hash,
        node(&spaced, "type:my_app::T").hash
    );
    assert_ne!(
        node(&before, "fn:my_app::T#f").hash,
        node(&spaced, "fn:my_app::T#f").hash
    );
}

#[test]
fn an_impl_fn_is_a_child_of_its_type_wherever_the_impl_stands() {
    let g = index(&[
        APP,
        (
            "src/lib.rs",
            "pub mod model;\npub mod other;\npub trait Show {\n    fn show(&self);\n}\n",
        ),
        (
            "src/model.rs",
            "use crate::Show;\n\
             pub struct Foo;\n\
             pub struct A;\npub struct B;\n\
             impl Foo {\n    pub fn new() -> Foo {\n        impl Foo {\n            fn hidden() {}\n        }\n        Foo\n    }\n}\n\
             impl Show for Foo {\n    fn show(&self) {}\n}\n\
             impl From<A> for Foo {\n    fn from(_: A) -> Foo { Foo }\n}\n\
             impl From<B> for Foo {\n    fn from(_: B) -> Foo { Foo }\n}\n",
        ),
        (
            "src/other.rs",
            "use crate::Show;\nuse crate::model::Foo;\n\
             impl Foo {\n    pub fn elsewhere() {}\n}\n\
             impl std::fmt::Display for Foo {\n    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result { Ok(()) }\n}\n\
             impl Show for Vec<Foo> {\n    fn show(&self) {}\n}\n\
             impl Iterator for std::vec::IntoIter<Foo> {\n    fn next(&mut self) -> Option<u8> { None }\n}\n",
        ),
    ]);

    let foo = "type:my_app::model::Foo";
    for id in [
        "fn:my_app::model::Foo#new",
        "fn:my_app::model::Foo#Show::show",
        "fn:my_app::model::Foo#From<A>::from",
        "fn:my_app::model::Foo#From<B>::from",
        "fn:my_app::model::Foo#elsewhere",
        "fn:my_app::model::Foo#std::fmt::Display::fmt",
    ] {
        assert_eq!(parent_of(&g, id), foo, "{id}");
    }
    assert_eq!(
        node(&g, "fn:my_app::model::Foo#elsewhere").file.as_deref(),
        Some("src/other.rs")
    );
    // The type is outside the repository: the trait, if it is inside, takes the function.
    assert_eq!(
        parent_of(&g, "fn:my_app::Show#for Vec<Foo>::show"),
        "type:my_app::Show"
    );
    // Neither type nor trait is inside: the mod does.
    assert_eq!(
        parent_of(
            &g,
            "fn:my_app::other#Iterator for std::vec::IntoIter<Foo>::next"
        ),
        "pkg::my_app::other"
    );
    assert!(g.nodes.iter().all(|n| n.name != "hidden"));
}

fn has(g: &Graph, from: &str, to: &str, kind: EdgeKind) -> bool {
    g.has_edge(from, to, kind)
}

fn edges_of(g: &Graph, kind: EdgeKind) -> Vec<(&str, &str)> {
    g.edges
        .iter()
        .filter(|e| e.kind == kind)
        .map(|e| (e.from.as_str(), e.to.as_str()))
        .collect()
}

#[test]
fn an_impl_of_a_repository_trait_gives_implements_and_a_supertrait_gives_extends() {
    let g = index(&[
        APP,
        (
            "src/lib.rs",
            "pub trait Base {}\n\
             pub trait Mid: Base {}\n\
             pub trait Top: Mid + std::fmt::Debug + Send {}\n\
             pub struct S;\n\
             impl Mid for S {}\n\
             impl Base for Vec<S> {}\n\
             impl std::fmt::Display for S {}\n\
             impl S {}\n",
        ),
    ]);

    assert_eq!(
        edges_of(&g, EdgeKind::Extends),
        [
            ("type:my_app::Mid", "type:my_app::Base"),
            ("type:my_app::Top", "type:my_app::Mid"),
        ]
    );
    assert_eq!(
        edges_of(&g, EdgeKind::Implements),
        [("type:my_app::S", "type:my_app::Mid")]
    );
}

#[test]
fn use_declarations_give_imports_from_the_mod_to_the_resolved_item() {
    let g = index(&[
        APP,
        (
            "src/lib.rs",
            "pub mod a;\npub mod b;\npub mod c;\npub mod reexport;\npub struct Root;\nmod inner {\n    use super::*;\n}\n",
        ),
        (
            "src/a.rs",
            "pub struct A;\npub struct A2;\npub fn helper() {}\npub mod deep {\n    pub struct D;\n}\n",
        ),
        (
            "src/b.rs",
            "use crate::a::A;\n\
             use crate::a::{A2 as Renamed, deep::{self, D}, helper};\n\
             use super::Root;\n\
             use crate::reexport::Thing;\n\
             use std::collections::HashMap;\n\
             use serde::Serialize;\n",
        ),
        ("src/c.rs", "use crate::a::*;\n"),
        ("src/reexport.rs", "pub use crate::a::A as Thing;\n"),
    ]);

    let b: Vec<&str> = g
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Imports && e.from == "pkg::my_app::b")
        .map(|e| e.to.as_str())
        .collect();
    assert_eq!(
        b,
        [
            "fn:my_app::a::helper",
            "pkg::my_app::a::deep",
            "type:my_app::Root",
            "type:my_app::a::A",
            "type:my_app::a::A2",
            "type:my_app::a::deep::D",
        ]
    );
    assert!(has(
        &g,
        "pkg::my_app::c",
        "pkg::my_app::a",
        EdgeKind::Imports
    ));
    assert!(has(
        &g,
        "pkg::my_app::reexport",
        "type:my_app::a::A",
        EdgeKind::Imports
    ));
    // `use super::*` in a mod directly below the root reaches the module: an ancestor.
    assert!(g.edges.iter().all(|e| e.from != "pkg::my_app::inner"));
    assert_eq!(
        g.edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Imports)
            .count(),
        8
    );
}

#[test]
fn a_type_named_in_a_field_or_signature_gives_type_ref_from_the_innermost_declaration() {
    let g = index(&[
        APP,
        (
            "src/lib.rs",
            "pub mod m;\nuse m::Foo;\npub struct Bar;\n\
          pub struct S {\n    a: m::Foo,\n    b: Vec<Bar>,\n    c: Option<Box<crate::m::Foo>>,\n}\n\
          impl S {\n    pub fn f(&self, x: &Foo) -> Result<Bar, ()> {\n        let y: Bar = todo!();\n        Err(())\n    }\n    pub fn g(&self) -> S { todo!() }\n}\n\
          pub fn free(x: &mut dyn Iterator<Item = Foo>) {}\n\
          pub mod consts {\n    pub const K: Option<crate::Bar> = None;\n}\n",
        ),
        ("src/m.rs", "pub struct Foo;\n"),
    ]);

    assert_eq!(
        edges_of(&g, EdgeKind::TypeRef),
        [
            ("fn:my_app::S#f", "type:my_app::Bar"),
            ("fn:my_app::S#f", "type:my_app::m::Foo"),
            ("fn:my_app::free", "type:my_app::m::Foo"),
            ("pkg::my_app::consts", "type:my_app::Bar"),
            ("type:my_app::S", "type:my_app::Bar"),
            ("type:my_app::S", "type:my_app::m::Foo"),
        ]
    );
}

#[test]
fn a_type_in_a_body_gives_type_ref_through_paths_struct_expressions_patterns_and_constructors() {
    let g = index(&[
        APP,
        (
            "src/lib.rs",
            "pub struct A;\npub struct B { x: u8 }\npub enum C { One, Two }\npub struct D(u8);\npub struct E;\npub struct F;\n\
             pub struct G;\npub mod util {\n    pub fn helper() {}\n}\n\
             pub fn body(c: u8) {\n    let a = A::new();\n    let b = B { x: 1 };\n    match c {\n        _ if matches(C::One) => {}\n        _ => {}\n    }\n    let d = D(3);\n    let e = Vec::<E>::new();\n    let f = crate::util::helper();\n    if let crate::C::Two = x {}\n    let g = <G as Default>::default();\n}\n",
        ),
    ]);

    assert_eq!(
        edges_of(&g, EdgeKind::TypeRef),
        [
            ("fn:my_app::body", "type:my_app::A"),
            ("fn:my_app::body", "type:my_app::B"),
            ("fn:my_app::body", "type:my_app::C"),
            ("fn:my_app::body", "type:my_app::D"),
            ("fn:my_app::body", "type:my_app::E"),
            // The qualified path `<G as Default>` names G.
            ("fn:my_app::body", "type:my_app::G"),
        ]
    );
}

#[test]
fn generic_parameters_and_local_items_hide_declared_types_and_self_is_the_impl_type() {
    let g = index(&[
        APP,
        (
            "src/lib.rs",
            "pub struct T;\npub struct Local;\npub struct Other;\npub struct Mine;\n\
             pub fn generic<T>(x: T) -> T { x }\n\
             pub fn local() {\n    struct Local;\n    let _ = Local;\n    let _: Local = Local;\n}\n\
             pub struct Gen<T> { x: T }\n\
             impl Mine {\n    pub fn new() -> Self { Self }\n    pub fn other() -> Other { Self::other() }\n}\n",
        ),
    ]);

    assert_eq!(
        edges_of(&g, EdgeKind::TypeRef),
        [("fn:my_app::Mine#other", "type:my_app::Other")]
    );
}

#[test]
fn a_workspace_gives_one_module_per_package_and_paths_into_a_sibling_crate_resolve() {
    let g = index(&[
        ("Cargo.toml", "[workspace]\nmembers = [\"crates/*\"]\n"),
        (
            "crates/my-core/Cargo.toml",
            "[package]\nname = \"my-core\"\n",
        ),
        (
            "crates/my-core/src/lib.rs",
            "pub mod model;\npub use model::Thing;\n",
        ),
        (
            "crates/my-core/src/model.rs",
            "pub struct Thing;\npub struct Other;\n",
        ),
        (
            "crates/app/Cargo.toml",
            "[package]\nname = 'app'\n\n[dependencies]\nmy-core = { path = \"../my-core\" }\n",
        ),
        (
            "crates/app/src/lib.rs",
            "use my_core::Thing;\npub struct S {\n    a: Thing,\n    b: my_core::model::Other,\n}\n",
        ),
    ]);

    let modules: Vec<&str> = g
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Module)
        .map(|n| n.id.as_str())
        .collect();
    assert_eq!(modules, ["mod:crates/app", "mod:crates/my-core"]);
    assert_eq!(node(&g, "mod:crates/my-core").name, "my-core");
    assert_eq!(
        parent_of(&g, "type:my_core::model::Thing"),
        "pkg:crates/my-core:my_core::model"
    );
    assert_eq!(
        edges_of(&g, EdgeKind::TypeRef),
        [
            ("type:app::S", "type:my_core::model::Other"),
            ("type:app::S", "type:my_core::model::Thing"),
        ]
    );
    // The re-export is followed to the definition.
    assert_eq!(
        edges_of(&g, EdgeKind::Imports),
        [("mod:crates/app", "type:my_core::model::Thing")]
    );
}

#[test]
fn the_report_counts_the_paths_that_resolved_left_the_repository_or_could_not_be_followed() {
    let (_, report) = run(&[
        APP,
        (
            "src/lib.rs",
            "pub mod a;\nuse crate::a::A;\nuse std::fmt::Debug;\nuse crate::missing::M;\n\
          pub struct S<T> {\n    a: A,\n    b: Vec<u8>,\n    c: Missing,\n    d: std::fmt::Result,\n    e: T,\n}\n",
        ),
        ("src/a.rs", "pub struct A;\n"),
    ]);

    assert_eq!(
        report.paths,
        PathReport {
            resolved: 2,
            external: 3,
            unresolved: 2
        }
    );
}

#[test]
fn declarations_with_the_same_qualified_name_give_one_node_the_first_in_the_source() {
    let g = index(&[
        APP,
        (
            "src/lib.rs",
            "#[cfg(unix)]\npub struct Os;\n#[cfg(windows)]\npub struct Os;\n\
             pub struct W<T>(T);\nimpl W<u8> {\n    fn new() {}\n}\nimpl W<u16> {\n    fn new() {}\n}\n",
        ),
    ]);

    let mut seen = std::collections::HashSet::new();
    assert!(g.nodes.iter().all(|n| seen.insert(&n.id)), "{:?}", ids(&g));
    assert_eq!(
        node(&g, "type:my_app::Os").span,
        Some(Span {
            start_line: 1,
            end_line: 2
        })
    );
    assert_eq!(
        node(&g, "fn:my_app::W#new").span,
        Some(Span {
            start_line: 7,
            end_line: 7
        })
    );
}

#[test]
fn a_path_that_starts_at_a_dependency_of_the_manifest_is_external() {
    let (_, report) = run(&[
        (
            "Cargo.toml",
            "[package]\nname = \"my-app\"\n\n[dependencies]\nserde = \"1\"\nuuid = { version = \"1\" }\nmy-dep = \"2\"\nrenamed.workspace = true\n\n[dev-dependencies.tempfile]\nversion = \"3\"\n",
        ),
        (
            "src/lib.rs",
            "use uuid::Uuid;\nuse my_dep::X;\nuse tempfile::T;\nuse renamed::R;\nuse nothere::Z;\n\
             pub struct S {\n    a: Uuid,\n    b: serde::Value,\n    c: Z,\n}\n",
        ),
    ]);

    assert_eq!(
        report.paths,
        PathReport {
            resolved: 0,
            external: 6,
            unresolved: 2
        }
    );
}

#[test]
fn constants_and_statics_resolve_and_inferred_types_and_associated_type_names_are_not_paths() {
    let (_, report) = run(&[
        APP,
        (
            "src/lib.rs",
            "pub const K: u8 = 1;\npub static ST: u8 = 2;\n\
             mod m {\n    use super::{K, ST};\n    pub fn f(x: Vec<_>, y: &mut dyn Iterator<Item = u8>) {}\n}\n",
        ),
    ]);

    assert_eq!(
        report.paths,
        PathReport {
            resolved: 2,
            external: 2,
            unresolved: 0
        }
    );
}

#[test]
fn paths_in_the_arguments_of_a_macro_call_give_type_ref_but_a_macro_definition_does_not() {
    let g = index(&[
        APP,
        (
            "src/lib.rs",
            "pub struct A;\npub struct B;\npub struct C;\npub struct D;\n\
             macro_rules! make {\n    () => { D::new() };\n}\n\
             pub fn f() {\n    assert_eq!(A::new(), crate::A);\n    let v = vec![B { x: 1 }, B::default()];\n    println!(\"{}\", C::name());\n}\n",
        ),
    ]);

    assert_eq!(
        edges_of(&g, EdgeKind::TypeRef),
        [
            ("fn:my_app::f", "type:my_app::A"),
            ("fn:my_app::f", "type:my_app::B"),
            ("fn:my_app::f", "type:my_app::C"),
        ]
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
fn a_call_of_a_free_function_gives_calls_from_the_function_that_holds_the_call() {
    let g = index(&[
        APP,
        (
            "src/lib.rs",
            "mod a {\n    pub fn foo() {}\n    pub mod b {\n        pub fn bar() {}\n    }\n}\n\
             fn local() {}\n\
             pub fn f() {\n    local();\n    a::foo();\n    crate::a::b::bar();\n    std::mem::drop(1);\n}\n",
        ),
    ]);

    assert_eq!(
        calls_from(&g, "fn:my_app::f"),
        [
            "fn:my_app::a::b::bar",
            "fn:my_app::a::foo",
            "fn:my_app::local"
        ]
    );
}

#[test]
fn a_function_is_found_through_super_use_renames_groups_globs_and_re_exports() {
    let g = index(&[
        APP,
        (
            "src/lib.rs",
            "mod util {\n    pub fn one() {}\n    pub fn two() {}\n    pub fn three() {}\n    pub fn four() {}\n}\n\
             mod facade {\n    pub use crate::util::four;\n}\n\
             fn top() {}\n\
             mod user {\n    use crate::util::{one as uno, two};\n    use crate::util::*;\n    use crate::facade::four as vier;\n\
             \x20   pub fn f() {\n        super::top();\n        uno();\n        two();\n        three();\n        vier();\n    }\n}\n",
        ),
    ]);

    assert_eq!(
        calls_from(&g, "fn:my_app::user::f"),
        [
            "fn:my_app::top",
            "fn:my_app::util::four",
            "fn:my_app::util::one",
            "fn:my_app::util::three",
            "fn:my_app::util::two",
        ]
    );
}

#[test]
fn a_local_variable_or_a_nested_function_hides_a_function_of_the_same_name() {
    let g = index(&[
        APP,
        (
            "src/lib.rs",
            "fn a() {}\nfn b() {}\nfn c() {}\n\
             pub fn f(a: fn()) {\n    let b = || ();\n    b();\n    a();\n    fn c() {}\n    c();\n}\n",
        ),
    ]);

    assert!(calls_from(&g, "fn:my_app::f").is_empty());
}

#[test]
fn an_associated_call_reaches_the_function_in_an_inherent_impl_a_trait_impl_or_a_trait_default() {
    let g = index(&[
        APP,
        (
            "src/lib.rs",
            "pub mod m {\n    pub struct W;\n    impl W {\n        pub fn new() -> W { W }\n    }\n}\n\
             pub trait Tr {\n    fn make() -> Self;\n    fn dflt() {}\n}\n\
             pub struct X;\n\
             impl Tr for X {\n    fn make() -> X { X }\n}\n\
             impl X {\n    fn helper() {}\n    pub fn run() {\n        Self::helper();\n        Self::make();\n    }\n}\n\
             pub fn f() {\n    m::W::new();\n    X::make();\n    X::dflt();\n    crate::X::run();\n}\n",
        ),
    ]);

    assert_eq!(
        calls_from(&g, "fn:my_app::f"),
        [
            "fn:my_app::Tr#dflt",
            "fn:my_app::X#Tr::make",
            "fn:my_app::X#run",
            "fn:my_app::m::W#new",
        ]
    );
    assert_eq!(
        calls_from(&g, "fn:my_app::X#run"),
        ["fn:my_app::X#Tr::make", "fn:my_app::X#helper"]
    );
}

#[test]
fn several_candidates_for_an_associated_function_give_an_edge_to_the_type() {
    let g = index(&[
        APP,
        (
            "src/lib.rs",
            "pub struct A;\npub struct B;\npub struct Two;\n\
             impl From<A> for Two {\n    fn from(_: A) -> Two { Two }\n}\n\
             impl From<B> for Two {\n    fn from(_: B) -> Two { Two }\n}\n\
             pub fn f() {\n    Two::from(A);\n}\n",
        ),
    ]);

    assert_eq!(calls_from(&g, "fn:my_app::f"), ["type:my_app::Two"]);
}

#[test]
fn constructors_of_tuple_structs_and_variants_and_struct_literals_are_no_calls() {
    let (g, report) = run(&[
        APP,
        (
            "src/lib.rs",
            "pub struct P(u8);\npub enum E {\n    V(u8),\n    W { x: u8 },\n}\n\
             impl E {\n    pub fn make() -> Self {\n        Self::V(1)\n    }\n}\n\
             impl P {\n    pub fn make() -> Self {\n        Self(1)\n    }\n}\n\
             pub fn f() {\n    P(1);\n    E::V(2);\n    let _ = E::W { x: 1 };\n}\n",
        ),
    ]);

    assert!(calls_from(&g, "fn:my_app::f").is_empty());
    assert!(calls_from(&g, "fn:my_app::E#make").is_empty());
    assert!(calls_from(&g, "fn:my_app::P#make").is_empty());
    assert_eq!(report.calls, CallReport::default());
}

#[test]
fn a_method_call_on_self_reaches_inherent_impl_trait_impl_and_default_methods_and_in_a_trait_its_supertraits()
 {
    let g = index(&[
        APP,
        (
            "src/lib.rs",
            "pub trait Base {\n    fn base(&self) {}\n}\n\
             pub trait Tr: Base {\n    fn required(&self);\n    fn dflt(&self) {}\n    fn user(&self) {\n        self.required();\n        self.dflt();\n        self.base();\n    }\n}\n\
             pub struct X;\n\
             impl Base for X {}\n\
             impl Tr for X {\n    fn required(&self) {}\n}\n\
             impl X {\n    fn inherent(&self) {}\n    pub fn run(&self) {\n        self.inherent();\n        self.required();\n        self.dflt();\n        self.base();\n    }\n}\n",
        ),
    ]);

    assert_eq!(
        calls_from(&g, "fn:my_app::X#run"),
        [
            "fn:my_app::Base#base",
            "fn:my_app::Tr#dflt",
            "fn:my_app::X#Tr::required",
            "fn:my_app::X#inherent",
        ]
    );
    assert_eq!(
        calls_from(&g, "fn:my_app::Tr#user"),
        [
            "fn:my_app::Base#base",
            "fn:my_app::Tr#dflt",
            "fn:my_app::Tr#required"
        ]
    );
}

const SHAPES: &str = "pub struct S;\n\
    impl S {\n    pub fn go(&self) {}\n    pub fn make() -> S { S }\n    pub fn me() -> Self { S }\n}\n\
    pub fn make_s() -> S { S }\n\
    pub struct Holder {\n    inner: S,\n    pub name: String,\n}\n";

#[test]
fn a_receiver_whose_type_is_written_or_proven_gives_the_method_of_that_type() {
    let g = index(&[
        APP,
        (
            "src/lib.rs",
            &format!(
                "{SHAPES}\
                 impl Holder {{\n    fn via_field(&self) {{ self.inner.go(); }}\n}}\n\
                 pub fn param(a: S) {{ a.go(); }}\n\
                 pub fn by_ref(a: &S) {{ a.go(); }}\n\
                 pub fn by_mut(a: &mut S) {{ a.go(); }}\n\
                 pub fn let_typed() {{ let d: S = S::make(); d.go(); }}\n\
                 pub fn let_literal() {{ let d = S {{}}; d.go(); }}\n\
                 pub fn let_assoc() {{ let d = S::make(); d.go(); }}\n\
                 pub fn let_self_return() {{ let d = S::me(); d.go(); }}\n\
                 pub fn let_free() {{ let d = make_s(); d.go(); }}\n\
                 pub fn field_of_param(h: Holder) {{ h.inner.go(); }}\n"
            ),
        ),
    ]);

    for from in [
        "Holder#via_field",
        "param",
        "by_ref",
        "by_mut",
        "let_typed",
        "let_literal",
        "let_assoc",
        "let_self_return",
        "let_free",
        "field_of_param",
    ] {
        let targets = calls_from(&g, &format!("fn:my_app::{from}"));
        assert!(targets.contains(&"fn:my_app::S#go"), "{from}: {targets:?}");
    }
}

#[test]
fn the_innermost_let_wins_and_a_binding_of_a_loop_or_a_pattern_is_unresolved() {
    let (g, report) = index_and_report_of(
        "pub struct S;\nimpl S {\n    pub fn go(&self) {}\n}\n\
         pub struct T;\nimpl T {\n    pub fn go(&self) {}\n    pub fn new() -> T { T }\n}\n\
         pub fn shadow(a: S) {\n    let a = T::new();\n    a.go();\n}\n\
         pub fn block_scope(a: S) {\n    {\n        let a = T::new();\n        a.go();\n    }\n    a.go();\n}\n\
         pub fn bindings(items: Vec<S>) {\n    for s in items { s.go(); }\n    if let Some(s) = items.first() { s.go(); }\n    match items.first() { Some(s) => s.go(), None => {} }\n}\n",
    );

    assert_eq!(
        calls_from(&g, "fn:my_app::shadow"),
        ["fn:my_app::T#go", "fn:my_app::T#new"]
    );
    assert_eq!(
        calls_from(&g, "fn:my_app::block_scope"),
        ["fn:my_app::S#go", "fn:my_app::T#go", "fn:my_app::T#new"]
    );
    assert!(calls_from(&g, "fn:my_app::bindings").is_empty());
    assert_eq!((report.calls.unresolved, report.calls.external), (3, 2));
}

fn index_and_report_of(source: &str) -> (Graph, Report) {
    run(&[APP, ("src/lib.rs", source)])
}

#[test]
fn references_boxes_await_question_marks_and_unwrap_are_seen_through() {
    let g = index(&[
        APP,
        (
            "src/lib.rs",
            "use std::future::Future;\nuse std::rc::Rc;\nuse std::sync::Arc;\n\
             pub struct S;\n\
             impl S {\n    pub fn a(&self) {}\n    pub fn b(&self) {}\n    pub fn c(&self) {}\n    pub fn d(&self) {}\n    pub fn e(&self) {}\n    pub fn f(&self) {}\n    pub fn g(&self) {}\n    pub fn h(&self) {}\n    pub fn i(&self) {}\n    pub fn j(&self) {}\n}\n\
             async fn make_async() -> S { S }\n\
             fn make_future() -> impl Future<Output = S> { async { S } }\n\
             fn make_opt() -> Option<S> { None }\n\
             fn make_res() -> Result<S, String> { Ok(S) }\n\
             pub fn boxed(x: Box<S>, y: Arc<S>, z: Rc<S>) {\n    x.a();\n    y.b();\n    z.c();\n}\n\
             pub async fn awaited() {\n    make_async().await.d();\n    make_future().await.e();\n}\n\
             pub fn questioned() -> Option<()> {\n    make_opt()?.f();\n    make_res().ok()?;\n    None\n}\n\
             pub fn questioned_result() -> Result<(), String> {\n    make_res()?.g();\n    Ok(())\n}\n\
             pub fn unwrapped() {\n    make_opt().unwrap().h();\n    make_res().expect(\"x\").i();\n    make_opt().unwrap_or_default().j();\n}\n\
             pub fn ends_the_chain() {\n    make_opt().map(|s| s).unwrap().a();\n    make_opt().is_some();\n}\n",
        ),
    ]);

    let s = |m: &str| format!("fn:my_app::S#{m}");
    assert_eq!(calls_from(&g, "fn:my_app::boxed"), [s("a"), s("b"), s("c")]);
    assert_eq!(
        calls_from(&g, "fn:my_app::awaited"),
        [
            &s("d"),
            &s("e"),
            "fn:my_app::make_async",
            "fn:my_app::make_future"
        ]
    );
    assert_eq!(
        calls_from(&g, "fn:my_app::questioned"),
        [&s("f"), "fn:my_app::make_opt", "fn:my_app::make_res"]
    );
    assert_eq!(
        calls_from(&g, "fn:my_app::questioned_result"),
        [&s("g"), "fn:my_app::make_res"]
    );
    assert_eq!(
        calls_from(&g, "fn:my_app::unwrapped"),
        [
            &s("h"),
            &s("i"),
            &s("j"),
            "fn:my_app::make_opt",
            "fn:my_app::make_res"
        ]
    );
    assert_eq!(
        calls_from(&g, "fn:my_app::ends_the_chain"),
        ["fn:my_app::make_opt"]
    );
}

#[test]
fn arc_new_and_box_new_keep_the_type_of_their_argument() {
    let (g, report) = index_and_report_of(
        "use std::sync::Arc;\n\
         pub struct S;\nimpl S {\n    pub fn a(&self) {}\n    pub fn make() -> S { S }\n}\n\
         pub fn f() {\n    let x = Arc::new(S::make());\n    x.a();\n    let y = Box::new(S);\n    y.a();\n    let z = Some(S);\n}\n",
    );

    assert_eq!(
        calls_from(&g, "fn:my_app::f"),
        ["fn:my_app::S#a", "fn:my_app::S#make"]
    );
    // Two `new`, one `Some` that is no call.
    assert_eq!(
        report.calls,
        CallReport {
            to_function: 3,
            to_type: 0,
            external: 2,
            unresolved: 0
        }
    );
}

#[test]
fn a_function_named_but_not_called_gives_a_calls_edge() {
    let (g, report) = index_and_report_of(
        "mod m {\n    pub fn handler() {}\n}\n\
         use m::handler as h;\n\
         const LIMIT: usize = 3;\n\
         fn parse_row() {}\n\
         pub struct S;\nimpl S {\n    fn helper() {}\n    pub fn run(items: Vec<u8>) {\n        items.iter().map(Self::helper);\n    }\n}\n\
         pub fn f(items: Vec<u8>) {\n    items.iter().map(parse_row);\n    route(\"/x\", get(m::handler));\n    let _alias = h;\n    let _n = LIMIT;\n}\n",
    );

    assert_eq!(calls_from(&g, "fn:my_app::S#run"), ["fn:my_app::S#helper"]);
    assert_eq!(
        calls_from(&g, "fn:my_app::f"),
        ["fn:my_app::m::handler", "fn:my_app::parse_row"]
    );
    assert_eq!(report.function_values, 4);
}

#[test]
fn a_receiver_typed_by_a_trait_gives_the_method_of_the_trait() {
    let (g, report) = index_and_report_of(
        "use std::sync::Arc;\n\
         pub trait Tr {\n    fn m1(&self);\n    fn m2(&self);\n    fn m3(&self);\n    fn m4(&self);\n    fn m5(&self);\n    fn m6(&self);\n    fn m7(&self);\n    fn make() -> u8;\n}\n\
         pub fn dyn_ref(a: &dyn Tr) { a.m1(); }\n\
         pub fn impl_arg(a: impl Tr) { a.m2(); }\n\
         pub fn bound<T: Tr>(a: T) { a.m3(); }\n\
         pub fn where_clause<T>(a: &T) where T: Send + Tr { a.m4(); }\n\
         pub fn boxed(a: Box<dyn Tr>, b: Arc<dyn Tr + Send + Sync>) { a.m5(); b.m6(); }\n\
         pub struct W<T>(T);\nimpl<T: Tr> W<T> {\n    pub fn run(&self, t: T) { t.m7(); T::make(); }\n}\n\
         pub fn missing(a: &dyn Tr) { a.not_declared(); }\n",
    );

    for (from, to) in [
        ("dyn_ref", "m1"),
        ("impl_arg", "m2"),
        ("bound", "m3"),
        ("where_clause", "m4"),
        ("W#run", "m7"),
    ] {
        let targets = calls_from(&g, &format!("fn:my_app::{from}"));
        assert!(
            targets.contains(&format!("fn:my_app::Tr#{to}").as_str()),
            "{from}: {targets:?}"
        );
    }
    assert_eq!(
        calls_from(&g, "fn:my_app::boxed"),
        ["fn:my_app::Tr#m5", "fn:my_app::Tr#m6"]
    );
    assert_eq!(
        calls_from(&g, "fn:my_app::W#run"),
        ["fn:my_app::Tr#m7", "fn:my_app::Tr#make"]
    );
    // A trait that declares no such function: it can come from a supertrait outside.
    assert!(calls_from(&g, "fn:my_app::missing").is_empty());
    assert_eq!(report.calls.external, 1);
}

#[test]
fn a_function_that_the_type_does_not_declare_is_external_if_std_traits_provide_it_else_the_type() {
    let (g, report) = index_and_report_of(
        "pub struct D;\nimpl D {\n    pub fn go(&self) {}\n}\n\
         pub fn std_names(d: D) {\n    d.clone();\n    d.to_string();\n    let _ = D::default();\n    let _ = D::from(1u8);\n    let _ = d.into();\n}\n\
         pub fn derived(d: D) {\n    d.generated();\n    D::builder();\n}\n\
         pub fn clone_keeps_the_type(d: D) {\n    let c = d.clone();\n    c.go();\n}\n",
    );

    assert!(calls_from(&g, "fn:my_app::std_names").is_empty());
    assert_eq!(calls_from(&g, "fn:my_app::derived"), ["type:my_app::D"]);
    assert_eq!(
        calls_from(&g, "fn:my_app::clone_keeps_the_type"),
        ["fn:my_app::D#go"]
    );
    assert_eq!(report.calls.to_type, 2);
    assert_eq!(report.calls.external, 6);
}

#[test]
fn a_chain_continues_through_the_written_return_type_of_a_unique_function() {
    let (g, report) = index_and_report_of(
        "pub struct A;\npub struct B;\n\
         impl A {\n    pub fn b(&self) -> B { B }\n    pub fn name(&self) -> String { String::new() }\n    pub fn maybe(&self) -> Option<B> { None }\n}\n\
         impl From<u8> for A {\n    fn from(_: u8) -> A { A }\n}\nimpl From<u16> for A {\n    fn from(_: u16) -> A { A }\n}\n\
         impl B {\n    pub fn c(&self) {}\n    pub fn d(&self) {}\n}\n\
         pub fn through(a: A) {\n    a.b().c();\n    a.maybe().unwrap().d();\n}\n\
         pub fn ends_external(a: A) {\n    a.name().len();\n}\n\
         pub fn ends_unresolved() {\n    A::from(1u8).b();\n}\n",
    );

    assert_eq!(
        calls_from(&g, "fn:my_app::through"),
        [
            "fn:my_app::A#b",
            "fn:my_app::A#maybe",
            "fn:my_app::B#c",
            "fn:my_app::B#d"
        ]
    );
    assert_eq!(
        calls_from(&g, "fn:my_app::ends_external"),
        ["fn:my_app::A#name"]
    );
    assert_eq!(
        calls_from(&g, "fn:my_app::ends_unresolved"),
        ["type:my_app::A"]
    );
    assert_eq!(
        report.calls,
        CallReport {
            to_function: 5,
            to_type: 1,
            external: 3,
            unresolved: 1
        }
    );
}

#[test]
fn a_const_or_static_initializer_counts_for_the_enclosing_mod_or_type() {
    let g = index(&[
        APP,
        (
            "src/lib.rs",
            "mod util {\n    pub fn make() -> u8 { 1 }\n}\n\
             const TOP: u8 = util::make();\n\
             mod inner {\n    pub static LOW: u8 = super::util::make();\n}\n\
             pub struct S;\nimpl S {\n    const C: u8 = util::make();\n}\n\
             pub trait Tr {\n    const D: u8 = util::make();\n}\n",
        ),
    ]);

    // The root mod is the module node, the ancestor of every node of the crate: no edge.
    assert!(calls_from(&g, "mod:").is_empty());
    assert_eq!(
        calls_from(&g, "pkg::my_app::inner"),
        ["fn:my_app::util::make"]
    );
    assert_eq!(calls_from(&g, "type:my_app::S"), ["fn:my_app::util::make"]);
    assert_eq!(calls_from(&g, "type:my_app::Tr"), ["fn:my_app::util::make"]);
}

#[test]
fn calls_in_the_arguments_of_a_macro_give_no_edge_but_are_counted_roughly() {
    let (g, report) = index_and_report_of(
        "fn g(_: u8) {}\nfn h() {}\nfn k() {}\npub struct A(u8);\n\
         pub fn f(x: Vec<u8>) {\n    println!(\"{}\", g(1));\n    let _v = vec![h(), x.len()];\n    assert_eq!(A(1).0, 1);\n    assert!(k() == ());\n}\n",
    );

    assert!(calls_from(&g, "fn:my_app::f").is_empty());
    // `g`, `h`, `len`, `k`: not `A`, a name with a capital letter.
    assert_eq!(report.macro_call_sites, 4);
    assert_eq!(report.calls, CallReport::default());
}

#[test]
fn the_receiver_of_a_literal_or_a_cast_is_a_known_external_type() {
    let (_, report) = index_and_report_of(
        "pub fn f(a: u8) {\n    \"x\".to_string();\n    (a as u32).to_le_bytes();\n    5.max(2);\n    (1.5).floor();\n    [1, 2].len();\n    (1, 2).clone();\n}\n",
    );

    assert_eq!(
        report.calls,
        CallReport {
            to_function: 0,
            to_type: 0,
            external: 6,
            unresolved: 0
        }
    );
}

#[test]
fn a_type_alias_is_followed_to_its_target_with_the_arguments_substituted() {
    let g = index(&[
        APP,
        (
            "src/lib.rs",
            "pub struct S;\nimpl S {\n    pub fn a(&self) {}\n    pub fn b() {}\n}\n\
             pub type Alias = S;\n\
             pub type Res<T> = Result<T, String>;\n\
             fn make() -> Res<S> { Ok(S) }\n\
             pub fn f(x: Alias) {\n    x.a();\n    Alias::b();\n    make().unwrap().a();\n}\n",
        ),
    ]);

    assert_eq!(
        calls_from(&g, "fn:my_app::f"),
        ["fn:my_app::S#a", "fn:my_app::S#b", "fn:my_app::make"]
    );
}
