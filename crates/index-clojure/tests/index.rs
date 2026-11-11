use cg_core::{EdgeKind, Graph, Node, NodeKind, Span};
use cg_index_clojure::{Report, SourceFile, index_sources, is_input};

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

const DEPS: (&str, &str) = ("deps.edn", "{}");

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
fn clojure_sources_and_deps_edn_are_inputs_but_not_below_hidden_or_build_directories() {
    assert!(is_input("src/a/b.clj"));
    assert!(is_input("src/a/b.cljc"));
    assert!(is_input("deps.edn"));
    assert!(is_input("modules/x/deps.edn"));
    assert!(!is_input("src/a/b.cljs"));
    assert!(!is_input("project.clj"));
    assert!(!is_input("bb.edn"));
    assert!(!is_input("shadow-cljs.edn"));
    assert!(!is_input("src/.clj"));
    assert!(!is_input(".cpcache/src/a.clj"));
    assert!(!is_input("target/src/a.clj"));
    assert!(!is_input("node_modules/x/deps.edn"));
}

#[test]
fn each_deps_edn_directory_is_a_module_named_by_its_last_segment_or_the_root_name() {
    let g = index(&[
        ("deps.edn", "{}"),
        ("modules/db/deps.edn", "{}"),
        ("modules/db/impl/deps.edn", "{}"),
    ]);

    let root = node(&g, "mod:");
    assert_eq!((root.kind, root.name.as_str()), (NodeKind::Module, "repo"));
    assert_eq!(root.parent, None);
    assert_eq!(node(&g, "mod:modules/db").name, "db");
    assert_eq!(node(&g, "mod:modules/db").parent, None);
    assert_eq!(node(&g, "mod:modules/db/impl").name, "impl");
    assert_eq!(g.nodes.len(), 3);
}

#[test]
fn a_namespace_is_a_package_and_defn_a_function_with_file_span_and_hash() {
    let g = index(&[
        DEPS,
        (
            "src/foo/bar.clj",
            "(ns foo.bar)\n\n(defn baz\n  \"doc\"\n  ([] (baz 1))\n  ([x] x))\n",
        ),
    ]);

    let ns = node(&g, "pkg::foo.bar");
    assert_eq!((ns.kind, ns.name.as_str()), (NodeKind::Package, "foo.bar"));
    assert_eq!(ns.parent.as_deref(), Some("mod:"));
    assert_eq!(
        (ns.file.clone(), ns.span, ns.hash.clone()),
        (None, None, None)
    );
    let f = node(&g, "fn:foo.bar/baz");
    assert_eq!((f.kind, f.name.as_str()), (NodeKind::Function, "baz"));
    assert_eq!(f.parent.as_deref(), Some("pkg::foo.bar"));
    assert_eq!(f.file.as_deref(), Some("src/foo/bar.clj"));
    assert_eq!(
        f.span,
        Some(Span {
            start_line: 3,
            end_line: 6
        })
    );
    assert!(!f.test);
    // Whitespace does not change the hash; a change of the text does.
    let same = index(&[
        DEPS,
        (
            "src/foo/bar.clj",
            "(ns foo.bar)\n(defn baz \"doc\" ([] (baz 1)) ([x] x))",
        ),
    ]);
    let other = index(&[
        DEPS,
        (
            "src/foo/bar.clj",
            "(ns foo.bar)\n(defn baz \"doc\" ([] (baz 2)) ([x] x))",
        ),
    ]);
    assert!(f.hash.is_some());
    assert_eq!(node(&same, "fn:foo.bar/baz").hash, f.hash);
    assert_ne!(node(&other, "fn:foo.bar/baz").hash, f.hash);
    assert_eq!(g.nodes.len(), 3);
}

#[test]
fn source_directories_are_the_paths_of_deps_edn_and_other_files_are_counted() {
    let (g, report) = run(&[
        (
            "deps.edn",
            "{:deps {} ;; a comment\n :paths [\"lib\" \"resources\"]}",
        ),
        ("lib/a.clj", "(ns a)\n(defn f [])"),
        ("src/b.clj", "(ns b)\n(defn g [])"),
        ("build.clj", "(ns build)\n(defn h [])"),
    ]);

    assert!(g.node("fn:a/f").is_some());
    assert!(g.node("pkg::b").is_none() && g.node("pkg::build").is_none());
    assert_eq!(report.files_outside_source_dirs, 2);
}

#[test]
fn without_paths_the_source_directory_is_src() {
    let (g, report) = run(&[
        ("deps.edn", "{:deps {}}"),
        ("src/a.clj", "(ns a)\n(defn f [])"),
        ("other/b.clj", "(ns b)"),
    ]);

    assert!(g.node("fn:a/f").is_some());
    assert!(g.node("pkg::b").is_none());
    assert_eq!(report.files_outside_source_dirs, 1);
}

#[test]
fn the_extra_paths_of_an_alias_are_source_directories() {
    let g = index(&[
        (
            "deps.edn",
            "{:paths [\"src\"]\n :aliases {:test {:extra-paths [\"test\"]} :dev {:extra-paths [\"dev\" \"./scripts/\"]}}}",
        ),
        ("test/a.clj", "(ns a)\n(defn f [])"),
        ("dev/b.clj", "(ns b)\n(defn g [])"),
        ("scripts/c.clj", "(ns c)\n(defn h [])"),
    ]);

    assert!(g.node("fn:a/f").is_some());
    assert!(g.node("fn:b/g").is_some());
    assert!(g.node("fn:c/h").is_some());
}

#[test]
fn a_file_belongs_to_the_nearest_deps_edn_above_it() {
    let (g, report) = run(&[
        DEPS,
        ("modules/db/deps.edn", "{:paths [\"src\"]}"),
        ("modules/db/src/a.clj", "(ns a)\n(defn f [])"),
        ("modules/db/test/t.clj", "(ns t)"),
        ("src/modules/x.clj", "(ns x)\n(defn g [])"),
    ]);

    assert_eq!(parent_of(&g, "pkg:modules/db:a"), "mod:modules/db");
    assert_eq!(parent_of(&g, "pkg::x"), "mod:");
    assert!(g.node("pkg:modules/db:t").is_none());
    assert_eq!(report.files_outside_source_dirs, 1);
}

#[test]
fn a_file_below_no_deps_edn_is_not_read_and_counted() {
    let (g, report) = run(&[
        ("src/a.clj", "(ns a)\n(defn f [])"),
        ("tools/deps.edn", "{}"),
        ("tools/src/b.clj", "(ns b)"),
    ]);

    assert!(g.node("pkg::a").is_none());
    assert!(g.node("pkg:tools:b").is_some());
    assert_eq!(report.files_without_deps_edn, 1);
}

#[test]
fn test_code_is_a_file_whose_longest_source_directory_comes_from_an_alias() {
    let g = index(&[
        (
            "deps.edn",
            "{:paths [\".\" \"src\"] :aliases {:test {:extra-paths [\"test\"]}}}",
        ),
        ("test/x.clj", "(ns x-test)\n(defn t [])"),
        ("src/x.clj", "(ns x)\n(defn f [])"),
        ("y.clj", "(ns y)\n(defn h [])"),
    ]);

    assert!(node(&g, "fn:x-test/t").test);
    assert!(!node(&g, "fn:x/f").test);
    assert!(!node(&g, "fn:y/h").test);
    // A directory named `test` under `src` is main code, whatever its name.
    let g = index(&[DEPS, ("src/test/z.clj", "(ns test.z)\n(defn z [])")]);
    assert!(!node(&g, "fn:test.z/z").test);
}

#[test]
fn a_namespace_in_test_code_is_test_code_even_when_it_holds_no_node() {
    let g = index(&[
        ("deps.edn", "{:aliases {:test {:extra-paths [\"test\"]}}}"),
        ("test/a_test.clj", "(ns a-test)\n(deftest t (is true))"),
        ("src/a.clj", "(ns a)\n(defn f [])"),
    ]);

    assert!(node(&g, "pkg::a-test").test);
    assert!(!node(&g, "pkg::a").test);
    let visible = g.without_tests();
    assert!(visible.node("pkg::a-test").is_none());
    assert!(visible.node("pkg::a").is_some());
}

#[test]
fn namespaces_nest_by_the_rule_of_adr_18_and_a_namespace_without_a_function_is_a_node() {
    let g = index(&[
        DEPS,
        ("src/app/core.clj", "(ns app.core)"),
        ("src/app/core/util.clj", "(ns app.core.util)\n(defn u [])"),
        ("src/app/db/a.clj", "(ns app.db.a)"),
        ("src/app/db/b.clj", "(ns app.db.b)"),
    ]);

    let shown = |id: &str| (parent_of(&g, id).to_string(), node(&g, id).name.clone());
    assert_eq!(shown("pkg::app.core"), ("mod:".into(), "app.core".into()));
    assert_eq!(
        shown("pkg::app.core.util"),
        ("pkg::app.core".into(), "util".into())
    );
    // The prefix is a node because two namespaces branch below it; `app` is not, as the
    // common prefix of all.
    assert_eq!(shown("pkg::app.db"), ("mod:".into(), "app.db".into()));
    assert_eq!(shown("pkg::app.db.a"), ("pkg::app.db".into(), "a".into()));
    assert_eq!(shown("pkg::app.db.b"), ("pkg::app.db".into(), "b".into()));
    assert!(g.node("pkg::app").is_none());
    assert_eq!(parent_of(&g, "fn:app.core.util/u"), "pkg::app.core.util");
}

#[test]
fn nesting_runs_per_module() {
    let g = index(&[
        DEPS,
        ("src/a/x.clj", "(ns a.x)"),
        ("modules/m/deps.edn", "{}"),
        ("modules/m/src/a/y.clj", "(ns a.y)"),
        ("modules/m/src/a/z.clj", "(ns a.z)"),
    ]);

    // In the root module `a.x` is alone, so its name stays whole; in `m` the common
    // prefix `a` is no node.
    assert_eq!(node(&g, "pkg::a.x").name, "a.x");
    assert_eq!(parent_of(&g, "pkg:modules/m:a.y"), "mod:modules/m");
    assert_eq!(node(&g, "pkg:modules/m:a.y").name, "a.y");
}

#[test]
fn defn_private_defmacro_and_defmulti_are_functions_also_with_metadata_on_the_name() {
    let g = index(&[
        DEPS,
        (
            "src/a.clj",
            "(ns a)\n(defn- p [])\n(defmacro m [x] x)\n(defmulti mm :k)\n(defn ^:private ^String q [] \"\")\n(defn ^{:doc \"d\"} r [])\n^:foo (defn s [])\n",
        ),
    ]);

    for name in ["p", "m", "mm", "q", "r", "s"] {
        let f = node(&g, &format!("fn:a/{name}"));
        assert_eq!((f.kind, f.name.as_str()), (NodeKind::Function, name));
        assert_eq!(f.parent.as_deref(), Some("pkg::a"));
    }
    assert_eq!(g.nodes.len(), 8);
}

#[test]
fn forms_without_a_node_are_counted_by_head_and_comment_and_discarded_forms_give_nothing() {
    let (g, report) = run(&[
        DEPS,
        (
            "src/a.clj",
            "(ns a)\n(def x 1)\n(def x 2)\n(defonce y 2)\n(declare z)\n(defmethod mm :k [x] x)\n(s/def ::k int?)\n(comment (defn in-comment []))\n#_(defn discarded [])\n(extend-protocol P Foo (m [_] 1))\n(do (defn nested []))\n(foo)\n(defn ok [])\n",
        ),
    ]);

    assert_eq!(
        ids(&g),
        [
            "mod:",
            "pkg::a",
            "val:a/x",
            "val:a/y",
            "fn:a/ok",
            "fn:a/mm :k",
            "ext:Foo#P/m"
        ]
    );
    assert_eq!(report.duplicate_declarations.get("def"), Some(&1));
    let counts: Vec<(&str, usize)> = report
        .forms_without_node
        .iter()
        .map(|(k, v)| (k.as_str(), *v))
        .collect();
    assert_eq!(
        counts,
        [
            ("comment", 1),
            ("declare", 1),
            ("do", 1),
            ("foo", 1),
            ("s/def", 1),
        ]
    );
}

#[test]
fn protocols_records_types_and_interfaces_are_types_with_their_methods_as_functions() {
    let (g, report) = run(&[
        DEPS,
        (
            "src/a.clj",
            "(ns a)\n(defprotocol P\n  \"doc\"\n  (m [this] \"d\")\n  (n [this x] [this x y])\n  :extend-via-metadata true)\n(defrecord R [a b]\n  P\n  (m [this] 1)\n  (n [this x] 2)\n  Object\n  (toString [this] \"r\"))\n(deftype T [x] P (m [this] 3))\n(definterface I (^String foo [x]) (bar []))\n",
        ),
    ]);

    for (id, name, parent) in [
        ("type:a/P", "P", "pkg::a"),
        ("fn:a/P#m", "m", "type:a/P"),
        ("fn:a/P#n", "n", "type:a/P"),
        ("type:a/R", "R", "pkg::a"),
        ("fn:a/R#m", "m", "type:a/R"),
        ("fn:a/R#n", "n", "type:a/R"),
        ("fn:a/R#toString", "toString", "type:a/R"),
        ("type:a/T", "T", "pkg::a"),
        ("fn:a/T#m", "m", "type:a/T"),
        ("type:a/I", "I", "pkg::a"),
        ("fn:a/I#foo", "foo", "type:a/I"),
        ("fn:a/I#bar", "bar", "type:a/I"),
    ] {
        let n = node(&g, id);
        let kind = if id.starts_with("type:") {
            NodeKind::Type
        } else {
            NodeKind::Function
        };
        assert_eq!(
            (n.kind, n.name.as_str(), n.parent.as_deref()),
            (kind, name, Some(parent))
        );
        assert_eq!(n.file.as_deref(), Some("src/a.clj"));
    }
    assert_eq!(g.nodes.len(), 14);
    assert!(report.forms_without_node.is_empty());
    assert_eq!(
        node(&g, "type:a/R").span,
        Some(Span {
            start_line: 7,
            end_line: 12
        })
    );
    assert_eq!(
        node(&g, "fn:a/R#m").span,
        Some(Span {
            start_line: 9,
            end_line: 9
        })
    );
}

#[test]
fn the_own_hash_of_a_type_leaves_out_its_methods() {
    let hashes = |body: &str| {
        let g = index(&[
            DEPS,
            (
                "src/a.clj",
                &format!("(ns a)\n(defrecord R [a]\n  P\n  (m [this] {body}))\n"),
            ),
        ]);
        (
            node(&g, "type:a/R").hash.clone(),
            node(&g, "fn:a/R#m").hash.clone(),
        )
    };

    let (type_one, method_one) = hashes("1");
    let (type_two, method_two) = hashes("2");
    assert_eq!(type_one, type_two);
    assert_ne!(method_one, method_two);
}

#[test]
fn the_name_of_the_ns_form_may_have_metadata_a_docstring_and_an_attribute_map() {
    let g = index(&[
        DEPS,
        (
            "src/a.clj",
            "(ns ^:no-doc ^{:author \"x\"} foo.bar\n  \"A docstring.\"\n  {:added \"1.0\"}\n  (:require [clojure.string :as str]))\n(defn f [])\n",
        ),
        (
            "src/b.clj",
            ";; header\n(set! *warn-on-reflection* true)\n(ns b)\n(defn g [])\n",
        ),
    ]);

    assert_eq!(
        node(&g, "fn:foo.bar/f").parent.as_deref(),
        Some("pkg::foo.bar")
    );
    assert!(g.node("fn:b/g").is_some());
}

#[test]
fn a_file_without_an_ns_form_gives_no_nodes_and_is_counted() {
    let (g, report) = run(&[
        DEPS,
        ("src/a.clj", "(defn f [])"),
        ("src/b.clj", "(in-ns 'b)\n(defn g [])"),
        ("src/c.clj", "(ns c)"),
    ]);

    assert_eq!(ids(&g), ["mod:", "pkg::c"]);
    assert_eq!(report.files_without_ns, 2);
    assert!(report.forms_without_node.is_empty());
}

#[test]
fn of_two_files_that_name_a_namespace_the_first_in_path_order_gives_the_nodes() {
    let files = [
        DEPS,
        ("modules/m/deps.edn", "{}"),
        ("src/a.clj", "(ns dup)\n(defn first-one [])"),
        ("modules/m/src/b.clj", "(ns dup)\n(defn second-one [])"),
        ("modules/m/src/dup2.clj", "(ns dup2)"),
    ];
    let (g, report) = run(&files);

    // `modules/m/src/b.clj` sorts before `src/a.clj` component by component.
    assert!(g.node("fn:dup/second-one").is_some());
    assert!(g.node("fn:dup/first-one").is_none());
    assert!(g.node("pkg::dup").is_none());
    assert!(g.node("pkg:modules/m:dup").is_some());
    assert_eq!(report.duplicate_namespaces, ["src/a.clj"]);
    let mut reversed = files;
    reversed.reverse();
    assert_eq!(run(&reversed).0, g);
}

#[test]
fn all_branches_of_a_reader_conditional_at_the_top_level_are_read_in_cljc() {
    let (g, _) = run(&[
        DEPS,
        (
            "src/a.cljc",
            "(ns a)\n#?(:clj (defn on-jvm []) :cljs (defn on-js []))\n#?@(:clj [(defn spliced []) (defrecord R [])] :cljs [(defn also [])])\n#?(:clj (defn both []) :cljs (defn both []))\n",
        ),
    ]);

    for id in ["on-jvm", "on-js", "spliced", "also", "both"] {
        assert!(g.node(&format!("fn:a/{id}")).is_some(), "{id}");
    }
    assert!(g.node("type:a/R").is_some());
    assert_eq!(g.nodes.iter().filter(|n| n.id == "fn:a/both").count(), 1);
}

fn imports(g: &Graph) -> Vec<(&str, &str)> {
    g.edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Imports)
        .map(|e| (e.from.as_str(), e.to.as_str()))
        .collect()
}

fn namespaces_file(name: &str) -> (String, String) {
    (
        format!("src/{}.clj", name.replace('.', "/")),
        format!("(ns {name})"),
    )
}

#[test]
fn require_and_use_give_imports_edges_to_the_namespaces_of_the_repository() {
    let mut files: Vec<(String, String)> = [
        "b", "c", "d", "e", "g.h.i", "g.h.k", "l", "m", "n", "o", "p", "a.sub",
    ]
    .iter()
    .map(|n| namespaces_file(n))
    .collect();
    files.push((
        "src/a.clj".into(),
        "(ns a\n  (:require [b :as x]\n            c\n            [d :refer [f]]\n            [e :refer :all]\n            (g.h [i :as j] k)\n            [m :as-alias mm]\n            [clojure.string :as str]\n            [a]\n            [a.sub]\n            #?(:clj [n] :cljs [zz])\n            [o :reload]\n            :reload)\n  (:use p)\n  (:import (java.util Date)))"
            .into(),
    ));
    let files: Vec<(&str, &str)> = std::iter::once(DEPS)
        .chain(files.iter().map(|(p, t)| (p.as_str(), t.as_str())))
        .collect();
    let g = index(&files);

    assert_eq!(
        imports(&g),
        [
            ("pkg::a", "pkg::b"),
            ("pkg::a", "pkg::c"),
            ("pkg::a", "pkg::d"),
            ("pkg::a", "pkg::e"),
            ("pkg::a", "pkg::g.h.i"),
            ("pkg::a", "pkg::g.h.k"),
            ("pkg::a", "pkg::n"),
            ("pkg::a", "pkg::o"),
            ("pkg::a", "pkg::p"),
        ]
    );
}

#[test]
fn a_namespace_gets_no_import_edge_to_its_ancestor_and_none_to_its_descendant() {
    let g = index(&[
        DEPS,
        ("src/a.clj", "(ns a (:require [a.b]))"),
        ("src/a/b.clj", "(ns a.b (:require [a] [a.b.c] [d]))"),
        ("src/a/b/c.clj", "(ns a.b.c (:require [a.b]))"),
        ("src/d.clj", "(ns d (:require [a.b.c]))"),
    ]);

    assert_eq!(
        imports(&g),
        [("pkg::a.b", "pkg::d"), ("pkg::d", "pkg::a.b.c")]
    );
}

#[test]
fn imports_edges_cross_modules_and_do_not_depend_on_the_order_of_the_files() {
    let files = [
        DEPS,
        ("modules/m/deps.edn", "{}"),
        ("src/a.clj", "(ns a (:require [b]))"),
        ("modules/m/src/b.clj", "(ns b)"),
    ];
    let g = index(&files);
    let mut reversed = files;
    reversed.reverse();

    assert_eq!(imports(&g), [("pkg::a", "pkg:modules/m:b")]);
    assert_eq!(index(&reversed), g);
}

fn edges_of(g: &Graph, kind: EdgeKind) -> Vec<(&str, &str)> {
    g.edges
        .iter()
        .filter(|e| e.kind == kind)
        .map(|e| (e.from.as_str(), e.to.as_str()))
        .collect()
}

fn calls(g: &Graph) -> Vec<(&str, &str)> {
    edges_of(g, EdgeKind::Calls)
}

#[test]
fn a_qualified_symbol_in_a_function_body_gives_a_calls_edge_to_the_function_of_the_alias_or_full_namespace()
 {
    let g = index(&[
        DEPS,
        (
            "src/x/y.clj",
            "(ns x.y)\n(defn f [])\n(defn- p [])\n(defmacro m [])\n(defmulti mm :k)\n",
        ),
        (
            "src/user.clj",
            "(ns user (:require [x.y :as a]))\n\
             (defn by-alias [] (a/f))\n\
             (defn- by-name [] (x.y/p))\n\
             (defmacro macro-caller [] (a/m))\n\
             (defmulti multi-caller (fn [x] (a/mm x)))\n",
        ),
        (
            "src/rec.clj",
            "(ns rec (:require [x.y :as a]))\n\
             (defrecord R [k] Object (toString [_] (a/f)))\n\
             (deftype T [k] Object (toString [_] (x.y/f)))\n",
        ),
    ]);

    let mut found = calls(&g);
    found.sort();
    assert_eq!(
        found,
        vec![
            ("fn:rec/R#toString", "fn:x.y/f"),
            ("fn:rec/T#toString", "fn:x.y/f"),
            ("fn:user/by-alias", "fn:x.y/f"),
            ("fn:user/by-name", "fn:x.y/p"),
            ("fn:user/macro-caller", "fn:x.y/m"),
            ("fn:user/multi-caller", "fn:x.y/mm"),
        ]
    );
}

#[test]
fn a_function_named_as_a_value_or_by_var_quote_gives_the_calls_edge_and_is_counted_as_a_value() {
    let (g, report) = run(&[
        DEPS,
        ("src/x.clj", "(ns x)\n(defn f [])\n"),
        (
            "src/user.clj",
            "(ns user (:require [x :as a]))\n\
             (defn g [xs] (a/f) (map a/f xs) (partial a/f 1) #'a/f)\n",
        ),
    ]);

    assert_eq!(calls(&g), vec![("fn:user/g", "fn:x/f")]);
    assert_eq!(report.calls.to_function, 4);
    assert_eq!(report.function_values, 3);
}

#[test]
fn a_protocol_method_is_a_var_of_the_namespace_of_the_protocol_and_an_interface_method_is_not() {
    let (g, report) = run(&[
        DEPS,
        (
            "src/x.clj",
            "(ns x)\n(defprotocol Store (-get [s k]) (-put [s k v]))\n(definterface Iface (poke [k]))\n",
        ),
        (
            "src/user.clj",
            "(ns user (:require [x :as p]))\n\
             (defn g [s] (p/-get s 1))\n\
             (defn h [s] (p/poke s 1))\n",
        ),
    ]);

    assert_eq!(calls(&g), vec![("fn:user/g", "fn:x/Store#-get")]);
    assert_eq!(report.calls.to_function, 1);
}

#[test]
fn a_protocol_an_interface_and_the_constructors_of_a_record_or_type_give_type_ref_edges() {
    let (g, report) = run(&[
        DEPS,
        (
            "src/x.clj",
            "(ns x)\n(defprotocol Proto (m [x]))\n(definterface Iface)\n(defrecord Rec [a])\n(deftype Ty [a])\n",
        ),
        (
            "src/user.clj",
            "(ns user (:require [x :as a]))\n\
             (defn g [] [a/Proto a/Iface (a/->Rec 1) (a/map->Rec {}) (a/->Ty 1) (a/map->Ty {}) a/Rec])\n",
        ),
    ]);

    let mut found = edges_of(&g, EdgeKind::TypeRef);
    found.sort();
    assert_eq!(
        found,
        vec![
            ("fn:user/g", "type:x/Proto"),
            ("fn:user/g", "type:x/Rec"),
            ("fn:user/g", "type:x/Ty"),
        ]
    );
    assert_eq!(calls(&g), vec![]);
    assert_eq!(report.calls.to_type, 4);
    // Clojure makes no var for `definterface`: `a/Iface` is no valid name.
    assert!(report.unresolved_names.contains_key("x/Iface"));
}

#[test]
fn a_symbol_that_gives_no_edge_is_counted_as_external_var_without_node_or_unresolved() {
    let (g, report) = run(&[
        DEPS,
        ("src/x.clj", "(ns x)\n(defn f [])\n(declare v w)\n"),
        (
            "src/user.clj",
            "(ns user (:require [clojure.string :as str] [x :as a]))\n\
             (defn g []\n\
               (str/join \",\" [])\n\
               (Math/abs 1)\n\
               (String/valueOf 1)\n\
               (a/v)\n\
               a/w\n\
               (a/nothing)\n\
               (x/also-nothing))\n",
        ),
    ]);

    assert_eq!(g.edges.iter().filter(|e| e.from == "fn:user/g").count(), 0);
    assert_eq!(report.calls.to_function + report.calls.to_type, 0);
    assert_eq!(report.calls.external, 3);
    assert_eq!(report.var_without_node, 2);
    // A var without a node is part of the unresolved calls, as a value is part of the
    // calls to a function.
    assert_eq!(report.calls.unresolved, 4);
    let names: Vec<(&str, usize)> = report
        .unresolved_names
        .iter()
        .map(|(k, v)| (k.as_str(), *v))
        .collect();
    assert_eq!(names, vec![("x/also-nothing", 1), ("x/nothing", 1)]);
}

#[test]
fn quoted_discarded_commented_metadata_keyword_and_destructuring_forms_are_not_read() {
    let (g, report) = run(&[
        DEPS,
        ("src/x.clj", "(ns x)\n(defn f [])\n"),
        (
            "src/user.clj",
            "(ns user (:require [x :as a]))\n\
             (defn g [^a/T x ^{:tag a/f} y {:keys [a/f] :syms [a/f]}]\n\
               '(a/f)\n\
               (quote (a/f))\n\
               #_(a/f)\n\
               (comment (a/f))\n\
               :a/f ::a/f :x/f\n\
               `(a/f ~x))\n\
             (defmacro m [x] `(a/f ~x))\n",
        ),
    ]);

    assert_eq!(calls(&g), vec![("fn:user/m", "fn:x/f")]);
    // The macro is the only site that is read.
    assert_eq!(report.calls.to_function, 1);
    assert_eq!(report.calls.external + report.calls.unresolved, 0);
}

#[test]
fn a_reference_in_code_without_a_function_node_gives_no_edge_and_is_counted_by_the_head_of_its_form()
 {
    let (g, report) = run(&[
        DEPS,
        ("src/x.clj", "(ns x)\n(defn f [])\n(defrecord R [])\n"),
        (
            "src/user.clj",
            "(ns user (:require [x :as a]))\n\
             (def e (Math/abs 1))\n\
             (defmethod mm :k [x] (a/f x) a/->R)\n\
             (defmethod mm :k [x] (a/f x) a/->R)\n\
             (a/f)\n\
             (defn g [] (a/f))\n",
        ),
    ]);

    // The second `defmethod` is a duplicate: no node.
    assert_eq!(
        sorted(calls(&g)),
        vec![("fn:user/g", "fn:x/f"), ("fn:user/mm :k", "fn:x/f")]
    );
    assert_eq!(
        edges_of(&g, EdgeKind::TypeRef),
        vec![("fn:user/mm :k", "type:x/R")]
    );
    let by_head: Vec<(&str, usize)> = report
        .no_source_node
        .iter()
        .map(|(k, v)| (k.as_str(), *v))
        .collect();
    assert_eq!(by_head, vec![("defmethod", 2), ("other", 1)]);
    // Only the sites with a source node are counted as calls to a function or a type.
    assert_eq!(report.calls.to_function, 2);
    assert_eq!(report.calls.to_type, 1);
    // A reference that leads nowhere in the repository is counted as before.
    assert_eq!(report.calls.external, 1);
}

#[test]
fn no_edge_joins_a_node_with_itself_or_its_ancestor_and_an_edge_exists_once_for_many_sites() {
    let (g, report) = run(&[
        DEPS,
        ("src/x/sub.clj", "(ns x.sub)\n(defn s [])\n"),
        (
            "src/x.clj",
            "(ns x (:require [x.sub :as sub]))\n\
             (defn f [] (x/f) (x/g) (x/g) (x/g))\n\
             (defn g [] (sub/s))\n\
             (defrecord R [] Object (toString [_] (x/->R) (x/g)))\n",
        ),
    ]);

    let mut all = calls(&g);
    all.sort();
    assert_eq!(
        all,
        vec![
            ("fn:x/R#toString", "fn:x/g"),
            ("fn:x/f", "fn:x/g"),
            ("fn:x/g", "fn:x.sub/s"),
        ]
    );
    assert_eq!(edges_of(&g, EdgeKind::TypeRef), vec![]);
    // The sites are counted, also those that give no edge.
    assert_eq!(report.calls.to_function, 6);
    assert_eq!(report.calls.to_type, 1);
}

#[test]
fn a_type_implements_the_protocols_and_interfaces_of_the_repository_that_its_body_names() {
    let g = index(&[
        DEPS,
        (
            "src/x.clj",
            "(ns x)\n(defprotocol P (pm [s]))\n(definterface I)\n(defrecord Rec [a])\n",
        ),
        (
            "src/user.clj",
            "(ns user (:require [x :as p :refer [P I Rec]]))\n\
             (defprotocol Local (lm [s]))\n\
             (defrecord R [a] p/P (pm [_] 1) Local (lm [_] 2) I Closeable java.io.Closeable Object (toString [_] \"\"))\n\
             (deftype T [a] P (pm [_] 1))\n\
             (defrecord Q [a] Rec x/Rec p/Nothing)\n",
        ),
    ]);

    let mut found = edges_of(&g, EdgeKind::Implements);
    found.sort();
    assert_eq!(
        found,
        vec![
            ("type:user/R", "type:user/Local"),
            ("type:user/R", "type:x/I"),
            ("type:user/R", "type:x/P"),
            ("type:user/T", "type:x/P"),
        ]
    );
}

fn sorted<'a>(mut edges: Vec<(&'a str, &'a str)>) -> Vec<(&'a str, &'a str)> {
    edges.sort();
    edges
}

#[test]
fn an_unqualified_symbol_at_the_head_of_a_list_that_names_a_function_of_the_namespace_gives_a_calls_edge()
 {
    let g = index(&[
        DEPS,
        (
            "src/x.clj",
            "(ns x)\n\
             (defn early [] (late))\n\
             (defn late [] 1)\n\
             (defn- p [] (early) (inc 1) (unknown))\n\
             (defmacro m [] (p))\n\
             (defmulti mm :k)\n\
             (defn use-mm [] (mm 1))\n\
             (defrecord R [k] Object (toString [_] (late)))\n\
             (deftype T [k] Object (toString [_] (p)))\n",
        ),
    ]);

    assert_eq!(
        sorted(calls(&g)),
        vec![
            ("fn:x/R#toString", "fn:x/late"),
            ("fn:x/T#toString", "fn:x/p"),
            ("fn:x/early", "fn:x/late"),
            ("fn:x/m", "fn:x/p"),
            ("fn:x/p", "fn:x/early"),
            ("fn:x/use-mm", "fn:x/mm"),
        ]
    );
}

#[test]
fn a_name_that_the_ns_form_brings_with_refer_all_or_use_gives_a_calls_edge() {
    let g = index(&[
        DEPS,
        (
            "src/x.clj",
            "(ns x)\n(defn f [])\n(defprotocol P (pm [s]))\n(defn- hidden [])\n(defn ^:private meta-hidden [])\n(defn other [])\n",
        ),
        ("src/y.clj", "(ns y)\n(defn g [])\n"),
        (
            "src/user.clj",
            "(ns user (:require [x :refer [f pm]] [y :refer :all] [clojure.string :refer [join]]))\n\
             (defn a [] (f) (pm 1) (g) (join 1) (other) (hidden) (meta-hidden))\n",
        ),
        (
            "src/legacy.clj",
            "(ns legacy (:use [x :only [f]] y))\n(defn a [] (f) (g) (other))\n",
        ),
        // `:refer :all` brings the public vars only.
        (
            "src/everything.clj",
            "(ns everything (:require [x :refer :all]))\n(defn a [] (f) (other) (hidden) (meta-hidden))\n",
        ),
        // `:exclude` and `:rename` leave a name out.
        (
            "src/some.clj",
            "(ns some (:use [x :exclude [f]]) (:require [y :refer [g] :rename {g gg}]))\n(defn a [] (f) (other) (g) (gg))\n",
        ),
    ]);

    assert_eq!(
        sorted(calls(&g)),
        vec![
            ("fn:everything/a", "fn:x/f"),
            ("fn:everything/a", "fn:x/other"),
            ("fn:legacy/a", "fn:x/f"),
            ("fn:legacy/a", "fn:y/g"),
            ("fn:some/a", "fn:x/other"),
            ("fn:user/a", "fn:x/P#pm"),
            ("fn:user/a", "fn:x/f"),
            ("fn:user/a", "fn:y/g"),
        ]
    );
}

#[test]
fn a_name_that_two_sources_declare_gives_no_edge_and_is_counted_as_ambiguous() {
    let (g, report) = run(&[
        DEPS,
        ("src/x.clj", "(ns x)\n(defn f [])\n(defn other [])\n"),
        ("src/z.clj", "(ns z)\n(defn f [])\n"),
        (
            "src/two_refers.clj",
            "(ns two-refers (:require [x :refer [f]] [z :refer [f]]))\n(defn a [] (f))\n",
        ),
        (
            "src/own.clj",
            "(ns own (:require [x :refer [other]]))\n(defn other [])\n(defn b [] (other))\n",
        ),
        (
            "src/both_all.clj",
            "(ns both-all (:require [x :refer :all] [z :refer :all]))\n(defn c [] (f) (other))\n",
        ),
    ]);

    // `other` is declared by one namespace only in `both_all`.
    assert_eq!(calls(&g), vec![("fn:both-all/c", "fn:x/other")]);
    assert_eq!(report.unqualified.ambiguous, 3);
    assert_eq!(report.calls.unresolved, 3);
}

#[test]
fn a_protocol_method_gives_calls_and_a_protocol_interface_or_constructor_gives_type_ref() {
    let (g, report) = run(&[
        DEPS,
        (
            "src/x.clj",
            "(ns x)\n(defprotocol Proto (-get [s k]))\n(definterface Iface)\n(defrecord Rec [a])\n(deftype Ty [a])\n",
        ),
        (
            "src/user.clj",
            "(ns user (:require [x :refer [Proto Rec ->Rec map->Rec -get]]))\n\
             (definterface Own)\n\
             (defprotocol OwnProto (-put [s k]))\n\
             (defrecord Mine [a])\n\
             (defn a [s] (-get s 1) (-put s 1) (->Rec 1) (map->Rec {}) (->Mine 1) (map->Mine {}) (satisfies? Proto s) (instance? Own s) (satisfies? OwnProto s) (.-x s) (Rec. 1))\n",
        ),
    ]);

    assert_eq!(
        sorted(calls(&g)),
        vec![
            ("fn:user/a", "fn:user/OwnProto#-put"),
            ("fn:user/a", "fn:x/Proto#-get"),
        ]
    );
    assert_eq!(
        sorted(edges_of(&g, EdgeKind::TypeRef)),
        vec![
            ("fn:user/a", "type:user/Mine"),
            ("fn:user/a", "type:user/Own"),
            ("fn:user/a", "type:user/OwnProto"),
            ("fn:user/a", "type:x/Proto"),
            ("fn:user/a", "type:x/Rec"),
        ]
    );
    assert_eq!(report.unqualified.to_type, 7);
    assert_eq!(report.calls.to_type, 7);
}

#[test]
fn an_unqualified_function_named_as_a_value_gives_the_edge_and_is_counted_as_a_value() {
    let (g, report) = run(&[
        DEPS,
        ("src/x.clj", "(ns x)\n(defn f [])\n"),
        (
            "src/user.clj",
            "(ns user (:require [x :refer [f]]))\n\
             (defn helper [])\n\
             (defn g [xs] (helper) (map helper xs) (partial helper 1) #'helper (map f xs))\n",
        ),
    ]);

    assert_eq!(
        sorted(calls(&g)),
        vec![("fn:user/g", "fn:user/helper"), ("fn:user/g", "fn:x/f")]
    );
    assert_eq!(report.unqualified.to_function, 5);
    assert_eq!(report.unqualified.function_values, 4);
    assert_eq!(report.calls.to_function, 5);
    assert_eq!(report.function_values, 4);
}

#[test]
fn a_name_that_a_local_could_bind_in_the_same_top_level_form_gives_no_edge_and_is_counted() {
    let (g, report) = run(&[
        DEPS,
        ("src/x.clj", "(ns x)\n(defn r-plain [])\n(defn rv [])\n"),
        (
            "src/user.clj",
            "(ns user (:require [x :refer [r-plain rv]]))\n\
             (defn plain [] 1)\n\
             (defn v1 [] 1)\n\
             (defn v1n [] 1)\n\
             (defn v2 [] 1)\n\
             (defn v3 [] 1)\n\
             (defn v4 [] 1)\n\
             (defn v5 [] 1)\n\
             (defn v6 [] 1)\n\
             (defn v7 [] 1)\n\
             (defn plain-user [] (plain) (r-plain) (inc 1) (unknown))\n\
             (defn t1 [v1] (v1))\n\
             (defn t1n [[a [v1n]]] (v1n))\n\
             (defn t2 [{v2 :k}] (v2))\n\
             (defn t3 [{:k v3}] (v3))\n\
             (defn t4 [] (as-> 1 v4 (v4)))\n\
             (defn t5 [] (try 1 (catch Exception v5 (v5))))\n\
             (defn t6 [] (fn v6 [] (v6)))\n\
             (defn t7 [] (letfn [(v7 [] 1)] (v7)))\n\
             (defn t8 [rv] (rv))\n",
        ),
    ]);

    assert_eq!(
        sorted(calls(&g)),
        vec![
            ("fn:user/plain-user", "fn:user/plain"),
            ("fn:user/plain-user", "fn:x/r-plain"),
        ]
    );
    // Only a call (the head of a list) is a site, not the binder and not a value: one per
    // form. The head of a `letfn` binding is the binder, not a call.
    assert_eq!(report.unqualified.maybe_local, 9);
    assert_eq!(report.unqualified.to_function, 2);
    assert_eq!(report.calls.unresolved, 9);
}

#[test]
fn the_names_after_or_as_keys_syms_and_strs_in_any_map_literal_could_be_bound() {
    let (g, report) = run(&[
        DEPS,
        (
            "src/user.clj",
            "(ns user)\n\
             (defn k1 [] 1)\n\
             (defn k2 [] 1)\n\
             (defn k3 [] 1)\n\
             (defn k4 [] 1)\n\
             (defn defaults [] 1)\n\
             (defn t1 [{:keys [a] :or {k1 (defaults)}}] (k1))\n\
             (defn t2 [] (with-options {:as k2} (k2)))\n\
             (defn t3 [] (with-options {:keys [k3]} (k3)))\n\
             (defn t4 [] (with-options {:strs [k4]} (k4)))\n",
        ),
    ]);

    // The default `(defaults)` is code, not a binder.
    assert_eq!(calls(&g), vec![("fn:user/t1", "fn:user/defaults")]);
    // One site per form: the call.
    assert_eq!(report.unqualified.maybe_local, 4);
}

#[test]
fn the_name_of_the_enclosing_function_is_no_site() {
    let (g, report) = run(&[
        DEPS,
        (
            "src/user.clj",
            "(ns user)\n\
             (defn fact [n] (if (zero? n) 1 (* n (fact (dec n)))))\n\
             (defn each [xs] (map each xs))\n\
             (defn a [] (fact 1))\n",
        ),
    ]);

    assert_eq!(calls(&g), vec![("fn:user/a", "fn:user/fact")]);
    assert_eq!(report.unqualified.to_function, 1);
    assert_eq!(report.calls.to_function, 1);
    assert_eq!(report.calls.unresolved, 0);
}

#[test]
fn a_function_of_the_namespace_wins_over_a_core_function_of_the_same_name_with_or_without_exclude()
{
    let g = index(&[
        DEPS,
        (
            "src/excluded.clj",
            "(ns excluded (:refer-clojure :exclude [get]))\n(defn get [])\n(defn a [] (get))\n",
        ),
        (
            "src/shadowing.clj",
            "(ns shadowing)\n(defn get [])\n(defn a [] (get))\n",
        ),
    ]);

    assert_eq!(
        sorted(calls(&g)),
        vec![
            ("fn:excluded/a", "fn:excluded/get"),
            ("fn:shadowing/a", "fn:shadowing/get"),
        ]
    );
}

#[test]
fn special_forms_core_functions_interop_and_plain_vars_are_no_sites() {
    let (g, report) = run(&[
        DEPS,
        (
            "src/user.clj",
            "(ns user)\n\
             (declare c)\n\
             (defn f [x]\n\
               (if x (do (inc 1) (.method x) (.-field x) (Foo. 1) (new Foo 1) (set! y 1)) c)\n\
               (c)\n\
               (recur (unknown x)))\n",
        ),
    ]);

    assert_eq!(g.edges.len(), 0);
    assert_eq!(report.unqualified, cg_index_clojure::Unqualified::default());
    assert_eq!(report.calls.unresolved, 0);
    assert_eq!(report.calls.external, 0);
}

#[test]
fn a_syntax_quoted_form_in_a_macro_is_read_and_a_name_after_unquote_quote_is_not() {
    let (g, report) = run(&[
        DEPS,
        (
            "src/user.clj",
            "(ns user)\n\
             (defn helper [] 1)\n\
             (defn other [] 1)\n\
             (defmacro m1 [] `(helper))\n\
             (defmacro m2 [] `(do ~'other ~'(other) (helper)))\n\
             (defmacro m3 [x] `(do ~(other) ~x))\n\
             (defn g [] `(helper) '(other))\n",
        ),
    ]);

    assert_eq!(
        sorted(calls(&g)),
        vec![
            ("fn:user/m1", "fn:user/helper"),
            ("fn:user/m2", "fn:user/helper"),
            ("fn:user/m3", "fn:user/other"),
        ]
    );
    assert_eq!(report.unqualified.to_function, 3);
}

#[test]
fn an_unqualified_reference_in_code_without_a_function_node_is_counted_by_the_head_of_its_form() {
    let (g, report) = run(&[
        DEPS,
        (
            "src/user.clj",
            "(ns user)\n(defn helper [])\n(defrecord R [])\n\
             (defmethod mm :k [x] (helper) ->R)\n\
             (defmethod mm :k [x] (helper) ->R)\n\
             (helper)\n",
        ),
    ]);

    // The first `defmethod` has a node, the second is a duplicate.
    assert_eq!(g.edges.len(), 2);
    let by_head: Vec<(&str, usize)> = report
        .no_source_node
        .iter()
        .map(|(k, v)| (k.as_str(), *v))
        .collect();
    assert_eq!(by_head, vec![("defmethod", 2), ("other", 1)]);
    assert_eq!(report.unqualified.to_function, 1);
    assert_eq!(report.unqualified.to_type, 1);
}

#[test]
fn the_name_of_a_method_in_reify_proxy_specify_or_extend_type_is_not_a_call() {
    let (g, report) = run(&[
        DEPS,
        (
            "src/user.clj",
            "(ns user)\n\
             (defprotocol P (-create [x]) (-form [x]))\n\
             (defn helper [] 1)\n\
             (defn get [] 1)\n\
             (defn mk [] (reify P (-create [_] (helper)) (-form ([_] 1) ([_ x] 2))))\n\
             (defn mk2 [] (proxy [Object] [] (get [] (helper))))\n\
             (defn mk3 [] (extend-type Foo P (-form [_] (helper))))\n\
             (defn mk4 [x] (specify! x P (-create [_] (helper))))\n\
             (defn use-it [] (-create 1))\n",
        ),
    ]);

    assert_eq!(
        sorted(calls(&g)),
        vec![
            ("fn:user/mk", "fn:user/helper"),
            ("fn:user/mk2", "fn:user/helper"),
            ("fn:user/mk3", "fn:user/helper"),
            ("fn:user/mk4", "fn:user/helper"),
            ("fn:user/use-it", "fn:user/P#-create"),
        ]
    );
    assert_eq!(report.unqualified.to_function, 5);
}

#[test]
fn code_in_a_defrecord_or_deftype_without_a_node_is_counted_under_its_own_head() {
    let (_, report) = run(&[
        DEPS,
        (
            "src/user.clj",
            "(ns user)\n(defn helper [])\n\
             (defrecord R [] Object (toString [_] (helper)))\n\
             (defrecord R [] Object (toString [_] (helper)))\n\
             (deftype T [] Object (toString [_] (helper)))\n\
             (deftype T [] Object (toString [_] (helper)))\n",
        ),
    ]);

    let by_head: Vec<(&str, usize)> = report
        .no_source_node
        .iter()
        .map(|(k, v)| (k.as_str(), *v))
        .collect();
    assert_eq!(by_head, vec![("defrecord", 1), ("deftype", 1)]);
}

#[test]
fn the_arities_of_a_method_written_as_separate_forms_are_one_function_node_read_as_a_whole() {
    let (g, report) = run(&[
        DEPS,
        (
            "src/x.clj",
            "(ns x)\n\
             (defn helper [] 1)\n\
             (defn other [] 1)\n\
             (defprotocol P (m [this] [this x]) (k [this]))\n\
             (defrecord R [a]\n\
               P\n\
               (m [this] (helper))\n\
               (m [this x] (other))\n\
               (k [this] 1))\n\
             (deftype T []\n\
               P\n\
               (m [this] (helper))\n\
               (k [this] 1)\n\
               (m [this x] (other)))\n",
        ),
    ]);

    let methods: Vec<&str> = g
        .nodes
        .iter()
        .map(|n| n.id.as_str())
        .filter(|id| id.starts_with("fn:x/R#") || id.starts_with("fn:x/T#"))
        .collect();
    assert_eq!(
        methods,
        vec!["fn:x/R#m", "fn:x/R#k", "fn:x/T#m", "fn:x/T#k"]
    );
    assert_eq!(
        sorted(calls(&g)),
        vec![
            ("fn:x/R#m", "fn:x/helper"),
            ("fn:x/R#m", "fn:x/other"),
            ("fn:x/T#m", "fn:x/helper"),
            ("fn:x/T#m", "fn:x/other"),
        ]
    );
    // The arities are adjacent in R: the span runs over both. In T another method lies
    // between them: the span of the first.
    let span = |id: &str| node(&g, id).span.unwrap();
    assert_eq!(
        (span("fn:x/R#m").start_line, span("fn:x/R#m").end_line),
        (7, 8)
    );
    assert_eq!(
        (span("fn:x/T#m").start_line, span("fn:x/T#m").end_line),
        (12, 12)
    );
    assert!(report.no_source_node.is_empty());
}

#[test]
fn the_own_hash_of_a_method_covers_all_its_arities() {
    let hashes = |first: &str, second: &str| {
        let g = index(&[
            DEPS,
            (
                "src/a.clj",
                &format!(
                    "(ns a)\n(defrecord R [a]\n  P\n  (m [this] {first})\n  (m [this x] {second}))\n"
                ),
            ),
        ]);
        (
            node(&g, "type:a/R").hash.clone(),
            node(&g, "fn:a/R#m").hash.clone(),
        )
    };

    let (type_a, method_a) = hashes("1", "2");
    let (type_b, method_b) = hashes("1", "3");
    let (_, method_c) = hashes("0", "2");
    assert_eq!(type_a, type_b);
    assert_ne!(method_a, method_b);
    assert_ne!(method_a, method_c);
}

#[test]
fn a_declared_name_is_a_var_without_node_and_not_an_unresolved_name() {
    let (g, report) = run(&[
        DEPS,
        (
            "src/x.clj",
            "(ns x)\n(declare later declared-only)\n(defn later [])\n",
        ),
        (
            "src/user.clj",
            "(ns user (:require [x :as a]))\n(defn g [] (a/later) (a/declared-only) (a/nothing))\n",
        ),
    ]);

    assert_eq!(calls(&g), vec![("fn:user/g", "fn:x/later")]);
    assert_eq!(report.var_without_node, 1);
    let names: Vec<&str> = report.unresolved_names.keys().map(String::as_str).collect();
    assert_eq!(names, vec!["x/nothing"]);
}

#[test]
fn a_hidden_name_that_is_not_called_is_no_site() {
    let (g, report) = run(&[
        DEPS,
        (
            "src/user.clj",
            "(ns user)\n\
             (defn v1 [] 1)\n\
             (defn t1 [v1 xs] (map v1 xs) [v1] v1)\n",
        ),
    ]);

    assert_eq!(g.edges.len(), 0);
    assert_eq!(report.unqualified, cg_index_clojure::Unqualified::default());
    assert_eq!(report.calls.unresolved, 0);
}

#[test]
fn import_gives_an_imports_edge_to_the_record_type_or_interface_of_the_repository_by_class_name() {
    let g = index(&[
        DEPS,
        (
            "src/x/y_z.clj",
            "(ns x.y-z)\n(defrecord Rec [a])\n(deftype Ty [a])\n(definterface Iface)\n(defprotocol Proto (m [x]))\n(defn f [])\n",
        ),
        (
            "src/u.clj",
            "(ns u\n  (:import [x.y_z Rec Ty]\n           (x.y_z Iface)\n           x.y_z.Proto\n           [java.util Date]\n           [x.y_z Nope]\n           x.y-z.Rec))\n",
        ),
        // A namespace gets no edge to a type in itself.
        (
            "src/x/own.clj",
            "(ns x.own (:import x.own.Mine))\n(defrecord Mine [])\n",
        ),
    ]);

    assert_eq!(
        imports(&g),
        [
            ("pkg::u", "type:x.y-z/Iface"),
            ("pkg::u", "type:x.y-z/Proto"),
            ("pkg::u", "type:x.y-z/Rec"),
            ("pkg::u", "type:x.y-z/Ty"),
        ]
    );
}

const CLASSES: &str = "(ns x.y-z)\n(defrecord R1 [a])\n(defrecord R2 [a])\n(defrecord R3 [a])\n\
(defrecord R4 [a])\n(defrecord R5 [a])\n(defrecord R6 [a])\n(defrecord R7 [a])\n\
(defrecord R8 [a])\n(defrecord R9 [a])\n(defrecord R10 [a])\n(defrecord R11 [a])\n\
(defrecord R12 [a])\n";

#[test]
fn an_imported_or_fully_named_class_in_the_code_of_a_function_gives_a_type_ref_edge() {
    let (g, report) = run(&[
        DEPS,
        ("src/x/y_z.clj", CLASSES),
        (
            "src/user.clj",
            "(ns user (:import [x.y_z R1 R2 R3 R4 R5 R6 R7 R9 R10 R12] [java.util Date]))\n\
             (defn a [] (R1. 1))\n\
             (defn b [] (new R2 1))\n\
             (defn c [] (R3/create 1))\n\
             (defn d [] R4/CONSTANT)\n\
             (defn e [x] (instance? R5 x))\n\
             (defn f [x] (try x (catch R6 e e)))\n\
             (defn g [x] (.isInstance R7 x))\n\
             (defn h [] [(x.y_z.R8. 1) x.y_z.R8/make x.y_z.R8])\n\
             (defn i [^R9 x] x)\n\
             (defn j [R10] R10)\n\
             (defn k [] (R11. 1))\n\
             (defn l [] (Date. 1))\n\
             (defn m [] [Date/from java.util.Date (java.util.Date. 1)])\n\
             (defn n [] '(R12 x.y_z.R12))\n",
        ),
    ]);

    let mut found = edges_of(&g, EdgeKind::TypeRef);
    found.sort();
    assert_eq!(
        found,
        vec![
            ("fn:user/a", "type:x.y-z/R1"),
            ("fn:user/b", "type:x.y-z/R2"),
            ("fn:user/c", "type:x.y-z/R3"),
            ("fn:user/d", "type:x.y-z/R4"),
            ("fn:user/e", "type:x.y-z/R5"),
            ("fn:user/f", "type:x.y-z/R6"),
            ("fn:user/g", "type:x.y-z/R7"),
            ("fn:user/h", "type:x.y-z/R8"),
        ]
    );
    // The sites: `a` to `g` one each, `h` three; outside the repository `Date.` in `l` and
    // three in `m`. `j` has the name of a class as a local: both occurrences are counted
    // as hidden, and give no edge.
    assert_eq!(report.classes.to_type, 10);
    assert_eq!(report.classes.external, 4);
    assert_eq!(report.classes.maybe_local, 2);
    assert_eq!(report.calls.to_type, 10);
    assert_eq!(report.calls.external, 4);
    assert_eq!(report.calls.unresolved, 0);
}

#[test]
fn a_full_namespace_name_and_an_alias_are_no_class_references_and_an_alias_wins_over_a_class() {
    let (g, report) = run(&[
        DEPS,
        ("src/x/y_z.clj", CLASSES),
        (
            "src/user.clj",
            "(ns user (:require [clojure.string :as s] [x.y-z :as R1]) (:import [x.y_z R1]))\n\
             (defn a [] [(clojure.string/join \",\" []) (s/join \",\" []) (R1/->R1 1) (clojure.core/inc 1)])\n",
        ),
    ]);

    // `R1/name` names the namespace `x.y-z` by its alias, as the compiler resolves it.
    let mut found = edges_of(&g, EdgeKind::TypeRef);
    found.sort();
    assert_eq!(found, vec![("fn:user/a", "type:x.y-z/R1")]);
    assert_eq!(report.classes.to_type, 0);
    assert_eq!(report.classes.external, 0);
    assert_eq!(report.calls.external, 3);
}

#[test]
fn an_imported_or_fully_named_interface_in_the_body_of_a_record_or_type_gives_an_implements_edge() {
    let g = index(&[
        DEPS,
        (
            "src/x/y_z.clj",
            "(ns x.y-z)\n(definterface I1)\n(definterface I2)\n(definterface I3)\n(defrecord NotAnInterface [])\n",
        ),
        (
            "src/user.clj",
            "(ns user (:import [x.y_z I1 I2 NotAnInterface] [java.io Closeable]))\n\
             (defrecord R [a] I1 (m [this] this) x.y_z.I3 java.io.Closeable (close [_]))\n\
             (deftype T [a] I2)\n\
             (defrecord Mine [a] Own)\n\
             (definterface Own)\n",
        ),
    ]);

    let mut found = edges_of(&g, EdgeKind::Implements);
    found.sort();
    assert_eq!(
        found,
        vec![
            ("type:user/Mine", "type:user/Own"),
            ("type:user/R", "type:x.y-z/I1"),
            ("type:user/R", "type:x.y-z/I3"),
            ("type:user/T", "type:x.y-z/I2"),
        ]
    );
}

#[test]
fn a_top_level_def_or_defonce_is_a_value_node_and_a_literal_fn_value_is_a_function() {
    let (g, report) = run(&[
        DEPS,
        (
            "src/a.clj",
            "(ns a)\n\
             (def limit 10)\n\
             (def ^:private secret \"doc\" 42)\n\
             (defonce ^{:doc \"x\"} state (atom {}))\n\
             (def handler (fn [x] x))\n\
             (def named (fn* named [x] x))\n\
             (def lazy #(inc %))\n\
             (def unbound)\n\
             (declare later)\n\
             (def ^:dynamic *x*)\n",
        ),
    ]);

    for (id, name) in [
        ("val:a/limit", "limit"),
        ("val:a/secret", "secret"),
        ("val:a/state", "state"),
        ("val:a/lazy", "lazy"),
        ("val:a/unbound", "unbound"),
        ("val:a/*x*", "*x*"),
    ] {
        let n = node(&g, id);
        assert_eq!((n.kind, n.name.as_str()), (NodeKind::Value, name), "{id}");
        assert_eq!(n.parent.as_deref(), Some("pkg::a"));
        assert_eq!(n.file.as_deref(), Some("src/a.clj"));
        assert!(n.hash.is_some() && n.span.is_some() && !n.test);
    }
    assert_eq!(
        node(&g, "val:a/limit").span,
        Some(Span {
            start_line: 2,
            end_line: 2
        })
    );
    for id in ["fn:a/handler", "fn:a/named"] {
        assert_eq!(node(&g, id).kind, NodeKind::Function, "{id}");
    }
    assert!(g.node("fn:a/later").is_none() && g.node("val:a/later").is_none());
    assert_eq!(report.value_nodes, 6);
    assert!(report.forms_without_node.contains_key("declare"));
    assert!(!report.forms_without_node.contains_key("def"));
}

#[test]
fn of_two_forms_that_declare_the_same_name_the_first_gives_the_node_and_declare_gives_none() {
    let (g, report) = run(&[
        DEPS,
        (
            "src/a.cljc",
            "(ns a)\n\
             (declare f g)\n\
             (defn f [] 1)\n\
             (def g 2)\n\
             (def h 1)\n\
             (defn h [] 2)\n\
             #?(:clj (defn k [] 1) :cljs (def k 2))\n",
        ),
    ]);

    assert_eq!(
        ids(&g),
        ["mod:", "pkg::a", "fn:a/f", "val:a/g", "val:a/h", "fn:a/k"]
    );
    assert_eq!(report.duplicate_declarations.get("defn"), Some(&1));
    assert_eq!(report.duplicate_declarations.get("def"), Some(&1));
}

fn value_refs(g: &Graph) -> Vec<(&str, &str)> {
    edges_of(g, EdgeKind::ValueRef)
}

#[test]
fn the_value_of_a_def_is_read_as_a_function_body_with_the_node_as_source() {
    let (g, report) = run(&[
        DEPS,
        ("src/x.clj", "(ns x)\n(defn f [])\n(defn g [])\n"),
        (
            "src/user.clj",
            "(ns user (:require [x :as a]))\n\
             (defn local [])\n\
             (def handler (comp a/f local))\n\
             (def doc \"documented\" (a/g))\n\
             (def inline (fn [x] (a/f x)))\n",
        ),
    ]);

    assert_eq!(
        sorted(calls(&g)),
        vec![
            ("fn:user/inline", "fn:x/f"),
            ("val:user/doc", "fn:x/g"),
            ("val:user/handler", "fn:user/local"),
            ("val:user/handler", "fn:x/f"),
        ]
    );
    assert!(report.no_source_node.is_empty());
}

#[test]
fn a_symbol_that_names_a_value_gives_a_value_ref_edge_also_at_the_head_and_is_counted_to_value() {
    let (g, report) = run(&[
        DEPS,
        (
            "src/x.clj",
            "(ns x)\n(def limit 10)\n(def ^:private hidden 1)\n(defonce state (atom {}))\n(declare only-declared)\n",
        ),
        (
            "src/user.clj",
            "(ns user (:require [x :as a] [x :refer [limit]]))\n\
             (def own 1)\n\
             (def derived (inc own))\n\
             (defn f [] (a/limit) a/state (a/only-declared) (a/nothing))\n\
             (defn g [] (+ own limit))\n\
             (defn h [own] (own))\n\
             (defn self [] (str own))\n",
        ),
    ]);

    assert_eq!(
        sorted(value_refs(&g)),
        vec![
            ("fn:user/f", "val:x/limit"),
            ("fn:user/f", "val:x/state"),
            ("fn:user/g", "val:user/own"),
            ("fn:user/g", "val:x/limit"),
            ("fn:user/self", "val:user/own"),
            ("val:user/derived", "val:user/own"),
        ]
    );
    // f: 2, g: 2, self: 1, derived: 1. `h` binds `own` as a parameter, and `(own)` is a hidden call.
    assert_eq!(report.calls.to_value, 6);
    assert_eq!(report.unqualified.to_value, 4);
    assert_eq!(report.unqualified.maybe_local, 1);
    // Only a name that `declare` alone introduces stays a var without node.
    assert_eq!(report.var_without_node, 1);
    assert!(calls(&g).is_empty());
    // A reference to a value from code without a node is counted with the head of its form.
    let (_, report) = run(&[
        DEPS,
        ("src/y.clj", "(ns y)\n(def v 1)\n(def w v)\n(inc v)\n"),
    ]);
    assert_eq!(report.calls.to_value, 1);
    assert_eq!(report.no_source_node.get("other"), Some(&1));
}

const MULTIS: (&str, &str) = (
    "src/m.clj",
    "(ns m)\n(defmulti area :shape)\n(defn helper [x] x)\n(def factor 2)\n",
);

#[test]
fn a_defmethod_is_a_function_below_the_namespace_named_by_multi_and_dispatch_value() {
    let (g, report) = run(&[
        DEPS,
        MULTIS,
        (
            "src/user.clj",
            "(ns user (:require [m :as mm] [ig.core :as ig :refer [init-key]]))\n\
             (defmulti local-m :k)\n\
             (defmethod mm/area :circle [x] (mm/helper x) mm/factor)\n\
             (defmethod mm/area :circle [x] 3)\n\
             (defmethod mm/area [:a   :b]\n  ([x] 1)\n  ([x y] 2))\n\
             (defmethod undeclared :k [x] x)\n\
             (defmethod local-m :x [x] x)\n\
             (defmethod ig/init-key :blaze.db/node [_ cfg] cfg)\n\
             (defmethod init-key :other [_ x] x)\n",
        ),
        (
            "src/user2.clj",
            "(ns user2 (:require [m :as other]))\n(defmethod other/area :circle [x] x)\n",
        ),
    ]);

    let expected = [
        ("fn:user/m/area :circle", "mm/area :circle", 3, 3),
        ("fn:user/m/area [:a :b]", "mm/area [:a :b]", 5, 7),
        ("fn:user/undeclared :k", "undeclared :k", 8, 8),
        ("fn:user/user/local-m :x", "local-m :x", 9, 9),
        (
            "fn:user/ig.core/init-key :blaze.db/node",
            "ig/init-key :blaze.db/node",
            10,
            10,
        ),
        ("fn:user/ig.core/init-key :other", "init-key :other", 11, 11),
    ];
    for (id, name, start, end) in expected {
        let n = node(&g, id);
        assert_eq!(
            (n.kind, n.name.as_str()),
            (NodeKind::Function, name),
            "{id}"
        );
        assert_eq!(n.parent.as_deref(), Some("pkg::user"), "{id}");
        assert_eq!(n.file.as_deref(), Some("src/user.clj"));
        assert_eq!(
            n.span,
            Some(Span {
                start_line: start,
                end_line: end
            }),
            "{id}"
        );
        assert!(n.hash.is_some() && !n.test);
    }
    // The id holds the full name of the namespace, not the alias.
    assert_eq!(
        node(&g, "fn:user2/m/area :circle").name,
        "other/area :circle"
    );
    assert_eq!(report.method_nodes, 7);
    assert_eq!(report.duplicate_declarations.get("defmethod"), Some(&1));
    assert!(!report.forms_without_node.contains_key("defmethod"));
    // Never the id of a defn: those hold no space.
    assert!(
        g.nodes
            .iter()
            .filter(|n| n.id.contains(' '))
            .all(|n| n.id.starts_with("fn:user"))
    );
}

#[test]
fn the_body_of_a_defmethod_is_read_with_the_method_as_source_and_it_implements_its_defmulti() {
    let (g, report) = run(&[
        DEPS,
        MULTIS,
        (
            "src/user.clj",
            "(ns user (:require [m :as mm :refer [area]] [ig.core :as ig]))\n\
             (defrecord Circle [r])\n\
             (defmulti local-m :k)\n\
             (defmethod mm/area :circle [x] (mm/helper x) mm/factor (->Circle 1))\n\
             (defmethod area :square [x] (mm/helper x))\n\
             (defmethod local-m :a ([x] x) ([x y] (mm/helper y)))\n\
             (defmethod ig/init-key :k [_ cfg] cfg)\n\
             (defmethod Circle-dispatch :k [x] x)\n\
             (defmethod mm/helper :not-a-multi [x] x)\n\
             (defmethod mm/area Circle [x] x)\n",
        ),
    ]);

    assert_eq!(
        sorted(calls(&g)),
        vec![
            ("fn:user/m/area :circle", "fn:m/helper"),
            ("fn:user/m/area :square", "fn:m/helper"),
            ("fn:user/user/local-m :a", "fn:m/helper"),
        ]
    );
    assert_eq!(
        sorted(value_refs(&g)),
        vec![("fn:user/m/area :circle", "val:m/factor")]
    );
    assert_eq!(
        sorted(edges_of(&g, EdgeKind::TypeRef)),
        vec![
            ("fn:user/m/area :circle", "type:user/Circle"),
            ("fn:user/m/area Circle", "type:user/Circle")
        ]
    );
    // To the `defmulti` through the alias, through `:refer`, and in the own namespace. Not
    // to a `defn`, not to a multi outside the repository.
    assert_eq!(
        sorted(edges_of(&g, EdgeKind::Implements)),
        vec![
            ("fn:user/m/area :circle", "fn:m/area"),
            ("fn:user/m/area :square", "fn:m/area"),
            ("fn:user/m/area Circle", "fn:m/area"),
            ("fn:user/user/local-m :a", "fn:user/local-m"),
        ]
    );
    assert!(report.no_source_node.is_empty());
}

const TYPES: (&str, &str) = (
    "src/types.clj",
    "(ns types)\n(defprotocol Store (-get [s k]) (-put [s k]))\n(defrecord Rec [a])\n(definterface Marker)\n",
);

#[test]
fn a_method_of_extend_protocol_or_extend_type_is_a_function_below_its_type_else_protocol_else_namespace()
 {
    let (g, report) = run(&[
        DEPS,
        TYPES,
        (
            "src/user.clj",
            "(ns user (:require [types :as t] [other.lib :as o]) (:import [java.util Date]))\n\
             (defrecord Local [x])\n\
             (defprotocol Own (own-m [x]))\n\
             (extend-protocol t/Store\n\
               Local\n\
               (-get [x k] (t/-put x k))\n\
               (-put [x k] k)\n\
               nil\n\
               (-get [_ k] nil)\n\
               Object\n\
               (-put ([x] 1) ([x y] 2))\n\
               Date\n\
               (-get [_ _] 1))\n\
             (extend-type types.Rec t/Store (-get [_ k] k))\n\
             (extend-type String o/Ext (m [s] s) (n [s] s))\n\
             (extend-type Local Own (own-m [x] x))\n\
             (extend Local Own {:own-m (fn [x] x)})\n",
        ),
    ]);

    let expected = [
        ("ext:user/Local#types/Store/-get", "type:user/Local", "-get"),
        ("ext:user/Local#types/Store/-put", "type:user/Local", "-put"),
        (
            "ext:nil#types/Store/-get",
            "type:types/Store",
            "-get for nil",
        ),
        (
            "ext:Object#types/Store/-put",
            "type:types/Store",
            "-put for Object",
        ),
        (
            "ext:java.util.Date#types/Store/-get",
            "type:types/Store",
            "-get for Date",
        ),
        ("ext:types/Rec#types/Store/-get", "type:types/Rec", "-get"),
        (
            "ext:String#other.lib/Ext/m",
            "pkg::user",
            "o/Ext/m for String",
        ),
        (
            "ext:String#other.lib/Ext/n",
            "pkg::user",
            "o/Ext/n for String",
        ),
        ("ext:user/Local#user/Own/own-m", "type:user/Local", "own-m"),
    ];
    for (id, parent, name) in expected {
        let n = node(&g, id);
        assert_eq!(
            (n.kind, n.name.as_str()),
            (NodeKind::Function, name),
            "{id}"
        );
        assert_eq!(n.parent.as_deref(), Some(parent), "{id}");
        assert_eq!(n.file.as_deref(), Some("src/user.clj"));
        assert!(n.hash.is_some() && n.span.is_some() && !n.test);
    }
    assert_eq!(
        node(&g, "ext:Object#types/Store/-put").span,
        Some(Span {
            start_line: 11,
            end_line: 11
        })
    );
    assert_eq!(report.extend_nodes, 9);
    assert_eq!(report.extend_forms, 1);
    assert!(!report.forms_without_node.contains_key("extend-protocol"));
    assert!(!report.forms_without_node.contains_key("extend-type"));
    // The id of a method of a type that is outside the repository is the same as written.
    assert_eq!(
        sorted(edges_of(&g, EdgeKind::Implements)),
        vec![
            ("type:types/Rec", "type:types/Store"),
            ("type:user/Local", "type:types/Store"),
            ("type:user/Local", "type:user/Own"),
        ]
    );
}

#[test]
fn the_body_of_an_extend_method_is_read_with_the_method_as_source() {
    let (g, report) = run(&[
        DEPS,
        TYPES,
        MULTIS,
        (
            "src/user.clj",
            "(ns user (:require [types :as t] [m :as mm]))\n\
             (defrecord Local [x])\n\
             (extend-protocol t/Store\n\
               Local\n\
               (-get [x k] (t/-put x k) (mm/helper k) mm/factor)\n\
               String\n\
               (-get [x k] (mm/helper k) (->Local k)))\n\
             (extend-type Local t/Store (-put [x k] (->Local k)))\n",
        ),
    ]);

    assert_eq!(
        sorted(calls(&g)),
        vec![
            ("ext:String#types/Store/-get", "fn:m/helper"),
            ("ext:user/Local#types/Store/-get", "fn:m/helper"),
            ("ext:user/Local#types/Store/-get", "fn:types/Store#-put"),
        ]
    );
    assert_eq!(
        value_refs(&g),
        vec![("ext:user/Local#types/Store/-get", "val:m/factor")]
    );
    assert_eq!(
        edges_of(&g, EdgeKind::TypeRef),
        vec![("ext:String#types/Store/-get", "type:user/Local")]
    );
    // The method below `Local` does not name its own type: no edge to an ancestor.
    assert!(report.no_source_node.is_empty());
}

#[test]
fn the_forms_of_one_extend_method_are_one_node_and_a_repeated_method_is_a_duplicate() {
    let (g, report) = run(&[
        DEPS,
        TYPES,
        (
            "src/user.clj",
            "(ns user (:require [types :as t]))\n\
             (extend-protocol t/Store\n\
               String\n\
               (-get [x k] 1)\n\
               (-put [x k] 2)\n\
               (-get [x] 3))\n\
             (extend-type String t/Store (-get [x k] 4))\n\
             (extend-type nil)\n",
        ),
    ]);

    let n = node(&g, "ext:String#types/Store/-get");
    // Two forms with a form between them: the span is that of the first.
    assert_eq!(
        n.span,
        Some(Span {
            start_line: 4,
            end_line: 4
        })
    );
    let single = index(&[
        DEPS,
        TYPES,
        (
            "src/user.clj",
            "(ns user (:require [types :as t]))\n(extend-protocol t/Store\n  String\n  (-get [x k] 1)\n  (-put [x k] 2))\n",
        ),
    ]);
    // The own text of the merged node covers both forms.
    assert_ne!(
        n.hash,
        node(&single, "ext:String#types/Store/-get").hash.clone()
    );
    assert_eq!(report.extend_nodes, 2);
    assert_eq!(report.duplicate_declarations.get("extend"), Some(&1));
    // A form without a method gives no node and is counted.
    assert_eq!(report.forms_without_node.get("extend-type"), Some(&1));
}

#[test]
fn methods_in_test_code_are_test_code() {
    let g = index(&[
        ("deps.edn", "{:aliases {:test {:extra-paths [\"test\"]}}}"),
        TYPES,
        (
            "test/user.clj",
            "(ns user (:require [types :as t]))\n(defmulti m :k)\n(defmethod m :a [x] x)\n(extend-type String t/Store (-get [x k] 4))\n(def v 1)\n",
        ),
    ]);

    for id in [
        "fn:user/user/m :a",
        "ext:String#types/Store/-get",
        "val:user/v",
    ] {
        assert!(node(&g, id).test, "{id}");
    }
}
