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

    assert_eq!(ids(&g), ["mod:", "pkg::a", "fn:a/ok"]);
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
            ("def", 2),
            ("defmethod", 1),
            ("defonce", 1),
            ("do", 1),
            ("extend-protocol", 1),
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
            ("fn:user/g", "type:x/Iface"),
            ("fn:user/g", "type:x/Proto"),
            ("fn:user/g", "type:x/Rec"),
            ("fn:user/g", "type:x/Ty"),
        ]
    );
    assert_eq!(calls(&g), vec![]);
    assert_eq!(report.calls.to_type, 5);
}

#[test]
fn a_symbol_that_gives_no_edge_is_counted_as_external_var_without_node_or_unresolved() {
    let (g, report) = run(&[
        DEPS,
        (
            "src/x.clj",
            "(ns x)\n(defn f [])\n(def v 1)\n(defonce w 2)\n",
        ),
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
             (def v (a/f))\n\
             (def e (Math/abs 1))\n\
             (defmethod mm :k [x] (a/f x) a/->R)\n\
             (extend-protocol P Foo (m [_] (a/f)))\n\
             (extend-type Foo P (m [_] (a/f)))\n\
             (a/f)\n\
             (defn g [] (a/f))\n",
        ),
    ]);

    assert_eq!(calls(&g), vec![("fn:user/g", "fn:x/f")]);
    assert_eq!(edges_of(&g, EdgeKind::TypeRef), vec![]);
    let by_head: Vec<(&str, usize)> = report
        .no_source_node
        .iter()
        .map(|(k, v)| (k.as_str(), *v))
        .collect();
    assert_eq!(
        by_head,
        vec![
            ("def", 1),
            ("defmethod", 2),
            ("extend-protocol", 1),
            ("other", 2)
        ]
    );
    // Only the sites with a source node are counted as calls to a function or a type.
    assert_eq!(report.calls.to_function, 1);
    assert_eq!(report.calls.to_type, 0);
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

#[test]
fn unqualified_heads_that_name_a_function_are_counted_with_those_that_a_local_could_hide() {
    let (g, report) = run(&[
        DEPS,
        (
            "src/x.clj",
            "(ns x)\n(defn r-plain [])\n(defn r-other [])\n(defn rv [])\n",
        ),
        (
            "src/user.clj",
            "(ns user (:require [x :refer [r-plain r-other rv]]))\n\
             (defn plain [] 1)\n\
             (defn v1 [] 1)\n\
             (defn v2 [] 1)\n\
             (defn v3 [] 1)\n\
             (defn v4 [] 1)\n\
             (defn v5 [] 1)\n\
             (defn v6 [] 1)\n\
             (defn v7 [] 1)\n\
             (defn plain-user [] (plain) (r-plain) (inc 1) (x/rv) (unknown))\n\
             (defn t1 [v1] (v1))\n\
             (defn t2 [{v2 :k}] (v2))\n\
             (defn t3 [{:k v3}] (v3))\n\
             (defn t4 [] (as-> 1 v4 (v4)))\n\
             (defn t5 [] (try 1 (catch Exception v5 (v5))))\n\
             (defn t6 [] (fn v6 [] (v6)))\n\
             (defn t7 [] (letfn [(v7 [] 1)] (v7)))\n\
             (defn t8 [rv] (rv) (r-other))\n\
             (def d (plain))\n\
             (defn quoted [] '(plain))\n",
        ),
    ]);

    // No edge comes from an unqualified symbol, except the one qualified call.
    assert_eq!(calls(&g), vec![("fn:user/plain-user", "fn:x/rv")]);
    let u = &report.unqualified;
    // The head of the binding `(v7 [] 1)` of a letfn is counted, too.
    assert_eq!(u.same_namespace, 9);
    assert_eq!(u.same_namespace_maybe_local, 8);
    assert_eq!(u.referred, 3);
    assert_eq!(u.referred_maybe_local, 1);
}
