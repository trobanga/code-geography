use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

const USAGE: &str = "usage: cg index <repo> [-o <graph.json>]
       cg diff <repo> <rev> [-o <diff.json>]";

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Vec<String>) -> Result<(), String> {
    let mut args = args.into_iter();
    let command = args.next();
    let default_output = match command.as_deref() {
        Some("index") => "graph.json",
        Some("diff") => "diff.json",
        _ => return Err(USAGE.into()),
    };
    let mut positional = Vec::new();
    let mut output = PathBuf::from(default_output);
    while let Some(arg) = args.next() {
        if arg == "-o" {
            output = args.next().ok_or(USAGE)?.into();
        } else {
            positional.push(arg);
        }
    }

    let mut report = None;
    let graph = match (command.as_deref(), positional.as_slice()) {
        (Some("index"), [repo]) => {
            let repo = PathBuf::from(repo);
            let (graph, found) = cg_index::index_with_report(&repo)
                .map_err(|e| format!("{}: {e}", repo.display()))?;
            report = Some(found);
            graph
        }
        (Some("diff"), [repo, rev]) => diff(Path::new(repo), rev)?,
        _ => return Err(USAGE.into()),
    };
    let json = serde_json::to_string(&graph).map_err(|e| e.to_string())?;
    std::fs::write(&output, json).map_err(|e| format!("{}: {e}", output.display()))?;
    eprintln!(
        "{} nodes, {} edges -> {}",
        graph.nodes.len(),
        graph.edges.len(),
        output.display()
    );
    if let Some(cg_index::Report {
        calls,
        rust,
        clojure,
    }) = report
    {
        eprintln!(
            "calls: {} to function, {} to type, {} external, {} unresolved",
            calls.to_function, calls.to_type, calls.external, calls.unresolved
        );
        let rust_calls = rust.calls;
        eprintln!(
            "rust calls: {} to function ({} as a value), {} to type, {} external, {} unresolved; {} more in macro arguments, not read",
            rust_calls.to_function,
            rust.function_values,
            rust_calls.to_type,
            rust_calls.external,
            rust_calls.unresolved,
            rust.macro_call_sites
        );
        let paths = rust.paths;
        eprintln!(
            "rust: {} paths resolved, {} external, {} unresolved; {} files not reached, {} #[path] attributes",
            paths.resolved,
            paths.external,
            paths.unresolved,
            rust.unreached_files.len(),
            rust.path_attributes.len()
        );
        let clojure_calls = clojure.calls;
        let no_source: usize = clojure.no_source_node.values().sum();
        eprintln!(
            "clojure calls: {} to function ({} as a value), {} to type, {} external, {} unresolved ({} var without node); {} more in code without a function node, not read",
            clojure_calls.to_function,
            clojure.function_values,
            clojure_calls.to_type,
            clojure_calls.external,
            clojure_calls.unresolved,
            clojure.var_without_node,
            no_source
        );
        if no_source > 0 {
            let by_head: Vec<String> = clojure
                .no_source_node
                .iter()
                .map(|(head, count)| format!("{head} {count}"))
                .collect();
            eprintln!(
                "clojure code without a function node, by head: {}",
                by_head.join(", ")
            );
        }
        let unqualified = &clojure.unqualified;
        eprintln!(
            "clojure unqualified: {} to function ({} as a value), {} to type, {} a local could hide, {} ambiguous",
            unqualified.to_function,
            unqualified.function_values,
            unqualified.to_type,
            unqualified.maybe_local,
            unqualified.ambiguous
        );
        eprintln!(
            "clojure classes: {} to a type of the repository, {} external, {} a local could hide",
            clojure.classes.to_type, clojure.classes.external, clojure.classes.maybe_local
        );
        // The most frequent names first: which macros of the repository make them.
        let mut names: Vec<_> = clojure.unresolved_names.iter().collect();
        names.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        if !names.is_empty() {
            let top: Vec<String> = names
                .iter()
                .take(15)
                .map(|(name, count)| format!("{name} {count}"))
                .collect();
            eprintln!("clojure unresolved: {}", top.join(", "));
        }
        let without_node: usize = clojure.forms_without_node.values().sum();
        eprintln!(
            "clojure: {} files outside the source directories, {} below no deps.edn, {} without ns form, {} duplicate namespaces; {} top-level forms without a node",
            clojure.files_outside_source_dirs,
            clojure.files_without_deps_edn,
            clojure.files_without_ns,
            clojure.duplicate_namespaces.len(),
            without_node
        );
        // The most frequent heads first: the size of the gap that macros of the
        // repository make.
        let mut heads: Vec<_> = clojure.forms_without_node.iter().collect();
        heads.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        if !heads.is_empty() {
            let top: Vec<String> = heads
                .iter()
                .take(10)
                .map(|(head, count)| format!("{head} {count}"))
                .collect();
            eprintln!("clojure forms without a node: {}", top.join(", "));
        }
    }
    Ok(())
}

/// The union graph of two revisions. `spec` is one revision, compared with its
/// parent, or a range `A..B`, which compares `A` with `B`.
fn diff(repo: &Path, spec: &str) -> Result<cg_core::Graph, String> {
    // Git would read a leading `-` as an option.
    if spec.starts_with('-') {
        return Err(format!("{spec}: not a revision"));
    }
    let (base, head, base_may_be_missing) = match spec.split_once("..") {
        Some((base, head))
            if !base.is_empty()
                && !head.is_empty()
                && !base.ends_with('.')
                && !head.starts_with('.') =>
        {
            (base.to_string(), head, false)
        }
        Some(_) => return Err(format!("{spec}: expected a range of the form A..B")),
        None => (format!("{spec}^"), spec, true),
    };
    // The head first: an unknown revision is reported as such, not as a missing parent.
    let after = read_revision(repo, head)?;
    // A root commit has no parent and is compared with the empty source set.
    let root_commit = base_may_be_missing && !has_commit(repo, &base)?;
    let before = if root_commit {
        Vec::new()
    } else {
        read_revision(repo, &base)?
    };
    let changed = changed_files(repo, &base, head, root_commit)?;
    let name = repo
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let spec = cg_index::DiffSpec {
        base,
        head: head.to_string(),
        changed,
    };
    Ok(cg_index::diff_sources(&name, &before, &after, spec).0)
}

/// The files that differ between `base` and `head`, sorted by path as git lists
/// them. A root commit has no base: all its files count as added.
fn changed_files(
    repo: &Path,
    base: &str,
    head: &str,
    root_commit: bool,
) -> Result<Vec<(String, cg_core::Change)>, String> {
    let mut git = Command::new("git");
    git.arg("-C").arg(repo);
    if root_commit {
        git.args(["diff-tree", "--root", "--no-commit-id", "-r"])
            .args(["--name-status", "--no-renames", "-z"])
            .arg(head);
    } else {
        git.args(["diff", "--name-status", "--no-renames", "-z"])
            .arg(base)
            .arg(head);
    }
    let output = git.arg("--").output().map_err(|e| format!("git: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim_end().into());
    }
    // With `-z`: `<status> NUL <path> NUL` per file, the path as stored.
    let text = String::from_utf8_lossy(&output.stdout);
    let mut fields = text.split('\0');
    let mut changed = Vec::new();
    while let (Some(status), Some(path)) = (fields.next(), fields.next()) {
        let change = match status.chars().next() {
            Some('A') => cg_core::Change::Added,
            Some('D') => cg_core::Change::Removed,
            // `M`, a changed file type, or anything else that touches the file.
            _ => cg_core::Change::Modified,
        };
        changed.push((path.to_string(), change));
    }
    Ok(changed)
}

fn has_commit(repo: &Path, revision: &str) -> Result<bool, String> {
    let status = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["rev-parse", "--verify", "--quiet"])
        .arg(format!("{revision}^{{commit}}"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| format!("git: {e}"))?;
    Ok(status.success())
}

/// Unpacks `git archive <revision>` into a temporary directory and reads the files that
/// a front end wants.
fn read_revision(repo: &Path, revision: &str) -> Result<Vec<cg_index::SourceFile>, String> {
    let dir = tempfile::tempdir().map_err(|e| format!("temporary directory: {e}"))?;
    let mut archive = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["archive", revision])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("git: {e}"))?;
    let tar = Command::new("tar")
        .arg("-x")
        .arg("-C")
        .arg(dir.path())
        .stdin(Stdio::from(archive.stdout.take().expect("stdout is piped")))
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("tar: {e}"))?;
    let archive = archive
        .wait_with_output()
        .map_err(|e| format!("git: {e}"))?;
    if !archive.status.success() {
        return Err(String::from_utf8_lossy(&archive.stderr).trim_end().into());
    }
    if !tar.status.success() {
        return Err(String::from_utf8_lossy(&tar.stderr).trim_end().into());
    }
    cg_walk::read_sources(dir.path(), cg_index::is_input).map_err(|e| format!("{revision}: {e}"))
}
