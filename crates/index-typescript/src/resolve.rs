//! What a specifier of an import means: a file of the repository or something else.

use crate::config::{Config, best_pattern};
use crate::layout::Source;
use crate::path::{dir_of, join};
use std::collections::HashMap;

/// What a specifier of a file reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// A file that is read: its index in the `Vec<Source>`.
    Source(usize),
    /// A package of the repository whose `package.json` proves no file: the module of the
    /// file with this index in the `Vec<Source>`.
    Module(usize),
    /// Leaves the repository: a package of `node_modules`, `node:fs`.
    External,
    /// Should reach the repository, and no file that is read fits.
    Unresolved,
    /// A relative path of a file of another kind (`./a.css`): not read.
    OtherFile,
}

/// Decides what the specifier of an import means. The one place for it: a relative path,
/// the `paths` of a `tsconfig`, the name of a package of the repository.
pub struct Resolver<'a> {
    paths: Vec<&'a str>,
    by_path: HashMap<&'a str, usize>,
    /// The first file of each module, by the directory of the module.
    by_module: HashMap<&'a str, usize>,
    config: &'a Config,
}

impl<'a> Resolver<'a> {
    pub fn new(sources: &[Source<'a>], config: &'a Config) -> Self {
        let paths: Vec<&str> = sources.iter().map(|s| s.path).collect();
        let by_path = paths.iter().enumerate().map(|(i, p)| (*p, i)).collect();
        let mut by_module = HashMap::new();
        for (index, source) in sources.iter().enumerate() {
            by_module.entry(source.module).or_insert(index);
        }
        Resolver {
            paths,
            by_path,
            by_module,
            config,
        }
    }

    /// What `specifier`, written in the file `from`, reaches.
    pub fn resolve(&self, from: usize, specifier: &str) -> Target {
        if is_relative(specifier) {
            self.relative(from, specifier)
        } else {
            self.not_relative(from, specifier)
        }
    }

    fn relative(&self, from: usize, specifier: &str) -> Target {
        let Some(joined) = join(dir_of(self.paths[from]), specifier) else {
            return Target::Unresolved;
        };
        match self.find(&joined) {
            Some(found) => Target::Source(found),
            None if has_other_extension(specifier) => Target::OtherFile,
            None => Target::Unresolved,
        }
    }

    /// The first file that is read among the files that the path `p` can name.
    fn find(&self, p: &str) -> Option<usize> {
        candidates(p)
            .into_iter()
            .find_map(|candidate| self.by_path.get(candidate.as_str()).copied())
    }

    /// A specifier that does not start with a dot.
    fn not_relative(&self, from: usize, specifier: &str) -> Target {
        let by_tsconfig = self.by_tsconfig(from, specifier);
        if let Some(found @ Target::Source(_)) = by_tsconfig {
            return found;
        }
        // A pattern that finds no file leaves the name to the packages, as the compiler
        // goes on to `node_modules`.
        if let Some(target) = self.by_package(specifier) {
            return target;
        }
        if let Some(target) = by_tsconfig {
            return target;
        }
        // No package of `node_modules` has a name that starts so: an alias that nothing here
        // resolves.
        if specifier.starts_with("@/") || specifier.starts_with(['~', '#', '$', '/']) {
            Target::Unresolved
        } else {
            Target::External
        }
    }

    /// What the `paths` of the nearest `tsconfig.json` make of `specifier`, if they claim it.
    fn by_tsconfig(&self, from: usize, specifier: &str) -> Option<Target> {
        let dir = self.config.tsconfig_dir(dir_of(self.paths[from]))?;
        let claims: Vec<Claim> = self
            .config
            .configurations(&dir)
            .iter()
            .filter_map(|tsconfig| self.claim(tsconfig, specifier))
            .collect();
        let Claim::File(first) = *claims.first()? else {
            return Some(Target::Unresolved);
        };
        // Every configuration that claims the name must give the same file.
        let same = claims
            .iter()
            .all(|claim| matches!(claim, Claim::File(file) if *file == first));
        Some(if same {
            Target::Source(first)
        } else {
            Target::Unresolved
        })
    }

    /// What the name of a package of the repository makes of `specifier`, if the name is one.
    fn by_package(&self, specifier: &str) -> Option<Target> {
        let (name, subpath) = split_package(specifier);
        let dir = match self.config.packages_named(name).as_slice() {
            [] => return None,
            [dir] => *dir,
            // Which of them the import means is not in the files.
            _ => return Some(Target::Unresolved),
        };
        // Every entry that names a file that is read must name the same one.
        let mut found: Vec<usize> = self
            .config
            .entries(dir, subpath)
            .iter()
            .filter_map(|p| self.find(p))
            .collect();
        found.dedup();
        Some(match (found.as_slice(), self.by_module.get(dir)) {
            ([file], _) => Target::Source(*file),
            (_, Some(first)) => Target::Module(*first),
            (_, None) => Target::Unresolved,
        })
    }

    /// What the configuration file `tsconfig` says about `specifier`, if anything.
    fn claim(&self, tsconfig: &str, specifier: &str) -> Option<Claim> {
        let options = self.config.options(tsconfig);
        if let Some((targets, matched)) = best_pattern(&options.patterns, specifier) {
            let found = targets
                .iter()
                .find_map(|target| self.find(&join(&options.base, &target.replace('*', matched))?));
            match found {
                Some(file) => return Some(Claim::File(file)),
                // The pattern `*` matches every name: it claims none that it does not find.
                None if matched == specifier => {}
                None => return Some(Claim::Missing),
            }
        }
        let base_url = options.base_url?;
        self.find(&join(&base_url, specifier)?).map(Claim::File)
    }
}

/// The name of the package in a specifier (the first segment, or two if it starts with `@`)
/// and the rest.
fn split_package(specifier: &str) -> (&str, Option<&str>) {
    let end = if specifier.starts_with('@') {
        specifier
            .match_indices('/')
            .nth(1)
            .map_or(specifier.len(), |(i, _)| i)
    } else {
        specifier.find('/').unwrap_or(specifier.len())
    };
    let rest = specifier[end..].strip_prefix('/').filter(|r| !r.is_empty());
    (&specifier[..end], rest)
}

/// What a configuration says about a specifier.
#[derive(Clone, Copy)]
enum Claim {
    /// It names this file.
    File(usize),
    /// A pattern matches and none of its targets is a file that is read.
    Missing,
}

/// Whether the last segment of the specifier has an extension that is none of a script.
fn has_other_extension(specifier: &str) -> bool {
    let last = specifier.rsplit('/').next().unwrap_or(specifier);
    match last.rsplit_once('.') {
        Some((stem, extension)) => {
            !stem.is_empty()
                && !extension.is_empty()
                && !["ts", "tsx", "js", "jsx", "mjs", "cjs", "mts", "cts"].contains(&extension)
        }
        None => false,
    }
}

/// The files that the path `p` (relative to the root, without a trailing slash) can name,
/// the first that is read wins.
fn candidates(p: &str) -> Vec<String> {
    if p.ends_with(".ts") || p.ends_with(".tsx") {
        vec![p.to_string()]
    } else if let Some(stem) = p.strip_suffix(".js") {
        vec![
            format!("{stem}.ts"),
            format!("{stem}.tsx"),
            format!("{stem}.d.ts"),
        ]
    } else if let Some(stem) = p.strip_suffix(".jsx") {
        vec![format!("{stem}.tsx")]
    } else {
        let index = |name: &str| {
            if p.is_empty() {
                name.to_string()
            } else {
                format!("{p}/{name}")
            }
        };
        vec![
            format!("{p}.ts"),
            format!("{p}.tsx"),
            format!("{p}.d.ts"),
            index("index.ts"),
            index("index.tsx"),
            index("index.d.ts"),
        ]
    }
}

fn is_relative(specifier: &str) -> bool {
    specifier == "."
        || specifier == ".."
        || specifier.starts_with("./")
        || specifier.starts_with("../")
}
