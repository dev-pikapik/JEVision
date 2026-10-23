//! Checks that a project carries every file its tests read, under the names the tests use, so it
//! runs the same once copied, cloned or on CI: no paths to elsewhere on this computer, out of the
//! project or through a link that leads out of it, no names that differ only in capital letters.
//!
//! Not checked: the files a check-file or database block looks at, which belong to the app under
//! test and may be anywhere, and the apps, secrets and texts a test names.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::issue::{Issue, Severity};
use crate::model::{
    AttachmentKind, BLOCK_SUFFIX, Block, BlockKind, CheckOf, Flow, TEST_SUFFIX, dark_variant,
};
use crate::notes::note_file_path;
use crate::package::package_of_path;
use crate::validate::{files_read, is_elsewhere, not_installed, reference_not_saved};

/// Files from this size on make the project slow to clone; Git LFS keeps them out of the history.
pub const LARGE_FILE_BYTES: u64 = 50 * 1024 * 1024;

/// The problems of one test or custom block of a project.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckedTest {
    /// Relative to the project folder, with `/`.
    pub file: String,
    pub issues: Vec<Issue>,
}

/// Checks the tests and custom blocks in `project` and its folders, except hidden ones, in the
/// order of their paths. Links to folders aren't followed.
pub fn check_project(project: &Path) -> Vec<CheckedTest> {
    let root = project
        .canonicalize()
        .unwrap_or_else(|_| project.to_path_buf());
    let mut found = Vec::new();
    tests_in(&root, "", &mut found);
    found.sort();
    let disk = Disk::new(&root);
    found
        .into_iter()
        .map(|(file, unreadable)| {
            let safety = if unreadable.is_none() {
                path_issues(
                    &disk,
                    "",
                    &file,
                    || Issue::error("").with("field", "test"),
                    |path| Some(Issue::error("check.file.missing").with("path", path)),
                )
            } else {
                Vec::new()
            };
            if safety.iter().any(|issue| issue.severity == Severity::Error) {
                return CheckedTest {
                    file,
                    issues: safety,
                };
            }
            let text = match unreadable {
                Some(reason) => Err(reason),
                None => fs::read_to_string(root.join(&file)).map_err(|e| e.to_string()),
            };
            let mut issues = match text.map(|t| crate::parse(&t)) {
                Err(reason) => vec![
                    Issue::error("check.unreadable")
                        .with("path", &file)
                        .with("reason", reason),
                ],
                Ok(Err(e)) => e.issues,
                Ok(Ok(flow)) => check_test(&flow, &root, &file),
            };
            issues.extend(safety);
            CheckedTest { file, issues }
        })
        .collect()
}

/// Adds the tests and custom blocks under `folder` of `root` to `out`, and each folder that can't
/// be read with the reason.
fn tests_in(root: &Path, folder: &str, out: &mut Vec<(String, Option<String>)>) {
    let entries = match fs::read_dir(root.join(folder)) {
        Ok(entries) => entries,
        Err(e) => {
            let shown = if folder.is_empty() { "." } else { folder };
            return out.push((shown.to_owned(), Some(e.to_string())));
        }
    };
    for entry in entries.flatten() {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        let path = if folder.is_empty() {
            name.clone()
        } else {
            format!("{folder}/{name}")
        };
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => tests_in(root, &path, out),
            Ok(kind)
                if (kind.is_file() || kind.is_symlink() && !entry.path().is_dir())
                    && (name.ends_with(TEST_SUFFIX) || name.ends_with(BLOCK_SUFFIX)) =>
            {
                out.push((path, None));
            }
            _ => {}
        }
    }
}

/// The problems with the files that `flow`, saved as `file` of the project folder `root`, reads.
/// `root` is a real path, without links.
pub fn check_test(flow: &Flow, root: &Path, file: &str) -> Vec<Issue> {
    let folder = file.rsplit_once('/').map_or("", |(folder, _)| folder);
    let disk = Disk::new(root);
    let mut out = blocks(flow, &disk, folder);
    for note in &flow.canvas.notes {
        for a in &note.attachments {
            let Some(path) =
                note_file_path(file, &note.id, &a.value).filter(|_| a.kind == AttachmentKind::File)
            else {
                continue;
            };
            // A note's file isn't needed to run the test.
            let base = || Issue::warning("").with("note", &note.id);
            match disk.look("", &path) {
                Look::Missing(path) => out.push(named(base(), "check.file.missing", &path)),
                Look::There(there) => out.extend(there.issues(base, &path)),
                _ => {}
            }
        }
    }
    out
}

/// What [`check_test`] fails a test in the folder `dir` for that only makes it run differently
/// elsewhere, as warnings, for the problems of the test that `found` doesn't tell already. Nothing
/// outside a project, as there is no telling where the project ends.
pub(crate) fn path_warnings(flow: &Flow, dir: &Path, found: &[Issue]) -> Vec<Issue> {
    // The editor validates on every change: a test that names no file costs no look at the disk.
    if flow.depends_on.is_empty() && flow.blocks.iter().all(|b| paths(&b.kind).is_empty()) {
        return Vec::new();
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let Some((root, folder)) = project_of(dir, home.as_deref()) else {
        return Vec::new();
    };
    let told = |issue: &Issue| {
        found.iter().any(|f| {
            f.severity == Severity::Error
                && f.param("block") == issue.param("block")
                && f.param("field") == issue.param("field")
        })
    };
    blocks(flow, &Disk::new(&root), &folder)
        .into_iter()
        .filter(|i| match i.code.as_str() {
            "check.path.absolute" | "check.path.backslash" => !told(i),
            "check.path.outside" | "check.path.link" | "check.path.case" | "check.file.large" => {
                true
            }
            _ => false,
        })
        .map(|i| Issue {
            severity: Severity::Warning,
            ..i
        })
        .collect()
}

/// The project folder that `dir` is in, as a real path, and `dir` from it: the nearest folder at
/// or above `dir` with the environments file or a `.jevision` folder, below the home folder
/// `home`. A run started in the home folder leaves a `.jevision` there, which doesn't make every
/// folder in it a project.
fn project_of(dir: &Path, home: Option<&Path>) -> Option<(PathBuf, String)> {
    let dir = dir.canonicalize().ok()?;
    let home = home.map(|home| home.canonicalize().unwrap_or_else(|_| home.to_path_buf()));
    let root = dir
        .ancestors()
        .take_while(|f| home.as_ref().is_none_or(|home| !home.starts_with(f)))
        .find(|f| crate::env::is_project_folder(f))?;
    let folder = dir.strip_prefix(root).ok()?.to_str()?.to_owned();
    Some((root.to_path_buf(), folder))
}

/// The problems with the files the blocks of `flow` read and the tests it depends on, for a test
/// in `folder` of `root`.
fn blocks(flow: &Flow, disk: &Disk, folder: &str) -> Vec<Issue> {
    let mut sorted: Vec<&Block> = flow.blocks.iter().collect();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));
    // The first run of a check block saves its reference, for the other blocks to find too.
    let saved: HashSet<String> = sorted
        .iter()
        .filter_map(|b| match &b.kind {
            BlockKind::Check {
                of: CheckOf::Screen(reference),
                ..
            } => match disk.look(folder, &reference.image) {
                Look::Missing(file) => Some(file),
                _ => None,
            },
            _ => None,
        })
        .collect();
    let mut out = Vec::new();
    for b in sorted {
        for (field, path) in paths(&b.kind) {
            let base = || Issue::error("").with("block", &b.id).with("field", &field);
            let package = package_of_path(path).filter(|_| field == "test");
            out.extend(path_issues(disk, folder, path, base, |file| {
                Some(match (&b.kind, package) {
                    (BlockKind::Check { .. }, _) => reference_not_saved(&b.id, path),
                    (_, Some(id)) => not_installed(b, &field, path, id),
                    (_, None) if saved.contains(file) => return None,
                    (BlockKind::Command { .. }, None) => {
                        named(base(), "check.folder.missing", file)
                    }
                    _ => named(base(), "check.file.missing", file),
                })
            }));
            if field.ends_with("image") || field.ends_with(".value") {
                // Without a dark reference, the light one stands in.
                let dark = dark_variant(path);
                if let Look::There(there) = disk.look(folder, &dark) {
                    out.extend(there.issues(base, &dark));
                }
            }
        }
    }
    for path in &flow.depends_on {
        let base = || Issue::error("").with("field", "dependsOn");
        out.extend(path_issues(disk, folder, path, base, |file| {
            Some(named(base(), "check.file.missing", file))
        }));
    }
    out
}

/// The fields of a block that name a file or folder of the project, with what they hold.
fn paths(kind: &BlockKind) -> Vec<(String, &str)> {
    let mut found = files_read(kind);
    match kind {
        BlockKind::Check {
            of: CheckOf::Screen(reference),
            ..
        } => {
            found.push(("reference.image".to_owned(), &reference.image));
        }
        BlockKind::Command {
            folder: Some(folder),
            ..
        } => found.push(("folder".to_owned(), folder)),
        _ => {}
    }
    found.retain(|(_, path)| !path.is_empty());
    found
}

/// The problems with `path`, read from `folder` of the project; `base` gives the place in the test and
/// `missing` the problem, if any, when there is no such file, given its path from the project
/// folder.
fn path_issues(
    disk: &Disk,
    folder: &str,
    path: &str,
    base: impl Fn() -> Issue,
    missing: impl FnOnce(&str) -> Option<Issue>,
) -> Vec<Issue> {
    match disk.look(folder, path) {
        Look::Absolute => vec![named(base(), "check.path.absolute", path)],
        Look::Backslash => {
            vec![named(base(), "check.path.backslash", path).with("fixed", path.replace('\\', "/"))]
        }
        Look::Outside => vec![named(base(), "check.path.outside", path)],
        Look::Unknown => Vec::new(),
        Look::Missing(file) => missing(&file).into_iter().collect(),
        Look::There(there) => there.issues(base, path),
    }
}

fn named(base: Issue, code: &str, path: &str) -> Issue {
    Issue {
        code: code.to_owned(),
        ..base
    }
    .with("path", path)
}

/// What a path names in the project.
enum Look {
    /// It starts at the top of the disk, the home folder or a Windows drive.
    Absolute,
    /// It separates folders with `\`, which only Windows does.
    Backslash,
    /// Its `..` lead out of the project folder.
    Outside,
    /// Placeholders choose it while the test runs.
    Unknown,
    /// Nothing is there; the path from the project folder.
    Missing(String),
    There(There),
}

/// A file or folder a path names.
struct There {
    /// From the project folder, as the test writes it.
    written: String,
    /// From the project folder, as the folders name it.
    actual: String,
    /// It is a link, or in a folder that is a link, to a place outside the project.
    linked_out: bool,
    /// The size of a file.
    bytes: u64,
}

impl There {
    fn issues(&self, base: impl Fn() -> Issue, path: &str) -> Vec<Issue> {
        let mut out = Vec::new();
        if self.linked_out {
            out.push(named(base(), "check.path.link", path));
        }
        if self.written != self.actual {
            out.push(
                named(base(), "check.path.case", path)
                    .with("file", &self.written)
                    .with("actual", &self.actual),
            );
        }
        if self.bytes >= LARGE_FILE_BYTES {
            out.push(Issue {
                severity: Severity::Warning,
                ..named(base(), "check.file.large", &self.actual)
                    .with("bytes", self.bytes.to_string())
            });
        }
        out
    }
}

/// The project folder, with the names of the folders listed so far: a test names many files in
/// the same few folders.
struct Disk<'a> {
    /// A real path, without links.
    root: &'a Path,
    listed: RefCell<HashMap<PathBuf, Option<Vec<String>>>>,
}

impl<'a> Disk<'a> {
    fn new(root: &'a Path) -> Self {
        Disk {
            root,
            listed: RefCell::default(),
        }
    }

    /// Finds what `path`, relative to `folder` of the project folder, names. With placeholders,
    /// only the part before the first one is known.
    fn look(&self, folder: &str, path: &str) -> Look {
        if is_elsewhere(path) {
            return Look::Absolute;
        }
        let known = path.find("${").map_or(path, |at| &path[..at]);
        if known.contains('\\') {
            return Look::Backslash;
        }
        let mut parts: Vec<&str> = folder.split('/').filter(|p| !p.is_empty()).collect();
        let mut steps: Vec<&str> = known.split('/').collect();
        if known.len() < path.len() {
            // A placeholder ends the last part.
            steps.pop();
        }
        for step in steps {
            match step {
                "" | "." => {}
                ".." => {
                    if parts.pop().is_none() {
                        return Look::Outside;
                    }
                }
                part => parts.push(part),
            }
        }
        if known.len() < path.len() {
            return Look::Unknown;
        }
        let written = parts.join("/");
        let mut at = self.root.to_path_buf();
        let mut actual = Vec::new();
        for part in &parts {
            let Some(name) = self.name_on_disk(&at, part) else {
                return Look::Missing(written);
            };
            at.push(&name);
            actual.push(name);
        }
        let Ok(real) = at.canonicalize() else {
            return Look::Missing(written);
        };
        Look::There(There {
            linked_out: !real.starts_with(self.root),
            bytes: fs::metadata(&real)
                .ok()
                .filter(|m| m.is_file())
                .map_or(0, |m| m.len()),
            actual: actual.join("/"),
            written,
        })
    }

    /// The name `folder` lists for `name`: the same, or the same but for capital letters, which a
    /// Mac's disk doesn't tell apart and Linux's does.
    fn name_on_disk(&self, folder: &Path, name: &str) -> Option<String> {
        let mut listed = self.listed.borrow_mut();
        let names = listed
            .entry(folder.to_path_buf())
            .or_insert_with(|| {
                let entries = fs::read_dir(folder).ok()?;
                Some(
                    entries
                        .flatten()
                        .filter_map(|e| e.file_name().into_string().ok())
                        .collect(),
                )
            })
            .as_ref()?;
        if names.iter().any(|n| n == name) {
            return Some(name.to_owned());
        }
        let lower = name.to_lowercase();
        names
            .iter()
            .find(|n| n.to_lowercase() == lower)
            .cloned()
            // The disk may keep the letters of the name composed another way.
            .or_else(|| {
                folder
                    .join(name)
                    .symlink_metadata()
                    .is_ok()
                    .then(|| name.to_owned())
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_project_is_the_nearest_marked_folder_below_the_home_folder() {
        let top = std::env::temp_dir().join(format!("jevision-check-root-{}", std::process::id()));
        let _ = fs::remove_dir_all(&top);
        let home = top.join("home");
        for folder in [".jevision", "Desktop", "ui/.jevision", "ui/login"] {
            fs::create_dir_all(home.join(folder)).unwrap();
        }
        fs::create_dir_all(top.join("disk/app/tests")).unwrap();
        fs::write(top.join("disk/app/jevision.environments.json"), "{}").unwrap();
        let top = top.canonicalize().unwrap();
        let home = top.join("home");
        let root = |dir: &str| project_of(&top.join(dir), Some(&home));

        assert_eq!(
            root("home/ui/login"),
            Some((home.join("ui"), "login".to_owned()))
        );
        assert_eq!(root("home/ui"), Some((home.join("ui"), String::new())));
        assert_eq!(
            root("home/Desktop"),
            None,
            "a run in the home folder made its .jevision"
        );
        assert_eq!(root("home"), None);
        assert_eq!(
            root("disk/app/tests"),
            Some((top.join("disk/app"), "tests".to_owned()))
        );
        assert_eq!(
            project_of(&top.join("home/Desktop"), None),
            Some((home.clone(), "Desktop".to_owned())),
            "without a home folder, any marked folder"
        );
        fs::remove_dir_all(top).unwrap();
    }
}
