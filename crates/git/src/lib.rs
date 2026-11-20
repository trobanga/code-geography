//! Reads a git repository from the bytes of its `.git` directory, held in memory (ADR 0039).
//! No file system: the caller gives `HEAD`, the refs, the packs and the loose objects, so
//! that the crate builds for `wasm32-unknown-unknown` and the browser can feed it.
//!
//! The interface answers what an overlay needs: resolve a ref, read a commit with its
//! parents, list the files of a tree, read a blob.

use std::collections::HashMap;
use std::path::PathBuf;

use gix_hash::Kind as HashKind;
pub use gix_hash::ObjectId as Id;
use gix_object::Kind;
use gix_pack::data::decode::entry::ResolvedBase;

/// A commit: its tree and its parents, the first parent first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub tree: Id,
    pub parents: Vec<Id>,
}

/// The objects and refs of one repository.
pub struct Repository {
    head: String,
    refs: HashMap<String, String>,
    packed_refs: String,
    /// Loose objects by id, still compressed: each is inflated when read.
    loose: HashMap<Id, Vec<u8>>,
    packs: Vec<Pack>,
}

impl Repository {
    /// A repository whose `.git/HEAD` holds `head`.
    pub fn new(head: &str) -> Repository {
        Repository {
            head: head.to_string(),
            refs: HashMap::new(),
            packed_refs: String::new(),
            loose: HashMap::new(),
            packs: Vec::new(),
        }
    }

    /// A loose ref: `name` is its path below `.git` (`refs/heads/main`), `text` its content.
    pub fn add_ref(&mut self, name: &str, text: &str) {
        self.refs.insert(name.to_string(), text.to_string());
    }

    /// The content of `.git/packed-refs`.
    pub fn set_packed_refs(&mut self, text: &str) {
        self.packed_refs = text.to_string();
    }

    /// A pack: `index` is the `.idx` file, `data` the `.pack` file of the same name. The
    /// whole pack stays in memory (ADR 0039).
    pub fn add_pack(&mut self, index: Vec<u8>, data: Vec<u8>) -> Result<(), String> {
        let index = gix_pack::index::File::from_data(index, PathBuf::new(), HashKind::Sha1)
            .map_err(|e| format!("pack index: {e}"))?;
        let data = gix_pack::data::File::from_data(data, PathBuf::new(), HashKind::Sha1)
            .map_err(|e| format!("pack: {e}"))?;
        if index.num_objects() != data.num_objects() {
            return Err(format!(
                "the pack index has {} objects, the pack {}",
                index.num_objects(),
                data.num_objects()
            ));
        }
        self.packs.push(Pack { index, data });
        Ok(())
    }

    /// A loose object: `id` is the hex of its name (the directory and the file name of
    /// `.git/objects/xx/yyyy…`), `bytes` the content of the file.
    pub fn add_loose(&mut self, id: &str, bytes: Vec<u8>) -> Result<(), String> {
        let id = parse_id(id)?;
        self.loose.insert(id, bytes);
        Ok(())
    }

    /// The commit that `name` names: `HEAD`, a full ref name (`refs/heads/main`) or the
    /// hex of an id. Follows symbolic refs.
    pub fn resolve(&self, name: &str) -> Result<Id, String> {
        let mut name = name.to_string();
        // A chain of symbolic refs longer than this is a loop.
        for _ in 0..8 {
            let text = match self.lookup(&name)? {
                Lookup::Id(id) => return Ok(id),
                Lookup::Text(text) => text,
            };
            let text = text.trim();
            match text.strip_prefix("ref:") {
                Some(target) => name = target.trim().to_string(),
                None => return parse_id(text),
            }
        }
        Err(format!("the symbolic ref {name} is part of a loop"))
    }

    /// What `name` stands for: the text of `HEAD` or of a loose ref, or an id.
    fn lookup(&self, name: &str) -> Result<Lookup, String> {
        if name == "HEAD" {
            return Ok(Lookup::Text(self.head.clone()));
        }
        if let Some(text) = self.refs.get(name) {
            return Ok(Lookup::Text(text.clone()));
        }
        if let Some(id) = self.packed(name) {
            return Ok(Lookup::Id(id));
        }
        match parse_id(name) {
            Ok(id) => Ok(Lookup::Id(id)),
            Err(_) => Err(format!("no ref {name}")),
        }
    }

    fn packed(&self, name: &str) -> Option<Id> {
        self.packed_refs.lines().find_map(|line| {
            let (id, ref_name) = line.split_once(' ')?;
            (ref_name.trim() == name && !line.starts_with(['#', '^']))
                .then(|| parse_id(id).ok())
                .flatten()
        })
    }

    /// The commit `id`.
    pub fn commit(&self, id: &Id) -> Result<Commit, String> {
        let bytes = self.object_of_kind(id, Kind::Commit)?;
        let commit = gix_object::CommitRef::from_bytes(&bytes, HashKind::Sha1)
            .map_err(|e| format!("commit {id}: {e}"))?;
        Ok(Commit {
            tree: commit.tree(),
            parents: commit.parents().collect(),
        })
    }

    /// The files below the tree `id` whose path `keep` wants, with the ids of their blobs,
    /// sorted by path. A directory whose path `keep_dir` rejects is not read. Paths are
    /// relative to the tree, with `/`. Symbolic links and submodules are no files.
    pub fn files(
        &self,
        id: &Id,
        keep_dir: &dyn Fn(&str) -> bool,
        keep: &dyn Fn(&str) -> bool,
    ) -> Result<Vec<(String, Id)>, String> {
        let mut out = Vec::new();
        let mut todo = vec![(String::new(), *id)];
        let keep = Keep {
            dir: keep_dir,
            file: keep,
        };
        while let Some((prefix, tree)) = todo.pop() {
            self.read_tree(&prefix, &tree, &keep, &mut todo, &mut out)?;
        }
        out.sort();
        Ok(out)
    }

    /// The entries of one tree: its subtrees go to `todo`, its wanted files to `out`.
    fn read_tree(
        &self,
        prefix: &str,
        tree: &Id,
        keep: &Keep,
        todo: &mut Vec<(String, Id)>,
        out: &mut Vec<(String, Id)>,
    ) -> Result<(), String> {
        let bytes = self.object_of_kind(tree, Kind::Tree)?;
        for entry in gix_object::TreeRefIter::from_bytes(&bytes, HashKind::Sha1) {
            let entry = entry.map_err(|e| format!("tree {tree}: {e}"))?;
            let path = format!("{prefix}{}", String::from_utf8_lossy(entry.filename));
            if entry.mode.is_tree() {
                if (keep.dir)(&path) {
                    todo.push((format!("{path}/"), entry.oid.to_owned()));
                }
            } else if is_file(entry.mode) && (keep.file)(&path) {
                out.push((path, entry.oid.to_owned()));
            }
        }
        Ok(())
    }

    /// The content of the blob `id`.
    pub fn blob(&self, id: &Id) -> Result<Vec<u8>, String> {
        self.object_of_kind(id, Kind::Blob)
    }

    fn object_of_kind(&self, id: &Id, want: Kind) -> Result<Vec<u8>, String> {
        let (kind, bytes) = self.object(id)?;
        if kind != want {
            return Err(format!("object {id} is a {kind}, not a {want}"));
        }
        Ok(bytes)
    }

    fn object(&self, id: &Id) -> Result<(Kind, Vec<u8>), String> {
        if let Some(bytes) = self.loose.get(id) {
            return inflate_loose(bytes).map_err(|e| format!("loose object {id}: {e}"));
        }
        for pack in &self.packs {
            if let Some(found) = pack.object(id) {
                return found.map_err(|e| format!("object {id} of a pack: {e}"));
            }
        }
        Err(format!("object {id} is in no pack and not loose"))
    }
}

/// The predicates of `Repository::files`.
struct Keep<'a> {
    dir: &'a dyn Fn(&str) -> bool,
    file: &'a dyn Fn(&str) -> bool,
}

/// The answer of a name lookup in `Repository::resolve`.
enum Lookup {
    Id(Id),
    Text(String),
}

fn is_file(mode: gix_object::tree::EntryMode) -> bool {
    mode.is_blob() || mode.is_executable()
}

/// A pack with its index, both in memory.
struct Pack {
    index: gix_pack::index::File<Vec<u8>>,
    data: gix_pack::data::File<Vec<u8>>,
}

impl Pack {
    /// The object `id`, or None when the pack does not hold it. A delta base must be in the
    /// same pack: git writes no other pack to disk (a thin pack is completed when it is
    /// stored, `index-pack --fix-thin`).
    fn object(&self, id: &Id) -> Option<Result<(Kind, Vec<u8>), String>> {
        let at = self.index.lookup(id)?;
        let entry = match self.data.entry(self.index.pack_offset_at_index(at)) {
            Ok(entry) => entry,
            Err(e) => return Some(Err(e.to_string())),
        };
        let resolve = |base: &gix_hash::oid, _: &mut Vec<u8>| {
            Ok(match self.index.lookup(base) {
                Some(at) => Some(ResolvedBase::InPack(
                    self.data.entry(self.index.pack_offset_at_index(at))?,
                )),
                None => None,
            })
        };
        let mut out = Vec::new();
        let decoded = self.data.decode_entry(
            entry,
            &mut out,
            &mut gix_zlib::Inflate::default(),
            &resolve,
            &mut gix_pack::cache::Never,
        );
        Some(
            decoded
                .map(|outcome| (outcome.kind, out))
                .map_err(|e| e.to_string()),
        )
    }
}

fn parse_id(hex: &str) -> Result<Id, String> {
    Id::from_hex(hex.as_bytes()).map_err(|_| format!("not an object id: {hex}"))
}

/// A loose object: zlib around `<kind> <size>\0<content>`.
fn inflate_loose(bytes: &[u8]) -> Result<(Kind, Vec<u8>), String> {
    let mut inflate = gix_zlib::Inflate::default();
    // Enough for the longest header: `commit 18446744073709551615\0`.
    let mut head = [0u8; 32];
    let (_, consumed, written) = inflate
        .prefix(bytes, &mut head)
        .map_err(|e| e.to_string())?;
    let (kind, size, header) =
        gix_object::decode::loose_header(&head[..written]).map_err(|e| e.to_string())?;
    let size = usize::try_from(size).map_err(|_| format!("too large: {size} bytes"))?;
    let mut out = Vec::new();
    let n = inflate
        .rest(&bytes[consumed..], &head[..written], header, &mut out, size)
        .map_err(|e| e.to_string())?;
    if n != size {
        return Err(format!("{n} bytes, but the header says {size}"));
    }
    Ok((kind, out))
}
