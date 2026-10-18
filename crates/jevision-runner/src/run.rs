//! The engine: walks a test's flowchart block by block and records what happened.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::{fs, io};

use jevision_core::{
    BlockKind, Compare, DEFAULT_WAIT_TIMEOUT_MS, Flow, Point, Reference, Severity, Target,
    WaitUntil,
};

use crate::control::{Cancelled, PauseReason};
use crate::data::DataTable;
use crate::image::{Comparison, Image};
use crate::report::{
    Attempt, Failure, REPORT_FORMAT, RunReport, Step, StepStatus, Verdict, VisionAnswer, percent,
};
use crate::{
    Event, EventSink, Hands, HandsError, Openable, Question, RunControl, SECRET_MASK, Screenshot,
    SecretSource, VisionError, VisionJudge,
};

/// How far a pixel channel may be off and still count as the same colour when comparing by
/// similarity; absorbs font smoothing and colour-profile noise.
const SIMILAR_TOLERANCE: u8 = 16;
/// The screen counts as settled when two looks in a row are at least this alike.
const STABLE_SIMILARITY: f64 = 0.999;
/// Longest side, in pixels, of a picture sent to a vision model; larger ones cost more and
/// are scaled down by the providers anyway.
const VISION_MAX_SIDE: u32 = 1568;

/// How a run starts.
#[derive(Debug, Clone)]
pub struct RunOptions {
    /// Block of the test to start from instead of its `start` block.
    pub start_at: Option<String>,
    /// Overrides the test's `retries`.
    pub retries: Option<u32>,
    /// Time between two looks at the screen while waiting.
    pub poll_interval: Duration,
    /// Screenshots must not leave this computer: checks by meaning fail instead of asking a
    /// cloud provider.
    pub local_only: bool,
    /// Name of the paired device that asked for the run, saved in the report.
    pub requested_by: Option<String>,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            start_at: None,
            retries: None,
            poll_interval: Duration::from_millis(250),
            local_only: false,
            requested_by: None,
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
        let clock = Instant::now();
        let project = fs::canonicalize(project)?;
        let test = normalize(&project.join(test));
        let file = relative(&project, &test);
        let started_at = rfc3339(started);
        let dir = create_run_dir(&project, &test, &started_at)?;

        let mut secrets = HashMap::new();
        let loaded = load_test(&project, &test);
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
            requested_by: options.requested_by.clone(),
            attempts: Vec::new(),
        };
        let prepared = loaded.and_then(|t| {
            fetch_secrets(&t.flow, self.secrets, &mut secrets)?;
            let start = options
                .start_at
                .clone()
                .unwrap_or_else(|| t.flow.start.clone());
            if !t.flow.blocks.iter().any(|b| b.id == start) {
                return Err(Failure::new("run.start.unknownBlock").with("block", start));
            }
            Ok((t, start))
        });
        let max_attempts = match &prepared {
            Ok((t, _)) => 1 + options.retries.unwrap_or(t.flow.retries),
            Err(_) => 1,
        };
        self.events.send(&Event::RunStarted {
            test: report.test.clone(),
            file,
            dir: dir.clone(),
            max_attempts,
        });

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
                        steps: Vec::new(),
                    },
                );
            }
            Ok((root, start)) => {
                let mut launched: Vec<String> = Vec::new();
                for number in 1..=max_attempts {
                    // Each retry starts from a clean slate: quit what the last attempt opened.
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
                        seq: 0,
                        steps: Vec::new(),
                        frames: Vec::new(),
                        tests: Vec::new(),
                        apps: Vec::new(),
                        launched: Vec::new(),
                        baselines: Vec::new(),
                    };
                    let (attempt, opened, baselines) = exec.run(&root, &start);
                    launched = opened;
                    report.baselines_created.extend(baselines);
                    let done = attempt.verdict != Verdict::Failed;
                    self.finish_attempt(&mut report, attempt);
                    if done {
                        break;
                    }
                }
            }
        }

        report.verdict = report
            .attempts
            .last()
            .map_or(Verdict::Failed, |a| a.verdict);
        report.flaky = is_flaky(&report.attempts);
        report.duration_ms = millis(clock.elapsed());
        let path = dir.join("report.json");
        let mut json = serde_json::to_string_pretty(&report).map_err(io::Error::other)?;
        json.push('\n');
        fs::write(&path, json)?;
        self.events.send(&Event::RunFinished {
            verdict: report.verdict,
            flaky: report.flaky,
            report: path,
        });
        Ok(Run { dir, report })
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

/// Attempts disagree: some passed and some failed, or they failed in different ways.
fn is_flaky(attempts: &[Attempt]) -> bool {
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
}

enum Judged {
    Pixels(Comparison),
    Model(VisionAnswer),
}

/// One attempt of a run.
struct Exec<'r> {
    runner: &'r Runner<'r>,
    options: &'r RunOptions,
    project: &'r Path,
    dir: &'r Path,
    attempt: u32,
    secrets: &'r mut HashMap<String, String>,
    seq: u32,
    steps: Vec<Step>,
    frames: Vec<Frame>,
    /// Test files running now, outermost first.
    tests: Vec<PathBuf>,
    /// Application each running test acts on.
    apps: Vec<String>,
    launched: Vec<String>,
    baselines: Vec<String>,
}

impl Exec<'_> {
    fn run(mut self, root: &TestFile, start: &str) -> (Attempt, Vec<String>, Vec<String>) {
        let clock = Instant::now();
        self.tests.push(root.path.clone());
        self.apps.push(root.flow.app.clone());
        let (verdict, failure) = match self.run_chain(root, start) {
            Ok(()) => (Verdict::Passed, None),
            Err(Stop::Cancelled) => (Verdict::Cancelled, None),
            Err(Stop::Failed { failure, .. }) => (Verdict::Failed, Some(failure)),
        };
        self.steps.sort_by_key(|s| s.seq);
        let attempt = Attempt {
            number: self.attempt,
            verdict,
            duration_ms: millis(clock.elapsed()),
            failure,
            steps: self.steps,
        };
        (attempt, self.launched, self.baselines)
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
            failure: None,
        };
        let result = match self.perform(t, &block.kind, &mut step) {
            Ok(next) => Ok(next),
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
            BlockKind::OpenApp { app, next } => {
                let app = app.clone().unwrap_or_else(|| t.flow.app.clone());
                self.hands(id, |h| h.open_app(&app))?;
                self.use_app(app);
                self.snapshot(step, "after")?;
                Ok(next.clone())
            }
            BlockKind::OpenFile {
                files,
                app,
                new_instance,
                next,
            } => {
                let mut items = Vec::with_capacity(files.len());
                for (i, file) in files.iter().enumerate() {
                    if file.starts_with("http://") || file.starts_with("https://") {
                        items.push(Openable::Url(file.clone()));
                    } else {
                        let copy = self.fresh_copy(t, id, file, step.seq, i)?;
                        items.push(Openable::File(copy));
                    }
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
                wait_for_change,
                next,
            } => {
                let app = self.app();
                self.hands(id, |h| h.activate(&app))?;
                let before = self.snapshot(step, "before")?;
                let at = self.locate(t, id, target, before.as_ref())?;
                self.hands(id, |h| h.click(&app, at, *button, *double))?;
                if *wait_for_change {
                    let before = match before {
                        Some(shot) => shot,
                        None => self.capture(id)?,
                    };
                    self.wait_for_change(id, &before)?;
                }
                self.snapshot(step, "after")?;
                Ok(next.clone())
            }
            BlockKind::Type { text, into, next } => {
                let app = self.app();
                self.hands(id, |h| h.activate(&app))?;
                let before = self.snapshot(step, "before")?;
                if let Some(target) = into {
                    let at = self.locate(t, id, target, before.as_ref())?;
                    self.hands(id, |h| {
                        h.click(&app, at, jevision_core::MouseButton::Left, false)
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
            BlockKind::Check { reference, next } => {
                let shot = self.capture(id)?;
                match self.evaluate(t, id, reference, &shot, true, &mut None)? {
                    Ok(e) => {
                        if let Some(loaded) = &e.reference {
                            let path = self.resolve(t, id, &loaded.name)?;
                            step.reference = Some(relative(self.project, &path));
                        }
                        self.record_comparison(step, reference, &e)?;
                        if !e.passed {
                            return Err(fail(mismatch(id, reference, &e)));
                        }
                    }
                    Err(name) => self.create_baseline(t, id, reference, &name, &shot, step)?,
                }
                Ok(next.clone())
            }
            BlockKind::If {
                reference,
                then,
                otherwise,
            } => {
                let shot = self.capture(id)?;
                let e = self
                    .evaluate(t, id, reference, &shot, false, &mut None)?
                    .map_err(|name| missing_reference(id, &name))?;
                self.record_comparison(step, reference, &e)?;
                let (branch, target) = if e.passed {
                    ("then", then)
                } else {
                    ("else", otherwise)
                };
                step.branch = Some(branch.to_owned());
                Ok(target.clone())
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
                let path = self.resolve(t, id, file)?;
                let table = DataTable::load(&path).map_err(|e| {
                    fail(
                        Failure::new("run.data.unreadable")
                            .with("block", id)
                            .with("file", file)
                            .with("reason", e),
                    )
                })?;
                let table = Rc::new(table);
                for row in 0..table.rows.len() {
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
            BlockKind::RunTest { test, next } => {
                let path = self.resolve(t, id, test)?;
                if self.tests.contains(&path) {
                    return Err(fail(
                        Failure::new("run.runTest.recursion")
                            .with("block", id)
                            .with("test", test),
                    ));
                }
                let sub = load_test(self.project, &path)
                    .and_then(|sub| {
                        fetch_secrets(&sub.flow, self.runner.secrets, self.secrets)?;
                        Ok(sub)
                    })
                    .map_err(|f| fail(f.with("block", id)))?;
                self.tests.push(path);
                self.apps.push(sub.flow.app.clone());
                let result = self.run_chain(&sub, &sub.flow.start);
                self.tests.pop();
                self.apps.pop();
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
        let (reached, target, reference) = match until {
            WaitUntil::Appears(r) | WaitUntil::Disappears(r) => {
                let want = matches!(until, WaitUntil::Appears(_));
                let mut cache = None;
                let reached = self.poll(id, timeout, || {
                    let shot = self.capture(id)?;
                    let passed = match self.evaluate(t, id, r, &shot, false, &mut cache) {
                        Ok(e) => {
                            let e = e.map_err(|name| missing_reference(id, &name))?;
                            cache = e.reference;
                            if let Judged::Model(answer) = e.judged {
                                step.vision = Some(answer);
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
                    Compare::Meaning { expect } if r.image.is_empty() => expect.as_str(),
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
        if let Some(shot) = last {
            step.after = Some(match reference.filter(|r| !r.hide.is_empty()) {
                Some(r) => {
                    let mut image = decode_screenshot(id, &shot)?;
                    image.cover(&r.hide);
                    self.record_image(step, "after", &image)?
                }
                None => self.record(step, "after", &shot.png)?,
            });
        }
        if reached.is_none() {
            return Err(fail(
                Failure::new("run.waitFor.timeout")
                    .with("block", id)
                    .with("target", target)
                    .with("seconds", seconds(timeout_ms)),
            ));
        }
        Ok(())
    }

    fn wait_for_change(&self, id: &str, before: &Screenshot) -> Result<(), Stop> {
        let before = decode_screenshot(id, before)?;
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
                Failure::new("run.click.noChange")
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
        let skip: Vec<_> = r.ignore.iter().chain(&r.hide).copied().collect();
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
        let judge = self.runner.vision.ok_or_else(|| {
            fail(Failure::new("run.compare.meaningUnavailable").with("block", id))
        })?;
        let expect = match &r.compare {
            Compare::Meaning { expect } => expect.trim(),
            _ => "",
        };
        if self.options.local_only && !judge.is_local() {
            return Err(fail(
                Failure::new("run.vision.cloudBlocked")
                    .with("block", id)
                    .with("provider", judge.provider()),
            ));
        }
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
        let screenshot = for_model(id, &actual, r)?;
        let reference_png = match &reference {
            Some(loaded) => Some(for_model(id, &loaded.image, r)?),
            None => None,
        };
        let question = Question {
            expect,
            screenshot: &screenshot,
            reference: reference_png.as_deref(),
        };
        let judgement = judge
            .judge(&question)
            .map_err(|e| fail(vision_failure(judge, &e).with("block", id)))?;
        Ok(Ok(Evaluation {
            reference,
            actual,
            passed: judgement.passed,
            judged: Judged::Model(VisionAnswer {
                provider: judge.provider().to_owned(),
                model: judge.model().to_owned(),
                passed: judgement.passed,
                // The model may read a secret off the screen.
                reason: self.mask(&judgement.reason),
            }),
        }))
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
    fn record_comparison(
        &self,
        step: &mut Step,
        r: &Reference,
        e: &Evaluation,
    ) -> Result<(), Stop> {
        let mut images = vec![("after", e.actual.clone())];
        images.extend(e.reference.as_ref().map(|l| ("expected", l.image.clone())));
        match &e.judged {
            Judged::Pixels(c) => {
                images.push(("diff", c.diff.clone()));
                step.similarity = Some(c.similarity);
            }
            Judged::Model(answer) => step.vision = Some(answer.clone()),
        }
        for (kind, mut image) in images {
            image.cover(&r.hide);
            let path = Some(self.record_image(step, kind, &image)?);
            match kind {
                "after" => step.after = path,
                "expected" => step.expected = path,
                _ => step.diff = path,
            }
        }
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
        let mut image = decode_screenshot(id, shot)?;
        image.cover(&r.hide);
        let png = image.encode().map_err(|e| write_failed(&path, e))?;
        write(&path, &png)?;
        step.after = Some(self.record(step, "after", &png)?);
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
    ) -> Result<Point, Stop> {
        let image = match target {
            Target::Point(p) => {
                // The test keeps the point as fractions of the window; hands click in points.
                let captured;
                let shot = match shot {
                    Some(shot) => shot,
                    None => {
                        captured = self.capture(id)?;
                        &captured
                    }
                };
                let window = decode_screenshot(id, shot)?;
                return Ok(Point {
                    x: p.x * f64::from(window.width) / shot.scale,
                    y: p.y * f64::from(window.height) / shot.scale,
                });
            }
            Target::Image(image) => image,
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

    /// Copies a file the test opens into the run folder, so the original stays intact.
    fn fresh_copy(
        &self,
        t: &TestFile,
        id: &str,
        file: &str,
        seq: u32,
        index: usize,
    ) -> Result<PathBuf, Stop> {
        let source = self.resolve(t, id, file)?;
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

    /// Replaces `${secret.NAME}` and `${data.COLUMN}` in text to be typed.
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
                Some(("data", column)) => {
                    let value = self.frames.iter().rev().find_map(|f| match f {
                        Frame::Data { table, row, .. } => table.value(*row, column),
                        Frame::Repeat { .. } => None,
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
        self.hands(id, |h| h.capture(&app))
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
        let path = Some(self.record(step, kind, &shot.png)?);
        if kind == "before" {
            step.before = path;
        } else {
            step.after = path;
        }
        Ok(Some(shot))
    }

    /// Writes a PNG into the run folder; returns its path relative to the folder.
    fn record(&self, step: &Step, kind: &str, png: &[u8]) -> Result<String, Stop> {
        let name = format!(
            "shots/{}-{:03}-{}-{kind}.png",
            self.attempt, step.seq, step.block
        );
        write(&self.dir.join(&name), png)?;
        Ok(name)
    }

    fn record_image(&self, step: &Step, kind: &str, image: &Image) -> Result<String, Stop> {
        let png = image
            .encode()
            .map_err(|e| write_failed(&self.dir.join("shots"), e))?;
        self.record(step, kind, &png)
    }

    /// A path from the test, relative to its file, that must stay inside the project.
    fn resolve(&self, t: &TestFile, id: &str, path: &str) -> Result<PathBuf, Stop> {
        let full = normalize(&t.dir.join(path));
        if full.starts_with(self.project) {
            Ok(full)
        } else {
            Err(fail(
                Failure::new("run.path.outsideProject")
                    .with("block", id)
                    .with("path", path),
            ))
        }
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

fn hands_failure(id: &str, e: HandsError) -> Failure {
    let f = match e {
        HandsError::PermissionDenied(p) => {
            Failure::new("run.hands.permissionDenied").with("permission", p)
        }
        HandsError::AppNotFound(app) => Failure::new("run.hands.appNotFound").with("app", app),
        HandsError::NoWindow(app) => Failure::new("run.hands.noWindow").with("app", app),
        HandsError::CannotOpen(file) => Failure::new("run.hands.cannotOpen").with("file", file),
        HandsError::ScreenLocked => Failure::new("run.hands.screenLocked"),
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
fn mismatch(id: &str, r: &Reference, e: &Evaluation) -> Failure {
    match &e.judged {
        Judged::Model(answer) => Failure::new("run.check.meaningMismatch")
            .with("block", id)
            .with("provider", &answer.provider)
            .with("reason", &answer.reason),
        Judged::Pixels(comparison) => {
            let threshold = match r.compare {
                Compare::Similar { threshold } => threshold,
                _ => 1.0,
            };
            Failure::new("run.check.mismatch")
                .with("block", id)
                .with(
                    "image",
                    e.reference.as_ref().map_or("", |l| l.name.as_str()),
                )
                .with("similarity", percent(comparison.similarity))
                .with("threshold", percent(threshold))
        }
    }
}

/// What a vision model gets to see: the compared area only, with the parts left out of the
/// comparison and the hidden ones covered, scaled down to a sensible size.
fn for_model(id: &str, image: &Image, r: &Reference) -> Result<Vec<u8>, Stop> {
    let mut image = image.clone();
    image.cover(&r.ignore);
    image.cover(&r.hide);
    if let Some(area) = &r.area {
        image = image.crop(area);
    }
    image.fit(VISION_MAX_SIDE).encode().map_err(|reason| {
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

/// `refs/inbox.png` → `refs/inbox.dark.png`: the reference for dark appearance.
fn dark_variant(image: &str) -> String {
    let name_start = image.rfind('/').map_or(0, |i| i + 1);
    match image[name_start..].rfind('.') {
        Some(dot) if dot > 0 => {
            let at = name_start + dot;
            format!("{}.dark{}", &image[..at], &image[at..])
        }
        _ => format!("{image}.dark"),
    }
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
fn rfc3339(time: SystemTime) -> String {
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
    fn dark_variants_and_paths() {
        assert_eq!(dark_variant("refs/inbox.png"), "refs/inbox.dark.png");
        assert_eq!(dark_variant("a.b/inbox"), "a.b/inbox.dark");
        assert_eq!(dark_variant(".hidden"), ".hidden.dark");
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
}
