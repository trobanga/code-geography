//! Reads a git repository from the bytes of its `.git` directory, held in memory (ADR 0039).
//! No file system: the caller gives `HEAD`, the refs, the packs and the loose objects, so
//! that the crate builds for `wasm32-unknown-unknown` and the browser can feed it.
//!
//! The interface answers what an overlay needs: resolve a ref, read a commit with its
//! parents, list the files of a tree, read a blob.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;

pub use gix_hash::ObjectId as Id;
use gix_hash::{Kind as HashKind, Prefix};
use gix_object::Kind;
use gix_pack::data::decode::entry::ResolvedBase;

/// The fewest hex digits of an abbreviated id that `resolve` takes (as `git`).
const MIN_PREFIX: usize = 7;
const SHA1_HEX_LEN: usize = 40;

/// A commit: its tree and its parents, the first parent first, and what `git log` shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub tree: Id,
    pub parents: Vec<Id>,
    /// The name of the author.
    pub author: String,
    /// The author date as ISO 8601 in the offset of the author: `2026-11-20T10:00:00+01:00`.
    pub date: String,
    /// The whole message (not UTF-8 bytes are replaced).
    pub message: String,
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
        if let Ok(id) = parse_id(name) {
            return Ok(Lookup::Id(id));
        }
        match self.abbreviated(name)? {
            Some(id) => Ok(Lookup::Id(id)),
            None => Err(format!("no ref {name}")),
        }
    }

    /// The one object whose id starts with `name`, if that is 7 to 39 hex digits. An error
    /// when several objects match.
    fn abbreviated(&self, name: &str) -> Result<Option<Id>, String> {
        let digits = name.len();
        if !(MIN_PREFIX..SHA1_HEX_LEN).contains(&digits)
            || !name.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Ok(None);
        }
        let padded = format!("{:0<40}", name.to_ascii_lowercase());
        let prefix = Prefix::new(&parse_id(&padded)?, digits)
            .map_err(|e| format!("not an object id prefix: {name}: {e}"))?;
        let mut found: Vec<Id> = Vec::new();
        let mut add = |id: Id| {
            if !found.contains(&id) {
                found.push(id);
            }
        };
        for id in self.loose.keys().filter(|id| prefix.cmp_oid(id).is_eq()) {
            add(*id);
        }
        for pack in &self.packs {
            match pack.index.lookup_prefix(prefix, None) {
                None => {}
                Some(Ok(at)) => add(pack.index.oid_at_index(at).to_owned()),
                Some(Err(())) => return Err(format!("the prefix {name} is ambiguous")),
            }
        }
        match found.as_slice() {
            [] => Ok(None),
            [id] => Ok(Some(*id)),
            _ => Err(format!("the prefix {name} is ambiguous")),
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
        let (name, time) = signature(commit.author)
            .ok_or_else(|| format!("commit {id}: cannot read the author {:?}", commit.author))?;
        let date =
            iso_date(time).ok_or_else(|| format!("commit {id}: cannot read the date {time:?}"))?;
        Ok(Commit {
            tree: commit.tree(),
            parents: commit.parents().collect(),
            author: name,
            date,
            message: String::from_utf8_lossy(commit.message).into_owned(),
        })
    }

    /// `from` and then its first parent, its first parent's first parent and so on, at most
    /// `n` commits, ending at a root commit (`git log --first-parent -n`).
    pub fn first_parents(&self, from: &Id, n: usize) -> Result<Vec<(Id, Commit)>, String> {
        let mut out = Vec::new();
        let mut next = Some(*from);
        while let Some(id) = next.filter(|_| out.len() < n) {
            // A shallow clone lacks the parent of its last commit (`.git/shallow`): the history ends there.
            if !out.is_empty() && !self.contains(&id) {
                break;
            }
            let commit = self.commit(&id)?;
            next = commit.parents.first().copied();
            out.push((id, commit));
        }
        Ok(out)
    }

    /// Whether the object `id` is among the loose objects or in a pack.
    fn contains(&self, id: &Id) -> bool {
        self.loose.contains_key(id) || self.packs.iter().any(|p| p.index.lookup(id).is_some())
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

    /// The files that differ between the tree `base` (None: an empty tree) and the tree
    /// `head`, sorted by path. A file is modified when its blob differs (a change of the mode
    /// alone is none). A subtree with the same id on both sides is not read. A path that is a
    /// file on one side and a directory on the other is a removed (or added) file and the
    /// added (or removed) files below it.
    pub fn changes(&self, base: Option<&Id>, head: &Id) -> Result<Vec<(String, Change)>, String> {
        let mut out = Vec::new();
        self.diff_trees("", base.copied(), Some(*head), &mut out)?;
        out.sort();
        Ok(out)
    }

    /// The changes between two trees (None: an empty one) at `prefix`, which ends in `/`.
    fn diff_trees(
        &self,
        prefix: &str,
        base: Option<Id>,
        head: Option<Id>,
        out: &mut Vec<(String, Change)>,
    ) -> Result<(), String> {
        let base = self.entries(base)?;
        let head = self.entries(head)?;
        let names: BTreeSet<&String> = base.keys().chain(head.keys()).collect();
        for name in names {
            let (b, h) = (base.get(name), head.get(name));
            let path = format!("{prefix}{name}");
            let tree = |e: Option<&Entry>| match e {
                Some(Entry::Tree(id)) => Some(*id),
                _ => None,
            };
            let file = |e: Option<&Entry>| match e {
                Some(Entry::File(id)) => Some(*id),
                _ => None,
            };
            let (b_tree, h_tree) = (tree(b), tree(h));
            if b_tree != h_tree {
                self.diff_trees(&format!("{path}/"), b_tree, h_tree, out)?;
            }
            match (file(b), file(h)) {
                (Some(x), Some(y)) if x != y => out.push((path, Change::Modified)),
                (Some(_), None) => out.push((path, Change::Removed)),
                (None, Some(_)) => out.push((path, Change::Added)),
                _ => {}
            }
        }
        Ok(())
    }

    /// The entries of the tree `id` by name; none for no tree.
    fn entries(&self, id: Option<Id>) -> Result<BTreeMap<String, Entry>, String> {
        let mut out = BTreeMap::new();
        let Some(id) = id else {
            return Ok(out);
        };
        let bytes = self.object_of_kind(&id, Kind::Tree)?;
        for entry in gix_object::TreeRefIter::from_bytes(&bytes, HashKind::Sha1) {
            let entry = entry.map_err(|e| format!("tree {id}: {e}"))?;
            let kind = if entry.mode.is_tree() {
                Entry::Tree(entry.oid.to_owned())
            } else if is_file(entry.mode) {
                Entry::File(entry.oid.to_owned())
            } else {
                continue;
            };
            out.insert(String::from_utf8_lossy(entry.filename).into_owned(), kind);
        }
        Ok(out)
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

/// How a file differs between two trees (`Repository::changes`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Change {
    Added,
    Removed,
    Modified,
}

/// An entry of a tree that `Repository::changes` looks at: symbolic links and submodules
/// are none.
enum Entry {
    Tree(Id),
    File(Id),
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

/// The name and the time of a signature header, `Name <email> <seconds> ±hhmm`: the name
/// without the space before `<` (as `git log --format=%an`), the time as written.
fn signature(raw: &[u8]) -> Option<(String, &str)> {
    let open = raw.iter().position(|&b| b == b'<')?;
    let close = open + raw[open..].iter().position(|&b| b == b'>')?;
    let name = String::from_utf8_lossy(raw[..open].trim_ascii()).into_owned();
    let time = std::str::from_utf8(&raw[close + 1..]).ok()?.trim();
    Some((name, time))
}

/// The ISO 8601 text of the time of a signature, `<seconds> ±hhmm` as git writes it, in its own
/// offset: `2026-11-20T10:00:00+01:30`, as `git log --format=%aI` (which writes `Z` for an
/// offset 0 where this writes `+00:00`). By hand: the formatter of `gix-date` adds about
/// 200 kB to the WASM module (ADR 0040).
fn iso_date(raw: &str) -> Option<String> {
    let (seconds, offset) = raw.trim().split_once(' ')?;
    let seconds: i64 = seconds.parse().ok()?;
    let (sign, hhmm) = match offset.as_bytes().first()? {
        b'+' => ('+', &offset[1..]),
        b'-' => ('-', &offset[1..]),
        _ => return None,
    };
    if hhmm.len() != 4 || !hhmm.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let (hours, minutes): (i64, i64) = (hhmm[..2].parse().ok()?, hhmm[2..].parse().ok()?);
    let shift = (hours * 3600 + minutes * 60) * if sign == '-' { -1 } else { 1 };
    let local = seconds.checked_add(shift)?;
    let (days, secs) = (local.div_euclid(86_400), local.rem_euclid(86_400));
    let (year, month, day) = civil_from_days(days);
    Some(format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}{sign}{hours:02}:{minutes:02}",
        secs / 3600,
        secs / 60 % 60,
        secs % 60,
    ))
}

/// Year, month and day of a count of days since 1970-01-01, in the proleptic Gregorian
/// calendar (`civil_from_days` of Howard Hinnant).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(month <= 2), month, day)
}
