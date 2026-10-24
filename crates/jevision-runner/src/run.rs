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
    AiChoice, BlockKind, Blocker, CheckOf, ClipboardAction, ClipboardContent, Compare, Condition,
    DEFAULT_WAIT_TIMEOUT_MS, DatabaseExpect, ELSE, ENVIRONMENT_NAME, ElementCheck, ElementNode,
    ElementQuery, ElementState, ExpectStatus, FileState, Flow, Missing, PROJECT_FILE, PackageNeed,
    Point, ProjectSettings, Rect, Reference, Severity, THEN, Target, ValueKind, WaitUntil,
    dark_variant, element_check_name, else_if_edge, parse_area,
};

use crate::app_settings;
use crate::control::{Cancelled, PauseReason};
use crate::home::{self, CleanHome};
use crate::report::{
    Attempt, DataRow, Failure, REPORT_FILE, REPORT_FORMAT, RunReport, Step, StepStatus,
    UsedPackage, Verdict, VisionAnswer, kept_percent, summarize,
};
use crate::screen::{ScreenHolder, ScreenLock};
use crate::system::{self, ProcessError};
use crate::usage::{LimitState, UsageBook, price_of};
use crate::{
    AppStart, CiOrigin, Event, EventSink, Hands, HandsError, Openable, Question, RunControl,
    SECRET_MASK, Screenshot, SecretSource, VisionError, VisionJudge,
};
use jevision_core::DataTable;
use jevision_vision::image::{Comparison, Image};
use jevision_vision::{MaskedImage, Provider};

/// How far a pixel channel may be off and still count as the same colour when comparing by
/// similarity; absorbs font smoothing and colour-profile noise.
const SIMILAR_TOLERANCE: u8 = 16;
/// How many of the elements that answer a query too vaguely a failure lists.
const LISTED_ELEMENTS: usize = 5;
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
    /// The CI job that started the run, saved in the report.
    pub ci: Option<CiOrigin>,
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
    /// Journal folder for the settings of apps that a reset starts as on their first launch:
    /// they are saved there and cleared before the app starts, and put back when the run ends.
    /// `None` leaves every app's settings alone; [`crate::default_saved_settings`] is where the app,
    /// the command line and the assistant's server keep them.
    pub saved_settings: Option<PathBuf>,
    /// Language of the app that started the run (`en`, `ru`): what a `dateTime` block writes
    /// month and day names in when it names no language of its own, and what a failure is worded
    /// in when it is recorded. `None` is the language set with `jevision_l10n::set_language`.
    pub language: Option<String>,
    /// Every check by meaning asks [`Runner::vision`] with its own model, whatever the block
    /// or the project chooses, and never the fallback: `jevision run --force-ai`, for CI.
    /// Finding an element by its description still asks [`Runner::locator`].
    pub force_vision: bool,
}

impl RunOptions {
    /// [`RunOptions::language`], or the language set for the process.
    fn language(&self) -> String {
        self.language
            .clone()
            .unwrap_or_else(jevision_l10n::language)
    }
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
            ci: None,
            video: VideoMode::Off,
            keep_videos_for: None,
            added_as_dependency: false,
            screen_lock: None,
            saved_settings: None,
            language: None,
            force_vision: false,
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
        Self::parse(dir, &fs::read_to_string(dir.join(REPORT_FILE))?)
    }

    /// The run saved in `dir` with the report `text`.
    pub(crate) fn parse(dir: &Path, text: &str) -> io::Result<Self> {
        let report: RunReport = serde_json::from_str(text)
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
            .and_then(|s| s.quarantined(&file, &today()).cloned());
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
            ci: options.ci.clone(),
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
            packages: Vec::new(),
            attempts: Vec::new(),
        };
        let mut json = serde_json::to_string_pretty(&report).map_err(io::Error::other)?;
        json.push('\n');
        fs::write(dir.join(REPORT_FILE), json)?;
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

    /// The run [`Run::list`] puts first, reading only the reports of the latest second there
    /// are runs in: run folders are named after the time they started.
    pub fn newest(project: &Path) -> Option<Self> {
        saved_runs(project).into_iter().find_map(|second| {
            second
                .into_iter()
                .filter_map(SavedRun::load)
                .max_by(|a, b| (&a.report.started_at, &a.dir).cmp(&(&b.report.started_at, &b.dir)))
        })
    }

    /// The newest run of each test file in `files`, relative to the project with `/` as
    /// reports name them, that has one: the first of its runs in [`Run::list`]. Reads only the
    /// reports of runs whose folder is named after one of these tests, newest first, until
    /// every test has its run, so it stays quick with thousands of runs.
    pub fn latest(project: &Path, files: &[String]) -> HashMap<String, Self> {
        let mut by_slug: HashMap<String, Vec<&str>> = HashMap::new();
        for file in files {
            by_slug.entry(slug(file)).or_default().push(file);
        }
        let mut open: BTreeSet<&str> = files.iter().map(String::as_str).collect();
        let mut latest: HashMap<String, Self> = HashMap::new();
        for second in saved_runs(project) {
            if open.is_empty() {
                break;
            }
            for saved in second {
                let wanted = saved.rest.as_deref().is_none_or(|rest| {
                    let numbered = rest
                        .rsplit_once('-')
                        .filter(|(_, n)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
                    [Some(rest), numbered.map(|(slug, _)| slug)]
                        .into_iter()
                        .flatten()
                        .filter_map(|slug| by_slug.get(slug))
                        .flatten()
                        .any(|file| open.contains(file))
                });
                let Some(run) = wanted.then(|| saved.load()).flatten() else {
                    continue;
                };
                if !open.contains(run.report.file.as_str()) {
                    continue;
                }
                let newer = latest.get(&run.report.file).is_none_or(|kept| {
                    (&run.report.started_at, &run.dir) > (&kept.report.started_at, &kept.dir)
                });
                if newer {
                    latest.insert(run.report.file.clone(), run);
                }
            }
            for file in latest.keys() {
                open.remove(file.as_str());
            }
        }
        latest
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
        // What runs that crashed or were killed left of the apps they started afresh.
        home::sweep();
        if let Some(folder) = &options.saved_settings {
            app_settings::put_back_left_settings(folder);
        }

        let mut secrets = HashMap::new();
        let loaded = load_test(&project, &test);
        let settings = ProjectSettings::load_checked(&project).map_err(|error| {
            if error.requires_newer() {
                let issue = error.issue(PROJECT_FILE, "run.project.unreadable");
                return Failure {
                    code: issue.code,
                    params: issue
                        .params
                        .into_iter()
                        .map(|param| (param.name, param.value))
                        .collect(),
                    message: String::new(),
                };
            }
            Failure::new("run.project.unreadable")
                .with("file", PROJECT_FILE)
                .with("reason", error)
        });
        let quarantine = settings
            .as_ref()
            .ok()
            .and_then(|s| s.quarantined(&file, &today()))
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
            ci: options.ci.clone(),
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
            packages: Vec::new(),
            attempts: Vec::new(),
        };
        let display_scale = Cell::new(None);
        let (local_only, hide, ai_screen, clean_home) = settings
            .as_ref()
            .map(|s| {
                (
                    s.local_only,
                    s.hide.clone(),
                    s.ai_screen.clone(),
                    s.clean_home,
                )
            })
            .unwrap_or_default();
        // A project whose settings can't be read keeps all its runs.
        let retention = settings.as_ref().ok().map(ProjectSettings::run_retention);
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
        // and servers leaves the screen to others and has nothing to show in a video.
        let uses_screen = prepared.is_ok() && crate::screen::uses_screen(&project, &file);
        let screen = if uses_screen {
            let waited = Instant::now();
            let screen = self.take_screen(&report.test, options);
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
                    options,
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
                    options,
                );
            }
            Ok((root, start, environment, data)) => {
                // The video's clock starts just before the recording does; step markers count
                // from it.
                let video =
                    if options.video != VideoMode::Off && uses_screen && !report.app.is_empty() {
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
                // Home folders that apps started with during the run; they go when it ends.
                let mut homes: Vec<CleanHome> = Vec::new();
                // Apps whose settings the run cleared; they come back when it ends.
                let mut saved: Vec<String> = Vec::new();
                for pass in passes {
                    let row = pass.zip(table.clone()).map(|(row, table)| (table, row));
                    let data_row = row.as_ref().map(|(table, row)| DataRow {
                        number: u32::try_from(row + 1).unwrap_or(u32::MAX),
                        label: mask_secrets(&secrets, &table.label(*row)),
                        hash: row_hash(&secrets, table, *row),
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
                            clean_home,
                            homes: &mut homes,
                            saved_settings: options.saved_settings.as_deref(),
                            saved: &mut saved,
                            ai_screen: ai_screen.as_ref(),
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
                        self.finish_attempt(&mut report, attempt, options);
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
                // An app quits before its home folder goes: none is left writing into a folder
                // that is gone, and the next test finds the app as the person keeps it.
                let mut quit: Vec<&str> = homes.iter().map(|h| h.app.as_str()).collect();
                quit.dedup();
                for app in quit {
                    let _ = self.hands.quit(app);
                }
                if let Some(folder) = &options.saved_settings {
                    for app in &saved {
                        if !app_settings::put_back(folder, app).unwrap_or(false) {
                            settings_come_back_later(&mut report, app);
                        }
                    }
                }
            }
        }

        drop(screen);
        (report.verdict, report.flaky) = summarize(&report.attempts);
        report.nothing_checked &= report.verdict == Verdict::Passed;
        report.packages = used_packages(&project, &report.attempts);
        report.duration_ms = millis(clock.elapsed());
        report.display_scale = display_scale.get();
        let path = dir.join(REPORT_FILE);
        let mut json = serde_json::to_string_pretty(&report).map_err(io::Error::other)?;
        json.push('\n');
        fs::write(&path, json)?;
        let run = Run { dir, report };
        // The run itself is saved; failing here only leaves the next CI job an older result.
        let _ = crate::last_results::record(&project, &run);
        if let Some(retention) = &retention {
            crate::saved::prune_after_run(
                &project,
                retention,
                options.keep_videos_for,
                SystemTime::now(),
            );
        }
        self.events.send(&Event::RunFinished {
            verdict: run.report.verdict,
            flaky: run.report.flaky,
            report: path,
        });
        Ok(run)
    }

    /// Takes the screen for a test that acts on it, waiting while another run has it: `Ok(None)`
    /// when no lock is asked for or the lock file can't be used (a locked-down home folder must
    /// not stop tests that would otherwise run), `Err` when the run is cancelled while waiting.
    fn take_screen(
        &self,
        test: &str,
        options: &RunOptions,
    ) -> Result<Option<ScreenLock>, Cancelled> {
        let Some(path) = &options.screen_lock else {
            return Ok(None);
        };
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

    fn finish_attempt(&self, report: &mut RunReport, mut attempt: Attempt, options: &RunOptions) {
        attempt.failure = attempt.failure.map(|f| f.worded(&options.language()));
        self.events.send(&Event::AttemptFinished {
            attempt: attempt.number,
            verdict: attempt.verdict,
            failure: attempt.failure.clone(),
        });
        report.attempts.push(attempt);
    }
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
        .find(|b| matches!(&b.kind, BlockKind::UseData { file: f, .. } if f == file));
    let typed = |f: Failure| match block {
        Some(b) => f.with("blockType", b.kind.type_name()),
        None => f,
    };
    let block = block.map_or("", |b| b.id.as_str());
    let path = normalize(&t.dir.join(file));
    if !path.starts_with(project) {
        return Err(typed(
            Failure::new("run.path.outsideProject")
                .with("block", block)
                .with("path", file),
        ));
    }
    let table = DataTable::load(&path).map_err(|e| typed(data_unreadable(block, file, &e)))?;
    // A table without rows would leave nothing to run.
    let chosen = chosen_rows(&table, rows)
        .ok()
        .filter(|r| !r.is_empty())
        .ok_or_else(|| typed(no_row(block, file, rows.map_or(0, |r| *r.start()), &table)))?;
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

/// Today's date on this computer as `YYYY-MM-DD`, the day a quarantine is checked against.
pub fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// `text` with the values of `secrets` covered.
fn mask_secrets(secrets: &HashMap<String, String>, text: &str) -> String {
    let mut values: Vec<&String> = secrets.values().filter(|v| !v.is_empty()).collect();
    values.sort_by_key(|v| std::cmp::Reverse(v.len()));
    values.into_iter().fold(text.to_owned(), |text, v| {
        text.replace(v.as_str(), SECRET_MASK)
    })
}

/// Short fingerprint of the values of row `row` of `table`, counted from 0, with secrets
/// covered so that it tells nothing about them.
fn row_hash(secrets: &HashMap<String, String>, table: &DataTable, row: usize) -> Option<String> {
    let values = table.rows.get(row)?;
    let text: String = table
        .columns
        .iter()
        .zip(values)
        .map(|(column, value)| format!("{column}\u{1f}{}\u{1e}", mask_secrets(secrets, value)))
        .collect();
    content_hash(text.as_bytes()).get(..16).map(str::to_owned)
}

/// SHA-256 of a test file's bytes in hex: runs with the same hash ran the same test.
pub fn content_hash(bytes: &[u8]) -> String {
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

/// Adds the version and address the block names for its package to the failure that it isn't
/// installed, for the person who has to install it.
fn with_package(failure: Failure, package: Option<&PackageNeed>) -> Failure {
    match package {
        Some(need) if failure.code == "run.block.notInstalled" => {
            let failure = failure.with("version", &need.version);
            match &need.source {
                Some(source) => failure.with("source", source),
                None => failure,
            }
        }
        _ => failure,
    }
}

fn load_test(project: &Path, path: &Path) -> Result<TestFile, Failure> {
    let file = relative(project, path);
    let unreadable = |reason: String| {
        Failure::new("run.test.unreadable")
            .with("test", &file)
            .with("reason", reason)
    };
    let package = jevision_core::package_of_path(&file);
    let text = fs::read_to_string(path).map_err(|e| match package {
        Some(id) if e.kind() == std::io::ErrorKind::NotFound => {
            Failure::new("run.block.notInstalled").with("package", id)
        }
        _ => unreadable(e.to_string()),
    })?;
    if let Some(id) = package {
        let manifest = read_manifest(path).map_err(unreadable)?;
        if let Some(needs) = manifest
            .min_version
            .filter(|v| *v > jevision_core::app_version())
        {
            return Err(Failure::new("run.block.needsNewer")
                .with("package", id)
                .with("needs", needs)
                .with("have", jevision_core::app_version()));
        }
    }
    let flow = jevision_core::parse(&text).map_err(|e| {
        match e.issues.iter().find(|i| i.code == "flow.format.tooNew") {
            Some(newer) => Failure::new("run.test.needsNewer")
                .with("test", &file)
                .with("found", newer.param("found").unwrap_or_default())
                .with("supported", newer.param("supported").unwrap_or_default()),
            None => unreadable(e.to_string()),
        }
    })?;
    let errors = jevision_core::validate(&flow)
        .iter()
        .filter(|i| i.severity == Severity::Error)
        .count();
    if errors > 0 {
        return Err(Failure::new("run.test.invalid")
            .with("test", &file)
            .with("errors", errors));
    }
    // The run works with paths: a label becomes the path of its file now. A file that is
    // missing stops the test here, so that no step makes a new reference in its place.
    let mut flow = flow;
    let dir = path.parent().unwrap_or(project);
    for issue in jevision_core::resolve_labels(&mut flow, dir, Some(project)).issues {
        let p = |name| issue.param(name).unwrap_or_default().to_owned();
        match issue.code.as_str() {
            "resource.missing" => {
                return Err(Failure::new("run.resource.missing")
                    .with("test", &file)
                    .with("block", p("block"))
                    .with("name", p("name")));
            }
            "resource.detached" => eprintln!("{file}: {}", jevision_core::describe(&issue)),
            _ => {}
        }
    }
    Ok(TestFile {
        path: path.to_owned(),
        dir: dir.to_owned(),
        file,
        flow,
    })
}

/// The packages whose steps ran in `attempts`, each once.
fn used_packages(project: &Path, attempts: &[Attempt]) -> Vec<UsedPackage> {
    let mut used: Vec<UsedPackage> = Vec::new();
    let sources = fs::read_to_string(project.join(jevision_core::PACKAGE_LOCK_FILE))
        .ok()
        .and_then(|text| jevision_core::PackageLock::parse(&text).ok())
        .unwrap_or_default()
        .sources;
    for step in attempts.iter().flat_map(|a| &a.steps) {
        let Some(id) = jevision_core::package_of_path(&step.test) else {
            continue;
        };
        if used.iter().any(|p| p.file == step.test) {
            continue;
        }
        if let Ok(manifest) = read_manifest(&project.join(&step.test)) {
            used.push(UsedPackage {
                id: id.to_owned(),
                name: manifest.name,
                version: manifest.version.to_string(),
                author: manifest.author,
                source: sources.get(id).cloned(),
                file: step.test.clone(),
            });
        }
    }
    used
}

/// The `block.json` beside the steps of a package.
fn read_manifest(flow_file: &Path) -> Result<jevision_core::BlockManifest, String> {
    let file = flow_file.with_file_name(jevision_core::MANIFEST_FILE);
    let text =
        fs::read_to_string(&file).map_err(|e| format!("{}: {e}", jevision_core::MANIFEST_FILE))?;
    jevision_core::parse_manifest(&text).map_err(|e| e.to_string())
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
    Model(Box<VisionAnswer>),
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
    /// The project starts apps whose state a block resets with an empty home folder.
    clean_home: bool,
    /// Home folders made for apps so far in the run.
    homes: &'r mut Vec<CleanHome>,
    /// Journal folder for the settings of apps the run starts with an empty home folder;
    /// `None` leaves their settings alone.
    saved_settings: Option<&'r Path>,
    /// Apps whose settings the run saved and cleared so far.
    saved: &'r mut Vec<String>,
    /// The AI the project chose for checks by meaning.
    ai_screen: Option<&'r AiChoice>,
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
            note: None,
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
                    // The block is named by its type wherever the failure is shown.
                    if failure.param("block") == id {
                        failure = failure.with("blockType", &step.block_type);
                    }
                    failure = failure.worded(&self.options.language());
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
                arguments,
                environment,
                window_size,
                window_position,
                next,
            } => {
                let app = app.clone().unwrap_or_else(|| t.flow.app.clone());
                let arguments = arguments
                    .iter()
                    .map(|a| self.substitute(id, a))
                    .collect::<Result<Vec<_>, _>>()?;
                let environment = environment
                    .iter()
                    .map(|(name, value)| Ok((name.clone(), self.substitute(id, value)?)))
                    .collect::<Result<HashMap<_, _>, Stop>>()?;
                let (start, note) = self.app_start(id, &app, *reset_state)?;
                self.hands(id, |h| h.open_app(&app, start, &arguments, &environment))?;
                step.note = note;
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
                modifiers,
                wait_for_change,
                next,
            } => {
                let app = self.app();
                self.hands(id, |h| h.activate(&app))?;
                let before = self.snapshot(step, "before")?;
                let at = self.locate(t, id, target, before.as_ref(), step)?;
                self.hands(id, |h| h.hover(&app, at, *modifiers))?;
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
                modifiers,
                wait_for_change,
                next,
            } => {
                let app = self.app();
                self.hands(id, |h| h.activate(&app))?;
                let before = self.snapshot(step, "before")?;
                let middle = Target::Point(Point { x: 0.5, y: 0.5 });
                let at =
                    self.locate(t, id, at.as_ref().unwrap_or(&middle), before.as_ref(), step)?;
                self.hands(id, |h| {
                    h.scroll(&app, at, *direction, *amount, *unit, *modifiers)
                })?;
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
            BlockKind::Check {
                of: CheckOf::Element(check),
                next,
            } => {
                let check = self.fill_check(id, check)?;
                let nodes = self.elements(id)?;
                self.snapshot(step, "after")?;
                let seen = check
                    .observe(&nodes)
                    .map_err(|found| fail(self.ambiguous(id, &check.element, &found)))?;
                match seen.element {
                    _ if seen.holds => Ok(next.clone()),
                    None => Err(fail(self.not_found(id, &check.element, &nodes))),
                    Some(_) => Err(fail(self.element_failure(
                        Failure::new("run.element.state").with("block", id),
                        &check,
                        &seen.now,
                    ))),
                }
            }
            BlockKind::Check {
                of: CheckOf::Screen(reference),
                next,
            } => {
                let shot = self.capture(id)?;
                match self.evaluate(t, id, reference, &shot, true, &mut None)? {
                    Ok(e) => {
                        if let Some(loaded) = &e.reference {
                            let path = if reference.image.contains("${") {
                                outside(t, &loaded.name)
                            } else {
                                self.resolve(t, id, &loaded.name)?
                            };
                            step.reference = Some(relative(self.project, &path));
                        }
                        self.record_comparison(step, &e)?;
                        if !e.passed {
                            return Err(fail(mismatch(id, reference, &e)));
                        }
                    }
                    // A picture from a variable is never made up from the screen: it is what
                    // the test wants the screen to look like.
                    Err(name) if reference.image.contains("${") => {
                        return Err(missing_reference(id, &name));
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
                // Screen conditions are checked on one screenshot and element conditions on one
                // look at the window, each taken only if needed; the first condition that holds
                // wins.
                let mut shot = None;
                let mut nodes = None;
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
                        Condition::Element(written) => {
                            let check = self.fill_check(id, written)?;
                            let nodes = match &mut nodes {
                                Some(nodes) => &*nodes,
                                slot => slot.insert(self.elements(id)?),
                            };
                            let seen = check.observe(nodes).map_err(|found| {
                                fail(self.ambiguous(id, &check.element, &found))
                            })?;
                            (seen.holds, element_check_name(written))
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
                    ClipboardAction::PutImage { image } => {
                        let path = self.outside_path(t, id, image)?;
                        if !system::is_png(&path) {
                            return Err(fail(
                                Failure::new("run.clipboard.notPicture")
                                    .with("block", id)
                                    .with("image", relative(self.project, &path)),
                            ));
                        }
                        let png = fs::read(&path)
                            .map_err(|e| fail(unreadable_reference(id, image, e.to_string())))?;
                        self.hands(id, |h| h.set_clipboard_image(&png))?;
                    }
                    ClipboardAction::Read { save_as, content } => {
                        let value = match content {
                            ClipboardContent::Text => self.hands(id, |h| h.clipboard())?,
                            ClipboardContent::Image => {
                                let png =
                                    self.hands(id, |h| h.clipboard_image())?.ok_or_else(|| {
                                        fail(
                                            Failure::new("run.clipboard.noImage").with("block", id),
                                        )
                                    })?;
                                let name = format!(
                                    "shots/{}-{:03}-{}-clipboard.png",
                                    self.attempt, step.seq, step.block
                                );
                                let path = self.dir.join(&name);
                                write(&path, &png)?;
                                path.display().to_string()
                            }
                            ClipboardContent::Files => {
                                let files = self.hands(id, |h| h.clipboard_files())?;
                                if files.is_empty() {
                                    return Err(fail(
                                        Failure::new("run.clipboard.noFiles").with("block", id),
                                    ));
                                }
                                files.join("\n")
                            }
                        };
                        self.save(step, save_as, value);
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
                language,
                time_zone,
                shift,
                next,
            } => {
                // The block's own language, then the app's, then the process's.
                let process = jevision_l10n::language();
                let language = system::DateLanguage::from_code(
                    language
                        .as_deref()
                        .or(self.options.language.as_deref())
                        .unwrap_or(&process),
                );
                let text = system::date_time_in(
                    chrono::Utc::now(),
                    time_zone.as_deref(),
                    format,
                    *shift,
                    language,
                )
                .map_err(|reason| {
                    fail(
                        Failure::new("run.dateTime.failed")
                            .with("block", id)
                            .with("reason", reason),
                    )
                })?;
                self.save(step, save_as, text);
                Ok(next.clone())
            }
            BlockKind::RunTest {
                test,
                package,
                inputs,
                next,
            } => {
                let path = self.resolve_filled(t, id, test)?;
                let mut given = HashMap::with_capacity(inputs.len());
                for (name, value) in inputs {
                    given.insert(name.clone(), self.substitute(id, value)?);
                }
                for (name, value) in jevision_core::input_defaults(&path) {
                    given.entry(name).or_insert(value);
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
                    .map_err(|f| fail(with_package(f, package.as_ref()).with("block", id)))?;
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
        if let WaitUntil::Element(check) = until {
            return self.wait_for_element(id, check, timeout_ms, step);
        }
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
                                step.vision = Some(*answer);
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
            WaitUntil::Element(_) => unreachable!("handled above"),
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

    /// Waits until an element of the window is as `check` expects; an element that isn't there
    /// yet is simply not the state waited for.
    fn wait_for_element(
        &self,
        id: &str,
        check: &ElementCheck,
        timeout_ms: u64,
        step: &mut Step,
    ) -> Result<(), Stop> {
        let check = self.fill_check(id, check)?;
        let mut last = None;
        let reached = self.poll(id, Duration::from_millis(timeout_ms), || {
            let nodes = self.elements(id)?;
            let seen = check
                .observe(&nodes)
                .map_err(|found| fail(self.ambiguous(id, &check.element, &found)))?;
            let (holds, now) = (seen.holds, seen.now);
            last = Some((now, nodes));
            Ok(holds.then_some(()))
        })?;
        self.snapshot(step, "after")?;
        match (reached, last) {
            (None, Some((now, nodes))) => {
                let failure = Failure::new("run.waitFor.elementTimeout")
                    .with("block", id)
                    .with("seconds", seconds(timeout_ms));
                let failure = self.element_failure(failure, &check, &now);
                Err(fail(if now == ElementState::Absent {
                    self.near(failure, &check.element, &nodes)
                } else {
                    failure
                }))
            }
            _ => Ok(()),
        }
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
        // The block's own choice, then the project's, then the app's, which empty stands for;
        // a run that forces its own AI asks only that.
        let chosen = if self.options.force_vision {
            AiChoice::default()
        } else {
            AiChoice::resolve(
                &AiChoice {
                    provider: provider.to_owned(),
                    model: model.to_owned(),
                },
                self.ai_screen,
            )
        };
        let (provider, model) = (chosen.provider.as_str(), chosen.model.trim());
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
        let fallback = self.runner.fallback.filter(|f| {
            !self.options.force_vision
                && (!local_only || f.is_local())
                && f.provider_id() != judge.provider_id()
        });
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
            judged: Judged::Model(Box::new(VisionAnswer {
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
            })),
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
        if let LimitState::Reached(f) = self.limit_state(id, judge)? {
            return Err(Unanswered::Unavailable(f.with("block", id)));
        }
        let judgement = judge.judge(question).map_err(|e| {
            let failure = vision_failure(judge, &e).with("block", id);
            if e.unavailable() {
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
        let (cost, warning) = self.log_call(t, id, judge, &model, judgement.usage)?;
        Ok(Answer {
            provider: judge.provider().to_owned(),
            model,
            judgement,
            cost,
            warning,
        })
    }

    /// Where `judge`'s provider stands against its limits; within them when nothing is logged.
    fn limit_state(&self, id: &str, judge: &dyn VisionJudge) -> Result<LimitState, Unanswered> {
        let Some(book) = self.runner.usage else {
            return Ok(LimitState::Within);
        };
        book.limit_state(judge.provider_id(), judge.provider(), SystemTime::now())
            .map_err(|e| Unanswered::Failed(book.journal_failure(&e).with("block", id)))
    }

    /// Logs a call of `judge` that used `usage`; returns what it cost, when the price is known,
    /// and a warning when a limit is close or used up now.
    fn log_call(
        &self,
        t: &TestFile,
        id: &str,
        judge: &dyn VisionJudge,
        model: &str,
        usage: crate::Usage,
    ) -> Result<(Option<f64>, Option<Failure>), Unanswered> {
        let Some(book) = self.runner.usage else {
            return Ok((None, None));
        };
        book.log(judge.provider_id(), model, &t.file, id, usage)
            .map_err(|e| Unanswered::Failed(book.journal_failure(&e).with("block", id)))?;
        let cost = price_of(&book.prices, judge.provider_id(), model)
            .map(|p| p.cost(usage.input_tokens, usage.output_tokens));
        let warning = match self.limit_state(id, judge)? {
            LimitState::Near(f) | LimitState::Reached(f) => Some(f),
            LimitState::Within => None,
        };
        Ok((cost, warning))
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
        // A picture a variable names has no dark twin and may be anywhere on this Mac.
        let variable = r.image.contains("${");
        let mut name = if variable {
            self.substitute(id, &r.image)?
        } else if shot.dark {
            dark_variant(&r.image)
        } else {
            r.image.clone()
        };
        if !variable && !strict && name != r.image && !self.resolve(t, id, &name)?.exists() {
            name = r.image.clone();
        }
        if let Some(loaded) = cache.take().filter(|c| c.name == name) {
            return Ok(Ok(loaded));
        }
        let path = if variable {
            outside(t, &name)
        } else {
            self.resolve(t, id, &name)?
        };
        let bytes = match fs::read(path) {
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
            Judged::Model(answer) => step.vision = Some(answer.as_ref().clone()),
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
            Target::Point(_) | Target::Description(_) | Target::Area(_) | Target::Element(_) => {
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
                        self.find_described(t, id, what, shot, &window, step)?
                    }
                    Target::Area(area) => self.area_centre(id, area)?,
                    Target::Element(query) => self.element_centre(id, query)?,
                    Target::Image(_) => unreachable!("handled above"),
                };
                // The test keeps the point as fractions of the window; hands click in points.
                return Ok(Point {
                    x: at.x * f64::from(window.width) / shot.scale,
                    y: at.y * f64::from(window.height) / shot.scale,
                });
            }
        };
        let (name, path) = if image.contains("${") {
            // A picture a variable names, such as one the test took from the clipboard: it has
            // no dark twin and may be anywhere on this Mac.
            let name = self.substitute(id, image)?;
            let path = outside(t, &name);
            (name, path)
        } else {
            let dark = dark_variant(image);
            let name = if shot.is_some_and(|s| s.dark) && self.resolve(t, id, &dark)?.exists() {
                dark
            } else {
                image.clone()
            };
            let path = self.resolve(t, id, &name)?;
            (name, path)
        };
        let name = &name;
        let bytes = match fs::read(path) {
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

    /// The centre of the area `text` names (`x,y,width,height`, written in the test or kept in a
    /// variable), in fractions of the window.
    fn area_centre(&self, id: &str, text: &str) -> Result<Point, Stop> {
        let filled = self.substitute(id, text)?;
        let area = parse_area(&filled).ok_or_else(|| {
            fail(
                Failure::new("run.target.badArea")
                    .with("block", id)
                    .with("value", &filled),
            )
        })?;
        Ok(Point {
            x: area.x + area.width / 2.0,
            y: area.y + area.height / 2.0,
        })
    }

    /// The centre of the element `query` asks for, in fractions of the window.
    fn element_centre(&self, id: &str, query: &ElementQuery) -> Result<Point, Stop> {
        let query = self.fill_query(id, query)?;
        let nodes = self.elements(id)?;
        let area = match query.find(&nodes) {
            Ok(node) => node.frame,
            Err(Missing::NotFound(_)) => return Err(fail(self.not_found(id, &query, &nodes))),
            Err(Missing::Ambiguous(found)) => return Err(fail(self.ambiguous(id, &query, &found))),
        };
        Ok(Point {
            x: area.x + area.width / 2.0,
            y: area.y + area.height / 2.0,
        })
    }

    /// The elements of the window of the app under test.
    fn elements(&self, id: &str) -> Result<Vec<ElementNode>, Stop> {
        let app = self.app();
        self.hands(id, |h| h.elements(&app))
    }

    /// `query` with the values of its placeholders.
    fn fill_query(&self, id: &str, query: &ElementQuery) -> Result<ElementQuery, Stop> {
        Ok(ElementQuery {
            id: self.substitute(id, &query.id)?,
            title: self.substitute(id, &query.title)?,
            ..query.clone()
        })
    }

    /// `check` with the values of its placeholders.
    fn fill_check(&self, id: &str, check: &ElementCheck) -> Result<ElementCheck, Stop> {
        let is = match check.is.text() {
            Some(text) => check.is.with_text(self.substitute(id, text)?),
            None => check.is.clone(),
        };
        Ok(ElementCheck {
            element: self.fill_query(id, &check.element)?,
            is,
        })
    }

    /// Elements for a failure, as JSON a failure can carry in any language; secrets are masked
    /// before the escaping could hide them from [`Self::redact`].
    fn elements_param<'q>(&self, queries: impl IntoIterator<Item = &'q ElementQuery>) -> String {
        let shown: Vec<_> = queries
            .into_iter()
            .map(|q| ElementQuery {
                id: self.mask(&q.id),
                title: self.mask(&q.title),
                ..q.clone()
            })
            .collect();
        serde_json::to_string(&shown).unwrap_or_default()
    }

    /// `failure` naming the element `check` looked at, what it expected and what it saw.
    fn element_failure(
        &self,
        failure: Failure,
        check: &ElementCheck,
        now: &ElementState,
    ) -> Failure {
        failure
            .with("element", self.elements_param([&check.element]))
            .with("is", check.is.name())
            .with("to", check.is.text().unwrap_or_default())
            .with("now", now.name())
            .with("actual", now.text().unwrap_or_default())
    }

    /// `failure` with the elements of the window like the one `query` asks for.
    fn near(&self, failure: Failure, query: &ElementQuery, nodes: &[ElementNode]) -> Failure {
        let similar: Vec<_> = query.similar(nodes).iter().map(|n| n.as_query()).collect();
        failure
            .with("similar", self.elements_param(&similar))
            .with("elements", nodes.len())
    }

    fn not_found(&self, id: &str, query: &ElementQuery, nodes: &[ElementNode]) -> Failure {
        let failure = Failure::new("run.element.notFound")
            .with("block", id)
            .with("element", self.elements_param([query]));
        self.near(failure, query, nodes)
    }

    /// Several elements answer `query`: the first few are listed with the index that picks each.
    fn ambiguous(&self, id: &str, query: &ElementQuery, found: &[&ElementNode]) -> Failure {
        let listed: Vec<_> = (0..)
            .zip(found.iter().take(LISTED_ELEMENTS))
            .map(|(i, n)| ElementQuery {
                index: Some(i),
                ..n.as_query()
            })
            .collect();
        Failure::new("run.element.ambiguous")
            .with("block", id)
            .with("count", found.len())
            .with("element", self.elements_param([query]))
            .with("found", self.elements_param(&listed))
    }

    /// Asks the locator where the element `what` describes is on `shot`, private areas covered;
    /// returns the centre of it in fractions of the window.
    fn find_described(
        &self,
        t: &TestFile,
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
        let stopped = |u: Unanswered| match u {
            Unanswered::Unavailable(f) | Unanswered::Failed(f) => fail(f),
        };
        if let LimitState::Reached(f) = self.limit_state(id, locator).map_err(stopped)? {
            return Err(fail(f.with("block", id)));
        }
        let private = self.private_areas(shot, &[]);
        let sent = masked(id, window, &private, None)?;
        let search = locator
            .locate(&sent, what)
            .map_err(|e| fail(vision_failure(locator, &e).with("block", id)))?;
        // The search cost tokens whether or not the element is there.
        self.log_call(t, id, locator, locator.model(), search.usage)
            .map_err(stopped)?;
        self.record_sent(step, &sent)?;
        let area = search.found.map(|f| f.area).ok_or_else(|| {
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
                    let own = self.environment.and_then(|e| e.values.get(name));
                    let value = match (own, self.environment) {
                        (Some(value), _) => value.as_str(),
                        (None, environment) if name == ENVIRONMENT_NAME => {
                            environment.map_or("", |e| e.name.as_str())
                        }
                        (None, None) => {
                            return Err(fail(Failure::new("run.env.notChosen").with("name", name)));
                        }
                        (None, Some(environment)) => {
                            return Err(fail(
                                Failure::new("run.env.missing")
                                    .with("name", name)
                                    .with("environment", &environment.name),
                            ));
                        }
                    };
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
            vars.insert(format!("env.{ENVIRONMENT_NAME}"), environment.name.clone());
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

    /// How an `openApp` block starts `app`, and what its step says about it. Without a reset
    /// the app starts as usual; with one it starts fresh, and with an empty home folder of its
    /// own when the project asks for that and macOS doesn't keep the app's data in the sandbox.
    fn app_start(
        &mut self,
        id: &str,
        app: &str,
        reset: bool,
    ) -> Result<(AppStart, Option<Failure>), Stop> {
        if !reset {
            return Ok((AppStart::AsUsual, None));
        }
        if !self.clean_home {
            return Ok((AppStart::Fresh, None));
        }
        if self.hands(id, |h| h.sandboxed(app))? {
            let note = Failure::new("run.openApp.sandboxed").with("app", app);
            return Ok((AppStart::Fresh, Some(note)));
        }
        if let Some(folder) = self.saved_settings {
            // Closed first, so the app can't write its settings back once they are cleared.
            self.hands(id, |h| {
                h.quit(app)?;
                if app_settings::still_open(app) {
                    return Err(HandsError::StillRunning(app.to_owned()));
                }
                Ok(())
            })?;
            app_settings::save_and_clear(folder, app).map_err(|e| {
                fail(
                    Failure::new("run.openApp.settingsNotSaved")
                        .with("app", app)
                        .with("reason", e),
                )
            })?;
            if !self.saved.iter().any(|a| a == app) {
                self.saved.push(app.to_owned());
            }
        }
        let home = home::make(app).map_err(|e| write_failed(&home::homes_folder(), e))?;
        let start = AppStart::CleanHome {
            home: home.path.to_string_lossy().into_owned(),
        };
        self.homes.push(home);
        let note = Failure::new("run.openApp.cleanHome").with("app", app);
        Ok((start, Some(note)))
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
        mask_secrets(&*self.secrets, text)
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

/// The steps that said the settings of `app` come back when the run ends say instead that they
/// come back at the next run or launch: the app was still open, or `defaults` failed.
fn settings_come_back_later(report: &mut RunReport, app: &str) {
    let steps = report.attempts.iter_mut().flat_map(|a| &mut a.steps);
    for note in steps.filter_map(|s| s.note.as_mut()) {
        if note.code == "run.openApp.cleanHome" && note.param("app") == app {
            *note = Failure::new("run.openApp.settingsLater").with("app", app);
        }
    }
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
    .with("similarity", kept_percent(comparison.similarity))
    .with("threshold", kept_percent(threshold));
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
pub fn create_run_dir(project: &Path, test: &Path, started_at: &str) -> io::Result<PathBuf> {
    let runs = project.join(".jevision").join("runs");
    fs::create_dir_all(&runs)?;
    let digits: String = started_at.chars().filter(char::is_ascii_digit).collect();
    let file = test
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let base = format!("{}-{}-{}", &digits[..8], &digits[8..14], slug(&file));
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

/// The part of a run folder's name that comes from the test file `test`: its name without
/// `.flow.json` and folders, with anything but ASCII letters, digits, `-` and `_` as `-`.
fn slug(test: &str) -> String {
    let file = test.rsplit('/').next().unwrap_or(test);
    let stem = file.strip_suffix(".flow.json").unwrap_or(file);
    stem.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// A run folder in `.jevision/runs`, known by its name before its report is read.
struct SavedRun {
    dir: PathBuf,
    /// The time the run started, `YYYYMMDDhhmmss` in UTC.
    second: String,
    /// What follows the time in the name: the test's [`slug`], with `-2` and on for runs of the
    /// same test in the same second. `None` for a folder named some other way.
    rest: Option<String>,
    /// The run, read already to learn its time when the name doesn't tell it.
    run: Option<Run>,
}

impl SavedRun {
    fn load(self) -> Option<Run> {
        self.run.or_else(|| Run::load(&self.dir).ok())
    }
}

/// The run folders of the project, grouped by the second they started in, newest first.
/// Folders are named `20261018-101500-login`, or `ci-31-20261018-101500-login` when imported
/// from CI; a folder named any other way is read to learn its time. Hidden folders, such as an
/// import still going, are left out.
fn saved_runs(project: &Path) -> Vec<Vec<SavedRun>> {
    let Ok(entries) = fs::read_dir(project.join(".jevision").join("runs")) else {
        return Vec::new();
    };
    let mut saved: Vec<SavedRun> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                return None;
            }
            let dir = entry.path();
            let stamped = name
                .strip_prefix("ci-")
                .and_then(|rest| rest.split_once('-'))
                .filter(|(id, _)| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()))
                .map_or(name.as_str(), |(_, rest)| rest);
            let time = stamped.get(..15).filter(|time| {
                time.bytes().enumerate().all(|(i, b)| {
                    if i == 8 {
                        b == b'-'
                    } else {
                        b.is_ascii_digit()
                    }
                })
            });
            let rest = stamped.get(15..).and_then(|rest| rest.strip_prefix('-'));
            if let (Some(time), Some(rest)) = (time, rest) {
                return Some(SavedRun {
                    second: time.replace('-', ""),
                    rest: Some(rest.to_owned()),
                    dir,
                    run: None,
                });
            }
            let run = Run::load(&dir).ok()?;
            let digits: String = run
                .report
                .started_at
                .chars()
                .filter(char::is_ascii_digit)
                .take(14)
                .collect();
            Some(SavedRun {
                second: digits,
                rest: None,
                dir,
                run: Some(run),
            })
        })
        .collect();
    saved.sort_by(|a, b| b.second.cmp(&a.second));
    let mut seconds: Vec<Vec<SavedRun>> = Vec::new();
    for run in saved {
        match seconds.last_mut() {
            Some(second) if second[0].second == run.second => second.push(run),
            _ => seconds.push(vec![run]),
        }
    }
    seconds
}

/// UTC time as `2026-10-18T14:30:05.123Z`.
pub fn rfc3339(time: SystemTime) -> String {
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
    fn cloudflare_failures_read_as_sentences_in_each_language() {
        let judge = jevision_vision::Client::new(
            jevision_vision::Config {
                provider: Provider::JevCloudflare,
                model: Provider::JevCloudflare.default_model().into(),
                address: String::new(),
                key: None,
                pictures: true,
                account_id: String::new(),
            },
            crate::TIMEOUT,
        );
        for (e, part) in [
            (VisionError::NoAccount, "JEV via Cloudflare"),
            (VisionError::BadAccount, "Account ID"),
            (VisionError::CloudflareAccess, "Account ID"),
            (VisionError::CloudflareNoBalance, "AI Gateway"),
            (VisionError::PictureSize, "Cloudflare"),
        ] {
            let failure = vision_failure(&judge, &e);
            for lang in ["en", "ru"] {
                let text = crate::describe_vision_in(lang, &failure);
                assert!(
                    text.contains(part) && !text.contains("%@"),
                    "{lang}: {text}"
                );
            }
        }
    }

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

    fn dir_name(dir: &Path) -> String {
        dir.file_name().unwrap().to_string_lossy().into_owned()
    }

    /// A saved run's test file, verdict and start time.
    type SavedReport<'a> = (&'a str, &'a str, &'a str);

    /// A project folder with these runs saved in it: folder name and report; a run without a
    /// report hasn't finished yet.
    fn project_with_runs(name: &str, runs: &[(&str, Option<SavedReport>)]) -> PathBuf {
        let project =
            std::env::temp_dir().join(format!("jevision-runs-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&project);
        for (folder, report) in runs {
            let dir = project.join(".jevision/runs").join(folder);
            fs::create_dir_all(&dir).unwrap();
            if let Some((file, verdict, started_at)) = report {
                let json = format!(
                    r#"{{"format":1,"test":"t","file":"{file}","app":"a","startedAt":"{started_at}",
                    "durationMs":1500,"verdict":"{verdict}","flaky":false,"virtualMachine":false,"attempts":[]}}"#
                );
                fs::write(dir.join("report.json"), json).unwrap();
            }
        }
        project
    }

    #[test]
    fn the_latest_run_of_each_test_is_the_one_the_history_shows_first() {
        let project = project_with_runs(
            "latest",
            &[
                (
                    "20261018-100000-login",
                    Some(("login.flow.json", "passed", "2026-10-18T10:00:00.000Z")),
                ),
                (
                    "20261018-110000-login",
                    Some(("login.flow.json", "failed", "2026-10-18T11:00:00.100Z")),
                ),
                (
                    "20261018-110000-login-2",
                    Some(("login.flow.json", "passed", "2026-10-18T11:00:00.900Z")),
                ),
                (
                    "20261018-120000-login",
                    Some((
                        "admin/login.flow.json",
                        "cancelled",
                        "2026-10-18T12:00:00.000Z",
                    )),
                ),
                (
                    "ci-31-20261018-130000-pay",
                    Some(("pay.flow.json", "failed", "2026-10-18T13:00:00.000Z")),
                ),
                ("20261018-140000-pay", None),
                (
                    ".ci-32.partial/20261018-150000-pay",
                    Some(("pay.flow.json", "passed", "2026-10-18T15:00:00.000Z")),
                ),
                (
                    "old-run",
                    Some(("cart.flow.json", "blocked", "2026-10-18T09:00:00.000Z")),
                ),
                (
                    "20261018-090000-gone",
                    Some(("gone.flow.json", "passed", "2026-10-18T09:00:00.000Z")),
                ),
                (
                    "20261018-160000-Sign-in-FLOW-JSON",
                    Some(("Sign in.FLOW.JSON", "passed", "2026-10-18T16:00:00.000Z")),
                ),
            ],
        );
        let files: Vec<String> = [
            "login.flow.json",
            "admin/login.flow.json",
            "pay.flow.json",
            "cart.flow.json",
            "never.flow.json",
            "Sign in.FLOW.JSON",
        ]
        .map(String::from)
        .into();
        let latest = Run::latest(&project, &files);
        let folder = |file: &str| latest.get(file).map(|run| dir_name(&run.dir));
        assert_eq!(
            folder("login.flow.json").as_deref(),
            Some("20261018-110000-login-2")
        );
        assert_eq!(
            folder("admin/login.flow.json").as_deref(),
            Some("20261018-120000-login")
        );
        assert_eq!(
            folder("pay.flow.json").as_deref(),
            Some("ci-31-20261018-130000-pay")
        );
        assert_eq!(folder("cart.flow.json").as_deref(), Some("old-run"));
        assert_eq!(
            folder("Sign in.FLOW.JSON").as_deref(),
            Some("20261018-160000-Sign-in-FLOW-JSON")
        );
        assert_eq!(latest.len(), 5);
        // The same runs the history puts first for each test.
        let history = Run::list(&project).unwrap();
        for file in &files {
            let first = history.iter().find(|run| &run.report.file == file);
            assert_eq!(
                first.map(|run| &run.dir),
                latest.get(file).map(|run| &run.dir),
                "{file}"
            );
        }
        assert_eq!(
            Run::newest(&project).map(|run| run.dir),
            history.first().map(|run| run.dir.clone())
        );
        assert_eq!(
            latest["login.flow.json"].report.finished_at(),
            Some(1_792_321_202)
        );
        fs::remove_dir_all(&project).unwrap();
    }

    #[test]
    fn the_newest_run_skips_runs_still_going() {
        let project = project_with_runs(
            "newest",
            &[
                ("20261018-120000-b", None),
                (
                    "20261018-110000-a",
                    Some(("a.flow.json", "passed", "2026-10-18T11:00:00.200Z")),
                ),
                (
                    "20261018-110000-a-2",
                    Some(("a.flow.json", "failed", "2026-10-18T11:00:00.700Z")),
                ),
            ],
        );
        let newest = Run::newest(&project).unwrap();
        assert_eq!(dir_name(&newest.dir), "20261018-110000-a-2");
        assert_eq!(newest.report.verdict, Verdict::Failed);
        fs::remove_dir_all(&project).unwrap();
        assert!(Run::newest(&project).is_none());
        assert!(Run::latest(&project, &["a.flow.json".into()]).is_empty());
    }
}
