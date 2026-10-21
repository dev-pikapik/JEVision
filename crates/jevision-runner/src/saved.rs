//! Saved runs as a project's lists need them, read once per process and again only when a
//! report changes, and the deleting of old runs a project's settings ask for.

use std::borrow::Cow;
use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::fmt;
use std::num::NonZero;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime};
use std::{fs, io};

use jevision_core::RunRetention;
use serde::de::value::MapDeserializer;
use serde::de::{self, DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

use crate::report::{Step, StepStatus};
use crate::run::{Run, VIDEO_FILE};

/// Folder of a run's screenshots.
const SHOTS: &str = "shots";
const REPORT: &str = "report.json";
const DAY: Duration = Duration::from_secs(24 * 60 * 60);
/// Projects whose summaries are kept; opening another forgets them all.
const KEPT_PROJECTS: usize = 4;
/// Reports one thread reads before another one is worth starting.
const REPORTS_PER_THREAD: usize = 64;
/// How often a project's runs are pruned after runs end, at most.
const PRUNE_EVERY: Duration = Duration::from_secs(10 * 60);

thread_local! {
    /// Set on a thread while it reads a report as a summary.
    static SUMMARY: Cell<bool> = const { Cell::new(false) };
}

/// A report's file as last read: when it was written, its length and the run it held.
struct Known {
    modified: SystemTime,
    len: u64,
    run: Run,
}

/// Summaries read, by runs folder and then by run folder name.
static SUMMARIES: LazyLock<Mutex<HashMap<PathBuf, HashMap<OsString, Known>>>> =
    LazyLock::new(Mutex::default);

/// When each project's runs were last pruned after a run.
static PRUNED: LazyLock<Mutex<HashMap<PathBuf, Instant>>> = LazyLock::new(Mutex::default);

impl Run {
    /// Runs saved in `<project>/.jevision/runs`, newest first, like [`Run::list`], but each
    /// attempt keeps only the steps that failed and the first step of every test file it ran:
    /// enough to list runs, tell a test's health and count statistics, at a small part of the
    /// memory. [`Run::load`] reads a run whole. Reports are read once per process and again
    /// only when one is written anew.
    pub fn summaries(project: &Path) -> io::Result<Vec<Self>> {
        let runs = project.join(".jevision").join("runs");
        let entries = match fs::read_dir(&runs) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let found: Vec<(OsString, SystemTime, u64)> = entries
            .flatten()
            .filter_map(|entry| {
                let meta = fs::metadata(entry.path().join(REPORT)).ok()?;
                Some((entry.file_name(), meta.modified().ok()?, meta.len()))
            })
            .collect();
        let mut known = {
            let mut cache = SUMMARIES.lock().unwrap_or_else(PoisonError::into_inner);
            cache.remove(&runs).unwrap_or_default()
        };
        let mut kept = HashMap::with_capacity(found.len());
        let mut missing = Vec::new();
        for (name, modified, len) in found {
            match known.remove(&name) {
                Some(k) if k.modified == modified && k.len == len => {
                    kept.insert(name, k);
                }
                _ => missing.push((name, modified, len)),
            }
        }
        for (name, modified, len, run) in read_summaries(&runs, missing) {
            kept.insert(name, Known { modified, len, run });
        }
        let mut list: Vec<Self> = kept.values().map(|k| k.run.clone()).collect();
        list.sort_by(|a, b| (&b.report.started_at, &b.dir).cmp(&(&a.report.started_at, &a.dir)));
        let mut cache = SUMMARIES.lock().unwrap_or_else(PoisonError::into_inner);
        if cache.len() >= KEPT_PROJECTS {
            cache.clear();
        }
        cache.insert(runs, kept);
        Ok(list)
    }
}

/// Reads the reports of the run folders `missing` of `runs` as summaries, on several threads
/// when there are many; leaves out those that can't be read.
fn read_summaries(
    runs: &Path,
    missing: Vec<(OsString, SystemTime, u64)>,
) -> Vec<(OsString, SystemTime, u64, Run)> {
    let read = |items: &[(OsString, SystemTime, u64)]| -> Vec<_> {
        items
            .iter()
            .filter_map(|(name, modified, len)| {
                let run = read_summary(&runs.join(name)).ok()?;
                Some((name.clone(), *modified, *len, run))
            })
            .collect()
    };
    let threads = std::thread::available_parallelism()
        .map_or(1, NonZero::get)
        .min(missing.len().div_ceil(REPORTS_PER_THREAD));
    if threads <= 1 {
        return read(&missing);
    }
    std::thread::scope(|scope| {
        let handles: Vec<_> = missing
            .chunks(missing.len().div_ceil(threads))
            .map(|chunk| scope.spawn(move || read(chunk)))
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap_or_else(|e| std::panic::resume_unwind(e)))
            .collect()
    })
}

/// The run saved in `dir` as a summary, see [`Run::summaries`].
fn read_summary(dir: &Path) -> io::Result<Run> {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            SUMMARY.set(false);
        }
    }
    let text = fs::read_to_string(dir.join(REPORT))?;
    SUMMARY.set(true);
    let _reset = Reset;
    Run::parse(dir, &text)
}

/// The steps of an attempt in a report; of a summary only the ones [`Run::summaries`] keeps.
pub(crate) fn steps<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Step>, D::Error> {
    if SUMMARY.get() {
        d.deserialize_seq(SummarySteps)
    } else {
        Vec::deserialize(d)
    }
}

struct SummarySteps;

impl<'de> Visitor<'de> for SummarySteps {
    type Value = Vec<Step>;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a list of steps")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<Step>, A::Error> {
        let mut steps = Vec::new();
        let mut tests: HashSet<String> = HashSet::new();
        let mut fields = Vec::new();
        while seq.next_element_seed(StepFields(&mut fields))?.is_some() {
            let field = |name: &str| {
                fields
                    .iter()
                    .find(|(key, _)| key.0 == name)
                    .map(|&(_, value)| value.get())
            };
            let test = field("test").and_then(|raw| serde_json::from_str::<Text>(raw).ok());
            let status =
                field("status").and_then(|raw| serde_json::from_str::<StepStatus>(raw).ok());
            let keep = match (test, status) {
                (Some(Text(test)), Some(status)) => {
                    let first = !tests.contains(test.as_ref());
                    if first {
                        tests.insert(test.into_owned());
                    }
                    first || status == StepStatus::Failed
                }
                // Read whole, so it fails as reading the whole report does.
                _ => true,
            };
            if keep {
                let fields = fields.drain(..).map(|(key, value)| (key.0, value));
                let step = Step::deserialize(MapDeserializer::<_, serde_json::Error>::new(fields))
                    .map_err(de::Error::custom)?;
                steps.push(step);
            }
        }
        Ok(steps)
    }
}

/// Text read without a copy when nothing in it is escaped.
#[derive(Deserialize)]
struct Text<'a>(#[serde(borrow)] Cow<'a, str>);

/// The fields of a step as written, each value left unread: one pass over a report tells which
/// steps a summary keeps, and only those are read whole.
struct StepFields<'a, 'de>(&'a mut Vec<(Text<'de>, &'de RawValue)>);

impl<'de> DeserializeSeed<'de> for StepFields<'_, 'de> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        d.deserialize_map(self)
    }
}

impl<'de> Visitor<'de> for StepFields<'_, 'de> {
    type Value = ();

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a step")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        self.0.clear();
        while let Some(field) = map.next_entry()? {
            self.0.push(field);
        }
        Ok(())
    }
}

/// What deleting old runs freed, or would free.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PruneSummary {
    /// Run folders deleted whole.
    pub removed_runs: u64,
    /// Videos and screenshots deleted from runs that stay.
    pub removed_media_files: u64,
    pub freed_bytes: u64,
}

/// Deletes the saved runs of `project`, and the videos and screenshots of others, that
/// `retention` doesn't keep as of `now`. Never deletes the newest run of a test, nor a run
/// without a readable report unless it is older than the oldest runs kept, so a run still
/// being written is left alone. Deleting is for good, not to the trash.
pub fn prune_runs(
    project: &Path,
    retention: &RunRetention,
    now: SystemTime,
) -> io::Result<PruneSummary> {
    prune(project, retention, None, now, true)
}

/// What [`prune_runs`] would delete, without deleting anything.
pub fn preview_prune_runs(
    project: &Path,
    retention: &RunRetention,
    now: SystemTime,
) -> io::Result<PruneSummary> {
    prune(project, retention, None, now, false)
}

/// Prunes the runs of `project` after a run ended, as its settings say, with videos also kept
/// no longer than `videos`; at most once every few minutes per project. Old runs only cost disk
/// space: failing to delete one must not fail the run.
pub(crate) fn prune_after_run(
    project: &Path,
    retention: &RunRetention,
    videos: Option<Duration>,
    now: SystemTime,
) {
    {
        let mut pruned = PRUNED.lock().unwrap_or_else(PoisonError::into_inner);
        if pruned
            .get(project)
            .is_some_and(|at| at.elapsed() < PRUNE_EVERY)
        {
            return;
        }
        pruned.insert(project.to_path_buf(), Instant::now());
    }
    let _ = prune(project, retention, videos, now, true);
}

/// A run folder as pruning sees it.
struct Saved {
    dir: PathBuf,
    /// Test file and start, from a readable report.
    report: Option<(String, String)>,
    /// When it started, or for a folder without a readable report when it last changed.
    at: SystemTime,
}

fn prune(
    project: &Path,
    retention: &RunRetention,
    videos: Option<Duration>,
    now: SystemTime,
    delete: bool,
) -> io::Result<PruneSummary> {
    let runs_dir = project.join(".jevision").join("runs");
    let entries = match fs::read_dir(&runs_dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(PruneSummary::default()),
        Err(e) => return Err(e),
    };
    let readable: HashMap<PathBuf, Run> = Run::summaries(project)?
        .into_iter()
        .map(|run| (run.dir.clone(), run))
        .collect();
    let mut saved = Vec::new();
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let dir = entry.path();
        saved.push(match readable.get(&dir) {
            Some(run) => Saved {
                at: started(&run.report.started_at).unwrap_or(now),
                report: Some((run.report.file.clone(), run.report.started_at.clone())),
                dir,
            },
            None => Saved {
                at: last_changed(&dir),
                report: None,
                dir,
            },
        });
    }
    // Newest first; the folder breaks ties within a millisecond, as in the list of runs.
    fn key(s: &Saved) -> (Option<&str>, &Path) {
        (s.report.as_ref().map(|r| r.1.as_str()), &s.dir)
    }
    saved.sort_by(|a, b| key(b).cmp(&key(a)));

    let older_than = |days: Option<u32>| {
        days.and_then(|d| now.checked_sub(DAY * d))
            .unwrap_or(SystemTime::UNIX_EPOCH)
    };
    let run_cutoff = older_than(retention.max_age_days);
    let mut removed = vec![false; saved.len()];
    let mut seen: HashMap<&str, u32> = HashMap::new();
    let mut latest = vec![false; saved.len()];
    for (i, s) in saved.iter().enumerate() {
        match &s.report {
            Some((file, _)) => {
                let count = seen.entry(file).or_default();
                *count += 1;
                latest[i] = *count == 1;
                removed[i] = !latest[i]
                    && (s.at < run_cutoff
                        || retention.max_runs_per_test.is_some_and(|n| *count > n));
            }
            None => removed[i] = s.at < run_cutoff,
        }
    }

    let media_cutoff = older_than(retention.media_max_age_days);
    let video_cutoff = videos
        .and_then(|age| now.checked_sub(age))
        .map_or(media_cutoff, |cutoff| cutoff.max(media_cutoff));
    let media: Vec<Vec<(PathBuf, u64)>> = saved
        .iter()
        .zip(&removed)
        .map(|(s, &gone)| {
            if gone {
                Vec::new()
            } else {
                old_media(s, media_cutoff, video_cutoff)
            }
        })
        .collect();

    // Bytes of each folder, worked out only when needed.
    let mut sizes: Vec<Option<u64>> = vec![None; saved.len()];
    if let Some(mb) = retention.max_total_mb {
        for (s, bytes) in saved.iter().zip(&mut sizes) {
            *bytes = Some(size(&s.dir));
        }
        let left = |i: usize| {
            let media: u64 = media[i].iter().map(|m| m.1).sum();
            sizes[i].unwrap_or_default().saturating_sub(media)
        };
        let mut total: u64 = (0..saved.len()).filter(|&i| !removed[i]).map(left).sum();
        let limit = u64::from(mb) * 1024 * 1024;
        // The oldest runs go first.
        for i in (0..saved.len()).rev() {
            if total <= limit {
                break;
            }
            if !removed[i] && !latest[i] && saved[i].report.is_some() {
                removed[i] = true;
                total -= left(i);
            }
        }
    }

    let mut summary = PruneSummary::default();
    for (i, s) in saved.iter().enumerate() {
        if removed[i] {
            summary.freed_bytes += sizes[i].unwrap_or_else(|| size(&s.dir));
            if delete {
                fs::remove_dir_all(&s.dir)?;
            }
            summary.removed_runs += 1;
            continue;
        }
        for (file, bytes) in &media[i] {
            if delete {
                fs::remove_file(file)?;
            }
            summary.removed_media_files += 1;
            summary.freed_bytes += bytes;
        }
        if delete && !media[i].is_empty() {
            // Only an emptied screenshots folder goes; one with other files in it stays.
            let _ = fs::remove_dir(s.dir.join(SHOTS));
        }
    }
    Ok(summary)
}

/// The video and screenshots of the run `s` older than the cutoffs, with their sizes.
fn old_media(s: &Saved, media: SystemTime, video: SystemTime) -> Vec<(PathBuf, u64)> {
    let mut old = Vec::new();
    // A run's files are as old as the run; a folder without a report has only their own times.
    let older = |meta: &fs::Metadata, cutoff: SystemTime| match s.report {
        Some(_) => s.at < cutoff,
        None => meta.modified().is_ok_and(|t| t < cutoff),
    };
    let path = s.dir.join(VIDEO_FILE);
    if let Ok(meta) = fs::symlink_metadata(&path)
        && meta.is_file()
        && older(&meta, video)
    {
        old.push((path, meta.len()));
    }
    if s.report.is_some() && s.at >= media {
        return old;
    }
    if let Ok(entries) = fs::read_dir(s.dir.join(SHOTS)) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata()
                && meta.is_file()
                && older(&meta, media)
            {
                old.push((entry.path(), meta.len()));
            }
        }
    }
    old
}

/// When the RFC 3339 time `text` was.
fn started(text: &str) -> Option<SystemTime> {
    let ms = chrono::DateTime::parse_from_rfc3339(text)
        .ok()?
        .timestamp_millis();
    SystemTime::UNIX_EPOCH.checked_add(Duration::from_millis(u64::try_from(ms).ok()?))
}

/// When anything in the run folder `dir` without a report last changed: the folder, its video
/// or its screenshots folder.
fn last_changed(dir: &Path) -> SystemTime {
    [dir.to_path_buf(), dir.join(VIDEO_FILE), dir.join(SHOTS)]
        .iter()
        .filter_map(|p| fs::symlink_metadata(p).and_then(|m| m.modified()).ok())
        .max()
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

/// Bytes of the files in `path` and the folders in it; links aren't followed.
fn size(path: &Path) -> u64 {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return 0;
    };
    if !meta.is_dir() {
        return meta.len();
    }
    fs::read_dir(path).map_or(0, |entries| {
        entries.flatten().map(|e| size(&e.path())).sum()
    })
}
