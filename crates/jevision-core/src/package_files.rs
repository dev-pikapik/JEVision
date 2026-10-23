//! Block packages on disk: reading a package folder, installing one into a project, listing and
//! removing what is installed, finding the tests that use a package, and writing a project's own
//! block as a package. The format itself is in [`crate::package`].
//!
//! Nothing here runs anything: a package is read as data, copied file by file without the
//! permission to run them, and checked before it is let into the project.

use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::{fs, io};

use jevision_l10n::tr;
use serde_json::json;

use crate::issue::{Issue, Severity};
use crate::model::{BLOCK_SUFFIX, BlockKind, Flow, TEST_SUFFIX, dark_variant};
use crate::package::{
    BlockManifest, MANIFEST_FILE, PACKAGE_FLOW_FILE, PACKAGE_LOCK_FILE, PACKAGE_SUFFIX,
    PackageError, PackageInput, PackageLock, Version, app_version, check_package, is_package_id,
    package_of_path, parse_manifest, unmet, visit_files,
};
use crate::validate::{is_relative_path, placeholders};
use crate::{parse, resolve_test, to_canonical_json};

/// Most bytes a package may hold, in all its files.
pub const MAX_PACKAGE_BYTES: u64 = 64 << 20;
/// Most files a package may hold.
pub const MAX_PACKAGE_FILES: usize = 1000;
/// Folder of a project where its own blocks and the installed packages are kept.
pub const BLOCKS_FOLDER: &str = "blocks";
const MAX_DEPTH: usize = 8;
const MAX_TEXT_FILE: u64 = 4 << 20;

/// A file of a package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageFile {
    /// Relative to the package folder, written with `/`.
    pub path: String,
    pub size: u64,
}

/// A package read from a folder.
#[derive(Debug, Clone)]
pub struct Package {
    pub manifest: BlockManifest,
    pub flow: Flow,
    /// Every file that would be installed, sorted by path.
    pub files: Vec<PackageFile>,
    pub bytes: u64,
    /// Everything [`check_package`] found; the package can't be installed with an error here.
    pub issues: Vec<Issue>,
    /// Links to other files found in the folder, by path, sorted. They are never installed:
    /// one could point anywhere on the computer. [`skipped_links_warning`] tells about them.
    pub skipped_links: Vec<String>,
}

impl Package {
    pub fn has_errors(&self) -> bool {
        self.issues.iter().any(|i| i.severity == Severity::Error)
    }
}

/// A package installed in a project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub manifest: BlockManifest,
    /// The folder, relative to the project: `blocks/ID.jevblock`.
    pub folder: String,
    /// The address it was installed from, if it was installed from one.
    pub source: Option<String>,
}

/// How a package compares with the one of the same id that is installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// Nothing with this id is installed.
    New,
    /// The same version is installed.
    Same,
    /// An older version is installed.
    Update { from: Version },
    /// A newer version is installed.
    Downgrade { from: Version },
}

fn failure(code: &str, params: &[(&str, &str)]) -> PackageError {
    PackageError::new(code, params)
}

fn io_failure(code: &str, id: &str, e: &io::Error) -> PackageError {
    failure(code, &[("id", id), ("reason", &e.to_string())])
}

/// The folder of the package `id` in `project`.
pub fn package_folder(project: &Path, id: &str) -> PathBuf {
    project
        .join(BLOCKS_FOLDER)
        .join(format!("{id}{PACKAGE_SUFFIX}"))
}

/// The file with the steps of the package `id`, relative to the project, as a test names it.
pub fn package_flow_path(id: &str) -> String {
    format!("{BLOCKS_FOLDER}/{id}{PACKAGE_SUFFIX}/{PACKAGE_FLOW_FILE}")
}

/// The values the inputs of the package whose steps are in `flow_file` have when a test leaves
/// them out, as its `block.json` says; empty for any other file.
pub fn input_defaults(flow_file: &Path) -> HashMap<String, String> {
    package_of_path(&flow_file.to_string_lossy())
        .and_then(|_| fs::read_to_string(flow_file.with_file_name(MANIFEST_FILE)).ok())
        .and_then(|text| parse_manifest(&text).ok())
        .map(|manifest| {
            (manifest.inputs.into_iter())
                .filter_map(|i| i.default_value.map(|value| (i.name, value)))
                .collect()
        })
        .unwrap_or_default()
}

fn read_text(dir: &Path, file: &str) -> Result<String, PackageError> {
    let path = dir.join(file);
    let unreadable = |reason: String| {
        failure(
            "block.file.unreadable",
            &[("file", file), ("reason", &reason)],
        )
    };
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Err(failure("block.file.missing", &[("file", file)]));
        }
        Err(e) => return Err(unreadable(e.to_string())),
        Ok(meta) if !meta.is_file() => return Err(unreadable(tr!("package.file.notFile"))),
        Ok(meta) if meta.len() > MAX_TEXT_FILE => {
            return Err(unreadable(tr!("package.file.tooLarge")));
        }
        Ok(_) => {}
    }
    fs::read_to_string(&path).map_err(|e| unreadable(e.to_string()))
}

/// Reads the package in the folder `dir` and checks it. Fails when `block.json` or `flow.json`
/// is missing or can't be read, or the package is too large; problems with the contents are in
/// [`Package::issues`].
pub fn read_package(dir: &Path) -> Result<Package, PackageError> {
    let manifest_text = read_text(dir, MANIFEST_FILE);
    let flow_text = read_text(dir, PACKAGE_FLOW_FILE);
    if matches!(
        (&manifest_text, &flow_text),
        (Err(a), Err(b)) if a.issues[0].code == "block.file.missing" && b.issues[0].code == "block.file.missing"
    ) {
        return Err(failure(
            "block.folder.notPackage",
            &[("path", &dir.display().to_string())],
        ));
    }
    let mut issues = Vec::new();
    let manifest = manifest_text.and_then(|text| parse_manifest(&text));
    let flow = flow_text.and_then(|text| parse(&text).map_err(PackageError::from));
    let (manifest, flow) = match (manifest, flow) {
        (Ok(manifest), Ok(flow)) => (manifest, flow),
        (manifest, flow) => {
            issues.extend(manifest.err().into_iter().flat_map(|e| e.issues));
            issues.extend(flow.err().into_iter().flat_map(|e| e.issues));
            return Err(PackageError { issues });
        }
    };
    let (files, bytes, skipped_links) = list_files(dir)?;
    let issues = check_package(&manifest, &flow, dir);
    Ok(Package {
        manifest,
        flow,
        files,
        bytes,
        issues,
        skipped_links,
    })
}

/// How many skipped links a warning names before it stops at "…".
const NAMED_LINKS: usize = 5;

/// The warning that a package leaves out `links` (paths inside the package), or `None` when
/// there are none.
pub fn skipped_links_warning(links: &[String]) -> Option<Issue> {
    if links.is_empty() {
        return None;
    }
    let mut named = links[..links.len().min(NAMED_LINKS)].join(", ");
    if links.len() > NAMED_LINKS {
        named.push_str(", …");
    }
    Some(
        Issue::warning("block.links.skipped")
            .with("count", links.len().to_string())
            .with("links", named),
    )
}

/// The files of a package folder that belong in a package: not hidden, not links. Also returns
/// the links it left out.
fn list_files(dir: &Path) -> Result<(Vec<PackageFile>, u64, Vec<String>), PackageError> {
    fn walk(
        dir: &Path,
        prefix: &str,
        depth: usize,
        files: &mut Vec<PackageFile>,
        bytes: &mut u64,
        links: &mut Vec<String>,
    ) -> Result<(), PackageError> {
        let unreadable = |e: io::Error| {
            failure(
                "block.file.unreadable",
                &[("file", prefix), ("reason", &e.to_string())],
            )
        };
        for entry in fs::read_dir(dir).map_err(unreadable)? {
            let entry = entry.map_err(unreadable)?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                return Err(failure(
                    "block.path.unsafe",
                    &[("path", &entry.file_name().to_string_lossy())],
                ));
            };
            if name.starts_with('.') || name == "__MACOSX" {
                continue;
            }
            let path = format!("{prefix}{name}");
            let kind = entry.file_type().map_err(unreadable)?;
            if kind.is_dir() && depth < MAX_DEPTH {
                walk(
                    &entry.path(),
                    &format!("{path}/"),
                    depth + 1,
                    files,
                    bytes,
                    links,
                )?;
            } else if kind.is_symlink() {
                links.push(path);
            } else if kind.is_file() {
                let size = entry.metadata().map_err(unreadable)?.len();
                *bytes += size;
                files.push(PackageFile { path, size });
                if files.len() > MAX_PACKAGE_FILES {
                    return Err(failure(
                        "block.package.tooManyFiles",
                        &[("max", &MAX_PACKAGE_FILES.to_string())],
                    ));
                }
                if *bytes > MAX_PACKAGE_BYTES {
                    return Err(failure(
                        "block.package.tooLarge",
                        &[("max", &MAX_PACKAGE_BYTES.to_string())],
                    ));
                }
            }
        }
        Ok(())
    }
    let (mut files, mut bytes, mut links) = (Vec::new(), 0, Vec::new());
    walk(dir, "", 0, &mut files, &mut bytes, &mut links)?;
    files.sort_by(|a, b| a.path.cmp(&b.path));
    links.sort();
    Ok((files, bytes, links))
}

fn write_file(from: &Path, to: &Path) -> io::Result<()> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(to, fs::read(from)?)
}

/// A hidden folder beside `target` to build in, and then rename: a half-written package is
/// never at its real place.
fn beside(target: &Path, ending: &str) -> PathBuf {
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    target.with_file_name(format!(".{name}.{ending}"))
}

/// Puts the finished folder `built` in the place of `target`; what was there stays until the new
/// folder is in place, and comes back if that fails.
fn swap_in(built: &Path, target: &Path) -> io::Result<()> {
    if !target.exists() {
        return fs::rename(built, target);
    }
    let old = beside(target, "old");
    let _ = fs::remove_dir_all(&old);
    fs::rename(target, &old)?;
    if let Err(e) = fs::rename(built, target) {
        let _ = fs::rename(&old, target);
        return Err(e);
    }
    let _ = fs::remove_dir_all(&old);
    Ok(())
}

fn read_lock(project: &Path) -> Result<PackageLock, PackageError> {
    match fs::read_to_string(project.join(PACKAGE_LOCK_FILE)) {
        Ok(text) => PackageLock::parse(&text)
            .map_err(|reason| failure("block.lock.invalid", &[("reason", &reason)])),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(PackageLock::default()),
        Err(e) => Err(failure("block.lock.invalid", &[("reason", &e.to_string())])),
    }
}

/// Writes the lock; an empty one is deleted, so a project without installed-from-address
/// packages has no extra file.
fn write_lock(project: &Path, lock: &PackageLock) -> Result<(), PackageError> {
    let path = project.join(PACKAGE_LOCK_FILE);
    let done = if lock.sources.is_empty() {
        match fs::remove_file(&path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    } else {
        fs::write(&path, lock.to_json())
    };
    done.map_err(|e| failure("block.lock.invalid", &[("reason", &e.to_string())]))
}

/// The packages installed in `project`, by name. A folder that isn't a readable package is left
/// out.
pub fn list_installed(project: &Path) -> Vec<Installed> {
    let lock = read_lock(project).unwrap_or_default();
    let Ok(entries) = fs::read_dir(project.join(BLOCKS_FOLDER)) else {
        return Vec::new();
    };
    let mut found: Vec<Installed> = entries
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let id = name.strip_suffix(PACKAGE_SUFFIX)?.to_owned();
            let text = read_text(&e.path(), MANIFEST_FILE).ok()?;
            let manifest = parse_manifest(&text).ok().filter(|m| m.id == id)?;
            Some(Installed {
                source: lock.sources.get(&id).cloned(),
                folder: format!("{BLOCKS_FOLDER}/{name}"),
                manifest,
            })
        })
        .collect();
    found.sort_by(|a, b| {
        (a.manifest.name.to_lowercase(), &a.manifest.id)
            .cmp(&(b.manifest.name.to_lowercase(), &b.manifest.id))
    });
    found
}

/// The installed package `id`, if there is one.
pub fn installed(project: &Path, id: &str) -> Option<Installed> {
    list_installed(project)
        .into_iter()
        .find(|i| i.manifest.id == id)
}

/// How `manifest` compares with what `project` has installed.
pub fn change_for(project: &Path, manifest: &BlockManifest) -> Change {
    match installed(project, &manifest.id) {
        None => Change::New,
        Some(old) => match manifest.version.cmp(&old.manifest.version) {
            std::cmp::Ordering::Equal => Change::Same,
            std::cmp::Ordering::Greater => Change::Update {
                from: old.manifest.version,
            },
            std::cmp::Ordering::Less => Change::Downgrade {
                from: old.manifest.version,
            },
        },
    }
}

/// Installs the package in the folder `source_dir` into `project`, in `blocks/ID.jevblock`
/// where its tests and the team's git find it. A package that is installed under the same id is
/// replaced only with `replace`. `source`, the address the package came from, is kept for
/// updates. Nothing is installed if the package has an error.
pub fn install_folder(
    project: &Path,
    source_dir: &Path,
    source: Option<&str>,
    replace: bool,
) -> Result<Installed, PackageError> {
    let package = read_package(source_dir)?;
    let errors: Vec<Issue> = package
        .issues
        .iter()
        .filter(|i| i.severity == Severity::Error)
        .cloned()
        .collect();
    if !errors.is_empty() {
        return Err(PackageError { issues: errors });
    }
    let id = package.manifest.id.clone();
    if let Some(old) = installed(project, &id).filter(|_| !replace) {
        return Err(failure(
            "block.install.exists",
            &[("id", &id), ("version", &old.manifest.version.to_string())],
        ));
    }
    let mut lock = read_lock(project)?;
    let target = package_folder(project, &id);
    let built = beside(&target, "partial");
    let _ = fs::remove_dir_all(&built);
    let result = (|| {
        for file in &package.files {
            write_file(&source_dir.join(&file.path), &built.join(&file.path))?;
        }
        swap_in(&built, &target)
    })();
    if let Err(e) = result {
        let _ = fs::remove_dir_all(&built);
        return Err(io_failure("block.install.failed", &id, &e));
    }
    match source {
        Some(source) => {
            lock.sources.insert(id.clone(), source.to_owned());
            lock.versions
                .insert(id.clone(), package.manifest.version.clone());
        }
        None => {
            lock.sources.remove(&id);
            lock.versions.remove(&id);
        }
    }
    write_lock(project, &lock)?;
    Ok(Installed {
        folder: format!("{BLOCKS_FOLDER}/{id}{PACKAGE_SUFFIX}"),
        manifest: package.manifest,
        source: source.map(str::to_owned),
    })
}

/// Deletes the installed package `id`. The tests that use it are not touched; see [`usage`].
pub fn remove_package(project: &Path, id: &str) -> Result<(), PackageError> {
    if !is_package_id(id) || installed(project, id).is_none() {
        return Err(failure("block.notInstalled", &[("id", id)]));
    }
    fs::remove_dir_all(package_folder(project, id))
        .map_err(|e| io_failure("block.remove.failed", id, &e))?;
    let mut lock = read_lock(project)?;
    let had_source = lock.sources.remove(id).is_some();
    if lock.versions.remove(id).is_some() || had_source {
        write_lock(project, &lock)?;
    }
    Ok(())
}

/// The tests and blocks of `project`, as files relative to it, sorted.
fn project_tests(project: &Path) -> Vec<String> {
    fn walk(dir: &Path, prefix: &str, depth: usize, out: &mut Vec<String>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.filter_map(Result::ok) {
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = format!("{prefix}{name}");
            if kind.is_dir() {
                // The packages a package runs are in its block.json.
                let skip = name.starts_with('.')
                    || name == "node_modules"
                    || name.ends_with(PACKAGE_SUFFIX)
                    || depth >= 12;
                if !skip {
                    walk(&entry.path(), &format!("{path}/"), depth + 1, out);
                }
            } else if kind.is_file()
                && (name.ends_with(TEST_SUFFIX) || name.ends_with(BLOCK_SUFFIX))
            {
                out.push(path);
            }
        }
    }
    let mut files = Vec::new();
    walk(project, "", 0, &mut files);
    files.sort();
    files
}

fn read_flow(project: &Path, file: &str) -> Option<Flow> {
    fs::read_to_string(project.join(file))
        .ok()
        .and_then(|text| parse(&text).ok())
}

/// Whether `test`, a step of the file `file`, runs the package `id`.
fn runs_package(file: &str, test: &str, id: &str) -> bool {
    package_of_path(test) == Some(id)
        && resolve_test(file, test).as_deref() == Some(package_flow_path(id).as_str())
}

/// The tests and blocks of `project`, as files relative to it, that run the package `id`, sorted.
pub fn usage(project: &Path, id: &str) -> Vec<String> {
    project_tests(project)
        .into_iter()
        .filter(|file| {
            read_flow(project, file).is_some_and(|flow| {
                flow.blocks.iter().any(|b| {
                    matches!(&b.kind, BlockKind::RunTest { test, .. } if runs_package(file, test, id))
                })
            })
        })
        .collect()
}

/// The installed packages of `project` that need the package `id`, by id.
pub fn block_dependents(project: &Path, id: &str) -> Vec<String> {
    let mut found: Vec<String> = list_installed(project)
        .into_iter()
        .filter(|i| i.manifest.dependencies.iter().any(|d| d.id == id))
        .map(|i| i.manifest.id)
        .collect();
    found.sort();
    found
}

/// What the installed package `id` of `project` needs, and what those need in turn, that isn't
/// installed in a version that fits: `block.dependency.missing` and `block.dependency.unmet`.
pub fn unmet_dependencies(project: &Path, id: &str) -> Vec<Issue> {
    unmet_in(&project.join(BLOCKS_FOLDER), id)
}

/// [`unmet_dependencies`] of the package `id` in the folder `blocks`.
pub(crate) fn unmet_in(blocks: &Path, id: &str) -> Vec<Issue> {
    let read = |id: &str| {
        read_text(&blocks.join(format!("{id}{PACKAGE_SUFFIX}")), MANIFEST_FILE)
            .ok()
            .and_then(|text| parse_manifest(&text).ok())
            .filter(|m| m.id == id)
    };
    let mut out = Vec::new();
    let mut seen = vec![id.to_owned()];
    let mut next: Vec<BlockManifest> = read(id).into_iter().collect();
    while let Some(manifest) = next.pop() {
        for d in &manifest.dependencies {
            let found = read(&d.id);
            out.extend(unmet(d, found.as_ref().map(|m| &m.version), &manifest.id));
            if let Some(found) = found.filter(|m| !seen.contains(&m.id)) {
                seen.push(found.id.clone());
                next.push(found);
            }
        }
    }
    out
}

/// A package that [`PACKAGE_LOCK_FILE`] names and `project` doesn't have, or has in another
/// version than the one the lock keeps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreEntry {
    pub id: String,
    /// The address it was installed from.
    pub source: String,
    /// `None` in a lock written before versions were kept.
    pub locked_version: Option<Version>,
}

/// The packages to install again so that `project` has what its lock file says, by id.
pub fn restore_entries(project: &Path) -> Result<Vec<RestoreEntry>, PackageError> {
    let lock = read_lock(project)?;
    let installed = list_installed(project);
    Ok(lock
        .sources
        .into_iter()
        .filter_map(|(id, source)| {
            let locked_version = lock.versions.get(&id).cloned();
            let current = installed.iter().any(|i| {
                i.manifest.id == id
                    && locked_version
                        .as_ref()
                        .is_none_or(|v| *v == i.manifest.version)
            });
            (!current).then_some(RestoreEntry {
                id,
                source,
                locked_version,
            })
        })
        .collect())
}

/// A copy of an installed package among the blocks of the project, made by [`make_own_block`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnBlock {
    /// Relative to the project: `blocks/NAME.block.json`.
    pub file: String,
    pub name: String,
}

/// Copies the installed package `id` of `project` into the project's own blocks, where it can be
/// changed, the way the app saves a block: `blocks/NAME.block.json` after the name of the block,
/// `NAME 2` and on when that is taken. The images and files it uses go to `blocks/images` and
/// `blocks/files`, under another name when one with other contents is there already; steps that
/// run another package keep running it. The package stays installed.
pub fn make_own_block(project: &Path, id: &str) -> Result<OwnBlock, PackageError> {
    let Some(package) = installed(project, id).filter(|_| is_package_id(id)) else {
        return Err(failure("block.notInstalled", &[("id", id)]));
    };
    let home = package_flow_path(id);
    let mut flow = parse(&read_text(&package_folder(project, id), PACKAGE_FLOW_FILE)?)?;
    let blocks = project.join(BLOCKS_FOLDER);
    let stem = file_stem(&package.manifest.name);
    let name = std::iter::once(stem.clone())
        .chain((2..).map(|n| format!("{stem} {n}")))
        .find(|name| !blocks.join(format!("{name}{BLOCK_SUFFIX}")).exists())
        .expect("a free name always exists");
    let file = format!("{BLOCKS_FOLDER}/{name}{BLOCK_SUFFIX}");
    for b in &mut flow.blocks {
        let BlockKind::RunTest { test, .. } = &mut b.kind else {
            continue;
        };
        if !is_relative_path(test) || !placeholders(test).is_empty() {
            continue;
        }
        match resolve_test(&home, test) {
            Some(to)
                if package_of_path(&to).is_some_and(|p| p != id && package_flow_path(p) == to) =>
            {
                *test = relative(&file, &to);
            }
            _ => return Err(failure("block.copy.nested", &[("test", test)])),
        }
    }

    // Where each file goes, relative to `blocks`, and from where it comes.
    let mut copies: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut taken: HashMap<String, PathBuf> = HashMap::new();
    visit_files(&mut flow, |_, image, path| {
        if !is_relative_path(path) || !placeholders(path).is_empty() {
            return;
        }
        let Some(inside) = resolve_test(&home, path) else {
            return;
        };
        let from = project.join(&inside);
        let name = path.rsplit('/').next().unwrap_or(path);
        let wanted = format!("{}/{name}", if image { "images" } else { "files" });
        let mut chosen = wanted.clone();
        for n in 2.. {
            let fits = match taken.get(&chosen) {
                Some(other) => *other == from,
                None => {
                    free_for(&from, &blocks.join(&chosen))
                        && (!image
                            || free_for(
                                &project.join(dark_variant(&inside)),
                                &blocks.join(dark_variant(&chosen)),
                            ))
                }
            };
            if fits {
                break;
            }
            chosen = numbered(&wanted, n);
        }
        if taken.insert(chosen.clone(), from.clone()).is_none() {
            if image {
                copies.push((
                    project.join(dark_variant(&inside)),
                    blocks.join(dark_variant(&chosen)),
                ));
            }
            copies.push((from, blocks.join(&chosen)));
        }
        *path = chosen;
    });
    flow.name = package.manifest.name.clone();
    flow.description = package.manifest.description.clone();
    flow.icon = package.manifest.icon.clone();

    let mut written = Vec::new();
    let result = (|| -> io::Result<()> {
        for (from, to) in &copies {
            if from.is_file() && !to.exists() {
                write_file(from, to)?;
                written.push(to.clone());
            }
        }
        fs::create_dir_all(&blocks)?;
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(project.join(&file))?
            .write_all(to_canonical_json(&flow).as_bytes())
    })();
    if let Err(e) = result {
        for file in written {
            let _ = fs::remove_file(file);
        }
        return Err(io_failure("block.copy.failed", id, &e));
    }
    Ok(OwnBlock {
        file,
        name: flow.name,
    })
}

/// Makes the tests and blocks of `project` that run the package `id` run `own_file`, its copy
/// from [`make_own_block`], instead; an input a step leaves out gets the default the package
/// gave it. The files changed, sorted.
pub fn switch_tests_to_copy(
    project: &Path,
    id: &str,
    own_file: &str,
) -> Result<Vec<String>, PackageError> {
    if !is_relative_path(own_file)
        || !own_file.ends_with(BLOCK_SUFFIX)
        || (own_file.split('/')).any(|part| part == ".." || part.ends_with(PACKAGE_SUFFIX))
        || !project.join(own_file).is_file()
    {
        return Err(failure("block.export.notBlock", &[("path", own_file)]));
    }
    let defaults = input_defaults(&project.join(package_flow_path(id)));
    let mut changed = Vec::new();
    for file in project_tests(project) {
        let Some(mut flow) = read_flow(project, &file) else {
            continue;
        };
        let mut uses = false;
        for b in &mut flow.blocks {
            let BlockKind::RunTest {
                test,
                package,
                inputs,
                ..
            } = &mut b.kind
            else {
                continue;
            };
            if runs_package(&file, test, id) {
                *test = relative(&file, own_file);
                *package = None;
                for (name, value) in &defaults {
                    inputs.entry(name.clone()).or_insert_with(|| value.clone());
                }
                uses = true;
            }
        }
        if uses {
            fs::write(project.join(&file), to_canonical_json(&flow))
                .map_err(|e| io_failure("block.copy.failed", id, &e))?;
            changed.push(file);
        }
    }
    Ok(changed)
}

/// The name of the file of a block called `name`, as the app writes it: characters a file name
/// can't hold become dashes.
fn file_stem(name: &str) -> String {
    let clean = name.trim().replace(['/', ':', '\\'], "-");
    match clean.trim_matches('.') {
        "" => "Untitled".to_owned(),
        clean => clean.to_owned(),
    }
}

/// The path from the folder of `file` to `to`, both relative to the project.
fn relative(file: &str, to: &str) -> String {
    let from: Vec<&str> = file.split('/').collect();
    let from = &from[..from.len() - 1];
    let to: Vec<&str> = to.split('/').collect();
    let shared = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut parts = vec![".."; from.len() - shared];
    parts.extend(&to[shared..]);
    parts.join("/")
}

/// `images/go.png` as `images/go-2.png`, for the `n`-th file that wants that name.
fn numbered(path: &str, n: usize) -> String {
    match path.rsplit_once('.') {
        Some((stem, ending)) => format!("{stem}-{n}.{ending}"),
        None => format!("{path}-{n}"),
    }
}

/// Whether a copy of `from` can be put at `to`: nothing is there, or the same contents.
fn free_for(from: &Path, to: &Path) -> bool {
    !to.exists()
        || (fs::read(from).ok())
            .zip(fs::read(to).ok())
            .is_some_and(|(a, b)| a == b)
}

/// What the author says about a block they share, beside what the block itself holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportMeta {
    pub id: String,
    /// `None` uses the name of the block.
    pub name: Option<String>,
    /// `None` uses the description of the block.
    pub description: Option<String>,
    pub author: String,
    pub version: String,
    pub license: String,
    /// Labels and defaults for the inputs the block reads; an input missing here is listed with
    /// its name only.
    pub inputs: Vec<PackageInput>,
}

/// A package written by [`export_package`].
#[derive(Debug, Clone)]
pub struct Exported {
    /// `ID.jevblock` in the chosen folder.
    pub folder: PathBuf,
    pub manifest: BlockManifest,
    pub files: Vec<PackageFile>,
    pub bytes: u64,
    /// Things worth a look that didn't stop the export, such as an image the block names that
    /// doesn't exist yet.
    pub warnings: Vec<Issue>,
}

/// Writes the block `block_file` of `project` (like `blocks/Sign in.block.json`) as a package in
/// a new folder `ID.jevblock` inside `destination`, replacing one with that name. The images and
/// files the block uses go with it; those from outside the block's own folder are gathered in
/// `images` and `files`, and the steps are changed to name them there. A step that runs an
/// installed package makes it a dependency; one that runs any other test or block refuses the
/// export.
pub fn export_package(
    project: &Path,
    block_file: &str,
    meta: &ExportMeta,
    destination: &Path,
) -> Result<Exported, PackageError> {
    if !is_relative_path(block_file)
        || !block_file.ends_with(BLOCK_SUFFIX)
        || block_file.split('/').any(|part| part == "..")
        || block_file
            .split('/')
            .any(|part| part.ends_with(PACKAGE_SUFFIX))
    {
        return Err(failure("block.export.notBlock", &[("path", block_file)]));
    }
    let unreadable = |e: &dyn ToString| {
        failure(
            "block.file.unreadable",
            &[("file", block_file), ("reason", &e.to_string())],
        )
    };
    let text = fs::read_to_string(project.join(block_file)).map_err(|e| unreadable(&e))?;
    let mut flow = parse(&text)?;
    // A step that runs an installed package keeps running it from beside this one, which then
    // needs it in the version installed here or a later one that keeps to it.
    let mut needs: BTreeMap<String, serde_json::Value> = BTreeMap::new();
    let sources = read_lock(project).unwrap_or_default().sources;
    for b in &mut flow.blocks {
        let BlockKind::RunTest { test, .. } = &mut b.kind else {
            continue;
        };
        let package = (resolve_test(block_file, test).filter(|_| placeholders(test).is_empty()))
            .and_then(|to| {
                let id = package_of_path(&to)?.to_owned();
                (package_flow_path(&id) == to).then(|| installed(project, &id))?
            });
        let Some(package) = package else {
            return Err(failure("block.export.nested", &[("test", test)]));
        };
        let id = package.manifest.id;
        *test = format!("../{id}{PACKAGE_SUFFIX}/{PACKAGE_FLOW_FILE}");
        let mut need = json!({ "id": id, "version": format!("^{}", package.manifest.version) });
        if let Some(source) = sources.get(&id) {
            need["source"] = json!(source);
        }
        needs.insert(id, need);
    }
    let manifest = export_manifest(&flow, meta, needs.into_values().collect())?;

    flow.name = manifest.name.clone();
    flow.description = manifest.description.clone();
    flow.icon = manifest.icon.clone();
    flow.author = Some(manifest.author.clone());
    flow.depends_on.clear();

    // Where each file goes in the package, and from where it comes.
    let mut copies: Vec<(PathBuf, String)> = Vec::new();
    let mut taken: HashMap<String, PathBuf> = HashMap::new();
    let mut dark: Vec<(PathBuf, String)> = Vec::new();
    let from_folder = Path::new(block_file)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let folder_file = format!("{from_folder}/{PACKAGE_FLOW_FILE}");
    visit_files(&mut flow, |_, image, path| {
        if !is_relative_path(path) || !placeholders(path).is_empty() {
            return;
        }
        let Some(inside) = resolve_test(&folder_file, path) else {
            return;
        };
        let source = project.join(&inside);
        let own = !path.split('/').any(|part| part == "..");
        let wanted = if own {
            path.clone()
        } else {
            let name = path.rsplit('/').next().unwrap_or(path);
            format!("{}/{name}", if image { "images" } else { "files" })
        };
        let mut chosen = wanted.clone();
        let mut n = 2;
        while taken.get(&chosen).is_some_and(|other| *other != source) {
            chosen = numbered(&wanted, n);
            n += 1;
        }
        if taken.insert(chosen.clone(), source.clone()).is_none() {
            copies.push((source.clone(), chosen.clone()));
            if image {
                dark.push((project.join(dark_variant(&inside)), dark_variant(&chosen)));
            }
        }
        *path = chosen;
    });
    copies.extend(dark);

    let folder = destination.join(format!("{}{PACKAGE_SUFFIX}", manifest.id));
    let built = beside(&folder, "partial");
    let _ = fs::remove_dir_all(&built);
    let written = (|| -> io::Result<()> {
        fs::create_dir_all(&built)?;
        fs::write(built.join(MANIFEST_FILE), manifest.to_json())?;
        fs::write(built.join(PACKAGE_FLOW_FILE), to_canonical_json(&flow))?;
        for (source, to) in &copies {
            if source.is_file() {
                write_file(source, &built.join(to))?;
            }
        }
        Ok(())
    })();
    let cleanup = |e: PackageError| {
        let _ = fs::remove_dir_all(&built);
        e
    };
    written.map_err(|e| {
        cleanup(failure(
            "block.export.failed",
            &[("reason", &e.to_string())],
        ))
    })?;
    let package = read_package(&built).map_err(cleanup)?;
    if package.has_errors() {
        let errors = package
            .issues
            .into_iter()
            .filter(|i| i.severity == Severity::Error)
            .collect();
        return Err(cleanup(PackageError { issues: errors }));
    }
    fs::create_dir_all(destination)
        .and_then(|()| swap_in(&built, &folder))
        .map_err(|e| {
            cleanup(failure(
                "block.export.failed",
                &[("reason", &e.to_string())],
            ))
        })?;
    Ok(Exported {
        folder,
        manifest: package.manifest,
        files: package.files,
        bytes: package.bytes,
        warnings: package.issues,
    })
}

/// The `block.json` for sharing `flow` as `meta` says, through the same reading that checks every
/// package, so what an author types is held to the rules of the format.
fn export_manifest(
    flow: &Flow,
    meta: &ExportMeta,
    dependencies: Vec<serde_json::Value>,
) -> Result<BlockManifest, PackageError> {
    let mut inputs: Vec<PackageInput> = Vec::new();
    for name in crate::required_inputs(flow) {
        let given = meta.inputs.iter().find(|i| i.name == name);
        inputs.push(PackageInput {
            label: given
                .and_then(|i| i.label.clone())
                .filter(|l| !l.trim().is_empty()),
            default_value: given.and_then(|i| i.default_value.clone()),
            name,
        });
    }
    let name = meta
        .name
        .clone()
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| flow.name.clone());
    let description = meta
        .description
        .clone()
        .or_else(|| flow.description.clone())
        .filter(|d| !d.trim().is_empty());
    let mut object = json!({
        "format": crate::package::PACKAGE_FORMAT,
        "id": meta.id,
        "name": name,
        "version": meta.version,
        "author": meta.author,
        "license": meta.license,
        "minVersion": app_version().to_string(),
        "inputs": inputs.iter().map(|i| {
            let mut input = json!({ "name": i.name });
            if let Some(label) = &i.label { input["label"] = json!(label); }
            if let Some(default) = &i.default_value { input["default"] = json!(default); }
            input
        }).collect::<Vec<_>>(),
    });
    if let Some(description) = description {
        object["description"] = json!(description);
    }
    if !dependencies.is_empty() {
        object["dependencies"] = json!(dependencies);
    }
    if let Some(icon) = flow.icon.as_ref().filter(|i| !i.is_empty()) {
        object["icon"] = json!(icon);
    }
    parse_manifest(&object.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("jevision-package-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn manifest(version: &str) -> String {
        format!(
            r#"{{"format":1,"id":"com.acme.sign-in","name":"Sign In","version":"{version}",
            "author":"Acme","license":"MIT","inputs":[{{"name":"email"}}]}}"#
        )
    }

    const STEPS: &str = r#"{"format":1,"name":"Sign In","app":"com.example.App","start":"mail","blocks":{
        "mail":{"type":"type","text":"${input.email}","next":"hit"},
        "hit":{"type":"click","target":{"image":"images/go.png"},"next":"ok"},
        "ok":{"type":"check","reference":{"image":"images/ok.png"}}}}"#;

    fn package(root: &Path, version: &str) -> PathBuf {
        let dir = root.join("src.jevblock");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("images")).unwrap();
        fs::write(dir.join(MANIFEST_FILE), manifest(version)).unwrap();
        fs::write(dir.join(PACKAGE_FLOW_FILE), STEPS).unwrap();
        fs::write(dir.join("images/go.png"), b"png").unwrap();
        fs::write(dir.join("images/ok.png"), b"png").unwrap();
        dir
    }

    #[test]
    fn a_package_is_read_with_its_files() {
        let root = temp("read");
        let read = read_package(&package(&root, "1.0.0")).unwrap();
        assert_eq!(read.manifest.id, "com.acme.sign-in");
        let paths: Vec<&str> = read.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(
            paths,
            ["block.json", "flow.json", "images/go.png", "images/ok.png"]
        );
        assert!(!read.has_errors(), "{:?}", read.issues);
    }

    #[test]
    fn a_folder_that_is_not_a_package_says_what_is_missing() {
        let root = temp("notpackage");
        let code = |dir: &Path| read_package(dir).unwrap_err().issues[0].code.clone();
        assert_eq!(code(&root), "block.folder.notPackage");
        fs::write(root.join(MANIFEST_FILE), manifest("1.0.0")).unwrap();
        assert_eq!(code(&root), "block.file.missing");
        fs::write(root.join(PACKAGE_FLOW_FILE), "{").unwrap();
        assert_eq!(code(&root), "flow.json.syntax");
        assert_eq!(code(&root.join("nowhere")), "block.folder.notPackage");
    }

    #[test]
    fn hidden_files_and_links_stay_out_of_a_package() {
        let root = temp("hidden");
        let dir = package(&root, "1.0.0");
        fs::write(dir.join(".DS_Store"), b"x").unwrap();
        fs::create_dir_all(dir.join("__MACOSX")).unwrap();
        fs::write(dir.join("__MACOSX/junk"), b"x").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("/etc/hosts", dir.join("hosts")).unwrap();
        let read = read_package(&dir).unwrap();
        assert_eq!(read.files.len(), 4);
    }

    #[cfg(unix)]
    #[test]
    fn links_in_a_package_folder_are_left_out_and_named() {
        use std::os::unix::fs::symlink;
        let root = temp("links");
        let dir = package(&root, "1.0.0");
        let plain = read_package(&dir).unwrap();
        assert!(plain.skipped_links.is_empty());
        assert_eq!(skipped_links_warning(&plain.skipped_links), None);

        symlink("/etc/hosts", dir.join("images/logo.png")).unwrap();
        symlink(&root, dir.join("shared")).unwrap();
        symlink("/etc/hosts", dir.join(".hidden")).unwrap();
        let read = read_package(&dir).unwrap();
        assert_eq!(read.files.len(), 4);
        assert_eq!(read.skipped_links, ["images/logo.png", "shared"]);

        let warning = skipped_links_warning(&read.skipped_links).unwrap();
        assert_eq!(warning.severity, Severity::Warning);
        assert_eq!(warning.param("links"), Some("images/logo.png, shared"));
        let en = crate::issue::describe_in("en", &warning);
        assert!(
            en.contains("2 links to other files were left out: images/logo.png, shared."),
            "{en}"
        );
        let ru = crate::issue::describe_in("ru", &warning);
        assert!(ru.contains("Пропущены 2 символические ссылки"), "{ru}");
        let many: Vec<String> = (1..=7).map(|n| format!("l{n}")).collect();
        let warning = skipped_links_warning(&many).unwrap();
        assert_eq!(warning.param("links"), Some("l1, l2, l3, l4, l5, …"));
        assert!(crate::issue::describe_in("en", &warning).contains(": 7 links"));
    }

    #[test]
    fn too_many_files_or_bytes_refuse_the_package() {
        let root = temp("limits");
        let dir = package(&root, "1.0.0");
        for i in 0..=MAX_PACKAGE_FILES {
            fs::write(dir.join(format!("images/f{i}.png")), b"x").unwrap();
        }
        assert_eq!(
            read_package(&dir).unwrap_err().issues[0].code,
            "block.package.tooManyFiles"
        );
    }

    #[test]
    fn installing_puts_the_package_in_the_project_and_remembers_the_address() {
        let root = temp("install");
        let project = root.join("project");
        fs::create_dir_all(&project).unwrap();
        let dir = package(&root, "1.0.0");
        let done = install_folder(
            &project,
            &dir,
            Some("https://github.com/acme/sign-in"),
            false,
        )
        .unwrap();
        assert_eq!(done.folder, "blocks/com.acme.sign-in.jevblock");
        assert!(
            project
                .join("blocks/com.acme.sign-in.jevblock/images/go.png")
                .is_file()
        );
        assert!(
            !project
                .join("blocks/.com.acme.sign-in.jevblock.partial")
                .exists()
        );
        let listed = list_installed(&project);
        assert_eq!(listed.len(), 1);
        assert_eq!(
            listed[0].source.as_deref(),
            Some("https://github.com/acme/sign-in")
        );
        assert!(
            fs::read_to_string(project.join(PACKAGE_LOCK_FILE))
                .unwrap()
                .contains("github.com/acme")
        );
        // Another version, from a file: refused unless replacing; replacing forgets the address.
        let newer = package(&root, "1.1.0");
        let error = install_folder(&project, &newer, None, false).unwrap_err();
        assert_eq!(error.issues[0].code, "block.install.exists");
        assert_eq!(error.issues[0].param("version"), Some("1.0.0"));
        assert_eq!(
            change_for(&project, &read_package(&newer).unwrap().manifest),
            Change::Update {
                from: "1.0.0".parse().unwrap()
            }
        );
        install_folder(&project, &newer, None, true).unwrap();
        assert_eq!(
            list_installed(&project)[0].manifest.version.to_string(),
            "1.1.0"
        );
        assert_eq!(list_installed(&project)[0].source, None);
        assert!(!project.join(PACKAGE_LOCK_FILE).exists());
        assert_eq!(
            change_for(&project, &read_package(&newer).unwrap().manifest),
            Change::Same
        );
        let older = package(&root, "0.9.0");
        assert_eq!(
            change_for(&project, &read_package(&older).unwrap().manifest),
            Change::Downgrade {
                from: "1.1.0".parse().unwrap()
            }
        );
    }

    #[test]
    fn a_package_with_an_error_is_not_installed() {
        let root = temp("refuse");
        let project = root.join("project");
        fs::create_dir_all(&project).unwrap();
        let dir = package(&root, "1.0.0");
        fs::write(
            dir.join(PACKAGE_FLOW_FILE),
            STEPS.replace("images/go.png", "../go.png"),
        )
        .unwrap();
        let error = install_folder(&project, &dir, None, false).unwrap_err();
        assert!(error.issues.iter().any(|i| i.code == "block.path.outside"));
        assert!(!project.join("blocks").exists());
    }

    #[test]
    fn removing_forgets_the_package_and_its_address() {
        let root = temp("remove");
        let project = root.join("project");
        fs::create_dir_all(&project).unwrap();
        install_folder(
            &project,
            &package(&root, "1.0.0"),
            Some("https://github.com/a/b"),
            false,
        )
        .unwrap();
        remove_package(&project, "com.acme.sign-in").unwrap();
        assert!(list_installed(&project).is_empty());
        assert!(!project.join(PACKAGE_LOCK_FILE).exists());
        for id in ["com.acme.sign-in", "../../etc", ""] {
            assert_eq!(
                remove_package(&project, id).unwrap_err().issues[0].code,
                "block.notInstalled"
            );
        }
    }

    #[test]
    fn usage_names_the_tests_that_run_the_package() {
        let root = temp("usage");
        let project = root.join("project");
        fs::create_dir_all(project.join("tests/deep")).unwrap();
        install_folder(&project, &package(&root, "1.0.0"), None, false).unwrap();
        let test = |path: &str| {
            format!(
                r#"{{"format":1,"name":"T","app":"com.example.App","start":"a","blocks":{{
                "a":{{"type":"runTest","test":"{path}","inputs":{{"email":"x"}}}}}}}}"#
            )
        };
        fs::write(
            project.join("top.flow.json"),
            test("blocks/com.acme.sign-in.jevblock/flow.json"),
        )
        .unwrap();
        fs::write(
            project.join("tests/deep/low.flow.json"),
            test("../../blocks/com.acme.sign-in.jevblock/flow.json"),
        )
        .unwrap();
        fs::write(
            project.join("tests/other.flow.json"),
            test("../blocks/com.acme.other.jevblock/flow.json"),
        )
        .unwrap();
        fs::write(
            project.join("tests/wrong.flow.json"),
            test("blocks/com.acme.sign-in.jevblock/flow.json"),
        )
        .unwrap();
        assert_eq!(
            usage(&project, "com.acme.sign-in"),
            ["tests/deep/low.flow.json", "top.flow.json"]
        );
        assert!(usage(&project, "com.acme.other").len() == 1);
    }

    #[test]
    fn a_block_of_the_project_is_exported_with_the_images_it_uses() {
        let root = temp("export");
        let project = root.join("project");
        fs::create_dir_all(project.join("blocks/shots")).unwrap();
        fs::create_dir_all(project.join("shared")).unwrap();
        fs::write(
            project.join("blocks/Sign in.block.json"),
            r#"{"format":1,"name":"Sign in","icon":"person","app":"com.example.App","start":"a1","blocks":{
            "a1":{"type":"type","text":"${input.email}","next":"b1"},
            "b1":{"type":"click","target":{"image":"shots/go.png"},"next":"c1"},
            "c1":{"type":"click","target":{"image":"../shared/go.png"},"next":"d1"},
            "d1":{"type":"click","target":{"image":"../other/go.png"},"next":"e1"},
            "e1":{"type":"check","reference":{"image":"../shared/ok.png"}}}}"#,
        )
        .unwrap();
        fs::write(project.join("blocks/shots/go.png"), b"a").unwrap();
        fs::write(project.join("blocks/shots/go.dark.png"), b"d").unwrap();
        fs::create_dir_all(project.join("other")).unwrap();
        fs::write(project.join("other/go.png"), b"e").unwrap();
        fs::write(project.join("shared/go.png"), b"b").unwrap();
        fs::write(project.join("shared/ok.png"), b"c").unwrap();
        let meta = ExportMeta {
            id: "com.acme.sign-in".into(),
            name: None,
            description: Some("Signs in.".into()),
            author: "Acme".into(),
            version: "1.0.0".into(),
            license: "MIT".into(),
            inputs: vec![PackageInput {
                name: "email".into(),
                label: Some("E-mail".into()),
                default_value: None,
            }],
        };
        let out = root.join("out");
        let done = export_package(&project, "blocks/Sign in.block.json", &meta, &out).unwrap();
        assert_eq!(done.folder, out.join("com.acme.sign-in.jevblock"));
        let paths: Vec<&str> = done.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "block.json",
                "flow.json",
                "images/go-2.png",
                "images/go.png",
                "images/ok.png",
                "shots/go.dark.png",
                "shots/go.png",
            ]
        );
        assert_eq!(done.manifest.inputs[0].label.as_deref(), Some("E-mail"));
        assert_eq!(done.manifest.min_version, Some(app_version()));
        assert_eq!(
            fs::read(out.join("com.acme.sign-in.jevblock/images/go.png")).unwrap(),
            b"b"
        );
        let steps = fs::read_to_string(out.join("com.acme.sign-in.jevblock/flow.json")).unwrap();
        assert!(
            steps.contains("shots/go.png")
                && steps.contains("images/go-2.png")
                && !steps.contains("..")
        );
        // The result installs as it is.
        let other = root.join("other");
        fs::create_dir_all(&other).unwrap();
        install_folder(&other, &done.folder, None, false).unwrap();
        // Exporting again replaces the folder.
        export_package(&project, "blocks/Sign in.block.json", &meta, &out).unwrap();
    }

    #[test]
    fn export_refuses_what_a_package_can_not_hold() {
        let root = temp("export-refuse");
        let project = root.join("project");
        fs::create_dir_all(project.join("blocks")).unwrap();
        let block = |steps: &str| {
            fs::write(project.join("blocks/B.block.json"), steps).unwrap();
        };
        let meta = |id: &str, version: &str| ExportMeta {
            id: id.into(),
            name: None,
            description: None,
            author: "Acme".into(),
            version: version.into(),
            license: "MIT".into(),
            inputs: Vec::new(),
        };
        let out = root.join("out");
        let code = |meta: &ExportMeta, file: &str| {
            export_package(&project, file, meta, &out)
                .unwrap_err()
                .issues[0]
                .code
                .clone()
        };
        block(
            r#"{"format":1,"name":"B","app":"com.example.App","start":"a","blocks":{"a":{"type":"pause","duration":"1s"}}}"#,
        );
        assert_eq!(
            code(&meta("acme", "1.0.0"), "blocks/B.block.json"),
            "block.id.invalid"
        );
        assert_eq!(
            code(&meta("com.acme.b", "1"), "blocks/B.block.json"),
            "block.version.invalid"
        );
        assert_eq!(
            code(&meta("com.acme.b", "1.0.0"), "../B.block.json"),
            "block.export.notBlock"
        );
        assert_eq!(
            code(&meta("com.acme.b", "1.0.0"), "blocks/B.flow.json"),
            "block.export.notBlock"
        );
        assert_eq!(
            code(&meta("com.acme.b", "1.0.0"), "blocks/nothing.block.json"),
            "block.file.unreadable"
        );
        block(
            r#"{"format":1,"name":"B","app":"com.example.App","start":"a","blocks":{"a":{"type":"runTest","test":"x.block.json"}}}"#,
        );
        assert_eq!(
            code(&meta("com.acme.b", "1.0.0"), "blocks/B.block.json"),
            "block.export.nested"
        );
        assert!(!out.exists() || fs::read_dir(&out).unwrap().next().is_none());
    }
}
