//! The Mix projects of a source set, read from their `mix.exs`.

use crate::syntax::{self, arguments, blocks, call_head, descendants, function_head, text};
use cg_core::SourceFile;
use tree_sitter::{Node as Ts, Parser};

/// Where a file of a project stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// Below the directory `test`.
    Test,
    /// An `.ex` file below another source directory.
    Source,
    /// Not read.
    Outside,
}

impl Place {
    /// Whether a file at this place is test code: `None` for a file that is not read.
    pub fn test(self) -> Option<bool> {
        match self {
            Place::Test => Some(true),
            Place::Source => Some(false),
            Place::Outside => None,
        }
    }
}

/// The facts of a `mix.exs` that the graph needs.
#[derive(Debug, Clone)]
pub struct Project {
    /// The directory of the `mix.exs`, relative to the root (empty: the root).
    pub dir: String,
    /// The atom of the first `app:`, if any.
    pub app: Option<String>,
    /// The `mix.exs` of an umbrella root (`apps_path:`): it gives no module.
    pub umbrella: bool,
    /// `lib`, or the string literals of `elixirc_paths`.
    source_dirs: Vec<String>,
}

impl Project {
    pub fn parse(parser: &mut Parser, dir: &str, source: &str) -> Project {
        let mut project = Project {
            dir: dir.to_string(),
            app: None,
            umbrella: false,
            source_dirs: vec!["lib".to_string()],
        };
        let Some(tree) = parser.parse(source, None) else {
            return project;
        };
        let nodes = descendants(tree.root_node());
        let pairs: Vec<_> = nodes
            .iter()
            .filter_map(|n| syntax::pair(*n, source))
            .collect();
        project.umbrella = pairs.iter().any(|(key, _)| *key == "apps_path");
        project.app = pairs
            .iter()
            .find(|(key, _)| *key == "app")
            .and_then(|(_, value)| {
                (value.kind() == "atom").then(|| text(*value, source).trim_start_matches(':'))
            })
            .map(str::to_string);
        let mut paths: Vec<&str> = Vec::new();
        for (_, value) in pairs.iter().filter(|(key, _)| *key == "elixirc_paths") {
            paths.extend(strings(*value, source));
        }
        for call in &nodes {
            if is_elixirc_paths_clause(*call, source) {
                for body in blocks(*call, source).into_iter().flatten() {
                    paths.extend(strings(body, source));
                }
            }
        }
        if !paths.is_empty() {
            project.source_dirs = paths.into_iter().map(clean).collect();
        }
        project
    }

    /// Where the file at `path` (below this project) stands, by this project alone.
    fn place(&self, path: &str) -> Place {
        let relative = relative(&self.dir, path);
        if in_directory("test", relative) {
            Place::Test
        } else if relative.ends_with(".ex")
            && self.source_dirs.iter().any(|d| in_directory(d, relative))
        {
            Place::Source
        } else {
            Place::Outside
        }
    }
}

/// Whether `node` is a clause `def elixirc_paths(..)` or `defp elixirc_paths(..)`.
fn is_elixirc_paths_clause(node: Ts, source: &str) -> bool {
    matches!(call_head(node, source), Some("def" | "defp"))
        && arguments(node)
            .first()
            .and_then(|head| function_head(*head, source))
            .is_some_and(|(name, _)| name == "elixirc_paths")
}

/// The string literals below `node` (and `node` itself).
fn strings<'a>(node: Ts, source: &'a str) -> Vec<&'a str> {
    descendants(node)
        .into_iter()
        .filter_map(|n| syntax::plain_string(n, source))
        .collect()
}

/// A source directory without `./` and a trailing `/`.
fn clean(dir: &str) -> String {
    dir.trim_start_matches("./")
        .trim_end_matches('/')
        .to_string()
}

/// Whether `path` is below the directory `dir` (empty or `.`: everything).
fn in_directory(dir: &str, path: &str) -> bool {
    dir.is_empty() || dir == "." || path.strip_prefix(dir).is_some_and(|r| r.starts_with('/'))
}

/// The `mix.exs` directory of a path: `Some(dir)` if `path` is a `mix.exs`.
pub fn mix_dir(path: &str) -> Option<&str> {
    match path.rsplit_once('/') {
        Some((dir, "mix.exs")) => Some(dir),
        None if path == "mix.exs" => Some(""),
        _ => None,
    }
}

/// All projects of a source set, in the order of their directories.
#[derive(Debug, Clone, Default)]
pub struct Projects(Vec<Project>);

impl Projects {
    /// The projects of the `mix.exs` files among `files`.
    pub fn new<'f>(
        parser: &mut Parser,
        files: impl IntoIterator<Item = &'f SourceFile>,
    ) -> Projects {
        Projects(
            files
                .into_iter()
                .filter_map(|f| Some(Project::parse(parser, mix_dir(&f.path)?, &f.text)))
                .collect(),
        )
    }

    pub fn all(&self) -> impl Iterator<Item = &Project> {
        self.0.iter()
    }

    /// The project of the nearest `mix.exs` above `path`: `None` if there is none or it is
    /// an umbrella root.
    pub fn project_of(&self, path: &str) -> Option<&Project> {
        self.0
            .iter()
            .filter(|p| in_directory(&p.dir, path))
            .max_by_key(|p| p.dir.len())
            .filter(|p| !p.umbrella)
    }

    /// Whether `path` is below the directory `test` of a `mix.exs` above it. A project
    /// there (a fixture) is test code as a whole.
    pub fn below_test(&self, path: &str) -> bool {
        self.0
            .iter()
            .any(|p| in_directory(&p.dir, path) && in_directory("test", relative(&p.dir, path)))
    }

    /// Where the file at `path` stands in `project`, its nearest project.
    pub fn place(&self, project: &Project, path: &str) -> Place {
        match project.place(path) {
            Place::Source if self.below_test(path) => Place::Test,
            place => place,
        }
    }
}

/// `path` without the directory `dir` above it.
fn relative<'a>(dir: &str, path: &'a str) -> &'a str {
    path.strip_prefix(dir)
        .map_or(path, |rest| rest.trim_start_matches('/'))
}
