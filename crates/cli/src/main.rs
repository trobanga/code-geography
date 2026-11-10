use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: cg index <repo> [-o <graph.json>]";

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
    if args.next().as_deref() != Some("index") {
        return Err(USAGE.into());
    }
    let mut repo = None;
    let mut output = PathBuf::from("graph.json");
    while let Some(arg) = args.next() {
        if arg == "-o" {
            output = args.next().ok_or(USAGE)?.into();
        } else {
            repo = Some(PathBuf::from(arg));
        }
    }
    let repo = repo.ok_or(USAGE)?;

    let graph = cg_index_java::index(&repo).map_err(|e| format!("{}: {e}", repo.display()))?;
    let json = serde_json::to_string(&graph).map_err(|e| e.to_string())?;
    std::fs::write(&output, json).map_err(|e| format!("{}: {e}", output.display()))?;
    eprintln!(
        "{} nodes, {} edges -> {}",
        graph.nodes.len(),
        graph.edges.len(),
        output.display()
    );
    Ok(())
}
