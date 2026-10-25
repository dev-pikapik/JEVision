//! Project-file requirements, retained settings fields, and backups before writing files in a
//! newer format, so that an older JEVision can have them back.
//!
//! A test is always written in the lowest format that holds it: opening and saving a test never
//! makes it unreadable for the JEVision that made it. Only a test that uses something newer is
//! raised, and then the first time that raises the project asks before it does.

mod settings;
pub use settings::{SettingsFileError, SettingsMetadata, parse_settings_document};
pub(crate) use settings::{
    UniqueValue, check_settings_header, check_settings_value, read_settings_json,
};

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Component, Path};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, SecondsFormat, Utc};
use jevision_l10n::tr;
use serde::{Deserialize, Serialize};

use crate::atomic;
use crate::decode::{FORMAT_CHANGES, OLDEST_FORMAT};
use crate::issue::Issue;
use crate::model::{BLOCK_SUFFIX, BlockKind, Flow, TEMPLATE_SUFFIX, TEST_SUFFIX};
use crate::package::visit_files;
use crate::package_files::project_files;
use crate::resources::{META_SUFFIX, RESOURCE_PREFIX, ResourceIndex, resource_id};
use crate::validate::is_app_link;
use crate::{parse, to_canonical_json};

pub const TEST_SCHEMA: &str = "https://dev-pikapik.github.io/JEVision/schemas/test.schema.json";
/// The format of tests that name a file by its label.
pub const LABELS_FORMAT: u32 = 2;
/// The format of tests that run their app in the iOS Simulator or open links into apps.
pub const SIMULATOR_FORMAT: u32 = 3;
/// The format of tests that have an `ask` block: the same format, which came with both.
pub const ASK_FORMAT: u32 = 3;
/// Where the backups of a project are, relative to it.
pub const BACKUPS_FOLDER: &str = ".jevision/backups";
/// The most backups a project keeps; the oldest go first.
pub const MAX_BACKUPS: usize = 20;
/// The most space the backups of a project take, in bytes; the newest backup is kept even when
/// it alone is larger.
pub const MAX_BACKUP_BYTES: u64 = 500 << 20;
const BACKUP_FILE: &str = "backup.json";
const BACKUP_FORMAT: u32 = 1;

/// What a JEVision must read to open a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Requirement {
    pub format: u32,
}

impl Requirement {
    /// The lowest format that holds `flow`.
    pub fn of(flow: &Flow) -> Self {
        let mut labels = false;
        visit_files(&mut flow.clone(), |_, _, path| {
            labels |= resource_id(path).is_some();
        });
        let app_links = flow.blocks.iter().any(|b| match &b.kind {
            BlockKind::OpenFile { files, .. } => files.iter().any(|f| is_app_link(f)),
            _ => false,
        });
        let ask = flow
            .blocks
            .iter()
            .any(|b| matches!(b.kind, BlockKind::Ask { .. }));
        let format = if flow.simulator.is_some() || app_links {
            SIMULATOR_FORMAT
        } else if ask {
            ASK_FORMAT
        } else if labels {
            LABELS_FORMAT
        } else {
            OLDEST_FORMAT
        };
        Self { format }
    }

    /// The format a saved file says it is in, without reading the rest of it.
    fn of_text(text: &str) -> Option<Self> {
        let value: serde_json::Value = serde_json::from_str(text).ok()?;
        let format = u32::try_from(value.get("format")?.as_u64()?).ok()?;
        Some(Self { format })
    }
}

/// The tests, custom blocks and templates of `project`, relative to it.
pub(crate) fn test_files(project: &Path) -> Vec<String> {
    project_files(project, &[TEST_SUFFIX, BLOCK_SUFFIX, TEMPLATE_SUFFIX])
}

/// The highest format among the tests of `project`: what a JEVision must read to open all of
/// them.
pub fn project_requirement(project: &Path) -> Requirement {
    test_files(project)
        .iter()
        .filter_map(|file| Requirement::of_text(&fs::read_to_string(project.join(file)).ok()?))
        .max()
        .unwrap_or(Requirement {
            format: OLDEST_FORMAT,
        })
}

/// What [`save_test`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveOutcome {
    Saved,
    /// Nothing was written: the test needs `format`, which no other test of the project needs
    /// yet, so an older JEVision could no longer open the project. `description` says, in the
    /// process language, what the format adds. Saving again with `allow_raise` writes it.
    NeedsConfirmation {
        format: u32,
        description: String,
    },
}

/// A path to a file in the project, refused when it leaves it.
fn checked(path: &str) -> Result<&str, Issue> {
    let fits = crate::validate::is_relative_path(path)
        && Path::new(path)
            .components()
            .all(|c| matches!(c, Component::Normal(_)));
    fits.then_some(path)
        .ok_or_else(|| Issue::error("save.path.invalid").with("path", path))
}

fn failed(path: &str, e: &io::Error) -> Issue {
    Issue::error("save.failed")
        .with("path", path)
        .with("reason", e)
}

fn write_creating(file: &Path, bytes: impl AsRef<[u8]>) -> io::Result<()> {
    if let Some(folder) = file.parent() {
        fs::create_dir_all(folder)?;
    }
    atomic::write(file, bytes)
}

/// Writes `flow` as the test `path` of `project`, all at once. A test that needs a newer format
/// than every other test of the project is written only with `allow_raise`; before a test is
/// written in a newer format than it had, the old file is copied to a backup.
pub fn save_test(
    project: &Path,
    path: &str,
    flow: &Flow,
    allow_raise: bool,
) -> Result<SaveOutcome, Issue> {
    let path = checked(path)?;
    let file = project.join(path);
    let text = to_canonical_json(flow);
    let needs = Requirement::of(flow);
    let old = match fs::read_to_string(&file) {
        Ok(old) => Some(old),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(failed(path, &e)),
    };
    let had = old.as_deref().and_then(Requirement::of_text);
    let replaced = had.unwrap_or(Requirement {
        format: OLDEST_FORMAT,
    });
    if needs > replaced && needs > project_requirement(project) && !allow_raise {
        return Ok(SaveOutcome::NeedsConfirmation {
            format: needs.format,
            description: format_change(needs.format),
        });
    }
    if old.as_deref() == Some(text.as_str()) {
        return Ok(SaveOutcome::Saved);
    }
    if let Some(had) = had.filter(|had| needs > *had) {
        backup(project, &[(path.to_owned(), had.format, needs.format)])
            .map_err(|e| Issue::error("backup.failed").with("reason", e))?;
    }
    write_creating(&file, text).map_err(|e| failed(path, &e))?;
    Ok(SaveOutcome::Saved)
}

/// What the format `format` of tests, newer than the oldest, adds, in the process language.
fn format_change(format: u32) -> String {
    tr!(FORMAT_CHANGES[(format - OLDEST_FORMAT - 1) as usize])
}

/// A copy of files of a project, made before they were written in a newer format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backup {
    /// Its folder in `.jevision/backups`: when it was made, in UTC, so that ids sort by age.
    pub id: String,
    /// When it was made, in UTC, like `2026-10-21T09:30:00Z`.
    pub created: String,
    pub files: Vec<BackupFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackupFile {
    /// Relative to the project.
    pub path: String,
    pub from_format: u32,
    pub to_format: u32,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BackupJson {
    format: u32,
    created: String,
    files: Vec<BackupFile>,
}

/// Copies the files of `project` listed with the formats they are written from and to into a
/// new backup, and returns its id. Files that don't exist yet are left out. Old backups are
/// removed after it, as [`MAX_BACKUPS`] and [`MAX_BACKUP_BYTES`] say.
pub(crate) fn backup(project: &Path, files: &[(String, u32, u32)]) -> io::Result<String> {
    backup_within(project, files, MAX_BACKUPS, MAX_BACKUP_BYTES)
}

fn backup_within(
    project: &Path,
    files: &[(String, u32, u32)],
    max_count: usize,
    max_bytes: u64,
) -> io::Result<String> {
    let backups = project.join(BACKUPS_FOLDER);
    fs::create_dir_all(&backups)?;
    keep_out_of_git(&backups)?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let now =
        DateTime::<Utc>::from_timestamp(i64::try_from(now).unwrap_or(0), 0).unwrap_or_default();
    let stamp = now.format("%Y%m%dT%H%M%SZ").to_string();
    let mut made = None;
    for n in 1..1000 {
        let id = format!("{stamp}-{n:03}");
        match fs::create_dir(backups.join(&id)) {
            Ok(()) => {
                made = Some(id);
                break;
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    let id = made.ok_or_else(|| io::Error::other("too many backups in one second"))?;
    let folder = backups.join(&id);
    let mut listed = Vec::new();
    for (path, from, to) in files {
        let bytes = match fs::read(project.join(path)) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        };
        write_creating(&folder.join(path), bytes)?;
        listed.push(BackupFile {
            path: path.clone(),
            from_format: *from,
            to_format: *to,
        });
    }
    let json = BackupJson {
        format: BACKUP_FORMAT,
        created: now.to_rfc3339_opts(SecondsFormat::Secs, true),
        files: listed,
    };
    let mut text = serde_json::to_string_pretty(&json).expect("a backup list serializes");
    text.push('\n');
    atomic::write(&folder.join(BACKUP_FILE), text)?;
    prune(&backups, &id, max_count, max_bytes);
    Ok(id)
}

/// Backups are for this Mac only: a `.gitignore` in their folder keeps them out of git, even in
/// a project whose own `.gitignore` doesn't name `.jevision`.
fn keep_out_of_git(backups: &Path) -> io::Result<()> {
    let file = backups.join(".gitignore");
    if fs::read_to_string(&file).is_ok_and(|text| text == "*\n") {
        return Ok(());
    }
    atomic::write(&file, "*\n")
}

/// Removes the oldest backups but `keep` until there are at most `max_count` and they take at
/// most `max_bytes`. A backup that can't be removed is left as it is.
fn prune(backups: &Path, keep: &str, max_count: usize, max_bytes: u64) {
    let mut ids: Vec<String> = listed(backups).into_iter().map(|b| b.id).collect();
    ids.sort();
    let mut sizes: BTreeMap<String, u64> = ids
        .iter()
        .map(|id| (id.clone(), folder_size(&backups.join(id))))
        .collect();
    let mut count = ids.len();
    for id in ids.iter().filter(|id| *id != keep) {
        let total: u64 = sizes.values().sum();
        if count <= max_count && total <= max_bytes {
            break;
        }
        if fs::remove_dir_all(backups.join(id)).is_ok() {
            sizes.remove(id);
            count -= 1;
        }
    }
}

fn folder_size(folder: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(folder) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match entry.file_type() {
            Ok(kind) if kind.is_dir() => folder_size(&entry.path()),
            Ok(kind) if kind.is_file() => entry.metadata().map_or(0, |m| m.len()),
            _ => 0,
        })
        .sum()
}

/// The backups in `backups`: folders with a `backup.json` that can be read, oldest first.
fn listed(backups: &Path) -> Vec<Backup> {
    let Ok(entries) = fs::read_dir(backups) else {
        return Vec::new();
    };
    let mut found: Vec<Backup> = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| {
            let id = entry.file_name().into_string().ok()?;
            let text = fs::read_to_string(entry.path().join(BACKUP_FILE)).ok()?;
            let json: BackupJson = serde_json::from_str(&text).ok()?;
            let valid =
                json.format == BACKUP_FORMAT && json.files.iter().all(|f| checked(&f.path).is_ok());
            valid.then_some(Backup {
                id,
                created: json.created,
                files: json.files,
            })
        })
        .collect();
    found.sort_by(|a, b| a.id.cmp(&b.id));
    found
}

/// The backups of `project`, newest first.
pub fn list_backups(project: &Path) -> Vec<Backup> {
    let mut backups = listed(&project.join(BACKUPS_FOLDER));
    backups.reverse();
    backups
}

/// Puts the files of the backup `id` back in `project` and returns their paths. The files as
/// they are now are backed up first, so that this can be undone the same way.
pub fn restore_backup(project: &Path, id: &str) -> Result<Vec<String>, Issue> {
    let backups = project.join(BACKUPS_FOLDER);
    let not_found = || Issue::error("backup.notFound").with("id", id);
    let backup = listed(&backups)
        .into_iter()
        .find(|b| b.id == id)
        .ok_or_else(not_found)?;
    let current: Vec<(String, u32, u32)> = backup
        .files
        .iter()
        .filter_map(|f| {
            let text = fs::read_to_string(project.join(&f.path)).ok()?;
            let now = Requirement::of_text(&text).map_or(f.to_format, |r| r.format);
            Some((f.path.clone(), now, f.from_format))
        })
        .collect();
    if !current.is_empty() {
        self::backup(project, &current)
            .map_err(|e| Issue::error("backup.failed").with("reason", e))?;
    }
    let folder = backups.join(id);
    let mut restored = Vec::new();
    for file in &backup.files {
        let bytes = fs::read(folder.join(&file.path)).map_err(|e| failed(&file.path, &e))?;
        write_creating(&project.join(&file.path), bytes).map_err(|e| failed(&file.path, &e))?;
        restored.push(file.path.clone());
    }
    Ok(restored)
}

/// What [`migrate_project`] did, or would do on a dry run.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MigrationReport {
    /// The files written in a newer format, by path.
    pub files: Vec<MigratedFile>,
    /// Files that couldn't be read, which were left as they are.
    pub problems: Vec<MigrationProblem>,
    /// The backup with the files as they were; `None` on a dry run or when nothing changed.
    pub backup: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigratedFile {
    /// Relative to the project.
    pub path: String,
    pub from_format: u32,
    pub to_format: u32,
    /// The paths in the file that name a file by its label now.
    pub labels: Vec<NewLabel>,
    /// The `.jevmeta` files made for them, relative to the project.
    pub new_sidecars: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewLabel {
    /// The path as the test wrote it.
    pub path: String,
    /// `res:…`; `None` on a dry run for a file that has no label yet.
    pub reference: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationProblem {
    pub path: String,
    pub issues: Vec<Issue>,
}

/// A test to migrate: its flow, and for each file path it names, in the order
/// [`visit_files`] goes, the file in the project to label, if any.
struct Planned {
    path: String,
    flow: Flow,
    from: u32,
    targets: Vec<Option<String>>,
}

/// Brings every test, custom block and template of `project` to the newest format: each path
/// to a file in the project becomes its label, and the file gets a `.jevmeta` when it has none.
/// Files that can't be read, or need a newer JEVision, are listed and left alone. All the
/// files changed are first copied to one backup. A dry run writes nothing.
pub fn migrate_project(project: &Path, dry_run: bool) -> Result<MigrationReport, Issue> {
    let mut index = ResourceIndex::scan(project, None);
    let mut report = MigrationReport::default();
    let mut planned = Vec::new();
    for path in test_files(project) {
        let text = match fs::read_to_string(project.join(&path)) {
            Ok(text) => text,
            Err(e) => {
                report.problems.push(MigrationProblem {
                    issues: vec![failed(&path, &e)],
                    path,
                });
                continue;
            }
        };
        let flow = match parse(&text) {
            Ok(flow) => flow,
            Err(e) => {
                report.problems.push(MigrationProblem {
                    path,
                    issues: e.issues,
                });
                continue;
            }
        };
        let mut targets = Vec::new();
        visit_files(&mut flow.clone(), |_, _, named| {
            targets.push(index.labelable(&path, named));
        });
        if targets.iter().any(Option::is_some) {
            planned.push(Planned {
                from: Requirement::of_text(&text).map_or(OLDEST_FORMAT, |r| r.format),
                path,
                flow,
                targets,
            });
        }
    }
    if planned.is_empty() {
        return Ok(report);
    }
    if !dry_run {
        let files: Vec<(String, u32, u32)> = (planned.iter())
            .map(|p| (p.path.clone(), p.from, LABELS_FORMAT))
            .collect();
        report.backup = Some(
            backup(project, &files).map_err(|e| Issue::error("backup.failed").with("reason", e))?,
        );
    }
    let mut sidecars_made = std::collections::BTreeSet::new();
    for mut p in planned {
        let mut labels = Vec::new();
        let mut new_sidecars = Vec::new();
        let mut targets = p.targets.into_iter();
        let mut error = None;
        visit_files(&mut p.flow, |_, _, named| {
            let Some(file) = targets.next().flatten() else {
                return;
            };
            let had = index.label_of(&file);
            if had.is_none() && sidecars_made.insert(file.clone()) {
                new_sidecars.push(format!("{file}{META_SUFFIX}"));
            }
            let id = if dry_run {
                had
            } else {
                match index.label(Path::new(&file)) {
                    Ok(id) => Some(id),
                    Err(issue) => {
                        error.get_or_insert(issue);
                        return;
                    }
                }
            };
            let reference = id.map(|id| format!("{RESOURCE_PREFIX}{id}"));
            labels.push(NewLabel {
                path: named.clone(),
                reference: reference.clone(),
            });
            if let Some(reference) = reference.filter(|_| !dry_run) {
                *named = reference;
            }
        });
        if let Some(issue) = error {
            return Err(issue);
        }
        if !dry_run {
            write_creating(&project.join(&p.path), to_canonical_json(&p.flow))
                .map_err(|e| failed(&p.path, &e))?;
        }
        report.files.push(MigratedFile {
            path: p.path,
            from_format: p.from,
            to_format: LABELS_FORMAT,
            labels,
            new_sidecars,
        });
    }
    if !dry_run {
        crate::resources::refresh_resources(project);
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("jevision-compat-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    fn put(root: &Path, path: &str, text: &str) {
        let file = root.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
    }

    fn read(root: &Path, path: &str) -> String {
        fs::read_to_string(root.join(path)).unwrap()
    }

    fn click(image: &str) -> String {
        to_canonical_json(
            &parse(&format!(
                r#"{{"format": 2, "name": "T", "app": "com.example.App", "start": "a",
                "blocks": {{"a": {{"type": "click", "target": {{"image": "{image}"}}}}}}}}"#
            ))
            .unwrap(),
        )
    }

    fn ids(project: &Path) -> Vec<String> {
        list_backups(project).into_iter().map(|b| b.id).collect()
    }

    const LABEL: &str = "res:0123456789abcdef";

    #[test]
    fn backups_stay_out_of_git() {
        let project = temp("git");
        let git = |args: &[&str]| {
            Command::new("git")
                .args(args)
                .current_dir(&project)
                .output()
        };
        if git(&["--version"]).is_err() {
            eprintln!("git isn't installed: the check that backups stay out of git is skipped");
            return;
        }
        assert!(git(&["init", "-q"]).unwrap().status.success());
        put(&project, "a.flow.json", &click("go.png"));
        backup(&project, &[("a.flow.json".to_owned(), 1, 2)]).unwrap();
        assert_eq!(read(&project, ".jevision/backups/.gitignore"), "*\n");
        let status = git(&["status", "--porcelain", "--untracked-files=all"]).unwrap();
        let status = String::from_utf8(status.stdout).unwrap();
        assert!(status.contains("a.flow.json"), "{status}");
        assert!(!status.contains(".jevision"), "{status}");
    }

    #[test]
    fn at_most_twenty_backups_are_kept() {
        let project = temp("count");
        put(&project, "a.flow.json", &click("go.png"));
        let made: Vec<String> = (0..=MAX_BACKUPS)
            .map(|_| backup(&project, &[("a.flow.json".to_owned(), 1, 2)]).unwrap())
            .collect();
        let mut kept = ids(&project);
        kept.reverse();
        assert_eq!(kept, made[1..]);
    }

    #[test]
    fn the_oldest_backups_go_when_they_take_too_much_space() {
        let project = temp("size");
        put(&project, "a.flow.json", &"x".repeat(1000));
        let files = [("a.flow.json".to_owned(), 1, 2)];
        let made: Vec<String> = (0..3)
            .map(|_| backup_within(&project, &files, MAX_BACKUPS, 2500).unwrap())
            .collect();
        assert_eq!(ids(&project), [made[2].clone(), made[1].clone()]);
        let newest = backup_within(&project, &files, MAX_BACKUPS, 10).unwrap();
        assert_eq!(
            ids(&project),
            [newest],
            "the newest is kept even when it alone is too big"
        );
    }

    #[test]
    fn only_folders_with_a_backup_list_are_backups() {
        let project = temp("list");
        put(&project, "a.flow.json", &click("go.png"));
        let id = backup(&project, &[("a.flow.json".to_owned(), 1, 2)]).unwrap();
        let backups = project.join(BACKUPS_FOLDER);
        put(&backups, "empty/a.flow.json", "{}");
        put(&backups, "broken/backup.json", "{");
        put(
            &backups,
            "outside/backup.json",
            r#"{"format": 1, "created": "2026-01-01T00:00:00Z",
                "files": [{"path": "../x", "fromFormat": 1, "toFormat": 2}]}"#,
        );
        let listed = list_backups(&project);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, id);
        assert_eq!(
            listed[0].files,
            [BackupFile {
                path: "a.flow.json".to_owned(),
                from_format: 1,
                to_format: 2,
            }]
        );
        assert!(listed[0].created.ends_with('Z') && listed[0].created.len() == 20);
        assert_eq!(
            restore_backup(&project, "outside").unwrap_err().code,
            "backup.notFound"
        );
    }

    #[test]
    fn a_newer_format_is_written_only_when_the_person_agrees() {
        let project = temp("save");
        let old = click("go.png");
        put(&project, "tests/a.flow.json", &old);
        let flow = parse(&click(LABEL)).unwrap();

        let asked = save_test(&project, "tests/a.flow.json", &flow, false).unwrap();
        let SaveOutcome::NeedsConfirmation {
            format,
            description,
        } = asked
        else {
            panic!("{asked:?}");
        };
        assert_eq!(format, LABELS_FORMAT);
        assert!(!description.is_empty());
        assert_eq!(read(&project, "tests/a.flow.json"), old);
        assert!(list_backups(&project).is_empty());

        let saved = save_test(&project, "tests/a.flow.json", &flow, true);
        assert_eq!(saved, Ok(SaveOutcome::Saved));
        assert_eq!(read(&project, "tests/a.flow.json"), click(LABEL));
        let [made] = &list_backups(&project)[..] else {
            panic!("one backup");
        };
        let copy = project.join(BACKUPS_FOLDER).join(&made.id);
        assert_eq!(read(&copy, "tests/a.flow.json"), old);

        // The project needs the format now: other tests take it without asking.
        let saved = save_test(&project, "tests/b.flow.json", &flow, false);
        assert_eq!(saved, Ok(SaveOutcome::Saved));
        assert_eq!(
            list_backups(&project).len(),
            1,
            "a new file has nothing to back up"
        );
        let plain = parse(&click("go.png")).unwrap();
        assert_eq!(
            save_test(&project, "tests/a.flow.json", &plain, false),
            Ok(SaveOutcome::Saved)
        );
        assert!(read(&project, "tests/a.flow.json").contains("\"format\": 1,"));
        assert_eq!(
            list_backups(&project).len(),
            1,
            "a lower format needs no backup"
        );

        let outside = save_test(&project, "../a.flow.json", &plain, true).unwrap_err();
        assert_eq!(outside.code, "save.path.invalid");
    }

    #[test]
    fn a_backup_is_put_back_after_the_files_as_they_are_are_backed_up() {
        let project = temp("restore");
        let old = click("go.png");
        put(&project, "a.flow.json", &old);
        save_test(
            &project,
            "a.flow.json",
            &parse(&click(LABEL)).unwrap(),
            true,
        )
        .unwrap();
        let id = ids(&project).remove(0);

        assert_eq!(
            restore_backup(&project, &id),
            Ok(vec!["a.flow.json".to_owned()])
        );
        assert_eq!(read(&project, "a.flow.json"), old);
        let listed = list_backups(&project);
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].files[0].from_format, LABELS_FORMAT);
        assert_eq!(listed[0].files[0].to_format, 1);
        let undo = project.join(BACKUPS_FOLDER).join(&listed[0].id);
        assert_eq!(read(&undo, "a.flow.json"), click(LABEL));
        assert_eq!(
            restore_backup(&project, "none").unwrap_err().code,
            "backup.notFound"
        );
    }

    #[test]
    fn migrating_a_project_labels_its_files_after_one_backup() {
        let project = temp("migrate");
        put(&project, "images/go.png", "go");
        put(&project, "tests/a.flow.json", &click("../images/go.png"));
        put(&project, "tests/b.flow.json", &click("../images/go.png"));
        put(&project, "tests/c.flow.json", &click("../missing.png"));
        put(&project, "tests/broken.flow.json", "{");
        let before = read(&project, "tests/a.flow.json");

        let dry = migrate_project(&project, true).unwrap();
        assert_eq!(dry.backup, None);
        assert_eq!(dry.problems.len(), 1);
        assert_eq!(dry.problems[0].path, "tests/broken.flow.json");
        assert_eq!(dry.problems[0].issues[0].code, "flow.json.syntax");
        let paths: Vec<&str> = dry.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["tests/a.flow.json", "tests/b.flow.json"]);
        assert_eq!(
            dry.files[0].labels,
            [NewLabel {
                path: "../images/go.png".to_owned(),
                reference: None,
            }]
        );
        assert_eq!(dry.files[0].new_sidecars, ["images/go.png.jevmeta"]);
        assert!(
            dry.files[1].new_sidecars.is_empty(),
            "each label is made once"
        );
        assert_eq!((dry.files[0].from_format, dry.files[0].to_format), (1, 2));
        assert_eq!(read(&project, "tests/a.flow.json"), before);
        assert!(!project.join("images/go.png.jevmeta").exists());
        assert!(
            !project.join(".jevision").exists(),
            "a dry run writes nothing"
        );

        let done = migrate_project(&project, false).unwrap();
        let reference = done.files[0].labels[0].reference.clone().unwrap();
        assert_eq!(done.files[1].labels[0].reference, Some(reference.clone()));
        assert_eq!(read(&project, "tests/a.flow.json"), click(&reference));
        assert_eq!(read(&project, "tests/c.flow.json"), click("../missing.png"));
        assert_eq!(read(&project, "tests/broken.flow.json"), "{");
        assert!(project.join("images/go.png.jevmeta").exists());
        let [made] = &list_backups(&project)[..] else {
            panic!("one backup");
        };
        assert_eq!(Some(&made.id), done.backup.as_ref());
        assert_eq!(made.files.len(), 2);
        let copy = project.join(BACKUPS_FOLDER).join(&made.id);
        assert_eq!(read(&copy, "tests/a.flow.json"), before);

        let again = migrate_project(&project, false).unwrap();
        assert!(again.files.is_empty() && again.backup.is_none());
    }
}
