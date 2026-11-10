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
    if let Some(cg_index::Report { calls, rust }) = report {
        eprintln!(
            "calls: {} to function, {} to type, {} external, {} unresolved",
            calls.to_function, calls.to_type, calls.external, calls.unresolved
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
    let after = index_revision(repo, head)?;
    // A root commit has no parent and is compared with the empty graph.
    let root_commit = base_may_be_missing && !has_commit(repo, &base)?;
    let before = if root_commit {
        cg_core::Graph::default()
    } else {
        index_revision(repo, &base)?
    };
    let mut graph = cg_core::diff(&before, &after);
    let changed = changed_files(repo, &base, head, root_commit)?;
    graph.diff = Some(cg_core::DiffInfo {
        base,
        head: head.to_string(),
        files: cg_core::file_changes(&graph, &changed),
    });
    Ok(graph)
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

/// Unpacks `git archive <revision>` into a temporary directory and indexes it.
fn index_revision(repo: &Path, revision: &str) -> Result<cg_core::Graph, String> {
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
    cg_index::index(dir.path()).map_err(|e| format!("{revision}: {e}"))
}
