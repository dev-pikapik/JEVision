//! Brings the run reports a CI job uploaded into the history of a project, so they sit next to
//! the runs made on this Mac.

use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Read, Seek, Write};
use std::path::{Component, Path, PathBuf};

use serde_json::{Value, json};
use zip::ZipArchive;

use crate::{CiOrigin, Run};

/// Which CI artifacts were brought in already, in `<project>/.jevision`.
const LEDGER: &str = "ci-artifacts.json";

/// All the files of one artifact together can't unpack to more than this.
const MAX_UNPACKED: u64 = 1 << 30;

/// The ids of the CI artifacts already brought into `project`'s history, including those that
/// held no runs, so none is downloaded twice. A run deleted from the history stays deleted.
pub fn imported_ci_artifacts(project: &Path) -> io::Result<BTreeSet<u64>> {
    let text = match fs::read_to_string(ledger(project)) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(BTreeSet::new()),
        Err(e) => return Err(e),
    };
    let ledger: Value = serde_json::from_str(&text)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("{LEDGER}: {e}")))?;
    Ok(ledger["artifacts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_u64)
        .collect())
}

/// Brings the runs in the artifact `id`, a zip, into `project`'s history and returns how many
/// came. Each lands in `.jevision/runs` as `ci-<id>-<name it had>`, so an artifact that is
/// brought in again adds nothing. Reports that don't say which CI job they come from get
/// `fallback`. Files that would leave the runs folder are never written. An artifact that is
/// not a readable zip is marked as done too, with [`io::ErrorKind::InvalidData`], so it isn't
/// fetched over and over.
pub fn import_ci_artifact(
    project: &Path,
    id: u64,
    zip: impl Read + Seek,
    fallback: &CiOrigin,
) -> io::Result<usize> {
    let mut done = imported_ci_artifacts(project)?;
    if done.contains(&id) {
        return Ok(0);
    }
    let runs = project.join(".jevision").join("runs");
    fs::create_dir_all(&runs)?;
    let staging = runs.join(format!(".ci-{id}.partial"));
    if let Err(e) = fs::remove_dir_all(&staging)
        && e.kind() != io::ErrorKind::NotFound
    {
        return Err(e);
    }
    let result = unpack(zip, &staging).and_then(|()| settle(&staging, &runs, id, fallback));
    let _ = fs::remove_dir_all(&staging);
    match &result {
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::InvalidData => {}
        Err(_) => return result,
    }
    done.insert(id);
    save_ledger(project, &done)?;
    result
}

/// Writes the run files of `zip` to `staging`: `<run folder>/<file>` for every file under
/// `.jevision/runs/`, wherever in the zip that folder is.
fn unpack(zip: impl Read + Seek, staging: &Path) -> io::Result<()> {
    let invalid = |e: zip::result::ZipError| match e {
        zip::result::ZipError::Io(e) => e,
        other => io::Error::new(io::ErrorKind::InvalidData, other),
    };
    let mut archive = ZipArchive::new(zip).map_err(invalid)?;
    let mut left = MAX_UNPACKED;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(invalid)?;
        if !entry.is_file() || entry.is_symlink() {
            continue;
        }
        let Some(inside) = entry.enclosed_name().and_then(|name| inside_runs(&name)) else {
            continue;
        };
        let target = staging.join(inside);
        if let Some(folder) = target.parent() {
            fs::create_dir_all(folder)?;
        }
        let mut file = fs::File::create(&target)?;
        // The size the zip declares is not trusted: count what really comes out. A zip that
        // can't be read to the end is broken; a file that can't be written is this Mac's trouble.
        let mut chunk = [0; 64 * 1024];
        loop {
            let n = entry
                .read(&mut chunk)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            if n == 0 {
                break;
            }
            left = left.checked_sub(n as u64).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "the CI files unpack to more than 1 GB",
                )
            })?;
            file.write_all(&chunk[..n])?;
        }
    }
    Ok(())
}

/// The part of `name` after `.jevision/runs/` (the last such place in the path); `None` for a
/// file elsewhere or one directly in the runs folder.
fn inside_runs(name: &Path) -> Option<PathBuf> {
    let parts: Vec<_> = name
        .components()
        .filter_map(|part| match part {
            Component::Normal(part) => Some(part),
            _ => None,
        })
        .collect();
    let at = parts
        .windows(2)
        .rposition(|pair| pair[0] == ".jevision" && pair[1] == "runs")?;
    let rest = &parts[at + 2..];
    (rest.len() >= 2).then(|| rest.iter().collect())
}

/// Moves the readable runs from `staging` into `runs`, and fills in where they came from.
fn settle(staging: &Path, runs: &Path, id: u64, fallback: &CiOrigin) -> io::Result<usize> {
    let mut found = match fs::read_dir(staging) {
        Ok(entries) => entries
            .filter_map(|entry| Some(entry.ok()?.path()))
            .filter(|dir| Run::load(dir).is_ok())
            .collect::<Vec<_>>(),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e),
    };
    found.sort();
    let mut brought = 0;
    for dir in found {
        let name = dir.file_name().unwrap_or_default().to_string_lossy();
        let target = runs.join(format!("ci-{id}-{name}"));
        if target.exists() {
            continue;
        }
        name_origin(&dir.join("report.json"), fallback)?;
        fs::rename(&dir, &target)?;
        brought += 1;
    }
    Ok(brought)
}

/// Adds `origin` to the report in `path` unless it already says where it came from. The report
/// is edited as it is, so what a newer version wrote there is kept.
fn name_origin(path: &Path, origin: &CiOrigin) -> io::Result<()> {
    let mut report: Value = serde_json::from_str(&fs::read_to_string(path)?)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let Some(fields) = report.as_object_mut() else {
        return Ok(());
    };
    if fields.get("ci").is_some_and(|ci| !ci.is_null()) {
        return Ok(());
    }
    fields.insert(
        "ci".into(),
        serde_json::to_value(origin).map_err(io::Error::other)?,
    );
    let mut text = serde_json::to_string_pretty(&report).map_err(io::Error::other)?;
    text.push('\n');
    fs::write(path, text)
}

fn ledger(project: &Path) -> PathBuf {
    project.join(".jevision").join(LEDGER)
}

fn save_ledger(project: &Path, done: &BTreeSet<u64>) -> io::Result<()> {
    let path = ledger(project);
    let mut text =
        serde_json::to_string_pretty(&json!({ "artifacts": done })).map_err(io::Error::other)?;
    text.push('\n');
    // Written beside it and renamed, so a crash never leaves half a file.
    let partial = path.with_extension("json.partial");
    fs::write(&partial, text)?;
    fs::rename(partial, path)
}
