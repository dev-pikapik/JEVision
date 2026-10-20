//! The engine: walks a test's flowchart block by block and records what happened.

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::ops::RangeInclusive;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::{fs, io};

use jevision_core::{
    BlockKind, Blocker, ClipboardAction, Compare, Condition, DEFAULT_WAIT_TIMEOUT_MS,
    DatabaseExpect, ELSE, ExpectStatus, FileState, Flow, PROJECT_FILE, Point, ProjectSettings,
    Rect, Reference, Severity, THEN, Target, ValueKind, WaitUntil, dark_variant, else_if_edge,
};

use crate::control::{Cancelled, PauseReason};
use crate::report::{
    Attempt, DataRow, Failure, REPORT_FORMAT, RunReport, Step, StepStatus, Verdict, VisionAnswer,
    percent, summarize,
};
use crate::screen::{ScreenHolder, ScreenLock};
use crate::system::{self, ProcessError};
use crate::usage::{LimitState, UsageBook, UsageRecord, price_of};
use crate::{
    Event, EventSink, Hands, HandsError, Openable, Question, RunControl, SECRET_MASK, Screenshot,
    SecretSource, VisionError, VisionJudge,
};
use jevision_core::DataTable;
use jevision_vision::image::{Comparison, Image};
use jevision_vision::{MaskedImage, Provider};

/// How far a pixel channel may be off and still count as the same colour when comparing by
/// similarity; absorbs font smoothing and colour-profile noise.
const SIMILAR_TOLERANCE: u8 = 16;
/// The screen counts as settled when two looks in a row are at least this alike.
const STABLE_SIMILARITY: f64 = 0.999;
/// Longest side, in pixels, of a picture sent to a vision model; larger ones cost more and
/// are scaled down by the providers anyway.
const VISION_MAX_SIDE: u32 = 1568;

/// Name of the run's video in its folder.
pub const VIDEO_FILE: &str = "video.mp4";

/// Whether a run records a video of the application under test, and which videos it keeps.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum VideoMode {
    #[default]
    Off,
    /// Records every run and keeps the video only when the run failed.
    OnFailure,
    Always,
}

impl VideoMode {
    /// Whether a run that ended with `verdict` keeps its video.
    pub fn keeps(self, verdict: Verdict) -> bool {
        match self {
            Self::Off => false,
            Self::OnFailure => verdict == Verdict::Failed,
            Self::Always => true,
        }
    }
}

/// How a run starts.
#[derive(Debug, Clone)]
pub struct RunOptions {
    /// Block of the test to start from instead of its `start` block.
    pub start_at: Option<String>,
    /// Overrides the test's `retries`.
    pub retries: Option<u32>,
    /// Environment of the project whose values `${env.NAME}` reads, from
    /// `jevision.environments.json`.
    pub environment: Option<String>,
    /// Rows of the data table to run, counted from 0; `None` runs every row. A test whose
    /// first `useData` block names a table runs once per row; other tables run these rows of
    /// theirs inside one run.
    pub rows: Option<RangeInclusive<usize>>,
    /// Time between two looks at the screen while waiting.
    pub poll_interval: Duration,
    /// Screenshots must not leave this computer: checks by meaning fail instead of asking a
    /// cloud provider.
    pub local_only: bool,
    /// Name of the paired device that asked for the run, saved in the report.
    pub requested_by: Option<String>,
    /// Name of the schedule that started the run, saved in the report.
    pub schedule: Option<String>,
    pub video: VideoMode,
    /// Videos of the project's runs older than this are deleted after the run; `None` keeps
    /// them all.
    pub keep_videos_for: Option<Duration>,
    /// The test wasn't chosen: it runs because a chosen test depends on it.
    pub added_as_dependency: bool,
    /// Lock file shared by every program that drives this Mac's screen: a test that acts on the
    /// screen takes it before its first block and waits if another run has it. `None` takes no
    /// lock; [`default_screen_lock`] is where the app, the command line and the assistant's
    /// server meet.
    pub screen_lock: Option<PathBuf>,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            start_at: None,
            retries: None,
            environment: None,
            rows: None,
            poll_interval: Duration::from_millis(250),
            local_only: false,
            requested_by: None,
            schedule: None,
            video: VideoMode::Off,
            keep_videos_for: None,
            added_as_dependency: false,
            screen_lock: None,
        }
    }
}

/// Runs tests. One runner runs one test at a time, on the calling thread; control it from
/// other threads through `control`.
pub struct Runner<'a> {
    pub hands: &'a dyn Hands,
    pub secrets: &'a dyn SecretSource,
    pub events: &'a dyn EventSink,
    pub control: RunControl,
    /// Judges checks by meaning; without one they fail with `run.compare.meaningUnavailable`.
    pub vision: Option<&'a dyn VisionJudge>,
    /// Judges instead when [`vision`](Self::vision) can't be reached, is overloaded or has used
    /// up its spending limit.
    pub fallback: Option<&'a dyn VisionJudge>,
    /// Judges for checks that name their provider, found by [`VisionJudge::provider_id`] after
    /// [`vision`](Self::vision); a check naming a provider in neither fails with
    /// `run.vision.noKey`.
    pub providers: &'a [&'a dyn VisionJudge],
    /// Where every AI call is logged, and the limits it is held to.
    pub usage: Option<&'a UsageBook>,
    /// Finds elements a test describes in words; without one they fail with
    /// `run.target.noLocator`.
    pub locator: Option<&'a dyn VisionJudge>,
}

/// A finished run: its folder and the report saved there as `report.json`.
#[derive(Debug, Clone)]
pub struct Run {
    pub dir: PathBuf,
    pub report: RunReport,
}

impl Run {
    /// Reads the run saved in `dir`.
    pub fn load(dir: &Path) -> io::Result<Self> {
        let text = fs::read_to_string(dir.join("report.json"))?;
        let report: RunReport = serde_json::from_str(&text)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        if report.format > REPORT_FORMAT {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "the report has format {}, this version reads up to {REPORT_FORMAT}",
                    report.format
                ),
            ));
        }
        Ok(Self {
            dir: dir.to_path_buf(),
            report,
        })
    }

    /// Saves the run of the test file `test` (relative to `project`, or absolute) that didn't
    /// happen because a test it depends on didn't pass, so history and reports show it
    /// blocked, with the reason.
    pub fn blocked(
        project: &Path,
        test: &Path,
        blocker: Blocker,
        options: &RunOptions,
    ) -> io::Result<Self> {
        let project = fs::canonicalize(project)?;
        let test = normalize(&project.join(test));
        let file = relative(&project, &test);
        let started_at = rfc3339(SystemTime::now());
        let dir = create_run_dir(&project, &test, &started_at)?;
        let bytes = fs::read(&test).ok();
        let flow = bytes
            .as_deref()
            .and_then(|b| std::str::from_utf8(b).ok())
            .and_then(|text| jevision_core::parse(text).ok());
        let quarantine = ProjectSettings::load(&project)
            .ok()
            .and_then(|s| s.quarantined(&file).cloned());
        let report = RunReport {
            format: REPORT_FORMAT,
            test: flow
                .as_ref()
                .map_or_else(|| file.clone(), |f| f.name.clone()),
            file,
            app: flow.map(|f| f.app).unwrap_or_default(),
            started_at,
            duration_ms: 0,
            verdict: Verdict::Blocked,
            flaky: false,
            baselines_created: Vec::new(),
            requested_by: options.requested_by.clone(),
            schedule: options.schedule.clone(),
            environment: options.environment.clone(),
            video: None,
            file_hash: bytes.as_deref().map(content_hash),
            quarantined: quarantine.is_some(),
            quarantine_reason: quarantine.and_then(|q| q.reason),
            virtual_machine: crate::machine::is_virtual(),
            display_scale: None,
            blocked: Some(blocker),
            added_as_dependency: options.added_as_dependency,
            nothing_checked: false,
            attempts: Vec::new(),
        };
        let mut json = serde_json::to_string_pretty(&report).map_err(io::Error::other)?;
        json.push('\n');
        fs::write(dir.join("report.json"), json)?;
        Ok(Self { dir, report })
    }

    /// Runs saved in `<project>/.jevision/runs`, newest first. Runs still going and folders
    /// without a readable report are left out.
    pub fn list(project: &Path) -> io::Result<Vec<Self>> {
        let entries = match fs::read_dir(project.join(".jevision").join("runs")) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let mut runs: Vec<Self> = entries
            .filter_map(|entry| Self::load(&entry.ok()?.path()).ok())
            .collect();
        // RFC 3339 times in UTC sort as text; the folder breaks ties within a millisecond.
        runs.sort_by(|a, b| (&b.report.started_at, &b.dir).cmp(&(&a.report.started_at, &a.dir)));
        Ok(runs)
    }
}

impl Runner<'_> {
    /// Runs the test file `test` (relative to `project`, or absolute) and saves the report and
    /// screenshots in a new folder `<project>/.jevision/runs/<time>-<test>`. A test that fails
    /// or cannot be read still produces a report; `Err` means the run folder or report could
    /// not be written.
    pub fn run(&self, project: &Path, test: &Path, options: &RunOptions) -> io::Result<Run> {
        let started = SystemTime::now();
        let mut clock = Instant::now();
        let project = fs::canonicalize(project)?;
        let test = normalize(&project.join(test));
        let file = relative(&project, &test);
        let started_at = rfc3339(started);
        let dir = create_run_dir(&project, &test, &started_at)?;

        let mut secrets = HashMap::new();
        let loaded = load_test(&project, &test);
        let settings = ProjectSettings::load(&project).map_err(|reason| {
            Failure::new("run.project.unreadable")
                .with("file", PROJECT_FILE)
                .with("reason", reason)
        });
        let quarantine = settings
            .as_ref()
            .ok()
            .and_then(|s| s.quarantined(&file))
            .cloned();
        let mut report = RunReport {
            format: REPORT_FORMAT,
            test: loaded
                .as_ref()
                .map_or_else(|_| file.clone(), |t| t.flow.name.clone()),
            file: file.clone(),
            app: loaded
                .as_ref()
                .map_or_else(|_| String::new(), |t| t.flow.app.clone()),
            started_at,
            duration_ms: 0,
            verdict: Verdict::Failed,
            flaky: false,
            baselines_created: Vec::new(),
            nothing_checked: true,
            requested_by: options.requested_by.clone(),
            schedule: options.schedule.clone(),
            environment: options.environment.clone(),
            video: None,
            file_hash: loaded
                .as_ref()
                .ok()
                .and_then(|_| fs::read(&test).ok())
                .map(|bytes| content_hash(&bytes)),
            quarantined: quarantine.is_some(),
            quarantine_reason: quarantine.and_then(|q| q.reason),
            virtual_machine: crate::machine::is_virtual(),
            display_scale: None,
            blocked: None,
            added_as_dependency: options.added_as_dependency,
            attempts: Vec::new(),
        };
        let display_scale = Cell::new(None);
        let (local_only, hide) = settings
            .as_ref()
            .map(|s| (s.local_only, s.hide.clone()))
            .unwrap_or_default();
        let environment = crate::env::load(&project, options.environment.as_deref());
        let prepared = loaded.and_then(|t| {
            let default_retries = settings?.retries;
            let environment = environment?;
            crate::env::check(&t.flow, environment.as_ref())?;
            fetch_secrets(&t.flow, self.secrets, &mut secrets)?;
            let start = options
                .start_at
                .clone()
                .unwrap_or_else(|| t.flow.start.clone());
            if !t.flow.blocks.iter().any(|b| b.id == start) {
                return Err(Failure::new("run.start.unknownBlock").with("block", start));
            }
            Ok((t, start, environment, default_retries))
        });
        let max_attempts = match &prepared {
            Ok((t, _, _, default_retries)) => {
                1 + options
                    .retries
                    .or(t.flow.retries)
                    .or(*default_retries)
                    .unwrap_or(0)
            }
            Err(_) => 1,
        };
        self.events.send(&Event::RunStarted {
            test: report.test.clone(),
            file: file.clone(),
            dir: dir.clone(),
            max_attempts,
        });

        let prepared = prepared.and_then(|(root, start, environment, _)| {
            let data = data_rows(&project, &root, options.rows.as_ref())?;
            Ok((root, start, environment, data))
        });
        // Held until the run's last block is done; a test that only talks to commands, files
        // and servers leaves the screen to others.
        let screen = if prepared.is_ok() {
            let waited = Instant::now();
            let screen = self.take_screen(&project, &file, &report.test, options);
            if waited.elapsed() > options.poll_interval {
                clock = Instant::now();
            }
            screen
        } else {
            Ok(None)
        };
        match prepared {
            Err(failure) => {
                self.events.send(&Event::AttemptStarted { attempt: 1 });
                self.finish_attempt(
                    &mut report,
                    Attempt {
                        number: 1,
                        verdict: Verdict::Failed,
                        duration_ms: 0,
                        failure: Some(failure),
                        row: None,
                        steps: Vec::new(),
                    },
                );
            }
            Ok(_) if screen.is_err() => {
                self.events.send(&Event::AttemptStarted { attempt: 1 });
                self.finish_attempt(
                    &mut report,
                    Attempt {
                        number: 1,
                        verdict: Verdict::Cancelled,
                        duration_ms: 0,
                        failure: None,
                        row: None,
                        steps: Vec::new(),
                    },
                );
            }
            Ok((root, start, environment, data)) => {
                // The video's clock starts just before the recording does; step markers count
                // from it.
                let video = if options.video != VideoMode::Off && !report.app.is_empty() {
                    let start = Instant::now();
                    self.hands
                        .start_video(&report.app, &dir.join(VIDEO_FILE), &hide)
                        .ok()
                        .map(|()| start)
                } else {
                    None
                };
                // A test with a data table runs once per row, each row with its own attempts.
                let (table, passes) = match data {
                    Some((table, rows)) => (Some(table), rows.into_iter().map(Some).collect()),
                    None => (None, vec![None]),
                };
                let mut launched: Vec<String> = Vec::new();
                for pass in passes {
                    let row = pass.zip(table.clone()).map(|(row, table)| (table, row));
                    let data_row = row.as_ref().map(|(table, row)| DataRow {
                        number: u32::try_from(row + 1).unwrap_or(u32::MAX),
                        label: table.label(*row),
                    });
                    if let (Some(data_row), Some(table)) = (&data_row, &table) {
                        self.events.send(&Event::RowStarted {
                            row: data_row.number,
                            rows: u32::try_from(table.rows.len()).unwrap_or(u32::MAX),
                            label: data_row.label.clone(),
                        });
                    }
                    for number in 1..=max_attempts {
                        // Each retry and row starts from a clean slate: quit what the last
                        // attempt opened.
                        launched.dedup();
                        for app in launched.drain(..).rev() {
                            let _ = self.hands.quit(&app);
                        }
                        self.events.send(&Event::AttemptStarted { attempt: number });
                        let exec = Exec {
                            runner: self,
                            options,
                            project: &project,
                            dir: &dir,
                            attempt: number,
                            secrets: &mut secrets,
                            environment: environment.as_ref(),
                            row: row.clone(),
                            rows: pass.map_or_else(|| options.rows.clone(), |r| Some(r..=r)),
                            seq: 0,
                            steps: Vec::new(),
                            frames: Vec::new(),
                            tests: Vec::new(),
                            apps: Vec::new(),
                            launched: Vec::new(),
                            to_close: Vec::new(),
                            inputs: Vec::new(),
                            baselines: Vec::new(),
                            checked: false,
                            video,
                            local_only,
                            hide: &hide,
                            display_scale: &display_scale,
                            vars: BTreeMap::new(),
                            started: SystemTime::now(),
                        };
                        let (mut attempt, opened, baselines, checked) = exec.run(&root, &start);
                        report.nothing_checked &= !checked;
                        attempt.row = data_row.clone();
                        launched = opened;
                        report.baselines_created.extend(baselines);
                        let done = attempt.verdict != Verdict::Failed;
                        self.finish_attempt(&mut report, attempt);
                        if done {
                            break;
                        }
                    }
                    if report
                        .attempts
                        .last()
                        .is_some_and(|a| a.verdict == Verdict::Cancelled)
                    {
                        break;
                    }
                }
                if video.is_some() {
                    self.finish_video(&mut report, &dir, options.video);
                }
            }
        }

        drop(screen);
        (report.verdict, report.flaky) = summarize(&report.attempts);
        report.nothing_checked &= report.verdict == Verdict::Passed;
        report.duration_ms = millis(clock.elapsed());
        report.display_scale = display_scale.get();
        let path = dir.join("report.json");
        let mut json = serde_json::to_string_pretty(&report).map_err(io::Error::other)?;
        json.push('\n');
        fs::write(&path, json)?;
        if let Some(age) = options.keep_videos_for {
            // Old videos only cost disk space; failing to delete one must not fail this run.
            let _ = prune_videos(&project, age, SystemTime::now());
        }
        self.events.send(&Event::RunFinished {
            verdict: report.verdict,
            flaky: report.flaky,
            report: path,
        });
        Ok(Run { dir, report })
    }

    /// Takes the screen if the test acts on it, waiting while another run has it: `Ok(None)`
    /// when no lock is needed or the lock file can't be used (a locked-down home folder must
    /// not stop tests that would otherwise run), `Err` when the run is cancelled while waiting.
    fn take_screen(
        &self,
        project: &Path,
        file: &str,
        test: &str,
        options: &RunOptions,
    ) -> Result<Option<ScreenLock>, Cancelled> {
        let Some(path) = &options.screen_lock else {
            return Ok(None);
        };
        if !crate::screen::uses_screen(project, file) {
            return Ok(None);
        }
        let me = ScreenHolder::this_process(test);
        let mut announced = None;
        loop {
            match ScreenLock::try_take(path, &me) {
                Ok(Ok(lock)) => return Ok(Some(lock)),
                Ok(Err(holder)) => {
                    if announced.as_ref() != Some(&holder) {
                        self.events.send(&Event::WaitingForScreen {
                            holder: holder.clone(),
                        });
                        announced = Some(holder);
                    }
                    self.control.sleep(options.poll_interval)?;
                }
                Err(_) => return Ok(None),
            }
        }
    }

    /// Stops the recording and keeps the video if `mode` wants it for this verdict; otherwise
    /// deletes it and drops the step markers that pointed into it.
    fn finish_video(&self, report: &mut RunReport, dir: &Path, mode: VideoMode) {
        let path = dir.join(VIDEO_FILE);
        let saved = self.hands.stop_video().is_ok() && path.is_file();
        let (verdict, _) = summarize(&report.attempts);
        if saved && mode.keeps(verdict) {
            report.video = Some(VIDEO_FILE.to_owned());
            return;
        }
        let _ = fs::remove_file(&path);
        for step in report.attempts.iter_mut().flat_map(|a| &mut a.steps) {
            step.video_at_ms = None;
        }
    }

    fn finish_attempt(&self, report: &mut RunReport, attempt: Attempt) {
        self.events.send(&Event::AttemptFinished {
            attempt: attempt.number,
            verdict: attempt.verdict,
            failure: attempt.failure.clone(),
        });
        report.attempts.push(attempt);
    }
}

/// Deletes the videos of the project's runs last written more than `age` before `now`; their
/// reports and screenshots stay. Returns how many were deleted.
pub fn prune_videos(project: &Path, age: Duration, now: SystemTime) -> io::Result<usize> {
    let entries = match fs::read_dir(project.join(".jevision").join("runs")) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e),
    };
    let mut deleted = 0;
    for entry in entries {
        let video = entry?.path().join(VIDEO_FILE);
        let Ok(written) = fs::metadata(&video).and_then(|m| m.modified()) else {
            continue;
        };
        if now.duration_since(written).is_ok_and(|old| old > age) {
            fs::remove_file(&video)?;
            deleted += 1;
        }
    }
    Ok(deleted)
}

/// A vision model problem as a localizable failure: the error's code and values, with the
/// provider and model named.
pub fn vision_failure(judge: &dyn VisionJudge, e: &VisionError) -> Failure {
    let failure = Failure::new(e.code())
        .with("provider", judge.provider())
        .with("model", judge.model());
    e.params()
        .into_iter()
        .fold(failure, |f, (name, value)| f.with(name, value))
}

/// A data table with the rows of it a run goes through, counted from 0.
type TableRows = (Rc<DataTable>, Vec<usize>);

/// The rows of the test's data table a run goes through one by one, when its first `useData`
/// block names a table that can be read before the run.
fn data_rows(
    project: &Path,
    t: &TestFile,
    rows: Option<&RangeInclusive<usize>>,
) -> Result<Option<TableRows>, Failure> {
    let Some(file) = jevision_core::primary_data_file(&t.flow) else {
        return Ok(None);
    };
    let block = t
        .flow
        .blocks
        .iter()
        .find(|b| matches!(&b.kind, BlockKind::UseData { file: f, .. } if f == file))
        .map_or("", |b| b.id.as_str());
    let path = normalize(&t.dir.join(file));
    if !path.starts_with(project) {
        return Err(Failure::new("run.path.outsideProject")
            .with("block", block)
            .with("path", file));
    }
    let table = DataTable::load(&path).map_err(|e| data_unreadable(block, file, &e))?;
    // A table without rows would leave nothing to run.
    let chosen = chosen_rows(&table, rows)
        .ok()
        .filter(|r| !r.is_empty())
        .ok_or_else(|| no_row(block, file, rows.map_or(0, |r| *r.start()), &table))?;
    Ok(Some((Rc::new(table), chosen.collect())))
}

/// The rows of `table` in `rows`, all when `None`; a range that goes past the end stops there.
/// `Err` is the first row asked for, counted from 0, when the table doesn't have it.
fn chosen_rows(
    table: &DataTable,
    rows: Option<&RangeInclusive<usize>>,
) -> Result<std::ops::Range<usize>, usize> {
    let count = table.rows.len();
    match rows {
        None => Ok(0..count),
        Some(r) if *r.start() < count => Ok(*r.start()..(*r.end()).min(count - 1) + 1),
        Some(r) => Err(*r.start()),
    }
}

fn data_unreadable(block: &str, file: &str, e: &jevision_core::DataError) -> Failure {
    let failure = Failure::new("run.data.unreadable")
        .with("block", block)
        .with("file", file)
        .with("reason", e)
        .with("problem", e.code());
    e.params()
        .into_iter()
        .fold(failure, |f, (name, value)| f.with(name, value))
}

fn no_row(block: &str, file: &str, row: usize, table: &DataTable) -> Failure {
    Failure::new("run.data.noRow")
        .with("block", block)
        .with("file", file)
        .with("row", row + 1)
        .with("rows", table.rows.len())
}

/// SHA-256 of a test file's bytes in hex: runs with the same hash ran the same test.
fn content_hash(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .fold(String::new(), |mut hex, b| {
            let _ = write!(hex, "{b:02x}");
            hex
        })
}

/// Attempts disagree: some passed and some failed, or they failed in different ways.
pub(crate) fn is_flaky(attempts: &[Attempt]) -> bool {
    let finished = attempts.iter().filter(|a| a.verdict != Verdict::Cancelled);
    let passed = finished.clone().any(|a| a.verdict == Verdict::Passed);
    let failures: BTreeSet<(&str, &str)> = finished
        .filter_map(|a| a.failure.as_ref())
        .map(|f| (f.code.as_str(), f.param("block")))
        .collect();
    failures.len() > 1 || (passed && !failures.is_empty())
}

/// A loaded test file.
struct TestFile {
    path: PathBuf,
    /// Relative to the project folder.
    file: String,
    dir: PathBuf,
    flow: Flow,
}

fn load_test(project: &Path, path: &Path) -> Result<TestFile, Failure> {
    let file = relative(project, path);
    let unreadable = |reason: String| {
        Failure::new("run.test.unreadable")
            .with("test", &file)
            .with("reason", reason)
    };
    let text = fs::read_to_string(path).map_err(|e| unreadable(e.to_string()))?;
    let flow = jevision_core::parse(&text).map_err(|e| unreadable(e.to_string()))?;
    let errors = jevision_core::validate(&flow)
        .iter()
        .filter(|i| i.severity == Severity::Error)
        .count();
    if errors > 0 {
        return Err(Failure::new("run.test.invalid")
            .with("test", &file)
            .with("errors", errors));
    }
    Ok(TestFile {
        path: path.to_owned(),
        dir: path.parent().unwrap_or(project).to_owned(),
        file,
        flow,
    })
}

fn fetch_secrets(
    flow: &Flow,
    source: &dyn SecretSource,
    into: &mut HashMap<String, String>,
) -> Result<(), Failure> {
    for name in jevision_core::required_secrets(flow) {
        if let std::collections::hash_map::Entry::Vacant(slot) = into.entry(name) {
            let value = source
                .secret(slot.key())
                .ok_or_else(|| Failure::new("run.secret.missing").with("name", slot.key()))?;
            slot.insert(value);
        }
    }
    Ok(())
}

/// Why a chain of blocks stopped early.
enum Stop {
    /// `recorded`: the failure is already attached to the step where it happened.
    Failed {
        failure: Failure,
        recorded: bool,
    },
    Cancelled,
}

impl From<Cancelled> for Stop {
    fn from(_: Cancelled) -> Self {
        Stop::Cancelled
    }
}

fn fail(failure: Failure) -> Stop {
    Stop::Failed {
        failure,
        recorded: false,
    }
}

/// A loop pass in progress.
enum Frame {
    Repeat {
        depth: usize,
        block: String,
        pass: u32,
        times: u32,
    },
    Data {
        depth: usize,
        block: String,
        table: Rc<DataTable>,
        row: usize,
    },
}

impl Frame {
    /// The loop block, and how many `runTest` levels deep it runs.
    fn owner(&self) -> (usize, &str) {
        match self {
            Frame::Repeat { depth, block, .. } | Frame::Data { depth, block, .. } => {
                (*depth, block)
            }
        }
    }
}

/// A reference image as loaded for one comparison.
struct Loaded {
    /// As written in the test, with `.dark` added for a dark-appearance reference.
    name: String,
    image: Image,
}

/// One screenshot compared with its reference, pixel by pixel or by a vision model.
struct Evaluation {
    /// `None` when a vision model judged by the expectation alone.
    reference: Option<Loaded>,
    actual: Image,
    judged: Judged,
    passed: bool,
    /// Areas covered before any of these pictures is saved or sent.
    private: Vec<Rect>,
    /// The screenshot as a vision model got it.
    sent: Option<MaskedImage>,
}

enum Judged {
    Pixels(Comparison),
    Model(VisionAnswer),
}

/// What a provider said, and what it cost.
struct Answer {
    provider: String,
    /// The model that answered.
    model: String,
    judgement: crate::Judgement,
    cost: Option<f64>,
    /// A spending limit is close or used up.
    warning: Option<Failure>,
}

/// Why a provider gave no verdict.
enum Unanswered {
    /// It can't be reached, is busy or has used up its limit: a fallback provider may answer.
    Unavailable(Failure),
    Failed(Failure),
}

/// The provider is down or busy for now, rather than set up wrong.
fn unavailable(e: &VisionError) -> bool {
    match e {
        VisionError::Unreachable { .. }
        | VisionError::Timeout { .. }
        | VisionError::RateLimited { .. } => true,
        VisionError::Service { status, .. } => *status >= 500,
        _ => false,
    }
}

/// The usage journal couldn't be read or written, so spending can't be kept to its limits.
fn journal_failure(id: &str, book: &UsageBook, e: &io::Error) -> Failure {
    Failure::new("run.vision.journalFailed")
        .with("block", id)
        .with("path", book.journal.display())
        .with("reason", e.to_string())
}

/// One attempt of a run.
struct Exec<'r> {
    runner: &'r Runner<'r>,
    options: &'r RunOptions,
    project: &'r Path,
    dir: &'r Path,
    attempt: u32,
    secrets: &'r mut HashMap<String, String>,
    environment: Option<&'r crate::env::Environment>,
    /// Row of the test's data table this attempt runs with, for `${data.COLUMN}` anywhere in
    /// the test.
    row: Option<(Rc<DataTable>, usize)>,
    /// Rows `useData` blocks go through; `None` for all of them.
    rows: Option<RangeInclusive<usize>>,
    seq: u32,
    steps: Vec<Step>,
    frames: Vec<Frame>,
    /// Test files running now, outermost first.
    tests: Vec<PathBuf>,
    /// Application each running test acts on.
    apps: Vec<String>,
    launched: Vec<String>,
    /// What `openFile` blocks that close after the test opened, in the order they ran.
    to_close: Vec<(Vec<Openable>, Option<String>)>,
    /// Values given to each running custom block, by input name; empty for a test.
    inputs: Vec<HashMap<String, String>>,
    baselines: Vec<String>,
    /// A block that looks at the screen ran to the end: a check, or a wait for something to
    /// appear or disappear.
    checked: bool,
    /// When the run's video started, if one is being recorded.
    video: Option<Instant>,
    /// The project allows only AI models on this computer.
    local_only: bool,
    /// Areas the project covers in every screenshot.
    hide: &'r [Rect],
    /// Scale of the first screenshot of the run, for its report.
    display_scale: &'r Cell<Option<f64>>,
    /// Values blocks saved for `${var.NAME}`, shared by sub-tests.
    vars: BTreeMap<String, String>,
    /// When the attempt started, for files that should change during it.
    started: SystemTime,
}

impl Exec<'_> {
    fn run(mut self, root: &TestFile, start: &str) -> (Attempt, Vec<String>, Vec<String>, bool) {
        let clock = Instant::now();
        self.tests.push(root.path.clone());
        self.apps.push(root.flow.app.clone());
        let (verdict, failure) = match self.run_chain(root, start) {
            Ok(()) => (Verdict::Passed, None),
            Err(Stop::Cancelled) => (Verdict::Cancelled, None),
            Err(Stop::Failed { failure, .. }) => (Verdict::Failed, Some(failure)),
        };
        // Whatever the verdict, nothing a block was asked to close stays open; a window that
        // won't close must not change the result.
        for (items, app) in std::mem::take(&mut self.to_close).into_iter().rev() {
            let _ = self.runner.hands.close(&items, app.as_deref());
        }
        self.steps.sort_by_key(|s| s.seq);
        let attempt = Attempt {
            number: self.attempt,
            verdict,
            duration_ms: millis(clock.elapsed()),
            failure,
            row: None,
            steps: self.steps,
        };
        (attempt, self.launched, self.baselines, self.checked)
    }

    /// Runs blocks from `from` along their edges until a block has nowhere to go or the chain
    /// comes back to a loop that is running, which ends the loop's current pass.
    fn run_chain(&mut self, t: &TestFile, from: &str) -> Result<(), Stop> {
        let depth = self.tests.len();
        let mut current = Some(from.to_owned());
        while let Some(id) = current {
            if self
                .frames
                .iter()
                .any(|f| f.owner() == (depth, id.as_str()))
            {
                return Ok(());
            }
            current = self.step(t, &id)?;
        }
        Ok(())
    }

    fn step(&mut self, t: &TestFile, id: &str) -> Result<Option<String>, Stop> {
        let Some(block) = t.flow.blocks.iter().find(|b| b.id == id) else {
            return Err(fail(
                Failure::new("run.start.unknownBlock").with("block", id),
            ));
        };
        self.hold(id, true)?;
        self.seq += 1;
        let block_type = block.kind.type_name().to_owned();
        self.runner.events.send(&Event::StepStarted {
            attempt: self.attempt,
            seq: self.seq,
            test: t.file.clone(),
            block: id.to_owned(),
            block_type: block_type.clone(),
            variables: self.variables(),
            timeout_ms: block.kind.wait_ms(),
        });
        let clock = Instant::now();
        let mut step = Step {
            seq: self.seq,
            test: t.file.clone(),
            block: id.to_owned(),
            block_type,
            status: StepStatus::Passed,
            duration_ms: 0,
            before: None,
            after: None,
            expected: None,
            diff: None,
            reference: None,
            similarity: None,
            vision: None,
            branch: None,
            branches: None,
            branch_condition: None,
            failure: None,
            video_at_ms: self.video.map(|start| millis(start.elapsed())),
            sent: None,
            sent_masked: false,
            outputs: BTreeMap::new(),
            found: Vec::new(),
        };
        let result = match self.perform(t, &block.kind, &mut step) {
            Ok(next) => {
                self.checked |= block.kind.checks();
                Ok(next)
            }
            Err(Stop::Cancelled) => {
                step.status = StepStatus::Cancelled;
                Err(Stop::Cancelled)
            }
            Err(Stop::Failed {
                mut failure,
                recorded,
            }) => {
                step.status = StepStatus::Failed;
                if !recorded {
                    self.redact(&mut failure);
                    step.failure = Some(failure.clone());
                }
                Err(Stop::Failed {
                    failure,
                    recorded: true,
                })
            }
        };
        step.duration_ms = millis(clock.elapsed());
        self.runner.events.send(&Event::StepFinished {
            attempt: self.attempt,
            step: Box::new(step.clone()),
        });
        self.steps.push(step);
        result
    }

    fn perform(
        &mut self,
        t: &TestFile,
        kind: &BlockKind,
        step: &mut Step,
    ) -> Result<Option<String>, Stop> {
        let id = step.block.clone();
        let id = id.as_str();
        match kind {
            BlockKind::OpenApp {
                app,
                reset_state,
                window_size,
                window_position,
                next,
            } => {
                let app = app.clone().unwrap_or_else(|| t.flow.app.clone());
                self.hands(id, |h| h.open_app(&app, *reset_state))?;
                if window_size.is_some() || window_position.is_some() {
                    self.hands(id, |h| {
                        h.arrange_window(&app, *window_size, *window_position)
                    })?;
                }
                self.use_app(app);
                self.snapshot(step, "after")?;
                Ok(next.clone())
            }
            BlockKind::OpenFile {
                files,
                app,
                new_instance,
                close_after,
                next,
            } => {
                let mut items = Vec::with_capacity(files.len());
                for (i, file) in files.iter().enumerate() {
                    let address = self.substitute(id, file)?;
                    if address.starts_with("http://") || address.starts_with("https://") {
                        items.push(Openable::Url(address));
                    } else {
                        let copy = self.fresh_copy(t, id, file, step.seq, i)?;
                        items.push(Openable::File(copy));
                    }
                }
                if *close_after {
                    // Recorded first: what opened before a failure is closed too.
                    self.to_close.push((items.clone(), app.clone()));
                }
                self.hands(id, |h| h.open(&items, app.as_deref(), *new_instance))?;
                if let Some(app) = app {
                    self.use_app(app.clone());
                }
                self.snapshot(step, "after")?;
                Ok(next.clone())
            }
            BlockKind::Click {
                target,
                button,
                double,
                modifiers,
                wait_for_change,
                next,
            } => {
                let app = self.app();
                self.hands(id, |h| h.activate(&app))?;
                let before = self.snapshot(step, "before")?;
                let at = self.locate(t, id, target, before.as_ref(), step)?;
                self.hands(id, |h| h.click(&app, at, *button, *double, *modifiers))?;
                if *wait_for_change {
                    self.wait_for_change(id, before, "run.click.noChange")?;
                }
                self.snapshot(step, "after")?;
                Ok(next.clone())
            }
            BlockKind::Hover {
                target,
                wait_for_change,
                next,
            } => {
                let app = self.app();
                self.hands(id, |h| h.activate(&app))?;
                let before = self.snapshot(step, "before")?;
                let at = self.locate(t, id, target, before.as_ref(), step)?;
                self.hands(id, |h| h.hover(&app, at))?;
                if *wait_for_change {
                    self.wait_for_change(id, before, "run.action.noChange")?;
                }
                self.snapshot(step, "after")?;
                Ok(next.clone())
            }
            BlockKind::Drag {
                from,
                to,
                button,
                modifiers,
                wait_for_change,
                next,
            } => {
                let app = self.app();
                self.hands(id, |h| h.activate(&app))?;
                let before = self.snapshot(step, "before")?;
                // Both ends are found on the screen as it is before anything moves.
                let start = self.locate(t, id, from, before.as_ref(), step)?;
                let end = self.locate(t, id, to, before.as_ref(), step)?;
                let path = drag_path(start, end);
                self.hands(id, |h| h.drag(&app, &path, *button, *modifiers))?;
                if *wait_for_change {
                    self.wait_for_change(id, before, "run.action.noChange")?;
                }
                self.snapshot(step, "after")?;
                Ok(next.clone())
            }
            BlockKind::Scroll {
                direction,
                amount,
                unit,
                at,
                wait_for_change,
                next,
            } => {
                let app = self.app();
                self.hands(id, |h| h.activate(&app))?;
                let before = self.snapshot(step, "before")?;
                let middle = Target::Point(Point { x: 0.5, y: 0.5 });
                let at =
                    self.locate(t, id, at.as_ref().unwrap_or(&middle), before.as_ref(), step)?;
                self.hands(id, |h| h.scroll(&app, at, *direction, *amount, *unit))?;
                if *wait_for_change {
                    self.wait_for_change(id, before, "run.action.noChange")?;
                }
                self.snapshot(step, "after")?;
                Ok(next.clone())
            }
            BlockKind::Type { text, into, next } => {
                let app = self.app();
                self.hands(id, |h| h.activate(&app))?;
                let before = self.snapshot(step, "before")?;
                if let Some(target) = into {
                    let at = self.locate(t, id, target, before.as_ref(), step)?;
                    self.hands(id, |h| {
                        h.click(
                            &app,
                            at,
                            jevision_core::MouseButton::Left,
                            false,
                            jevision_core::Modifiers::default(),
                        )
                    })?;
                }
                let text = self.substitute(id, text)?;
                self.hands(id, |h| h.type_text(&text))?;
                self.snapshot(step, "after")?;
                Ok(next.clone())
            }
            BlockKind::PressKeys { keys, next } => {
                let app = self.app();
                self.hands(id, |h| h.activate(&app))?;
                self.snapshot(step, "before")?;
                self.hands(id, |h| h.press_keys(keys))?;
                self.snapshot(step, "after")?;
                Ok(next.clone())
            }
            BlockKind::WaitFor {
                until,
                timeout_ms,
                next,
            } => {
                self.wait_for(t, id, until, *timeout_ms, step)?;
                Ok(next.clone())
            }
            BlockKind::Pause { duration_ms, next } => {
                // Time the run spends paused by the user doesn't count.
                self.poll(id, Duration::from_millis(*duration_ms), || Ok(None::<()>))?;
                Ok(next.clone())
            }
            BlockKind::Check { reference, next } => {
                let shot = self.capture(id)?;
                match self.evaluate(t, id, reference, &shot, true, &mut None)? {
                    Ok(e) => {
                        if let Some(loaded) = &e.reference {
                            let path = self.resolve(t, id, &loaded.name)?;
                            step.reference = Some(relative(self.project, &path));
                        }
                        self.record_comparison(step, &e)?;
                        if !e.passed {
                            return Err(fail(mismatch(id, reference, &e)));
                        }
                    }
                    Err(name) => self.create_baseline(t, id, reference, &name, &shot, step)?,
                }
                Ok(next.clone())
            }
            BlockKind::If {
                condition,
                then,
                else_if,
                otherwise,
            } => {
                // Screen conditions are checked on one screenshot, taken only if there is one;
                // the first condition that holds wins.
                let mut shot = None;
                let cases = std::iter::once((THEN.to_owned(), condition, then)).chain(
                    else_if
                        .iter()
                        .enumerate()
                        .map(|(n, c)| (else_if_edge(n), &c.condition, &c.then)),
                );
                step.branches = u32::try_from(else_if.len() + 2).ok();
                let mut first = None;
                for (branch, condition, target) in cases {
                    let (passed, name) = match condition {
                        Condition::Screen(reference) => {
                            let shot = match &mut shot {
                                Some(shot) => &*shot,
                                slot => slot.insert(self.capture(id)?),
                            };
                            let e = self
                                .evaluate(t, id, reference, shot, false, &mut None)?
                                .map_err(|name| missing_reference(id, &name))?;
                            let passed = e.passed;
                            if passed {
                                self.record_comparison(step, &e)?;
                            } else {
                                first.get_or_insert(e);
                            }
                            (passed, condition_name(reference))
                        }
                        Condition::Value(check) => {
                            let of = self.substitute(id, &check.of)?;
                            let to = self.substitute(id, &check.to)?;
                            let passed = system::compare(&of, check.is, &to).map_err(|value| {
                                fail(
                                    Failure::new("run.value.notNumber")
                                        .with("block", id)
                                        .with("value", value),
                                )
                            })?;
                            let name =
                                format!("{} {} {}", check.of, system::sign(check.is), check.to);
                            (passed, name)
                        }
                    };
                    if passed {
                        step.branch = Some(branch);
                        step.branch_condition = Some(self.mask(&name));
                        return Ok(target.clone());
                    }
                }
                if let Some(e) = &first {
                    self.record_comparison(step, e)?;
                }
                step.branch = Some(ELSE.to_owned());
                Ok(otherwise.clone())
            }
            BlockKind::Repeat { times, body, next } => {
                for pass in 1..=*times {
                    self.frames.push(Frame::Repeat {
                        depth: self.tests.len(),
                        block: id.to_owned(),
                        pass,
                        times: *times,
                    });
                    let result = self.run_chain(t, body);
                    self.frames.pop();
                    result?;
                }
                Ok(next.clone())
            }
            BlockKind::UseData { file, body, next } => {
                let path = self.resolve_filled(t, id, file)?;
                let table =
                    DataTable::load(&path).map_err(|e| fail(data_unreadable(id, file, &e)))?;
                let table = Rc::new(table);
                let rows = chosen_rows(&table, self.rows.as_ref())
                    .map_err(|row| fail(no_row(id, file, row, &table)))?;
                for row in rows {
                    self.frames.push(Frame::Data {
                        depth: self.tests.len(),
                        block: id.to_owned(),
                        table: Rc::clone(&table),
                        row,
                    });
                    let result = self.run_chain(t, body);
                    self.frames.pop();
                    result?;
                }
                Ok(next.clone())
            }
            BlockKind::Script {
                language,
                code,
                inputs,
                outputs,
                timeout_ms,
                next,
            } => {
                let mut values = Vec::with_capacity(inputs.len());
                for input in inputs {
                    let given = self.substitute(id, &input.value)?;
                    let value = typed_value(t, input.kind, &given).ok_or_else(|| {
                        fail(
                            Failure::new("run.script.badInput")
                                .with("block", id)
                                .with("name", &input.name)
                                .with("type", input.kind.name())
                                .with("value", system::excerpt(&given)),
                        )
                    })?;
                    values.push((&input.name, value));
                }
                static SCRIPTS: AtomicU64 = AtomicU64::new(0);
                let work = std::env::temp_dir().join(format!(
                    "jevision-script-{}-{}",
                    std::process::id(),
                    SCRIPTS.fetch_add(1, Ordering::Relaxed)
                ));
                let script = work.join("script");
                let output = work.join("output");
                fs::create_dir_all(&work)
                    .and_then(|()| fs::write(&script, code))
                    .and_then(|()| fs::write(&output, ""))
                    .map_err(|e| write_failed(&script, e))?;
                let program = system::interpreter(*language);
                let mut command = std::process::Command::new(program);
                command
                    .arg(&script)
                    .current_dir(&t.dir)
                    .envs(values)
                    .env("JEV_OUTPUT", &output)
                    .env("JEV_PROJECT", self.project);
                let result = system::run_process(
                    command,
                    Duration::from_millis(*timeout_ms),
                    &self.runner.control,
                );
                let written = fs::read_to_string(&output).unwrap_or_default();
                let _ = fs::remove_dir_all(&work);
                let done = finished(id, program, *timeout_ms, result)?;
                if done.code != Some(0) {
                    return Err(fail(
                        Failure::new("run.script.failed")
                            .with("block", id)
                            .with("code", exit_code(&done))
                            .with("reason", said(&done)),
                    ));
                }
                let written = system::parse_outputs(&written);
                for out in outputs {
                    let Some(value) = written.get(&out.name) else {
                        return Err(fail(
                            Failure::new("run.script.missingOutput")
                                .with("block", id)
                                .with("name", &out.name),
                        ));
                    };
                    let value = typed_value(t, out.kind, value).ok_or_else(|| {
                        fail(
                            Failure::new("run.script.badOutput")
                                .with("block", id)
                                .with("name", &out.name)
                                .with("type", out.kind.name())
                                .with("value", system::excerpt(value)),
                        )
                    })?;
                    self.save(step, &out.name, value);
                }
                Ok(next.clone())
            }
            BlockKind::Command {
                command,
                folder,
                exit_code: expected,
                save_output,
                timeout_ms,
                next,
            } => {
                let line = self.substitute(id, command)?;
                let folder = match folder {
                    Some(folder) => {
                        let path = self.resolve_filled(t, id, folder)?;
                        if !path.is_dir() {
                            return Err(fail(
                                Failure::new("run.file.missing")
                                    .with("block", id)
                                    .with("file", folder),
                            ));
                        }
                        path
                    }
                    None => t.dir.clone(),
                };
                let mut shell = std::process::Command::new("/bin/zsh");
                shell
                    .arg("-c")
                    .arg(&line)
                    .current_dir(&folder)
                    .env("JEV_PROJECT", self.project);
                let result = system::run_process(
                    shell,
                    Duration::from_millis(*timeout_ms),
                    &self.runner.control,
                );
                let done = finished(id, "zsh", *timeout_ms, result)?;
                if done.code != Some(*expected) {
                    return Err(fail(
                        Failure::new("run.command.exitCode")
                            .with("block", id)
                            .with("expected", expected)
                            .with("actual", exit_code(&done))
                            .with("reason", said(&done)),
                    ));
                }
                if let Some(name) = save_output {
                    let text = done.stdout.trim_end_matches(['\n', '\r']).to_owned();
                    self.save(step, name, text);
                }
                Ok(next.clone())
            }
            BlockKind::Http {
                method,
                url,
                headers,
                body,
                expect_status,
                save,
                timeout_ms,
                next,
            } => {
                let address = self.substitute(id, url)?;
                let mut sent = Vec::with_capacity(headers.len());
                for header in headers {
                    sent.push((header.name.clone(), self.substitute(id, &header.value)?));
                }
                let body = self.substitute(id, body)?;
                let timeout = Duration::from_millis(*timeout_ms);
                let answer =
                    system::http(*method, &address, &sent, &body, timeout).map_err(|reason| {
                        fail(
                            Failure::new("run.http.failed")
                                .with("block", id)
                                .with("url", &address)
                                .with("reason", reason),
                        )
                    })?;
                if !expect_status.allows(answer.status) {
                    let expected = match expect_status {
                        ExpectStatus::Exactly(code) => code.to_string(),
                        _ => "200–299".to_owned(),
                    };
                    return Err(fail(
                        Failure::new("run.http.status")
                            .with("block", id)
                            .with("url", &address)
                            .with("status", answer.status)
                            .with("expected", expected)
                            .with("body", system::excerpt(&answer.body)),
                    ));
                }
                for saved in save {
                    let value = system::answer_part(&answer, &saved.from).ok_or_else(|| {
                        fail(
                            Failure::new("run.http.noValue")
                                .with("block", id)
                                .with("from", &saved.from),
                        )
                    })?;
                    self.save(step, &saved.name, value);
                }
                Ok(next.clone())
            }
            BlockKind::CheckFile {
                path,
                until,
                timeout_ms,
                next,
            } => {
                let full = self.outside_path(t, id, path)?;
                let until = match until {
                    FileState::Contains(text) => FileState::Contains(self.substitute(id, text)?),
                    other => other.clone(),
                };
                let since = self.started;
                let found = self.poll(id, Duration::from_millis(*timeout_ms), || {
                    Ok(system::file_holds(&full, &until, since).then_some(()))
                })?;
                if found.is_none() {
                    let (state, text) = match &until {
                        FileState::Exists => ("exists", ""),
                        FileState::Missing => ("missing", ""),
                        FileState::Changed => ("changed", ""),
                        FileState::Contains(text) => ("contains", text.as_str()),
                    };
                    return Err(fail(
                        Failure::new("run.file.timeout")
                            .with("block", id)
                            .with("path", full.display())
                            .with("state", state)
                            .with("text", text)
                            .with("seconds", seconds(*timeout_ms)),
                    ));
                }
                Ok(next.clone())
            }
            BlockKind::Database {
                file,
                query,
                save_as,
                expect,
                timeout_ms,
                next,
            } => {
                let full = self.outside_path(t, id, file)?;
                if !full.is_file() {
                    return Err(fail(
                        Failure::new("run.file.missing")
                            .with("block", id)
                            .with("file", full.display()),
                    ));
                }
                let sql = self.substitute(id, query)?;
                let expect = match expect {
                    Some(DatabaseExpect::Value(v)) => {
                        Some(DatabaseExpect::Value(self.substitute(id, v)?))
                    }
                    other => other.clone(),
                };
                let mut last = None;
                let rows = self.poll(id, Duration::from_millis(*timeout_ms), || {
                    let rows = system::query(&full, &sql).map_err(|reason| {
                        fail(
                            Failure::new("run.database.failed")
                                .with("block", id)
                                .with("reason", reason),
                        )
                    })?;
                    let holds = match &expect {
                        None => true,
                        Some(DatabaseExpect::Value(v)) => system::same(&rows.first, v),
                        Some(DatabaseExpect::Rows(n)) => rows.count == *n,
                    };
                    if holds {
                        return Ok(Some(rows));
                    }
                    last = Some(rows);
                    Ok(None)
                })?;
                let Some(rows) = rows else {
                    let last = last.unwrap_or(system::Rows {
                        first: String::new(),
                        count: 0,
                    });
                    let failure = match &expect {
                        None => unreachable!("a query without an expectation passes at once"),
                        Some(DatabaseExpect::Rows(n)) => Failure::new("run.database.rows")
                            .with("expected", n)
                            .with("actual", last.count),
                        Some(DatabaseExpect::Value(v)) => Failure::new("run.database.value")
                            .with("expected", v)
                            .with("actual", system::excerpt(&last.first)),
                    };
                    return Err(fail(
                        failure
                            .with("block", id)
                            .with("seconds", seconds(*timeout_ms)),
                    ));
                };
                if let Some(name) = save_as {
                    self.save(step, name, rows.first);
                }
                Ok(next.clone())
            }
            BlockKind::Clipboard { action, next } => {
                match action {
                    ClipboardAction::Put { text } => {
                        let text = self.substitute(id, text)?;
                        self.hands(id, |h| h.set_clipboard(&text))?;
                    }
                    ClipboardAction::Read { save_as } => {
                        let text = self.hands(id, |h| h.clipboard())?;
                        self.save(step, save_as, text);
                    }
                    ClipboardAction::Check { is, to, timeout_ms } => {
                        let to = self.substitute(id, to)?;
                        let mut last = String::new();
                        let held = self.poll(id, Duration::from_millis(*timeout_ms), || {
                            let text = self.hands(id, |h| h.clipboard())?;
                            let passed = match system::compare(&text, *is, &to) {
                                Ok(passed) => passed,
                                // The clipboard may hold a number later, the other side won't.
                                Err(value) if value == to => {
                                    return Err(fail(
                                        Failure::new("run.value.notNumber")
                                            .with("block", id)
                                            .with("value", value),
                                    ));
                                }
                                Err(_) => false,
                            };
                            last = text;
                            Ok(passed.then_some(()))
                        })?;
                        if held.is_none() {
                            return Err(fail(
                                Failure::new("run.clipboard.mismatch")
                                    .with("block", id)
                                    .with("test", system::test_name(*is))
                                    .with("expected", &to)
                                    .with("actual", system::excerpt(&last))
                                    .with("seconds", seconds(*timeout_ms)),
                            ));
                        }
                    }
                }
                Ok(next.clone())
            }
            BlockKind::DateTime {
                save_as,
                format,
                shift,
                next,
            } => {
                let text =
                    system::date_time(chrono::Local::now(), format, *shift).map_err(|reason| {
                        fail(
                            Failure::new("run.dateTime.failed")
                                .with("block", id)
                                .with("reason", reason),
                        )
                    })?;
                self.save(step, save_as, text);
                Ok(next.clone())
            }
            BlockKind::RunTest { test, inputs, next } => {
                let path = self.resolve_filled(t, id, test)?;
                let mut given = HashMap::with_capacity(inputs.len());
                for (name, value) in inputs {
                    given.insert(name.clone(), self.substitute(id, value)?);
                }
                if self.tests.contains(&path) {
                    return Err(fail(
                        Failure::new("run.runTest.recursion")
                            .with("block", id)
                            .with("test", test),
                    ));
                }
                let sub = load_test(self.project, &path)
                    .and_then(|sub| {
                        crate::env::check(&sub.flow, self.environment)?;
                        fetch_secrets(&sub.flow, self.runner.secrets, self.secrets)?;
                        Ok(sub)
                    })
                    .map_err(|f| fail(f.with("block", id)))?;
                self.tests.push(path);
                self.apps.push(sub.flow.app.clone());
                self.inputs.push(given);
                let result = self.run_chain(&sub, &sub.flow.start);
                self.tests.pop();
                self.apps.pop();
                self.inputs.pop();
                result?;
                Ok(next.clone())
            }
        }
    }

    fn wait_for(
        &self,
        t: &TestFile,
        id: &str,
        until: &WaitUntil,
        timeout_ms: u64,
        step: &mut Step,
    ) -> Result<(), Stop> {
        let timeout = Duration::from_millis(timeout_ms);
        let mut last: Option<Screenshot> = None;
        let mut sent = None;
        let (reached, target, reference) = match until {
            WaitUntil::Appears(r) | WaitUntil::Disappears(r) => {
                let want = matches!(until, WaitUntil::Appears(_));
                let mut cache = None;
                // The screen a vision model last judged and its verdict: an unchanged screen
                // gets the same answer, so it is not paid for again.
                let mut asked: Option<(Image, bool)> = None;
                let reached = self.poll(id, timeout, || {
                    let shot = self.capture(id)?;
                    if let Some((seen, passed)) = &asked {
                        let image = decode_screenshot(id, &shot)?;
                        let skip: Vec<_> = r
                            .ignore
                            .iter()
                            .chain(&self.private_areas(&shot, &r.hide))
                            .copied()
                            .collect();
                        if image
                            .compare(seen, r.area.as_ref(), &skip, 0)
                            .is_ok_and(|c| c.differing == 0)
                        {
                            let passed = *passed;
                            last = Some(shot);
                            return Ok((passed == want).then_some(()));
                        }
                    }
                    let passed = match self.evaluate(t, id, r, &shot, false, &mut cache) {
                        Ok(e) => {
                            let e = e.map_err(|name| missing_reference(id, &name))?;
                            cache = e.reference;
                            if let Judged::Model(answer) = e.judged {
                                step.vision = Some(answer);
                                sent = e.sent;
                                asked = Some((e.actual, e.passed));
                            }
                            e.passed
                        }
                        // A window of another size, such as the one before a dialog opens, is
                        // simply not the state waited for.
                        Err(Stop::Failed { failure, .. })
                            if failure.code == "run.compare.shapeMismatch" =>
                        {
                            false
                        }
                        Err(stop) => return Err(stop),
                    };
                    last = Some(shot);
                    Ok((passed == want).then_some(()))
                })?;
                let target = match &r.compare {
                    Compare::Meaning { expect, .. } if r.image.is_empty() => expect.as_str(),
                    _ => r.image.as_str(),
                };
                (reached, target, Some(r))
            }
            WaitUntil::ScreenStable => {
                let mut previous: Option<Image> = None;
                let reached = self.poll(id, timeout, || {
                    let shot = self.capture(id)?;
                    let image = decode_screenshot(id, &shot)?;
                    let settled = previous.as_ref().is_some_and(|p| {
                        image
                            .compare(p, None, &[], 0)
                            .is_ok_and(|c| c.similarity >= STABLE_SIMILARITY)
                    });
                    previous = Some(image);
                    last = Some(shot);
                    Ok(settled.then_some(()))
                })?;
                (reached, "screenStable", None)
            }
        };
        // A dark window compared with the light picture only: the usual reason for waiting in
        // vain, so the failure names the picture that is missing.
        let missing_dark = match (reference, &last) {
            (Some(r), Some(shot))
                if shot.dark
                    && !r.image.is_empty()
                    && !matches!(r.compare, Compare::Meaning { .. }) =>
            {
                let dark = dark_variant(&r.image);
                (!self.resolve(t, id, &dark)?.exists()).then_some(dark)
            }
            _ => None,
        };
        if let Some(shot) = last {
            let hide = reference.map_or(&[][..], |r| &r.hide);
            step.after = Some(self.record_shot(step, "after", &shot, hide)?);
        }
        if let Some(sent) = sent {
            self.record_sent(step, &sent)?;
        }
        if reached.is_none() {
            let failure = Failure::new("run.waitFor.timeout")
                .with("block", id)
                .with("target", target)
                .with("seconds", seconds(timeout_ms));
            return Err(fail(match missing_dark {
                Some(dark) => failure.with("dark", dark),
                None => failure,
            }));
        }
        Ok(())
    }

    /// Waits until the screen differs from `before`, the look before the action, or from the
    /// look now when there was none; fails with `code` when it does not change in time.
    fn wait_for_change(
        &self,
        id: &str,
        before: Option<Screenshot>,
        code: &str,
    ) -> Result<(), Stop> {
        let before = match before {
            Some(shot) => shot,
            None => self.capture(id)?,
        };
        let before = decode_screenshot(id, &before)?;
        let timeout = Duration::from_millis(DEFAULT_WAIT_TIMEOUT_MS);
        let changed = self.poll(id, timeout, || {
            let now = decode_screenshot(id, &self.capture(id)?)?;
            let same = now
                .compare(&before, None, &[], SIMILAR_TOLERANCE)
                .is_ok_and(|c| c.differing == 0);
            Ok((!same).then_some(()))
        })?;
        changed.map(|_| ()).ok_or_else(|| {
            fail(
                Failure::new(code)
                    .with("block", id)
                    .with("seconds", seconds(DEFAULT_WAIT_TIMEOUT_MS)),
            )
        })
    }

    /// Looks at the screen until `probe` returns a value or `timeout` passes; time spent
    /// paused does not count.
    fn poll<T>(
        &self,
        id: &str,
        timeout: Duration,
        mut probe: impl FnMut() -> Result<Option<T>, Stop>,
    ) -> Result<Option<T>, Stop> {
        let mut deadline = Instant::now() + timeout;
        loop {
            if let Some(found) = probe()? {
                return Ok(Some(found));
            }
            let now = Instant::now();
            if now >= deadline {
                return Ok(None);
            }
            let pause = self.options.poll_interval.min(deadline - now);
            self.runner.control.sleep(pause)?;
            deadline += self.hold(id, false)?;
        }
    }

    /// Compares a screenshot with a reference. The inner `Err` holds the reference name when
    /// the file does not exist. In dark appearance `name.dark.png` is used; with `strict`
    /// unset, the light reference stands in when there is no dark one.
    fn evaluate(
        &self,
        t: &TestFile,
        id: &str,
        r: &Reference,
        shot: &Screenshot,
        strict: bool,
        cache: &mut Option<Loaded>,
    ) -> Result<Result<Evaluation, String>, Stop> {
        let (tolerance, threshold) = match &r.compare {
            Compare::Exact => (0, None),
            Compare::Similar { threshold } => (SIMILAR_TOLERANCE, Some(*threshold)),
            Compare::Meaning { .. } => return self.judge(t, id, r, shot, strict, cache),
        };
        let reference = match self.load_reference(t, id, r, shot, strict, cache)? {
            Ok(loaded) => loaded,
            Err(name) => return Ok(Err(name)),
        };
        let actual = decode_screenshot(id, shot)?;
        // Covered areas are left out: the reference has them covered too.
        let private = self.private_areas(shot, &r.hide);
        let skip: Vec<_> = r.ignore.iter().chain(&private).copied().collect();
        let comparison = actual
            .compare(&reference.image, r.area.as_ref(), &skip, tolerance)
            .map_err(|_| {
                fail(
                    Failure::new("run.compare.shapeMismatch")
                        .with("block", id)
                        .with("image", &reference.name)
                        .with("actual", size(&actual))
                        .with("expected", size(&reference.image)),
                )
            })?;
        let passed = match threshold {
            None => comparison.differing == 0,
            Some(threshold) => comparison.similarity >= threshold,
        };
        Ok(Ok(Evaluation {
            reference: Some(reference),
            actual,
            judged: Judged::Pixels(comparison),
            passed,
            private,
            sent: None,
        }))
    }

    /// Asks the vision model whether the screen shows what `expect` says, or means the same
    /// as the reference when `expect` is empty. With an expectation the reference is optional.
    fn judge(
        &self,
        t: &TestFile,
        id: &str,
        r: &Reference,
        shot: &Screenshot,
        strict: bool,
        cache: &mut Option<Loaded>,
    ) -> Result<Result<Evaluation, String>, Stop> {
        let (expect, model, provider) = match &r.compare {
            Compare::Meaning {
                expect,
                model,
                provider,
            } => (expect.trim(), model.trim(), provider.as_str()),
            _ => ("", "", ""),
        };
        let judge = if provider.is_empty() {
            self.runner.vision.ok_or_else(|| {
                fail(Failure::new("run.compare.meaningUnavailable").with("block", id))
            })?
        } else {
            // The run's own provider first: its settings may be the command line's.
            let named = self.runner.providers.iter().copied();
            self.runner
                .vision
                .into_iter()
                .chain(named)
                .find(|j| j.provider_id() == provider)
                .ok_or_else(|| {
                    let name = Provider::from_id(provider)
                        .map_or(provider.to_owned(), |p| p.name().to_owned());
                    fail(
                        Failure::new("run.vision.noKey")
                            .with("block", id)
                            .with("provider", name),
                    )
                })?
        };
        let local_only = self.options.local_only || self.local_only;
        self.allow(id, judge)?;
        let reference = if expect.is_empty() {
            match self.load_reference(t, id, r, shot, strict, cache)? {
                Ok(loaded) => Some(loaded),
                Err(name) => return Ok(Err(name)),
            }
        } else if r.image.is_empty() {
            None
        } else {
            self.load_reference(t, id, r, shot, strict, cache)?.ok()
        };
        let actual = decode_screenshot(id, shot)?;
        let private = self.private_areas(shot, &r.hide);
        let screenshot = for_model(id, &actual, r, &private)?;
        let reference_picture = match &reference {
            Some(loaded) => Some(for_model(id, &loaded.image, r, &private)?),
            None => None,
        };
        let question = Question {
            expect,
            screenshot: &screenshot,
            reference: reference_picture.as_ref(),
            model,
        };
        let fallback = self.runner.fallback.filter(|f| !local_only || f.is_local());
        let (answer, fallback_reason) = match (self.ask(t, id, judge, &question), fallback) {
            (Ok(answer), _) => (answer, None),
            (Err(Unanswered::Unavailable(reason)), Some(fallback)) => {
                // A model named in the block belongs to the main provider.
                let question = Question {
                    model: "",
                    ..question
                };
                match self.ask(t, id, fallback, &question) {
                    Ok(answer) => (answer, Some(reason)),
                    Err(_) => return Err(fail(reason)),
                }
            }
            (Err(Unanswered::Unavailable(f) | Unanswered::Failed(f)), _) => return Err(fail(f)),
        };
        let judgement = answer.judgement;
        Ok(Ok(Evaluation {
            reference,
            actual,
            passed: judgement.passed,
            judged: Judged::Model(VisionAnswer {
                provider: answer.provider,
                model: answer.model,
                passed: judgement.passed,
                // The model may read a secret off the screen.
                reason: self.mask(&judgement.reason),
                input_tokens: judgement.usage.input_tokens,
                output_tokens: judgement.usage.output_tokens,
                cost: answer.cost,
                warning: answer.warning,
                fallback_reason,
            }),
            private,
            sent: Some(screenshot),
        }))
    }

    /// Asks one provider, held to its spending limits, and logs what the call used.
    fn ask(
        &self,
        t: &TestFile,
        id: &str,
        judge: &dyn VisionJudge,
        question: &Question<'_>,
    ) -> Result<Answer, Unanswered> {
        let book = self.runner.usage;
        let state = |book: &UsageBook| {
            book.limit_state(judge.provider_id(), judge.provider(), SystemTime::now())
                .map_err(|e| Unanswered::Failed(journal_failure(id, book, &e)))
        };
        if let Some(book) = book
            && let LimitState::Reached(f) = state(book)?
        {
            return Err(Unanswered::Unavailable(f.with("block", id)));
        }
        let judgement = judge.judge(question).map_err(|e| {
            let failure = vision_failure(judge, &e).with("block", id);
            if unavailable(&e) {
                Unanswered::Unavailable(failure)
            } else {
                Unanswered::Failed(failure)
            }
        })?;
        let model = match question.model.trim() {
            "" => judge.model(),
            model => model,
        }
        .to_owned();
        let mut answer = Answer {
            provider: judge.provider().to_owned(),
            model,
            judgement,
            cost: None,
            warning: None,
        };
        if let Some(book) = book {
            let usage = answer.judgement.usage;
            let record = UsageRecord {
                time: rfc3339(SystemTime::now()),
                provider: judge.provider_id().to_owned(),
                model: answer.model.clone(),
                test: t.file.clone(),
                block: id.to_owned(),
                input_tokens: usage.input_tokens,
                output_tokens: usage.output_tokens,
            };
            book.record(&record)
                .map_err(|e| Unanswered::Failed(journal_failure(id, book, &e)))?;
            answer.cost = price_of(&book.prices, judge.provider_id(), &answer.model)
                .map(|p| p.cost(usage.input_tokens, usage.output_tokens));
            answer.warning = match state(book)? {
                LimitState::Near(f) | LimitState::Reached(f) => Some(f),
                LimitState::Within => None,
            };
        }
        Ok(answer)
    }

    /// Stops when only models on this computer are allowed and `judge` is not one of them.
    fn allow(&self, id: &str, judge: &dyn VisionJudge) -> Result<(), Stop> {
        if (self.options.local_only || self.local_only) && !judge.is_local() {
            let mut failure = Failure::new("run.vision.cloudBlocked")
                .with("block", id)
                .with("provider", judge.provider());
            if self.local_only {
                failure = failure.with("setting", "project");
            }
            return Err(fail(failure));
        }
        Ok(())
    }

    /// The reference to compare with; the inner `Err` holds its name when it does not exist.
    fn load_reference(
        &self,
        t: &TestFile,
        id: &str,
        r: &Reference,
        shot: &Screenshot,
        strict: bool,
        cache: &mut Option<Loaded>,
    ) -> Result<Result<Loaded, String>, Stop> {
        let mut name = if shot.dark {
            dark_variant(&r.image)
        } else {
            r.image.clone()
        };
        if !strict && name != r.image && !self.resolve(t, id, &name)?.exists() {
            name = r.image.clone();
        }
        if let Some(loaded) = cache.take().filter(|c| c.name == name) {
            return Ok(Ok(loaded));
        }
        let bytes = match fs::read(self.resolve(t, id, &name)?) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Err(name)),
            Err(e) => return Err(fail(unreadable_reference(id, &name, e.to_string()))),
        };
        let image = Image::decode(&bytes)
            .map_err(|reason| fail(unreadable_reference(id, &name, reason)))?;
        Ok(Ok(Loaded { name, image }))
    }

    /// Saves the screenshot, the reference and the diff picture, private areas covered.
    fn record_comparison(&self, step: &mut Step, e: &Evaluation) -> Result<(), Stop> {
        let mut images = vec![("after", e.actual.clone())];
        images.extend(e.reference.as_ref().map(|l| ("expected", l.image.clone())));
        match &e.judged {
            Judged::Pixels(c) => {
                images.push(("diff", c.diff.clone()));
                step.similarity = Some(c.similarity);
            }
            Judged::Model(answer) => step.vision = Some(answer.clone()),
        }
        for (kind, image) in images {
            let path = Some(self.record_image(step, kind, &image, &e.private)?);
            match kind {
                "after" => step.after = path,
                "expected" => step.expected = path,
                _ => step.diff = path,
            }
        }
        if let Some(sent) = &e.sent {
            self.record_sent(step, sent)?;
        }
        Ok(())
    }

    /// Saves the picture a vision model got, so the report shows exactly what left the computer.
    fn record_sent(&self, step: &mut Step, sent: &MaskedImage) -> Result<(), Stop> {
        step.sent = Some(self.record(step, "sent", sent)?);
        step.sent_masked = sent.is_masked();
        Ok(())
    }

    /// There is no reference yet: the current screen, private areas covered, becomes it.
    fn create_baseline(
        &mut self,
        t: &TestFile,
        id: &str,
        r: &Reference,
        name: &str,
        shot: &Screenshot,
        step: &mut Step,
    ) -> Result<(), Stop> {
        let path = self.resolve(t, id, name)?;
        let image = decode_screenshot(id, shot)?;
        let picture = MaskedImage::new(&image, &self.private_areas(shot, &r.hide))
            .map_err(|e| write_failed(&path, e))?;
        write(&path, picture.png())?;
        step.after = Some(self.record(step, "after", &picture)?);
        step.status = StepStatus::BaselineCreated;
        step.reference = Some(relative(self.project, &path));
        self.baselines.push(relative(self.project, &path));
        Ok(())
    }

    /// Where to click, in points from the window's top-left corner: a fixed point, or the
    /// centre of the target image found on screen.
    fn locate(
        &self,
        t: &TestFile,
        id: &str,
        target: &Target,
        shot: Option<&Screenshot>,
        step: &mut Step,
    ) -> Result<Point, Stop> {
        let image = match target {
            Target::Image(image) => image,
            Target::Point(_) | Target::Description(_) => {
                let captured;
                let shot = match shot {
                    Some(shot) => shot,
                    None => {
                        captured = self.capture(id)?;
                        &captured
                    }
                };
                let window = decode_screenshot(id, shot)?;
                let at = match target {
                    Target::Point(p) => *p,
                    Target::Description(what) => {
                        self.find_described(id, what, shot, &window, step)?
                    }
                    Target::Image(_) => unreachable!("handled above"),
                };
                // The test keeps the point as fractions of the window; hands click in points.
                return Ok(Point {
                    x: at.x * f64::from(window.width) / shot.scale,
                    y: at.y * f64::from(window.height) / shot.scale,
                });
            }
        };
        let dark = dark_variant(image);
        let name = if shot.is_some_and(|s| s.dark) && self.resolve(t, id, &dark)?.exists() {
            &dark
        } else {
            image
        };
        let bytes = match fs::read(self.resolve(t, id, name)?) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Err(missing_reference(id, name));
            }
            Err(e) => return Err(fail(unreadable_reference(id, name, e.to_string()))),
        };
        let app = self.app();
        self.hands(id, |h| h.locate(&app, &bytes, Compare::DEFAULT_THRESHOLD))?
            .ok_or_else(|| {
                fail(
                    Failure::new("run.target.notFound")
                        .with("block", id)
                        .with("image", name),
                )
            })
    }

    /// Asks the locator where the element `what` describes is on `shot`, private areas covered;
    /// returns the centre of it in fractions of the window.
    fn find_described(
        &self,
        id: &str,
        what: &str,
        shot: &Screenshot,
        window: &Image,
        step: &mut Step,
    ) -> Result<Point, Stop> {
        let locator = self
            .runner
            .locator
            .ok_or_else(|| fail(Failure::new("run.target.noLocator").with("block", id)))?;
        self.allow(id, locator)?;
        let private = self.private_areas(shot, &[]);
        let sent = masked(id, window, &private, None)?;
        let found = locator
            .locate(&sent, what)
            .map_err(|e| fail(vision_failure(locator, &e).with("block", id)))?;
        self.record_sent(step, &sent)?;
        let area = found.map(|f| f.area).ok_or_else(|| {
            fail(
                Failure::new("run.target.descriptionNotFound")
                    .with("block", id)
                    .with("description", what.trim()),
            )
        })?;
        step.found.push(area);
        Ok(Point {
            x: area.x + area.width / 2.0,
            y: area.y + area.height / 2.0,
        })
    }

    /// Copies a file the test opens into the run folder, so the original stays intact.
    fn fresh_copy(
        &self,
        t: &TestFile,
        id: &str,
        file: &str,
        seq: u32,
        index: usize,
    ) -> Result<PathBuf, Stop> {
        let source = self.resolve_filled(t, id, file)?;
        if !source.exists() {
            return Err(fail(
                Failure::new("run.file.missing")
                    .with("block", id)
                    .with("file", file),
            ));
        }
        let name = source.file_name().unwrap_or(source.as_os_str());
        let copy = self
            .dir
            .join("files")
            .join(format!("{}-{seq:03}-{index}", self.attempt))
            .join(name);
        copy_all(&source, &copy).map_err(|e| {
            fail(
                Failure::new("run.file.copyFailed")
                    .with("block", id)
                    .with("file", file)
                    .with("reason", e),
            )
        })?;
        Ok(copy)
    }

    /// Replaces `${secret.NAME}`, `${env.NAME}` and `${data.COLUMN}` in text to be typed or in
    /// the path of a file a block reads.
    fn substitute(&self, id: &str, text: &str) -> Result<String, Stop> {
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(at) = rest.find("${") {
            out.push_str(&rest[..at]);
            rest = &rest[at..];
            let Some(end) = rest.find('}') else { break };
            match rest[2..end].split_once('.') {
                Some(("secret", name)) => {
                    let value = self.secrets.get(name).ok_or_else(|| {
                        fail(Failure::new("run.secret.missing").with("name", name))
                    })?;
                    out.push_str(value);
                }
                Some(("env", name)) => {
                    let environment = self.environment.ok_or_else(|| {
                        fail(Failure::new("run.env.notChosen").with("name", name))
                    })?;
                    let value = environment.values.get(name).ok_or_else(|| {
                        fail(
                            Failure::new("run.env.missing")
                                .with("name", name)
                                .with("environment", &environment.name),
                        )
                    })?;
                    out.push_str(value);
                }
                Some(("var", name)) => {
                    let value = self.vars.get(name).ok_or_else(|| {
                        fail(
                            Failure::new("run.var.missing")
                                .with("block", id)
                                .with("name", name),
                        )
                    })?;
                    out.push_str(value);
                }
                Some(("data", column)) => {
                    let value = self
                        .frames
                        .iter()
                        .rev()
                        .find_map(|f| match f {
                            Frame::Data { table, row, .. } => table.value(*row, column),
                            Frame::Repeat { .. } => None,
                        })
                        .or_else(|| {
                            let (table, row) = self.row.as_ref()?;
                            table.value(*row, column)
                        });
                    let value = value.ok_or_else(|| {
                        fail(
                            Failure::new("run.data.unknownColumn")
                                .with("block", id)
                                .with("column", column),
                        )
                    })?;
                    out.push_str(value);
                }
                Some(("input", name)) => {
                    let value = self
                        .inputs
                        .last()
                        .and_then(|given| given.get(name))
                        .ok_or_else(|| {
                            fail(
                                Failure::new("run.input.missing")
                                    .with("block", id)
                                    .with("name", name),
                            )
                        })?;
                    out.push_str(value);
                }
                _ => out.push_str(&rest[..=end]),
            }
            rest = &rest[end + 1..];
        }
        out.push_str(rest);
        Ok(out)
    }

    /// Values visible to the next block, secrets masked.
    fn variables(&self) -> BTreeMap<String, String> {
        let mut vars: BTreeMap<String, String> = self
            .secrets
            .keys()
            .map(|name| (format!("secret.{name}"), SECRET_MASK.to_owned()))
            .collect();
        if let Some(environment) = self.environment {
            for (name, value) in &environment.values {
                vars.insert(format!("env.{name}"), self.mask(value));
            }
        }
        if let Some((table, row)) = &self.row {
            for (column, value) in table.columns.iter().zip(&table.rows[*row]) {
                vars.insert(format!("data.{column}"), self.mask(value));
            }
        }
        for (name, value) in &self.vars {
            vars.insert(format!("var.{name}"), self.mask(value));
        }
        for (name, value) in self.inputs.last().into_iter().flatten() {
            vars.insert(format!("input.{name}"), self.mask(value));
        }
        for frame in &self.frames {
            match frame {
                Frame::Repeat {
                    block, pass, times, ..
                } => {
                    vars.insert(format!("repeat.{block}"), format!("{pass} of {times}"));
                }
                Frame::Data {
                    block, table, row, ..
                } => {
                    let count = table.rows.len();
                    vars.insert(
                        format!("useData.{block}"),
                        format!("row {} of {count}", row + 1),
                    );
                    for (column, value) in table.columns.iter().zip(&table.rows[*row]) {
                        vars.insert(format!("data.{column}"), self.mask(value));
                    }
                }
            }
        }
        vars
    }

    /// Pauses here if asked to, a breakpoint or step mode says so (`before_block` only), or a
    /// person took the mouse. Returns the time spent paused.
    fn hold(&self, block: &str, before_block: bool) -> Result<Duration, Cancelled> {
        let control = &self.runner.control;
        let reason = match control.pause_reason(block, before_block)? {
            Some(reason) => reason,
            None if self.runner.hands.user_input_detected() => {
                control.enter_pause();
                PauseReason::UserInput
            }
            None => return Ok(Duration::ZERO),
        };
        let since = Instant::now();
        self.runner.events.send(&Event::Paused {
            block: block.to_owned(),
            reason,
            variables: self.variables(),
        });
        control.wait_while_paused()?;
        self.runner.events.send(&Event::Resumed);
        Ok(since.elapsed())
    }

    /// Calls the platform layer unless the run was cancelled.
    fn hands<T>(
        &self,
        id: &str,
        call: impl FnOnce(&dyn Hands) -> Result<T, HandsError>,
    ) -> Result<T, Stop> {
        if self.runner.control.is_cancelled() {
            return Err(Stop::Cancelled);
        }
        call(self.runner.hands).map_err(|e| fail(hands_failure(id, e)))
    }

    fn capture(&self, id: &str) -> Result<Screenshot, Stop> {
        let app = self.app();
        let shot = self.hands(id, |h| h.capture(&app))?;
        self.saw(&shot);
        Ok(shot)
    }

    fn saw(&self, shot: &Screenshot) {
        if self.display_scale.get().is_none() {
            self.display_scale.set(Some(shot.scale));
        }
    }

    /// Saves a screenshot for the report if one can be taken; a missing window is not an error
    /// here, since the step itself reports what matters.
    fn snapshot(&self, step: &mut Step, kind: &str) -> Result<Option<Screenshot>, Stop> {
        if self.runner.control.is_cancelled() {
            return Err(Stop::Cancelled);
        }
        let Ok(shot) = self.runner.hands.capture(&self.app()) else {
            return Ok(None);
        };
        self.saw(&shot);
        let path = Some(self.record_shot(step, kind, &shot, &[])?);
        if kind == "before" {
            step.before = path;
        } else {
            step.after = path;
        }
        Ok(Some(shot))
    }

    /// Areas covered before `shot`, or another picture of its window, is saved or sent: those
    /// the project hides, `hide` of the block, and the password fields on screen.
    fn private_areas(&self, shot: &Screenshot, hide: &[Rect]) -> Vec<Rect> {
        self.hide
            .iter()
            .chain(hide)
            .chain(&shot.secure)
            .copied()
            .collect()
    }

    /// Writes a picture into the run folder; returns its path relative to the folder. Only
    /// masked pictures are saved, so a report never shows what the project hides.
    fn record(&self, step: &Step, kind: &str, picture: &MaskedImage) -> Result<String, Stop> {
        let name = format!(
            "shots/{}-{:03}-{}-{kind}.png",
            self.attempt, step.seq, step.block
        );
        write(&self.dir.join(&name), picture.png())?;
        Ok(name)
    }

    fn record_shot(
        &self,
        step: &Step,
        kind: &str,
        shot: &Screenshot,
        hide: &[Rect],
    ) -> Result<String, Stop> {
        let picture = MaskedImage::from_png(&shot.png, &self.private_areas(shot, hide)).map_err(
            |reason| {
                fail(
                    Failure::new("run.screenshot.unreadable")
                        .with("block", &step.block)
                        .with("reason", reason),
                )
            },
        )?;
        self.record(step, kind, &picture)
    }

    fn record_image(
        &self,
        step: &Step,
        kind: &str,
        image: &Image,
        private: &[Rect],
    ) -> Result<String, Stop> {
        let picture = MaskedImage::new(image, private)
            .map_err(|e| write_failed(&self.dir.join("shots"), e))?;
        self.record(step, kind, &picture)
    }

    /// [`Self::resolve`] for a path that may contain placeholders; problems name the path as
    /// written in the test, so no secret shows.
    fn resolve_filled(&self, t: &TestFile, id: &str, path: &str) -> Result<PathBuf, Stop> {
        let filled = self.substitute(id, path)?;
        self.inside_project(t, id, &filled, path)
    }

    /// A path from the test, relative to its file, that must stay inside the project.
    fn resolve(&self, t: &TestFile, id: &str, path: &str) -> Result<PathBuf, Stop> {
        self.inside_project(t, id, path, path)
    }

    fn inside_project(
        &self,
        t: &TestFile,
        id: &str,
        path: &str,
        shown: &str,
    ) -> Result<PathBuf, Stop> {
        let full = normalize(&t.dir.join(path));
        if full.starts_with(self.project) {
            Ok(full)
        } else {
            Err(fail(
                Failure::new("run.path.outsideProject")
                    .with("block", id)
                    .with("path", shown),
            ))
        }
    }

    /// A file another app keeps: an absolute path, one in the home folder (`~/…`) or one
    /// relative to the test.
    fn outside_path(&self, t: &TestFile, id: &str, path: &str) -> Result<PathBuf, Stop> {
        Ok(outside(t, &self.substitute(id, path)?))
    }

    /// Keeps `value` for `${var.NAME}` and shows it in the step, secrets masked.
    fn save(&mut self, step: &mut Step, name: &str, value: String) {
        step.outputs.insert(name.to_owned(), self.mask(&value));
        self.vars.insert(name.to_owned(), value);
    }

    fn app(&self) -> String {
        self.apps.last().cloned().unwrap_or_default()
    }

    fn use_app(&mut self, app: String) {
        self.launched.push(app.clone());
        if let Some(current) = self.apps.last_mut() {
            *current = app;
        }
    }

    fn mask(&self, text: &str) -> String {
        let mut values: Vec<&String> = self.secrets.values().filter(|v| !v.is_empty()).collect();
        values.sort_by_key(|v| std::cmp::Reverse(v.len()));
        values.into_iter().fold(text.to_owned(), |text, v| {
            text.replace(v.as_str(), SECRET_MASK)
        })
    }

    fn redact(&self, failure: &mut Failure) {
        for value in failure.params.values_mut() {
            *value = self.mask(value);
        }
    }
}

/// A program that ran to its end, or why it did not.
fn finished(
    id: &str,
    program: &str,
    timeout_ms: u64,
    result: Result<system::Finished, ProcessError>,
) -> Result<system::Finished, Stop> {
    result.map_err(|e| match e {
        ProcessError::NotFound => fail(
            Failure::new("run.process.notFound")
                .with("block", id)
                .with("program", program),
        ),
        ProcessError::CannotStart(reason) => fail(
            Failure::new("run.process.cannotStart")
                .with("block", id)
                .with("program", program)
                .with("reason", reason),
        ),
        ProcessError::TimedOut => fail(
            Failure::new("run.process.timeout")
                .with("block", id)
                .with("seconds", seconds(timeout_ms)),
        ),
        ProcessError::Cancelled => Stop::Cancelled,
    })
}

/// Why a program failed, in its own words: what it wrote to stderr, or else to stdout.
fn said(done: &system::Finished) -> String {
    let stderr = system::excerpt(&done.stderr);
    if stderr.is_empty() {
        system::excerpt(&done.stdout)
    } else {
        stderr
    }
}

fn exit_code(done: &system::Finished) -> String {
    done.code
        .map_or_else(|| "signal".to_owned(), |c| c.to_string())
}

/// Most points a drag moves between two steps of its path, so apps see the pointer travel.
const DRAG_STEP: f64 = 12.0;
/// Fewest and most steps of a drag, however short or long it is.
const DRAG_STEPS: RangeInclusive<u32> = 4..=60;

/// Where a drag passes, in points: `from`, evenly spaced places on the straight line, `to`.
fn drag_path(from: Point, to: Point) -> Vec<Point> {
    let distance = (to.x - from.x).hypot(to.y - from.y);
    let steps =
        ((distance / DRAG_STEP).ceil() as u32).clamp(*DRAG_STEPS.start(), *DRAG_STEPS.end());
    (0..=steps)
        .map(|i| {
            let share = f64::from(i) / f64::from(steps);
            Point {
                x: from.x + (to.x - from.x) * share,
                y: from.y + (to.y - from.y) * share,
            }
        })
        .collect()
}

fn hands_failure(id: &str, e: HandsError) -> Failure {
    let f = match e {
        HandsError::PermissionDenied(p) => {
            Failure::new("run.hands.permissionDenied").with("permission", p)
        }
        HandsError::AppNotFound(app) => Failure::new("run.hands.appNotFound").with("app", app),
        HandsError::NoWindow(app) => Failure::new("run.hands.noWindow").with("app", app),
        HandsError::CannotOpen(file) => Failure::new("run.hands.cannotOpen").with("file", file),
        HandsError::ScreenLocked => Failure::new("run.hands.screenLocked"),
        HandsError::StillRunning(app) => Failure::new("run.hands.stillRunning").with("app", app),
        HandsError::WindowFixed(app) => Failure::new("run.hands.windowFixed").with("app", app),
        HandsError::WindowDiffers {
            app,
            what,
            wanted,
            actual,
        } => Failure::new("run.hands.windowDiffers")
            .with("app", app)
            .with("what", what)
            .with("wanted", wanted)
            .with("actual", actual),
        HandsError::Failed(reason) => Failure::new("run.hands.failed").with("reason", reason),
    };
    f.with("block", id)
}

fn missing_reference(id: &str, name: &str) -> Stop {
    fail(
        Failure::new("run.reference.missing")
            .with("block", id)
            .with("image", name),
    )
}

fn unreadable_reference(id: &str, name: &str, reason: String) -> Failure {
    Failure::new("run.reference.unreadable")
        .with("block", id)
        .with("image", name)
        .with("reason", reason)
}

fn write_failed(path: &Path, e: impl ToString) -> Stop {
    fail(
        Failure::new("run.io.writeFailed")
            .with("path", path.display())
            .with("reason", e.to_string()),
    )
}

/// Why a `check` failed: too few matching pixels, or a vision model said no.
/// What a condition looks for, for people: what the screen should show, or the name of its
/// reference image, `refs/update-dialog.png` → `update dialog`.
fn condition_name(r: &Reference) -> String {
    if let Compare::Meaning { expect, .. } = &r.compare
        && !expect.trim().is_empty()
    {
        return expect.trim().to_owned();
    }
    let file = r.image.rsplit('/').next().unwrap_or_default();
    let stem = file.rsplit_once('.').map_or(file, |(stem, _)| stem);
    stem.split(['-', '_'])
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn mismatch(id: &str, r: &Reference, e: &Evaluation) -> Failure {
    let comparison = match &e.judged {
        Judged::Model(answer) => {
            return Failure::new("run.check.meaningMismatch")
                .with("block", id)
                .with("provider", &answer.provider)
                .with("reason", &answer.reason);
        }
        Judged::Pixels(comparison) => comparison,
    };
    let threshold = match r.compare {
        Compare::Similar { threshold } => threshold,
        _ => 1.0,
    };
    let expected = e.reference.as_ref().map(|l| &l.image);
    let density = expected.filter(|expected| other_density(&e.actual, expected));
    let failure = Failure::new(if density.is_some() {
        "run.check.mismatchDensity"
    } else {
        "run.check.mismatch"
    })
    .with("block", id)
    .with(
        "image",
        e.reference.as_ref().map_or("", |l| l.name.as_str()),
    )
    .with("similarity", percent(comparison.similarity))
    .with("threshold", percent(threshold));
    match density {
        Some(expected) => failure
            .with("expected", size(expected))
            .with("actual", size(&e.actual)),
        None => failure,
    }
}

/// The two pictures are the same window on displays of different pixel density: one is a
/// whole number of times as wide as the other, such as a Retina screenshot of a window whose
/// reference was saved in a virtual machine.
fn other_density(a: &Image, b: &Image) -> bool {
    let (wide, narrow) = (a.width.max(b.width), a.width.min(b.width));
    if narrow == 0 || wide == narrow {
        return false;
    }
    let ratio = f64::from(wide) / f64::from(narrow);
    ratio >= 1.9 && (ratio - ratio.round()).abs() <= 0.02 * ratio
}

/// What a vision model gets to see: the compared area only, with the parts left out of the
/// comparison and the `private` ones covered, scaled down to a sensible size.
fn for_model(
    id: &str,
    image: &Image,
    r: &Reference,
    private: &[Rect],
) -> Result<MaskedImage, Stop> {
    let covered: Vec<_> = r.ignore.iter().chain(private).copied().collect();
    masked(id, image, &covered, r.area.as_ref())
}

/// `image` as a vision model gets it: `covered` painted over, cut to `crop`, made small enough.
fn masked(
    id: &str,
    image: &Image,
    covered: &[Rect],
    crop: Option<&Rect>,
) -> Result<MaskedImage, Stop> {
    MaskedImage::view(image, covered, crop, VISION_MAX_SIDE).map_err(|reason| {
        fail(
            Failure::new("run.screenshot.unreadable")
                .with("block", id)
                .with("reason", reason),
        )
    })
}

fn decode_screenshot(id: &str, shot: &Screenshot) -> Result<Image, Stop> {
    Image::decode(&shot.png).map_err(|reason| {
        fail(
            Failure::new("run.screenshot.unreadable")
                .with("block", id)
                .with("reason", reason),
        )
    })
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), Stop> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| write_failed(path, e))?;
    }
    fs::write(path, bytes).map_err(|e| write_failed(path, e))
}

fn size(image: &Image) -> String {
    format!("{}×{}", image.width, image.height)
}

/// Milliseconds as seconds without trailing zeros: `10`, `1.5`.
fn seconds(ms: u64) -> String {
    let text = format!("{:.3}", ms as f64 / 1000.0);
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

fn millis(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

fn copy_all(from: &Path, to: &Path) -> io::Result<()> {
    if from.is_dir() {
        fs::create_dir_all(to)?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            copy_all(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(from, to).map(|_| ())
    }
}

/// A file another app keeps, as `outside_path` reads it, from a path already filled in.
fn outside(t: &TestFile, filled: &str) -> PathBuf {
    match filled.strip_prefix("~/") {
        Some(rest) => PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(rest),
        None => normalize(&t.dir.join(filled)),
    }
}

/// `text` as a script value of `kind`, written the way the script gets it or the run keeps it:
/// a picture becomes the absolute path of a PNG file that is there.
fn typed_value(t: &TestFile, kind: ValueKind, text: &str) -> Option<String> {
    let value = kind.parse(text)?;
    if kind != ValueKind::Image {
        return Some(value);
    }
    let path = outside(t, &value);
    system::is_png(&path).then(|| path.display().to_string())
}

/// Resolves `.` and `..` without touching the file system.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// `path` relative to `base`, with `/` separators; unchanged if it is not inside `base`.
fn relative(base: &Path, path: &Path) -> String {
    match path.strip_prefix(base) {
        Ok(rel) => rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"),
        Err(_) => path.display().to_string(),
    }
}

/// Makes `<project>/.jevision/runs/<YYYYMMDD-hhmmss>-<test>`, with a number added if two runs
/// start in the same second.
fn create_run_dir(project: &Path, test: &Path, started_at: &str) -> io::Result<PathBuf> {
    let runs = project.join(".jevision").join("runs");
    fs::create_dir_all(&runs)?;
    let digits: String = started_at.chars().filter(char::is_ascii_digit).collect();
    let file = test
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let stem = file.strip_suffix(".flow.json").unwrap_or(&file);
    let slug: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let base = format!("{}-{}-{slug}", &digits[..8], &digits[8..14]);
    let mut n = 1u32;
    loop {
        let dir = runs.join(if n == 1 {
            base.clone()
        } else {
            format!("{base}-{n}")
        });
        match fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => n += 1,
            Err(e) => return Err(e),
        }
    }
}

/// UTC time as `2026-10-18T14:30:05.123Z`.
pub(crate) fn rfc3339(time: SystemTime) -> String {
    let since = time.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = since.as_secs() as i64;
    let (days, day_secs) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Days to civil date, after Howard Hinnant's `civil_from_days`.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        day_secs / 3600,
        day_secs % 3600 / 60,
        day_secs % 60,
        since.subsec_millis()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_timestamps() {
        let t = UNIX_EPOCH + Duration::from_millis(1_700_000_000_123);
        assert_eq!(rfc3339(t), "2023-11-14T22:13:20.123Z");
        assert_eq!(rfc3339(UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
        let leap = UNIX_EPOCH + Duration::from_secs(951_782_400);
        assert_eq!(rfc3339(leap), "2000-02-29T00:00:00.000Z");
    }

    #[test]
    fn paths_and_durations() {
        assert_eq!(
            normalize(Path::new("/p/t/../refs/./a.png")),
            Path::new("/p/refs/a.png")
        );
        assert_eq!(
            relative(Path::new("/p"), Path::new("/p/a/b.json")),
            "a/b.json"
        );
        assert_eq!(seconds(10_000), "10");
        assert_eq!(seconds(1500), "1.5");
    }

    #[test]
    fn which_videos_are_kept() {
        let verdicts = [Verdict::Passed, Verdict::Failed, Verdict::Cancelled];
        let kept = |mode: VideoMode| verdicts.map(|v| mode.keeps(v));
        assert_eq!(kept(VideoMode::Off), [false, false, false]);
        assert_eq!(kept(VideoMode::OnFailure), [false, true, false]);
        assert_eq!(kept(VideoMode::Always), [true, true, true]);
    }
}
