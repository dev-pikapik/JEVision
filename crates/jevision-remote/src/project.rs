//! Reading a shared project for a phone. Every path a phone sends is relative and is checked
//! to stay inside the project folder after symlinks are resolved.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use jevision_core::{BlockKind, DataTable, Target, WaitUntil};
use jevision_runner::{Run, RunReport};

use crate::RemoteError;
use crate::wire::{
    Area, BlockSummary, EdgeSummary, FrameSummary, LastRun, MAX_MESSAGE, MAX_PICTURE, Position,
    RunState, RunStatus, RunSummary, TestEntry, TestSummary,
};

const TEST_SUFFIX: &str = ".flow.json";

/// The project folder with symlinks resolved.
pub(crate) fn root(folder: &Path) -> Result<PathBuf, RemoteError> {
    fs::canonicalize(folder).map_err(|_| RemoteError::ProjectNotFound)
}

/// The test file `path` inside the project at `root` (canonical).
pub(crate) fn test_file(root: &Path, path: &str) -> Result<PathBuf, RemoteError> {
    if !path.ends_with(TEST_SUFFIX) {
        return Err(RemoteError::TestNotFound);
    }
    let file = inside(root, &relative(path)?, RemoteError::TestNotFound)?;
    if file.is_file() {
        Ok(file)
    } else {
        Err(RemoteError::TestNotFound)
    }
}

/// Every test in the project and the folders that hold them. Hidden files and folders, such
/// as `.jevision` and `.git`, are left out, and so are folders reached through symlinks.
pub(crate) fn tests(root: &Path) -> Result<Vec<TestEntry>, RemoteError> {
    let mut last_runs: HashMap<String, LastRun> = HashMap::new();
    for run in Run::list(root).unwrap_or_default() {
        last_runs
            .entry(run.report.file.clone())
            .or_insert_with(|| LastRun {
                run: dir_name(&run.dir),
                verdict: run.report.verdict,
                started_at: run.report.started_at.clone(),
            });
    }
    let mut files = Vec::new();
    let mut folders = vec![PathBuf::new()];
    while let Some(folder) = folders.pop() {
        let Ok(entries) = fs::read_dir(root.join(&folder)) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if name.starts_with('.') {
                continue;
            }
            let path = folder.join(&name);
            if kind.is_dir() {
                folders.push(path);
            } else if name.ends_with(TEST_SUFFIX)
                && (kind.is_file()
                    || (kind.is_symlink()
                        && inside(root, &path, RemoteError::TestNotFound)
                            .is_ok_and(|file| file.is_file())))
            {
                files.push(path);
            }
        }
    }

    let mut entries: HashMap<PathBuf, TestEntry> = HashMap::new();
    for file in files {
        let path = slash_path(&file);
        let name = fs::read_to_string(root.join(&file))
            .ok()
            .and_then(|text| jevision_core::parse(&text).ok())
            .map_or_else(
                || {
                    path.rsplit('/')
                        .next()
                        .unwrap_or(&path)
                        .trim_end_matches(TEST_SUFFIX)
                        .to_owned()
                },
                |flow| flow.name,
            );
        for folder in file
            .ancestors()
            .skip(1)
            .filter(|f| !f.as_os_str().is_empty())
        {
            entries
                .entry(folder.to_path_buf())
                .or_insert_with(|| TestEntry {
                    path: slash_path(folder),
                    name: folder
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                    is_folder: true,
                    last_run: None,
                });
        }
        let last_run = last_runs.remove(&path);
        entries.insert(
            file,
            TestEntry {
                path,
                name,
                is_folder: false,
                last_run,
            },
        );
    }
    let mut entries: Vec<(PathBuf, TestEntry)> = entries.into_iter().collect();
    entries.sort_by(|(a, _), (b, _)| a.components().cmp(b.components()));
    Ok(entries.into_iter().map(|(_, entry)| entry).collect())
}

/// The test at `path` as the phone draws it.
pub(crate) fn summary(root: &Path, path: &str) -> Result<TestSummary, RemoteError> {
    let (file, flow) = read_test(root, path)?;
    let layout = jevision_core::layout(&flow);
    let area = |r: jevision_core::Rect| Area {
        x: r.x,
        y: r.y,
        width: r.width,
        height: r.height,
    };
    let blocks = flow
        .blocks_in_flow_order()
        .into_iter()
        .map(|block| BlockSummary {
            id: block.id.clone(),
            kind: block.kind.type_name().to_owned(),
            title: title(&block.kind),
            area: layout
                .blocks
                .iter()
                .find(|laid| laid.id == block.id)
                .map(|laid| area(laid.rect)),
        })
        .collect();
    Ok(TestSummary {
        name: flow.name.clone(),
        description: flow.description.clone(),
        app: flow.app.clone(),
        start: flow.start.clone(),
        width: layout.width,
        height: layout.height,
        blocks,
        frames: layout
            .frames
            .iter()
            .map(|f| FrameSummary {
                block: f.block.clone(),
                area: area(f.rect),
            })
            .collect(),
        edges: layout
            .edges
            .iter()
            .map(|e| EdgeSummary {
                from: e.from.clone(),
                edge: e.edge.clone(),
                to: e.to.clone(),
                points: e
                    .points
                    .iter()
                    .map(|p| Position { x: p.x, y: p.y })
                    .collect(),
            })
            .collect(),
        rows: row_labels(root, &file, &flow),
    })
}

/// What each row of the data table a run of the test at `path` goes through is called.
pub(crate) fn data_rows(root: &Path, path: &str) -> Result<Vec<String>, RemoteError> {
    let (file, flow) = read_test(root, path)?;
    Ok(row_labels(root, &file, &flow))
}

fn read_test(root: &Path, path: &str) -> Result<(PathBuf, jevision_core::Flow), RemoteError> {
    let file = test_file(root, path)?;
    if fs::metadata(&file).map_or(true, |m| m.len() > MAX_MESSAGE as u64) {
        return Err(RemoteError::TestUnreadable);
    }
    let text = fs::read_to_string(&file).map_err(|_| RemoteError::TestUnreadable)?;
    let flow = jevision_core::parse(&text).map_err(|_| RemoteError::TestUnreadable)?;
    Ok((file, flow))
}

/// Labels of the rows of the test's data table, the way the Mac's Data Rows menu reads them;
/// empty when there is no table, it can't be read or it lies outside the project, as the run
/// would refuse it then.
fn row_labels(root: &Path, file: &Path, flow: &jevision_core::Flow) -> Vec<String> {
    let Some(data) = jevision_core::primary_data_file(flow) else {
        return Vec::new();
    };
    let Some(dir) = file.parent() else {
        return Vec::new();
    };
    match fs::canonicalize(dir.join(data)) {
        Ok(path) if path.starts_with(root) => DataTable::load(&path)
            .map(|table| (0..table.rows.len()).map(|row| table.label(row)).collect())
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn title(kind: &BlockKind) -> String {
    match kind {
        BlockKind::OpenApp { app, .. } => app.clone().unwrap_or_default(),
        BlockKind::OpenFile { files, .. } => files.join(", "),
        BlockKind::Click { target, .. } => match target {
            Target::Image(image) => image.clone(),
            Target::Point(p) => format!("{:.2}, {:.2}", p.x, p.y),
        },
        BlockKind::Type { text, .. } => text.clone(),
        BlockKind::PressKeys { keys, .. } => keys.clone(),
        BlockKind::WaitFor { until, .. } => match until {
            WaitUntil::Appears(r) | WaitUntil::Disappears(r) => r.image.clone(),
            WaitUntil::ScreenStable => String::new(),
        },
        BlockKind::Check { reference, .. } | BlockKind::If { reference, .. } => {
            reference.image.clone()
        }
        BlockKind::Repeat { times, .. } => times.to_string(),
        BlockKind::UseData { file, .. } => file.clone(),
        BlockKind::RunTest { test, .. } => test.clone(),
    }
}

/// Saved runs, newest first.
pub(crate) fn saved_runs(root: &Path) -> Vec<RunSummary> {
    Run::list(root)
        .unwrap_or_default()
        .into_iter()
        .map(|run| RunSummary {
            run: dir_name(&run.dir),
            test: run.report.test,
            file: run.report.file,
            started_at: run.report.started_at,
            duration_ms: run.report.duration_ms,
            verdict: run.report.verdict,
            flaky: run.report.flaky,
        })
        .collect()
}

/// The report of the saved run `run`.
pub(crate) fn report(root: &Path, run: &str) -> Result<RunReport, RemoteError> {
    Run::load(&run_dir(root, run)?)
        .map(|run| run.report)
        .map_err(|_| RemoteError::RunNotFound)
}

/// The status of a saved run.
pub(crate) fn saved_status(root: &Path, run: &str) -> Result<RunStatus, RemoteError> {
    let report = report(root, run)?;
    Ok(RunStatus {
        run: run.to_owned(),
        test: report.file,
        state: RunState::Finished,
        verdict: Some(report.verdict),
        attempt: report.attempts.len() as u32,
        block: None,
    })
}

/// The PNG bytes of `picture`, one of the images the run's report refers to.
pub(crate) fn picture(root: &Path, run: &str, picture: &str) -> Result<Vec<u8>, RemoteError> {
    let dir = run_dir(root, run)?;
    let report = Run::load(&dir)
        .map_err(|_| RemoteError::RunNotFound)?
        .report;
    let listed = report
        .attempts
        .iter()
        .flat_map(|a| &a.steps)
        .flat_map(|s| [&s.before, &s.after, &s.expected, &s.diff])
        .any(|image| image.as_deref() == Some(picture));
    let path = relative(picture)?;
    if !listed {
        return Err(RemoteError::PictureNotFound);
    }
    let file = inside(&dir, &path, RemoteError::PictureNotFound)?;
    match fs::metadata(&file) {
        Ok(m) if m.is_file() && m.len() > MAX_PICTURE as u64 => Err(RemoteError::TooLarge),
        Ok(m) if m.is_file() => fs::read(&file).map_err(|_| RemoteError::PictureNotFound),
        _ => Err(RemoteError::PictureNotFound),
    }
}

/// The folder of the run `run`: a single folder name inside `<project>/.jevision/runs`.
fn run_dir(root: &Path, run: &str) -> Result<PathBuf, RemoteError> {
    check_run(run)?;
    let runs = fs::canonicalize(root.join(".jevision").join("runs"))
        .map_err(|_| RemoteError::RunNotFound)?;
    if !runs.starts_with(root) {
        return Err(RemoteError::PathOutsideProject);
    }
    inside(&runs, Path::new(run), RemoteError::RunNotFound)
}

/// A run is named by its folder: one plain name, no separators, `.` or `..`.
pub(crate) fn check_run(run: &str) -> Result<(), RemoteError> {
    let mut parts = Path::new(run).components();
    match (parts.next(), parts.next()) {
        (Some(Component::Normal(_)), None) if !run.contains('/') => Ok(()),
        _ => Err(RemoteError::PathOutsideProject),
    }
}

/// A relative path made of plain names only: no root, no `.` or `..`.
fn relative(path: &str) -> Result<PathBuf, RemoteError> {
    let path = Path::new(path);
    let plain = path.components().all(|c| matches!(c, Component::Normal(_)));
    if path.as_os_str().is_empty() || !plain {
        return Err(RemoteError::PathOutsideProject);
    }
    Ok(path.to_path_buf())
}

/// `base/path` with symlinks resolved, refused unless it is still inside `base` (canonical).
fn inside(base: &Path, path: &Path, missing: RemoteError) -> Result<PathBuf, RemoteError> {
    let resolved = fs::canonicalize(base.join(path)).map_err(|e| match e.kind() {
        io::ErrorKind::NotFound => missing,
        _ => RemoteError::HostFailed,
    })?;
    if resolved.starts_with(base) {
        Ok(resolved)
    } else {
        Err(RemoteError::PathOutsideProject)
    }
}

fn slash_path(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn dir_name(dir: &Path) -> String {
    dir.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}
