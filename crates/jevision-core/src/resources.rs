//! Resource labels: a test can name an image or a file by a label, `res:<id>`, instead of by
//! its path, so that the file can be moved or renamed in Finder without breaking the test.
//!
//! The label lives beside the file, in `<file>.jevmeta`, together with what the file was when it
//! got the label: its kind, sha256 hash, size and name. The index of a project is made by
//! reading those sidecars, never the files themselves; a file is hashed only to find one that
//! lost its sidecar, and then only the files of the same size.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::SystemTime;

use serde::Serialize;
use serde::ser::{SerializeMap, Serializer};
use sha2::{Digest, Sha256};

use crate::board::is_image_file;
use crate::decode::{META_FORMAT, parse_meta};
use crate::issue::Issue;
use crate::model::Flow;
use crate::package::visit_files;
use crate::package_files::relative;
use crate::resolve_test;
use crate::validate::placeholders;

/// What a label starts with where a path could be: `res:3f9c2a7b81d04e65`.
pub const RESOURCE_PREFIX: &str = "res:";
/// The ending of the sidecar that holds the label of a file: `login.png.jevmeta`.
pub const META_SUFFIX: &str = ".jevmeta";
/// The newest resource-label format this version reads and writes.
pub const RESOURCE_FORMAT: u32 = META_FORMAT;
pub const META_SCHEMA: &str = "https://dev-pikapik.github.io/JEVision/schemas/jevmeta.schema.json";
/// From this size on, a file is better kept in Git LFS than in the repository itself.
pub const LARGE_RESOURCE_BYTES: u64 = 50 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind {
    Image,
    Table,
    File,
}

pub(crate) const RESOURCE_KINDS: &[(&str, ResourceKind)] = &[
    ("image", ResourceKind::Image),
    ("table", ResourceKind::Table),
    ("file", ResourceKind::File),
];

impl ResourceKind {
    /// The kind of the file `name`, by its ending.
    pub fn of(name: &str) -> Self {
        let ending = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
        if is_image_file(name) {
            Self::Image
        } else if matches!(ending.as_deref(), Some("csv" | "tsv")) {
            Self::Table
        } else {
            Self::File
        }
    }

    fn name(self) -> &'static str {
        RESOURCE_KINDS
            .iter()
            .find(|k| k.1 == self)
            .map_or("file", |k| k.0)
    }
}

/// The content of a `.jevmeta` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceMeta {
    pub id: String,
    pub kind: ResourceKind,
    /// `sha256:` and 64 lowercase hex digits.
    pub hash: String,
    pub size: u64,
    /// The name the file had when it got the label.
    pub name: String,
}

impl Serialize for ResourceMeta {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(Some(7))?;
        map.serialize_entry("$schema", META_SCHEMA)?;
        map.serialize_entry("format", &META_FORMAT)?;
        map.serialize_entry("id", &self.id)?;
        map.serialize_entry("kind", self.kind.name())?;
        map.serialize_entry("hash", &self.hash)?;
        map.serialize_entry("size", &self.size)?;
        map.serialize_entry("name", &self.name)?;
        map.end()
    }
}

impl ResourceMeta {
    /// The text of its `.jevmeta` file, in the one form JEVision writes.
    pub fn to_json(&self) -> String {
        let mut text = serde_json::to_string_pretty(self).expect("a label always serializes");
        text.push('\n');
        text
    }
}

/// Where the file a label names is now. Paths are relative to the project folder, with `/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceStatus {
    /// The file with the label.
    Found { path: String, name: String },
    /// The labeled file is gone, but this file without a label has the same content: most
    /// likely it was moved without its `.jevmeta`. [`restore_resource_label`] labels it again.
    Detached { path: String, name: String },
    /// No file has the label, or its content. `name` is the name the file had, or the label
    /// itself when even its `.jevmeta` is gone.
    Missing { name: String },
}

/// What [`relink_resource`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Relinked {
    /// The file now has the label.
    Labeled,
    /// The file already has another label: the references to the old one should be changed
    /// to this one, `res:…`.
    UseOther { reference: String },
}

/// The id in a label, `3f9c…` in `res:3f9c…`; `None` for a path.
pub fn resource_id(reference: &str) -> Option<&str> {
    reference
        .strip_prefix(RESOURCE_PREFIX)
        .filter(|id| is_resource_id(id))
}

/// 16 lowercase hex digits: 64 random bits.
pub(crate) fn is_resource_id(id: &str) -> bool {
    id.len() == 16 && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

pub(crate) fn is_content_hash(hash: &str) -> bool {
    hash.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    })
}

/// The hash of the content of `path`, as a `.jevmeta` file writes it.
pub(crate) fn content_hash(path: &Path) -> io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 1 << 16];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    let hex: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Ok(format!("sha256:{hex}"))
}

fn new_id(taken: impl Fn(&str) -> bool) -> io::Result<String> {
    loop {
        let mut bytes = [0; 8];
        getrandom::fill(&mut bytes).map_err(|e| io::Error::other(e.to_string()))?;
        let id = format!("{:016x}", u64::from_be_bytes(bytes));
        if !taken(&id) {
            return Ok(id);
        }
    }
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn sidecar(file: &Path) -> PathBuf {
    let mut name = file.as_os_str().to_owned();
    name.push(META_SUFFIX);
    PathBuf::from(name)
}

fn read_meta(file: &Path) -> Option<ResourceMeta> {
    parse_meta(&fs::read_to_string(sidecar(file)).ok()?).ok()
}

fn writable_meta(file: &Path) -> Result<Option<ResourceMeta>, Issue> {
    let path = sidecar(file);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(unreadable(&path.to_string_lossy(), &error)),
    };
    parse_meta(&text).map(Some).map_err(|error| {
        error
            .issues
            .first()
            .cloned()
            .unwrap_or_else(|| {
                unreadable(
                    &path.to_string_lossy(),
                    &io::Error::other(error.to_string()),
                )
            })
            .with("path", path.to_string_lossy())
    })
}

/// Refuses a write when a resource's existing label cannot be read by this version. Call before
/// changing the resource itself, so a malformed or newer label never leads to lost metadata.
pub fn check_resource_write(file: &Path) -> Result<(), Issue> {
    writable_meta(file).map(|_| ())
}

/// The label of `file` as it is now, under the id and name given.
fn describe_file(file: &Path, id: String, name: String) -> io::Result<ResourceMeta> {
    Ok(ResourceMeta {
        kind: ResourceKind::of(&name),
        hash: content_hash(file)?,
        size: fs::metadata(file)?.len(),
        id,
        name,
    })
}

fn modified(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Brings the hash and size in the label of `file` up to date after the file was written
/// again; a file without a label is left alone. `true` when the label changed.
pub fn update_resource_label(file: &Path) -> Result<bool, Issue> {
    let shown = file.to_string_lossy();
    let Some(meta) = writable_meta(file)? else {
        return Ok(false);
    };
    let now = describe_file(file, meta.id.clone(), meta.name.clone())
        .map_err(|e| unreadable(&shown, &e))?;
    if now.hash == meta.hash && now.size == meta.size {
        return Ok(false);
    }
    let now = ResourceMeta {
        kind: meta.kind,
        ..now
    };
    crate::atomic::write(&sidecar(file), now.to_json()).map_err(|e| unwritable(&shown, &e))?;
    Ok(true)
}

#[derive(Debug, Clone)]
struct Label {
    /// The labeled file, relative to the project folder.
    file: String,
    meta: ResourceMeta,
    /// The file is there; when not, only its sidecar is.
    present: bool,
}

/// The labels of a project, from its `.jevmeta` files.
#[derive(Debug, Clone)]
pub(crate) struct ResourceIndex {
    root: PathBuf,
    labels: BTreeMap<String, Label>,
    /// Files without a label, with their sizes, in path order.
    unlabeled: Vec<(String, u64)>,
    /// Hashes worked out so far, by path, for the size and time of change they were for.
    hashes: HashMap<String, (u64, Option<SystemTime>, String)>,
}

/// Every file below `root`, except in hidden folders, relative to it: `(path, size)`.
fn walk(root: &Path, folder: &str, files: &mut Vec<(String, u64)>) {
    let Ok(entries) = fs::read_dir(root.join(folder)) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str().filter(|n| !n.starts_with('.')) else {
            continue;
        };
        let path = if folder.is_empty() {
            name.to_owned()
        } else {
            format!("{folder}/{name}")
        };
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => walk(root, &path, files),
            Ok(kind) if kind.is_file() => {
                files.push((path, entry.metadata().map_or(0, |m| m.len())));
            }
            _ => {}
        }
    }
}

impl ResourceIndex {
    /// Reads the labels of the project in `root`. Two files with the same label are copies:
    /// the one that keeps it is the one `previous` had, then the one still under the name in
    /// the label, then the one made first, then the first by path; every other gets a label of
    /// its own, written to its sidecar.
    pub(crate) fn scan(root: &Path, previous: Option<&ResourceIndex>) -> Self {
        let root = &canonical(root);
        let mut files = Vec::new();
        walk(root, "", &mut files);
        files.sort();
        let sizes: HashMap<&str, u64> = files.iter().map(|(p, s)| (p.as_str(), *s)).collect();
        let mut found: BTreeMap<String, Vec<Label>> = BTreeMap::new();
        for (path, _) in &files {
            let Some(file) = path.strip_suffix(META_SUFFIX) else {
                continue;
            };
            let Some(meta) = read_meta(&root.join(file)) else {
                continue;
            };
            let present = sizes.contains_key(file);
            found.entry(meta.id.clone()).or_default().push(Label {
                file: file.to_owned(),
                meta,
                present,
            });
        }
        let mut index = Self {
            root: root.to_owned(),
            labels: BTreeMap::new(),
            unlabeled: Vec::new(),
            hashes: previous.map(|p| p.hashes.clone()).unwrap_or_default(),
        };
        let mut copies = Vec::new();
        for (id, mut labels) in found {
            if labels.iter().any(|l| l.present) {
                labels.retain(|l| l.present);
            }
            let before = previous.and_then(|p| p.labels.get(&id));
            labels.sort_by_cached_key(|l| {
                let created = fs::metadata(root.join(&l.file)).and_then(|m| m.created());
                (
                    before.is_none_or(|b| b.file != l.file),
                    file_name(&l.file) != l.meta.name,
                    created.is_err(),
                    created.ok(),
                    l.file.clone(),
                )
            });
            let mut labels = labels.into_iter();
            let keeper = labels.next().expect("every id was found in a sidecar");
            copies.extend(labels.filter(|l| l.present));
            index.labels.insert(id, keeper);
        }
        for copy in copies {
            let file = root.join(&copy.file);
            let relabeled = new_id(|id| index.labels.contains_key(id)).and_then(|id| {
                let meta = ResourceMeta {
                    id,
                    name: file_name(&copy.file).to_owned(),
                    ..copy.meta
                };
                crate::atomic::write(&sidecar(&file), meta.to_json())?;
                Ok(meta)
            });
            if let Ok(meta) = relabeled {
                index.labels.insert(
                    meta.id.clone(),
                    Label {
                        file: copy.file,
                        meta,
                        present: true,
                    },
                );
            }
        }
        // A file written again after it got its label: the label says what it is now.
        let rewritten: Vec<(String, String)> = (index.labels.iter())
            .filter(|(_, l)| {
                let file = root.join(&l.file);
                l.present && modified(&file) > modified(&sidecar(&file))
            })
            .map(|(id, l)| (id.clone(), l.file.clone()))
            .collect();
        for (id, path) in rewritten {
            let (Some(hash), Some(&size)) = (index.hash(&path), sizes.get(path.as_str())) else {
                continue;
            };
            let label = index.labels.get_mut(&id).expect("listed above");
            if hash != label.meta.hash || size != label.meta.size {
                let meta = ResourceMeta {
                    hash,
                    size,
                    ..label.meta.clone()
                };
                if crate::atomic::write(&sidecar(&root.join(&path)), meta.to_json()).is_ok() {
                    label.meta = meta;
                }
            }
        }
        let labeled: HashSet<&str> = index
            .labels
            .values()
            .filter(|l| l.present)
            .map(|l| l.file.as_str())
            .collect();
        index.unlabeled = files
            .iter()
            .filter(|(p, _)| !p.ends_with(META_SUFFIX) && !labeled.contains(p.as_str()))
            .filter(|(p, _)| !sizes.contains_key(format!("{p}{META_SUFFIX}").as_str()))
            .cloned()
            .collect();
        index
    }

    /// The label `id` still names the file the index has for it.
    fn fresh(&self, id: &str) -> bool {
        self.labels.get(id).is_some_and(|l| {
            let file = self.root.join(&l.file);
            l.present && file.is_file() && read_meta(&file).is_some_and(|m| m.id == id)
        })
    }

    pub(crate) fn resolve(&mut self, id: &str) -> ResourceStatus {
        let Some(label) = self.labels.get(id).cloned() else {
            return ResourceStatus::Missing {
                name: format!("{RESOURCE_PREFIX}{id}"),
            };
        };
        let name = label.meta.name.clone();
        if label.present {
            return ResourceStatus::Found {
                path: label.file,
                name,
            };
        }
        match self.same_content(&label.meta) {
            Some(path) => ResourceStatus::Detached { path, name },
            None => ResourceStatus::Missing { name },
        }
    }

    /// The first file without a label whose content is the one `meta` describes.
    fn same_content(&mut self, meta: &ResourceMeta) -> Option<String> {
        let candidates: Vec<String> = self
            .unlabeled
            .iter()
            .filter(|(_, size)| *size == meta.size)
            .map(|(path, _)| path.clone())
            .collect();
        candidates
            .into_iter()
            .find(|path| self.hash(path).as_deref() == Some(meta.hash.as_str()))
    }

    fn hash(&mut self, path: &str) -> Option<String> {
        let file = self.root.join(path);
        let size = fs::metadata(&file).ok()?.len();
        let changed = modified(&file);
        if let Some((s, c, hash)) = self.hashes.get(path)
            && *s == size
            && *c == changed
        {
            return Some(hash.clone());
        }
        let hash = content_hash(&file).ok()?;
        self.hashes
            .insert(path.to_owned(), (size, changed, hash.clone()));
        Some(hash)
    }

    /// `file`, given relative to the project folder or not, as a path in it; refused when it
    /// is outside, reached through `..` or a link that leads out, in a hidden folder, or not a
    /// file.
    pub(crate) fn inside(&self, file: &Path) -> Result<String, Issue> {
        let shown = file.to_string_lossy();
        let outside = || Issue::error("resource.outside").with("path", &shown);
        if file.components().any(|c| c == Component::ParentDir) {
            return Err(outside());
        }
        let real = fs::canonicalize(self.root.join(file))
            .map_err(|_| Issue::error("resource.notFound").with("path", &shown))?;
        let path = real.strip_prefix(&self.root).map_err(|_| outside())?;
        let parts: Option<Vec<&str>> = path.iter().map(|p| p.to_str()).collect();
        let parts = parts.filter(|p| !p.is_empty()).ok_or_else(outside)?;
        if parts.iter().any(|p| p.starts_with('.')) {
            return Err(outside());
        }
        if !real.is_file() || parts.last().is_some_and(|p| p.ends_with(META_SUFFIX)) {
            return Err(Issue::error("resource.notFound").with("path", &shown));
        }
        Ok(parts.join("/"))
    }

    /// Gives `file` a label, or tells the one it has.
    pub(crate) fn label(&mut self, file: &Path) -> Result<String, Issue> {
        let path = self.inside(file)?;
        let full = self.root.join(&path);
        if let Some(meta) = writable_meta(&full)? {
            let id = meta.id.clone();
            self.set(id.clone(), path, meta);
            return Ok(id);
        }
        let id = new_id(|id| self.labels.contains_key(id)).map_err(|e| unwritable(&path, &e))?;
        let meta = describe_file(&full, id.clone(), file_name(&path).to_owned())
            .map_err(|e| unreadable(&path, &e))?;
        crate::atomic::write(&sidecar(&full), meta.to_json()).map_err(|e| unwritable(&path, &e))?;
        self.set(id.clone(), path, meta);
        Ok(id)
    }

    /// Moves the label `id` to `file`, which has none or has this one: its sidecar is written
    /// and the one the label had before is removed. The name in the label stays when `keep_name`.
    fn attach(&mut self, id: &str, path: String, keep_name: bool) -> Result<(), Issue> {
        let full = self.root.join(&path);
        check_resource_write(&full)?;
        let before = self.labels.get(id).cloned();
        let name = match &before {
            Some(b) if keep_name => b.meta.name.clone(),
            _ => file_name(&path).to_owned(),
        };
        let meta = describe_file(&full, id.to_owned(), name).map_err(|e| unreadable(&path, &e))?;
        crate::atomic::write(&sidecar(&full), meta.to_json()).map_err(|e| unwritable(&path, &e))?;
        if let Some(before) = before.filter(|b| b.file != path) {
            let old = sidecar(&self.root.join(&before.file));
            if read_meta(&self.root.join(&before.file)).is_some_and(|m| m.id == id) {
                let _ = fs::remove_file(old);
            }
            if before.present {
                let size = fs::metadata(self.root.join(&before.file)).map_or(0, |m| m.len());
                self.unlabeled.push((before.file, size));
                self.unlabeled.sort();
            }
        }
        self.set(id.to_owned(), path, meta);
        Ok(())
    }

    fn set(&mut self, id: String, file: String, meta: ResourceMeta) {
        self.unlabeled.retain(|(p, _)| *p != file);
        self.labels.insert(
            id,
            Label {
                file,
                meta,
                present: true,
            },
        );
    }

    /// The file in the project that `named`, a path written in the test `test`, is, when it is
    /// one that can have a label.
    pub(crate) fn labelable(&self, test: &str, named: &str) -> Option<String> {
        if resource_id(named).is_some() || !placeholders(named).is_empty() {
            return None;
        }
        let file = resolve_test(test, named)?;
        self.inside(Path::new(&file)).ok()
    }

    /// The id in the label of `path`, a file in the project.
    pub(crate) fn label_of(&self, path: &str) -> Option<String> {
        read_meta(&self.root.join(path)).map(|m| m.id)
    }

    /// The label of a file, other than `id`.
    fn other_label(&self, path: &str, id: &str) -> Option<String> {
        read_meta(&self.root.join(path))
            .map(|m| m.id)
            .filter(|other| other != id)
    }
}

fn unreadable(path: &str, e: &io::Error) -> Issue {
    Issue::error("resource.unreadable")
        .with("path", path)
        .with("reason", e)
}

fn unwritable(path: &str, e: &io::Error) -> Issue {
    Issue::error("resource.unwritable")
        .with("path", path)
        .with("reason", e)
}

/// The index of every project looked at, by its folder.
static INDEXES: Mutex<BTreeMap<PathBuf, ResourceIndex>> = Mutex::new(BTreeMap::new());

fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_owned())
}

/// Runs `f` on the index of `project`, read again first when it is new or no longer right
/// about one of `ids`: a file moved or renamed since, or a label made somewhere else.
fn with_index<T>(project: &Path, ids: &[&str], f: impl FnOnce(&mut ResourceIndex) -> T) -> T {
    let root = canonical(project);
    let mut indexes = INDEXES.lock().unwrap_or_else(PoisonError::into_inner);
    let stale = indexes
        .get(&root)
        .is_none_or(|index| ids.iter().any(|id| !index.fresh(id)));
    if stale {
        let previous = indexes.remove(&root);
        let index = ResourceIndex::scan(&root, previous.as_ref());
        indexes.insert(root.clone(), index);
    }
    f(indexes.get_mut(&root).expect("the index was just made"))
}

/// The project a test in `folder` belongs to: a project whose labels were looked at already,
/// else the nearest folder up with `jevision.project.json`, `.jevision` or `.git`, else
/// `folder` itself.
pub(crate) fn project_of(folder: &Path) -> PathBuf {
    let folder = canonical(folder);
    let known = INDEXES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .keys()
        .filter(|root| folder.starts_with(root))
        .max_by_key(|root| root.as_os_str().len())
        .cloned();
    known.unwrap_or_else(|| {
        folder
            .ancestors()
            .find(|f| {
                [crate::PROJECT_FILE, ".jevision", ".git"]
                    .iter()
                    .any(|marker| f.join(marker).exists())
            })
            .unwrap_or(&folder)
            .to_owned()
    })
}

/// Where the file `reference` names is in `project`; `None` when `reference` is a path.
pub fn resolve_resource(project: &Path, reference: &str) -> Option<ResourceStatus> {
    let id = resource_id(reference)?;
    Some(with_index(project, &[id], |index| index.resolve(id)))
}

/// Gives `file`, in `project`, a label and returns it as `res:…`; a file that has one keeps it.
pub fn label_resource(project: &Path, file: &Path) -> Result<String, Issue> {
    with_index(project, &[], |index| index.label(file)).map(|id| format!("{RESOURCE_PREFIX}{id}"))
}

/// Labels `file` again with `reference` after it lost its `.jevmeta`, which is what
/// [`ResourceStatus::Detached`] offers; the sidecar left where the file was is removed.
pub fn restore_resource_label(project: &Path, reference: &str, file: &Path) -> Result<(), Issue> {
    let id = resource_id(reference)
        .ok_or_else(|| Issue::error("resource.invalid").with("value", reference))?;
    with_index(project, &[id], |index| {
        let path = index.inside(file)?;
        if let Some(other) = index.other_label(&path, id) {
            return Err(Issue::error("resource.labeled")
                .with("path", &path)
                .with("id", format!("{RESOURCE_PREFIX}{other}")));
        }
        index.attach(id, path, true)
    })
}

/// Makes `reference` name `file` from now on, for a file that is missing and was found
/// elsewhere by hand. A file with a label of its own keeps it, and the tests should use that.
pub fn relink_resource(project: &Path, reference: &str, file: &Path) -> Result<Relinked, Issue> {
    let id = resource_id(reference)
        .ok_or_else(|| Issue::error("resource.invalid").with("value", reference))?;
    with_index(project, &[id], |index| {
        let path = index.inside(file)?;
        match index.other_label(&path, id) {
            Some(other) => Ok(Relinked::UseOther {
                reference: format!("{RESOURCE_PREFIX}{other}"),
            }),
            None => index.attach(id, path, false).map(|()| Relinked::Labeled),
        }
    })
}

/// Reads the labels of `project` again, for a project changed outside the app.
pub fn refresh_resources(project: &Path) {
    let root = canonical(project);
    let mut indexes = INDEXES.lock().unwrap_or_else(PoisonError::into_inner);
    let previous = indexes.remove(&root);
    let index = ResourceIndex::scan(&root, previous.as_ref());
    indexes.insert(root, index);
}

/// Takes the label `reference` off its file again, for a label made for a test that was then
/// saved with plain paths: its `.jevmeta` goes, unless a test of `project` still names it.
/// `true` when it went.
pub fn remove_resource_label(project: &Path, reference: &str) -> Result<bool, Issue> {
    let id = resource_id(reference)
        .ok_or_else(|| Issue::error("resource.invalid").with("value", reference))?;
    let quoted = format!("\"{RESOURCE_PREFIX}{id}\"");
    let named = (crate::compat::test_files(project).iter()).any(|test| {
        (fs::read_to_string(project.join(test)).ok()).is_none_or(|t| t.contains(&quoted))
    });
    if named {
        return Ok(false);
    }
    with_index(project, &[id], |index| {
        let Some(label) = index.labels.get(id).filter(|l| l.present).cloned() else {
            return Ok(false);
        };
        let file = index.root.join(&label.file);
        if read_meta(&file).is_none_or(|m| m.id != id) {
            return Ok(false);
        }
        fs::remove_file(sidecar(&file)).map_err(|e| unwritable(&label.file, &e))?;
        index.labels.remove(id);
        let size = fs::metadata(&file).map_or(0, |m| m.len());
        index.unlabeled.push((label.file, size));
        index.unlabeled.sort();
        Ok(true)
    })
}

/// The name of the file the label `reference` names, for a place that knows no project: its
/// name now, in a project whose labels were read already, or the name in the label when the
/// file is missing. `None` when none of those projects has the label, or for a path.
pub fn resource_name(reference: &str) -> Option<String> {
    let id = resource_id(reference)?;
    let indexes = INDEXES.lock().unwrap_or_else(PoisonError::into_inner);
    let label = indexes.values().find_map(|index| index.labels.get(id))?;
    Some(if label.present {
        file_name(&label.file).to_owned()
    } else {
        label.meta.name.clone()
    })
}

/// Puts in `flow`, the test `test` of `project`, the label of each file it names by its path,
/// labeling the files that have none.
pub fn label_files(project: &Path, test: &str, flow: &mut Flow) -> Result<(), Issue> {
    with_index(project, &[], |index| {
        let mut error = None;
        visit_files(flow, |_, _, named| {
            let Some(file) = index.labelable(test, named).filter(|_| error.is_none()) else {
                return;
            };
            match index.label(Path::new(&file)) {
                Ok(id) => *named = format!("{RESOURCE_PREFIX}{id}"),
                Err(issue) => error = Some(issue),
            }
        });
        error.map_or(Ok(()), Err)
    })
}

/// The labels of a test, after [`resolve_labels`] put paths in their place.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResolvedLabels {
    /// Labels whose file is missing, was found without its label or is large.
    pub issues: Vec<Issue>,
    /// `(block, label, path)` for every label replaced, the path relative to the test's folder.
    pub paths: Vec<(String, String, String)>,
}

/// Puts the path of each labeled file in place of its label in `flow`, a test in `folder`,
/// relative to that folder. A label whose file is missing stays as it is, with a
/// `resource.missing` problem; one found only by its content gets a `resource.detached`
/// warning that names where. `project` is the project folder when known.
pub fn resolve_labels(flow: &mut Flow, folder: &Path, project: Option<&Path>) -> ResolvedLabels {
    let mut ids = Vec::new();
    visit_files(flow, |_, _, path| {
        if let Some(id) = resource_id(path) {
            ids.push(id.to_owned());
        }
    });
    let mut out = ResolvedLabels::default();
    if ids.is_empty() {
        return out;
    }
    let root = project.map_or_else(|| project_of(folder), canonical);
    let from = canonical(folder)
        .strip_prefix(&root)
        .ok()
        .and_then(|p| p.to_str())
        .map(|p| {
            if p.is_empty() {
                "-".to_owned()
            } else {
                format!("{p}/-")
            }
        });
    let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
    with_index(&root, &ids, |index| {
        visit_files(flow, |block, _, path| {
            let Some(id) = resource_id(path) else {
                return;
            };
            let reference = path.clone();
            let issue = |issue: Issue| issue.with("block", block).with("id", &reference);
            let (found, name) = match index.resolve(id) {
                ResourceStatus::Found { path, name } => (path, name),
                ResourceStatus::Detached { path, name } => {
                    out.issues.push(
                        issue(Issue::warning("resource.detached"))
                            .with("name", &name)
                            .with("path", &path),
                    );
                    (path, name)
                }
                ResourceStatus::Missing { name } => {
                    out.issues
                        .push(issue(Issue::error("resource.missing")).with("name", name));
                    return;
                }
            };
            let size = fs::metadata(index.root.join(&found)).map_or(0, |m| m.len());
            if size >= LARGE_RESOURCE_BYTES {
                out.issues.push(
                    issue(Issue::warning("resource.large"))
                        .with("name", &name)
                        .with("size", size),
                );
            }
            *path = match &from {
                Some(from) => relative(from, &found),
                None => index.root.join(&found).to_string_lossy().into_owned(),
            };
            out.paths.push((block.to_owned(), reference, path.clone()));
        });
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BlockKind, Reference, Severity, Target, parse};

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("jevision-resources-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        canonical(&dir)
    }

    fn put(root: &Path, path: &str, bytes: &[u8]) {
        let file = root.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, bytes).unwrap();
    }

    fn rename(root: &Path, from: &str, to: &str) {
        fs::create_dir_all(root.join(to).parent().unwrap()).unwrap();
        fs::rename(root.join(from), root.join(to)).unwrap();
    }

    fn label(root: &Path, path: &str) -> String {
        label_resource(root, Path::new(path)).unwrap()
    }

    fn found(path: &str, name: &str) -> Option<ResourceStatus> {
        Some(ResourceStatus::Found {
            path: path.to_owned(),
            name: name.to_owned(),
        })
    }

    fn id_in(root: &Path, file: &str) -> String {
        read_meta(&root.join(file)).unwrap().id
    }

    #[test]
    fn a_label_is_written_beside_its_file_in_one_canonical_form() {
        let root = temp("canonical");
        put(&root, "images/go.png", b"go");
        let reference = label(&root, "images/go.png");
        let id = resource_id(&reference).unwrap();
        let hash = content_hash(&root.join("images/go.png")).unwrap();
        assert_eq!(
            fs::read_to_string(root.join("images/go.png.jevmeta")).unwrap(),
            format!(
                "{{\n  \"$schema\": \"{META_SCHEMA}\",\n  \"format\": 1,\n  \"id\": \"{id}\",\n  \
                 \"kind\": \"image\",\n  \"hash\": \"{hash}\",\n  \"size\": 2,\n  \
                 \"name\": \"go.png\"\n}}\n"
            )
        );
        assert_eq!(
            label(&root, "images/go.png"),
            reference,
            "a label is made once"
        );
        assert_eq!(ResourceKind::of("users.CSV"), ResourceKind::Table);
        assert_eq!(ResourceKind::of("notes.txt"), ResourceKind::File);
    }

    #[test]
    fn the_hash_is_sha256_of_the_content() {
        let root = temp("hash");
        put(&root, "a.txt", b"abc");
        assert_eq!(
            content_hash(&root.join("a.txt")).unwrap(),
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn a_file_moved_or_renamed_with_its_label_is_found() {
        let root = temp("moved");
        put(&root, "images/go.png", b"go");
        let reference = label(&root, "images/go.png");
        rename(&root, "images/go.png", "pictures/go.png");
        rename(&root, "images/go.png.jevmeta", "pictures/go.png.jevmeta");
        assert_eq!(
            resolve_resource(&root, &reference),
            found("pictures/go.png", "go.png")
        );
        rename(&root, "pictures/go.png", "pictures/start.png");
        rename(
            &root,
            "pictures/go.png.jevmeta",
            "pictures/start.png.jevmeta",
        );
        assert_eq!(
            resolve_resource(&root, &reference),
            found("pictures/start.png", "go.png")
        );
        assert_eq!(resolve_resource(&root, "pictures/start.png"), None);
    }

    #[test]
    fn a_file_moved_without_its_label_is_found_by_content_and_labeled_again() {
        let root = temp("detached");
        put(&root, "images/go.png", b"go");
        put(&root, "images/no.png", b"no");
        let reference = label(&root, "images/go.png");
        rename(&root, "images/go.png", "elsewhere/moved.png");
        let detached = ResourceStatus::Detached {
            path: "elsewhere/moved.png".to_owned(),
            name: "go.png".to_owned(),
        };
        assert_eq!(resolve_resource(&root, &reference), Some(detached));

        restore_resource_label(&root, &reference, Path::new("elsewhere/moved.png")).unwrap();
        assert_eq!(
            resolve_resource(&root, &reference),
            found("elsewhere/moved.png", "go.png")
        );
        assert!(
            !root.join("images/go.png.jevmeta").exists(),
            "the old one is gone"
        );
        refresh_resources(&root);
        assert_eq!(
            resolve_resource(&root, &reference),
            found("elsewhere/moved.png", "go.png")
        );
    }

    #[test]
    fn a_deleted_file_is_missing_by_the_name_it_had() {
        let root = temp("deleted");
        put(&root, "images/go.png", b"go");
        let reference = label(&root, "images/go.png");
        fs::remove_file(root.join("images/go.png")).unwrap();
        let missing = |name: &str| {
            Some(ResourceStatus::Missing {
                name: name.to_owned(),
            })
        };
        assert_eq!(resolve_resource(&root, &reference), missing("go.png"));
        fs::remove_file(root.join("images/go.png.jevmeta")).unwrap();
        assert_eq!(resolve_resource(&root, &reference), missing(&reference));
    }

    #[test]
    fn a_copy_with_its_label_gets_a_label_of_its_own() {
        let root = temp("copy");
        put(&root, "images/go.png", b"go");
        let reference = label(&root, "images/go.png");
        put(&root, "copies/go.png", b"go");
        fs::copy(
            root.join("images/go.png.jevmeta"),
            root.join("copies/go.png.jevmeta"),
        )
        .unwrap();
        refresh_resources(&root);
        assert_eq!(
            resolve_resource(&root, &reference),
            found("images/go.png", "go.png")
        );
        let copy = id_in(&root, "copies/go.png");
        assert_ne!(Some(copy.as_str()), resource_id(&reference));
        assert_eq!(
            resolve_resource(&root, &format!("{RESOURCE_PREFIX}{copy}")),
            found("copies/go.png", "go.png")
        );
    }

    #[test]
    fn copies_keep_the_label_in_the_order_the_rules_say() {
        let root = temp("rules");
        put(&root, "b/go.png", b"go");
        let id = resource_id(&label(&root, "b/go.png")).unwrap().to_owned();
        let before = ResourceIndex::scan(&root, None);
        let copy = |to: &str| {
            put(&root, to, b"go");
            let sidecar = root.join(format!("{to}{META_SUFFIX}"));
            fs::copy(root.join("b/go.png.jevmeta"), sidecar).unwrap();
        };
        let holder = |index: &ResourceIndex| index.labels[&id].file.clone();

        // The file under the name in the label keeps it, though a renamed copy comes first.
        copy("a/renamed.png");
        let index = ResourceIndex::scan(&root, Some(&before));
        assert_eq!(holder(&index), "b/go.png");
        let other = id_in(&root, "a/renamed.png");
        assert_ne!(other, id);
        assert_eq!(index.labels[&other].file, "a/renamed.png");
        assert_eq!(
            read_meta(&root.join("a/renamed.png")).unwrap().name,
            "renamed.png"
        );

        // Above that, the file that had the label the last time keeps it.
        copy("a/renamed.png");
        let mut moved = before.clone();
        moved.labels.get_mut(&id).unwrap().file = "a/renamed.png".to_owned();
        let index = ResourceIndex::scan(&root, Some(&moved));
        assert_eq!(holder(&index), "a/renamed.png");
        assert_ne!(id_in(&root, "b/go.png"), id);
    }

    #[test]
    fn relinking_a_missing_label() {
        let root = temp("relink");
        put(&root, "images/go.png", b"go");
        put(&root, "images/start.png", b"start");
        put(&root, "images/other.png", b"other");
        let reference = label(&root, "images/go.png");
        let other = label(&root, "images/other.png");
        fs::remove_file(root.join("images/go.png")).unwrap();

        let relinked = relink_resource(&root, &reference, Path::new("images/start.png"));
        assert_eq!(relinked, Ok(Relinked::Labeled));
        assert_eq!(
            resolve_resource(&root, &reference),
            found("images/start.png", "start.png")
        );
        assert!(!root.join("images/go.png.jevmeta").exists());

        let relinked = relink_resource(&root, &reference, Path::new("images/other.png"));
        assert_eq!(relinked, Ok(Relinked::UseOther { reference: other }));
        assert_eq!(
            resolve_resource(&root, &reference),
            found("images/start.png", "start.png"),
            "nothing changed"
        );

        let outside = temp("relink-outside").join("x.png");
        fs::write(&outside, b"x").unwrap();
        let refused = relink_resource(&root, &reference, &outside).unwrap_err();
        assert_eq!(refused.code, "resource.outside");
    }

    #[test]
    fn files_outside_the_project_get_no_label() {
        let root = temp("outside");
        let elsewhere = temp("outside-elsewhere");
        put(&elsewhere, "x.png", b"x");
        put(&root, "inside/y.png", b"y");
        put(&root, ".hidden/z.png", b"z");
        std::os::unix::fs::symlink(elsewhere.join("x.png"), root.join("link.png")).unwrap();
        for file in [
            elsewhere.join("x.png"),
            PathBuf::from("../outside-elsewhere/x.png"),
            PathBuf::from("inside/../inside/y.png"),
            PathBuf::from("link.png"),
            PathBuf::from(".hidden/z.png"),
        ] {
            let refused = label_resource(&root, &file).unwrap_err();
            assert_eq!(refused.code, "resource.outside", "{file:?}");
        }
        let refused = label_resource(&root, Path::new("none.png")).unwrap_err();
        assert_eq!(refused.code, "resource.notFound");
        assert!(!elsewhere.join("x.png.jevmeta").exists());
    }

    #[test]
    fn a_file_written_again_gets_its_label_up_to_date() {
        let root = temp("rewritten");
        put(&root, "a.png", b"one");
        let reference = label(&root, "a.png");
        put(&root, "a.png", b"three");
        assert_eq!(update_resource_label(&root.join("a.png")), Ok(true));
        let meta = read_meta(&root.join("a.png")).unwrap();
        assert_eq!(
            (meta.size, Some(meta.id.as_str())),
            (5, resource_id(&reference))
        );
        assert_eq!(update_resource_label(&root.join("a.png")), Ok(false));
        assert_eq!(update_resource_label(&root.join("none.png")), Ok(false));
    }

    #[test]
    fn a_newer_or_broken_label_is_preserved_when_a_resource_is_edited_or_labeled() {
        for (name, contents, code) in [
            ("future", r#"{"format":9999}"#, "flow.format.tooNew"),
            (
                "version",
                r#"{"format":1,"minVersion":"9999.0.0"}"#,
                "flow.needsNewer",
            ),
            ("broken", "{", "flow.json.invalid"),
        ] {
            let root = temp(name);
            put(&root, "data/users.csv", b"name\nAnn\n");
            let file = root.join("data/users.csv");
            let meta = sidecar(&file);
            fs::write(&meta, contents).unwrap();
            let check = check_resource_write(&file).unwrap_err();
            if name != "broken" {
                assert_eq!(check.code, code);
            }
            assert!(label_resource(&root, &file).is_err());
            assert!(update_resource_label(&file).is_err());
            assert_eq!(fs::read_to_string(&meta).unwrap(), contents);
            assert_eq!(fs::read(&file).unwrap(), b"name\nAnn\n");
        }
    }

    #[test]
    fn relinking_does_not_replace_a_newer_label_on_the_destination() {
        let root = temp("future-destination");
        put(&root, "old.csv", b"name\nAnn\n");
        let reference = label(&root, "old.csv");
        let original = fs::read(sidecar(&root.join("old.csv"))).unwrap();
        put(&root, "new.csv", b"name\nBo\n");
        let future = r#"{"format":9999}"#;
        fs::write(sidecar(&root.join("new.csv")), future).unwrap();
        assert!(relink_resource(&root, &reference, Path::new("new.csv")).is_err());
        assert_eq!(fs::read(sidecar(&root.join("old.csv"))).unwrap(), original);
        assert_eq!(
            fs::read_to_string(sidecar(&root.join("new.csv"))).unwrap(),
            future
        );
    }

    #[test]
    fn labels_in_a_test_become_paths_from_its_folder() {
        let root = temp("flow");
        fs::create_dir_all(root.join(".git")).unwrap();
        put(&root, "images/go.png", b"go");
        put(&root, "images/ok.png", b"ok");
        let go = label(&root, "images/go.png");
        let ok = label(&root, "images/ok.png");
        rename(&root, "images/ok.png", "moved/ok.png");
        let gone = "res:0123456789abcdef";
        let mut flow = parse(&format!(
            r#"{{"format":2,"name":"T","app":"com.example.App","start":"a","blocks":{{
            "a":{{"type":"click","target":{{"image":"{go}"}},"next":"b"}},
            "b":{{"type":"check","reference":{{"image":"{ok}"}},"next":"c"}},
            "c":{{"type":"click","target":{{"image":"{gone}"}}}}}}}}"#
        ))
        .unwrap();
        let labels = resolve_labels(&mut flow, &root.join("tests"), None);
        let image = |id: &str| match &flow.blocks.iter().find(|b| b.id == id).unwrap().kind {
            BlockKind::Click {
                target: Target::Image(path),
                ..
            } => path.clone(),
            BlockKind::Check {
                of: crate::CheckOf::Screen(Reference { image, .. }),
                ..
            } => image.clone(),
            _ => unreachable!(),
        };
        assert_eq!(image("a"), "../images/go.png");
        assert_eq!(image("b"), "../moved/ok.png");
        assert_eq!(image("c"), gone, "a missing label stays");
        let codes: Vec<(&str, Severity, Option<&str>)> = (labels.issues.iter())
            .map(|i| (i.code.as_str(), i.severity, i.param("block")))
            .collect();
        assert_eq!(
            codes,
            [
                ("resource.detached", Severity::Warning, Some("b")),
                ("resource.missing", Severity::Error, Some("c")),
            ]
        );
        assert_eq!(labels.issues[0].param("path"), Some("moved/ok.png"));
        assert_eq!(
            labels.paths[0],
            ("a".to_owned(), go.clone(), "../images/go.png".to_owned())
        );
    }

    #[test]
    fn a_large_file_suggests_git_lfs() {
        let root = temp("large");
        let file = root.join("video.mov");
        fs::File::create(&file)
            .unwrap()
            .set_len(LARGE_RESOURCE_BYTES)
            .unwrap();
        let reference = label(&root, "video.mov");
        let mut flow = parse(&format!(
            r#"{{"format":2,"name":"T","app":"com.example.App","start":"a","blocks":{{
            "a":{{"type":"openFile","files":["{reference}"]}}}}}}"#
        ))
        .unwrap();
        let labels = resolve_labels(&mut flow, &root, Some(&root));
        assert_eq!(labels.issues.len(), 1);
        assert_eq!(labels.issues[0].code, "resource.large");
        assert_eq!(labels.issues[0].severity, Severity::Warning);
    }

    #[test]
    fn a_label_no_test_names_is_taken_off_its_file() {
        let root = temp("remove");
        put(&root, "images/go.png", b"go");
        put(&root, "images/ok.png", b"ok");
        let go = label(&root, "images/go.png");
        let ok = label(&root, "images/ok.png");
        let test =
            format!(r#"{{"blocks":{{"a":{{"type":"click","target":{{"image":"{ok}"}}}}}}}}"#);
        put(&root, "tests/a.flow.json", test.as_bytes());
        assert_eq!(
            remove_resource_label(&root, &ok),
            Ok(false),
            "a test names it"
        );
        assert!(root.join("images/ok.png.jevmeta").is_file());
        assert_eq!(remove_resource_label(&root, &go), Ok(true));
        assert!(!root.join("images/go.png.jevmeta").exists());
        assert!(root.join("images/go.png").is_file(), "the file stays");
        assert!(matches!(
            resolve_resource(&root, &go),
            Some(ResourceStatus::Missing { .. })
        ));
        assert_eq!(remove_resource_label(&root, &go), Ok(false), "already gone");
        assert!(remove_resource_label(&root, "images/go.png").is_err());
    }

    #[test]
    fn a_label_is_named_by_its_file_in_the_projects_read() {
        assert_eq!(
            resource_name("res:89abcdef01234567"),
            None,
            "no project has it"
        );
        assert_eq!(resource_name("images/go.png"), None);
        let root = temp("name");
        put(&root, "images/go.png", b"go");
        let go = label(&root, "images/go.png");
        assert_eq!(resource_name(&go).as_deref(), Some("go.png"));
        rename(&root, "images/go.png", "moved/start.png");
        rename(&root, "images/go.png.jevmeta", "moved/start.png.jevmeta");
        refresh_resources(&root);
        assert_eq!(resource_name(&go).as_deref(), Some("start.png"));
        fs::remove_file(root.join("moved/start.png")).unwrap();
        refresh_resources(&root);
        assert_eq!(
            resource_name(&go).as_deref(),
            Some("go.png"),
            "a missing file goes by the name in its label"
        );
    }
}
