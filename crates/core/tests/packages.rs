use cg_core::{PackageHolds, marked_package_nodes_by};

#[test]
fn packages_nest_by_a_separator_that_the_caller_chooses() {
    let holds: PackageHolds = [("a/b/x", false), ("a/b/y.test", true), ("a/c", false)]
        .into_iter()
        .map(|(name, test)| (("m".to_string(), name.to_string()), test))
        .collect();

    let modules = marked_package_nodes_by(&holds, '/');

    assert_eq!(modules.len(), 1);
    let (module, nodes) = &modules[0];
    assert_eq!(module, "m");
    let list: Vec<(&str, &str, Option<&str>, bool)> = nodes
        .iter()
        .map(|n| {
            (
                n.name.as_str(),
                n.display.as_str(),
                n.parent.as_deref(),
                n.test,
            )
        })
        .collect();
    // `a` is the common directory of all packages: no node. `a/b` has two next segments.
    // The dot of `y.test` is no separator.
    assert_eq!(
        list,
        [
            ("a/b", "a/b", None, false),
            ("a/b/x", "x", Some("a/b"), false),
            ("a/b/y.test", "y.test", Some("a/b"), true),
            ("a/c", "a/c", None, false),
        ]
    );
}
