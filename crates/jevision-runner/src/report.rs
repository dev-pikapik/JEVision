//! What a run leaves behind: the report model and its HTML and JUnit XML exports.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Version of the `report.json` layout.
pub const REPORT_FORMAT: u32 = 1;

/// Outcome of a whole run, saved as `report.json` in the run folder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunReport {
    pub format: u32,
    /// Name of the test.
    pub test: String,
    /// Test file, relative to the project folder.
    pub file: String,
    /// Bundle id of the application under test.
    pub app: String,
    /// UTC time, RFC 3339.
    pub started_at: String,
    pub duration_ms: u64,
    /// Result of the last attempt.
    pub verdict: Verdict,
    /// Attempts disagreed: a failure was followed by a pass, or attempts failed differently.
    /// A failed run that is not flaky failed the same way every time.
    pub flaky: bool,
    /// References saved by this run because they did not exist, relative to the project folder.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub baselines_created: Vec<String>,
    /// The run passed, but no block looked at the screen — no check, no wait for something to
    /// appear or disappear — so it couldn't have caught a wrong result.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub nothing_checked: bool,
    /// Name of the paired device that started the run from afar, such as `Anna's iPhone`;
    /// absent for a run started on this computer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_by: Option<String>,
    /// Name of the schedule of the project that started the run; absent for a run someone
    /// started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<String>,
    /// Environment of the project the run used; absent when it used none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    /// Video of the run, relative to the run folder; absent when none was recorded or kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video: Option<String>,
    /// SHA-256 of the test file's bytes in hex, so runs of the same test content can be told
    /// apart from runs after a change; absent when the file couldn't be read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_hash: Option<String>,
    /// The test was in the project's quarantine: its failure doesn't fail a batch of runs.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub quarantined: bool,
    /// Why the test was in quarantine, as the project's settings say.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quarantine_reason: Option<String>,
    /// The run ran in a virtual machine; absent where that couldn't be told.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub virtual_machine: Option<bool>,
    /// Device pixels per point of the display the run looked at: 2 on Retina, 1 on most
    /// virtual machines and external displays; absent when the run took no screenshot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_scale: Option<f64>,
    pub attempts: Vec<Attempt>,
}

impl RunReport {
    /// The run failed while the test was in quarantine, so it doesn't fail a batch of runs.
    pub fn is_quarantined_failure(&self) -> bool {
        self.quarantined && self.verdict == Verdict::Failed
    }

    /// Reported as skipped rather than passed: the run was cancelled, failed in quarantine or
    /// passed without checking anything.
    fn is_skipped(&self) -> bool {
        self.verdict == Verdict::Cancelled || self.is_quarantined_failure() || self.nothing_checked
    }

    /// Where the run ran, for the report's header: `Virtual machine` or `Physical Mac`, then the
    /// display, such as `Retina display (2×)`; `None` when the report doesn't say.
    pub fn machine(&self) -> Option<String> {
        let computer = self
            .virtual_machine
            .map(|v| if v { "Virtual machine" } else { "Physical Mac" });
        let display = self.display_scale.map(|scale| {
            let density = if scale >= 2.0 { "Retina" } else { "Standard" };
            format!("{density} display ({scale}×)")
        });
        match (computer, display) {
            (None, None) => None,
            (computer, display) => Some(
                [computer.map(str::to_owned), display]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
        }
    }
}

/// What the retries of a run say about its result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryVerdict {
    /// Failed, then passed on retry `retry`, 1 being the first retry: the test is flaky.
    PassedOnRetry { retry: u32 },
    /// Failed every attempt, not always the same way: the test may be flaky.
    FailedDifferently { attempts: u32 },
    /// Failed every attempt the same way: a real failure, not flakiness.
    FailedEveryTime { attempts: u32 },
}

impl RetryVerdict {
    /// The verdict of a run that ended with `verdict` after `attempts` attempts; `None` for a
    /// run tried once or stopped.
    pub fn of(verdict: Verdict, flaky: bool, attempts: usize) -> Option<Self> {
        let attempts = u32::try_from(attempts).unwrap_or(u32::MAX);
        if attempts < 2 {
            return None;
        }
        match verdict {
            Verdict::Passed => Some(Self::PassedOnRetry {
                retry: attempts - 1,
            }),
            Verdict::Failed if flaky => Some(Self::FailedDifferently { attempts }),
            Verdict::Failed => Some(Self::FailedEveryTime { attempts }),
            Verdict::Cancelled => None,
        }
    }

    /// The verdict in one English sentence, for the HTML report and the command line.
    pub fn describe(self) -> String {
        match self {
            Self::PassedOnRetry { retry } => {
                format!("Passed on retry {retry} after failing — the test is flaky.")
            }
            Self::FailedDifferently { attempts } => {
                format!("Failed {attempts} times, each time differently — the test may be flaky.")
            }
            Self::FailedEveryTime { attempts } => {
                format!("Failed {attempts} times in a row — a real failure, not flakiness.")
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Verdict {
    Passed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attempt {
    /// 1 for the first run, 2 for the first retry and so on; counted anew for each data row.
    pub number: u32,
    pub verdict: Verdict,
    pub duration_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<Failure>,
    /// Row of the test's data table the attempt ran with; absent for a test without one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row: Option<DataRow>,
    /// In the order the blocks started.
    pub steps: Vec<Step>,
}

/// A row of the test's data table, as a run names it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataRow {
    /// Counted from 1.
    pub number: u32,
    /// The row's value in its `name` column, or else in its first; may be empty.
    pub label: String,
}

impl DataRow {
    /// `Row 2: anna@example.com`, or `Row 2` when the label is empty.
    pub fn title(&self) -> String {
        if self.label.is_empty() {
            format!("Row {}", self.number)
        } else {
            format!("Row {}: {}", self.number, self.label)
        }
    }
}

/// How one data row of a run went: the verdict of its last attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowResult {
    pub row: DataRow,
    pub verdict: Verdict,
    pub flaky: bool,
    pub attempts: usize,
    /// Why the row's last attempt failed.
    pub failure: Option<Failure>,
}

impl RunReport {
    /// One result per data row, in the order the rows ran; empty for a run without a data
    /// table.
    pub fn rows(&self) -> Vec<RowResult> {
        self.attempts
            .chunk_by(|a, b| a.row == b.row)
            .filter_map(|group| {
                let last = group.last()?;
                Some(RowResult {
                    row: last.row.clone()?,
                    verdict: last.verdict,
                    flaky: crate::run::is_flaky(group),
                    attempts: group.len(),
                    failure: last.failure.clone(),
                })
            })
            .collect()
    }

    /// `4 of 5 rows passed` for a run with a data table.
    pub fn rows_summary(&self) -> Option<String> {
        let rows = self.rows();
        if rows.is_empty() {
            return None;
        }
        let passed = rows.iter().filter(|r| r.verdict == Verdict::Passed).count();
        Some(match rows.len() {
            1 => format!("{passed} of 1 row passed"),
            n => format!("{passed} of {n} rows passed"),
        })
    }

    /// The report of each data row on its own, named like `Login [Row 2: anna]`; just the
    /// report for a run without a data table.
    pub fn split_rows(&self) -> Vec<RunReport> {
        let rows = self.rows();
        if rows.is_empty() {
            return vec![self.clone()];
        }
        rows.into_iter()
            .map(|r| {
                let attempts: Vec<Attempt> = self
                    .attempts
                    .iter()
                    .filter(|a| a.row.as_ref() == Some(&r.row))
                    .cloned()
                    .collect();
                RunReport {
                    test: format!("{} [{}]", self.test, r.row.title()),
                    duration_ms: attempts.iter().map(|a| a.duration_ms).sum(),
                    verdict: r.verdict,
                    flaky: r.flaky,
                    attempts,
                    ..self.clone()
                }
            })
            .collect()
    }
}

/// The verdict of a run and whether it was flaky, from its attempts. A run with a data table
/// passes when every row's last attempt passed and is flaky when any row was.
pub(crate) fn summarize(attempts: &[Attempt]) -> (Verdict, bool) {
    let mut verdict = None;
    let mut flaky = false;
    for group in attempts.chunk_by(|a, b| a.row == b.row) {
        flaky |= crate::run::is_flaky(group);
        let last = group.last().map(|a| a.verdict);
        verdict = match (verdict, last) {
            (Some(Verdict::Cancelled), _) | (_, Some(Verdict::Cancelled)) => {
                Some(Verdict::Cancelled)
            }
            (Some(Verdict::Failed), _) | (_, Some(Verdict::Failed)) => Some(Verdict::Failed),
            (_, last) => last,
        };
    }
    (verdict.unwrap_or(Verdict::Failed), flaky)
}

/// One block as it ran. Image paths are relative to the run folder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    /// Order of the step within its attempt, from 1.
    pub seq: u32,
    /// Test file the block belongs to, relative to the project folder.
    pub test: String,
    pub block: String,
    #[serde(rename = "type")]
    pub block_type: String,
    pub status: StepStatus,
    pub duration_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
    /// Copy of the reference the screen was compared with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff: Option<String>,
    /// Reference file of a `check`, relative to the project folder: the one compared with, or
    /// the one this step saved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    /// Share of compared pixels that matched the reference, 0–1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub similarity: Option<f64>,
    /// What the vision model said, for a comparison by meaning.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vision: Option<VisionAnswer>,
    /// The screenshot exactly as a vision model got it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sent: Option<String>,
    /// Private areas of [`sent`](Self::sent) were covered before sending.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub sent_masked: bool,
    /// Edge an `if` block followed: `then`, `elseIf1`, `elseIf2`… or `else`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// How many ways out the `if` had: two are Yes and No, more are Condition 1, 2… and
    /// Otherwise. Absent in reports written before an `if` could have more conditions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branches: Option<u32>,
    /// What the condition of the way taken looks for, in the test's own words: what the screen
    /// should show, or the name of the reference image. Absent for `else`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch_condition: Option<String>,
    /// Why the step failed. Absent on a loop or `runTest` step whose inner step failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<Failure>,
    /// When the block started, in milliseconds from the start of the run's video.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video_at_ms: Option<u64>,
}

/// A vision model's verdict on a screen, as kept in the report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VisionAnswer {
    /// Provider name, such as `Anthropic`.
    pub provider: String,
    pub model: String,
    pub passed: bool,
    /// The model's explanation, in a sentence.
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StepStatus {
    Passed,
    Failed,
    /// There was no reference yet; the screenshot became the reference.
    BaselineCreated,
    Cancelled,
}

/// Why a run failed: a localization key and the values its text refers to, e.g.
/// `run.waitFor.timeout` with `block`, `target` and `seconds`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Failure {
    pub code: String,
    pub params: BTreeMap<String, String>,
}

impl Step {
    /// Which way an `if` went, in English: `Took “Condition 2: Inbox is empty”`,
    /// `Took “Otherwise”`. `None` for other blocks.
    pub fn branch_text(&self) -> Option<String> {
        branch_text(
            self.branch.as_deref()?,
            self.branches,
            self.branch_condition.as_deref(),
        )
    }
}

/// [`Step::branch_text`] from its parts, named as the editor names the ways out of an `if`.
pub fn branch_text(branch: &str, branches: Option<u32>, condition: Option<&str>) -> Option<String> {
    let index = jevision_core::else_if_index(branch);
    let plain = branches.map_or(index.is_none(), |n| n <= 2);
    let name = match (branch, index) {
        (jevision_core::THEN, _) if plain => "Yes".to_owned(),
        (jevision_core::THEN, _) => "Condition 1".to_owned(),
        (jevision_core::ELSE, _) if plain => "No".to_owned(),
        (jevision_core::ELSE, _) => "Otherwise".to_owned(),
        (_, Some(n)) => format!("Condition {}", n + 2),
        _ => return None,
    };
    Some(match condition {
        Some(c) => format!("Took “{name}: {c}”"),
        None => format!("Took “{name}”"),
    })
}

impl Failure {
    pub fn new(code: &str) -> Self {
        Self {
            code: code.to_owned(),
            params: BTreeMap::new(),
        }
    }

    pub fn with(mut self, name: &str, value: impl ToString) -> Self {
        self.params.insert(name.to_owned(), value.to_string());
        self
    }

    pub fn param(&self, name: &str) -> &str {
        self.params.get(name).map_or("", String::as_str)
    }
}

/// English sentence for a failure, for exports that have no localization at hand.
pub fn describe(f: &Failure) -> String {
    let p = |name| f.param(name);
    match f.code.as_str() {
        "run.test.unreadable" => {
            let reason = p("reason").trim_end_matches('.');
            format!("Test file {} cannot be read: {reason}.", p("test"))
        }
        "run.test.invalid" => format!(
            "Test file {} has {} errors; fix them in the editor first.",
            p("test"),
            p("errors")
        ),
        "run.start.unknownBlock" => format!("There is no block {} to start from.", p("block")),
        "run.secret.missing" => format!("Secret {} is not set.", p("name")),
        "run.env.notChosen" => format!(
            "The test reads the environment value {}, but no environment was chosen.",
            p("name")
        ),
        "run.env.missing" => format!(
            "Environment {} has no value {}.",
            p("environment"),
            p("name")
        ),
        "run.env.unknown" => format!(
            "There is no environment {} in {}.",
            p("environment"),
            p("file")
        ),
        "run.environments.unreadable" | "run.project.unreadable" => {
            format!("{} can't be read: {}.", p("file"), p("reason"))
        }
        "run.data.noRow" => format!(
            "Block {}: {} has {} rows, so there is no row {}.",
            p("block"),
            p("file"),
            p("rows"),
            p("row")
        ),
        "run.path.outsideProject" => format!(
            "Block {}: {} is outside the project folder.",
            p("block"),
            p("path")
        ),
        "run.file.missing" => format!("Block {}: file {} does not exist.", p("block"), p("file")),
        "run.file.copyFailed" => format!(
            "Block {}: file {} cannot be copied: {}.",
            p("block"),
            p("file"),
            p("reason")
        ),
        "run.target.notFound" => format!(
            "Block {}: {} was not found on screen.",
            p("block"),
            p("image")
        ),
        "run.reference.missing" => format!(
            "Block {}: reference image {} does not exist.",
            p("block"),
            p("image")
        ),
        "run.reference.unreadable" => format!(
            "Block {}: image {} cannot be read: {}.",
            p("block"),
            p("image"),
            p("reason")
        ),
        "run.screenshot.unreadable" => format!(
            "Block {}: the screenshot cannot be read: {}.",
            p("block"),
            p("reason")
        ),
        "run.compare.shapeMismatch" => format!(
            "Block {}: the window is {} but reference {} is {}.",
            p("block"),
            p("actual"),
            p("image"),
            p("expected")
        ),
        "run.compare.meaningUnavailable" => format!(
            "Block {}: comparing by meaning needs an AI model. Choose one in JEVision Settings → AI.",
            p("block")
        ),
        "run.click.noChange" => format!(
            "Block {}: the screen did not change within {} s after the click.",
            p("block"),
            p("seconds")
        ),
        "run.waitFor.timeout" => format!(
            "Block {}: {} did not happen within {} s.",
            p("block"),
            wait_target(p("target")),
            p("seconds")
        ),
        "run.check.mismatch" => format!(
            "Block {}: the screen matches {} by {}, at least {} is required.",
            p("block"),
            p("image"),
            p("similarity"),
            p("threshold")
        ),
        "run.check.mismatchDensity" => format!(
            "Block {}: the screen matches {} by {}, at least {} is required. The reference is {} \
             pixels and the screen {}: the reference was saved on a display with a different \
             pixel density (Retina or not), where text and edges are drawn differently. Run on a \
             display like the one the reference was saved on, or save the reference again.",
            p("block"),
            p("image"),
            p("similarity"),
            p("threshold"),
            p("expected"),
            p("actual")
        ),
        "run.data.unreadable" => format!(
            "Block {}: table {} cannot be read: {}.",
            p("block"),
            p("file"),
            p("reason")
        ),
        "run.data.unknownColumn" => format!(
            "Block {}: no table column {} is available here.",
            p("block"),
            p("column")
        ),
        "run.runTest.recursion" => format!(
            "Block {}: test {} is already running; a test cannot run itself.",
            p("block"),
            p("test")
        ),
        "run.io.writeFailed" => format!("{} cannot be saved: {}.", p("path"), p("reason")),
        "run.hands.permissionDenied" => format!(
            "Block {}: permission {} is not granted.",
            p("block"),
            p("permission")
        ),
        "run.hands.appNotFound" => format!(
            "Block {}: application {} is not installed.",
            p("block"),
            p("app")
        ),
        "run.hands.noWindow" => format!(
            "Block {}: application {} has no open window.",
            p("block"),
            p("app")
        ),
        "run.hands.cannotOpen" => format!("Block {}: {} cannot be opened.", p("block"), p("file")),
        "run.hands.screenLocked" => format!(
            "Block {}: the screen is locked. Unlock it and run the test again.",
            p("block")
        ),
        "run.hands.failed" => format!("Block {}: {}.", p("block"), p("reason")),
        "run.check.meaningMismatch" => format!(
            "Block {}: the screen does not show what is expected. {}: {}",
            p("block"),
            p("provider"),
            p("reason")
        ),
        "run.vision.cloudBlocked" => format!(
            "Block {}: only local models are allowed, and {} is a cloud service.",
            p("block"),
            p("provider")
        ),
        code if code.starts_with("run.vision.") => {
            format!("Block {}: {}", p("block"), describe_vision(f))
        }
        _ => {
            let params: Vec<String> = f.params.iter().map(|(k, v)| format!("{k}={v}")).collect();
            format!("{} ({})", f.code, params.join(", "))
        }
    }
}

/// English sentence for a problem with a vision model, without the block.
pub fn describe_vision(f: &Failure) -> String {
    let p = |name| f.param(name);
    let provider = p("provider");
    match f.code.as_str() {
        "run.vision.noModel" => format!("No model is chosen for {provider}."),
        "run.vision.noKey" => format!("The API key for {provider} is not set."),
        "run.vision.badAddress" => format!(
            "{} is not a web address; it should start with http:// or https://.",
            p("address")
        ),
        "run.vision.unreachable" => format!("{provider} cannot be reached at {}.", p("address")),
        "run.vision.timeout" => {
            format!("{provider} did not answer within {} s.", p("seconds"))
        }
        "run.vision.badKey" => format!("{provider} did not accept the API key."),
        "run.vision.unknownModel" => format!("{provider} has no model {}.", p("model")),
        "run.vision.rateLimited" => format!(
            "{provider} has had too many requests or the quota ran out: {}",
            p("message")
        ),
        "run.vision.serviceError" => format!(
            "{provider} answered with error {}: {}",
            p("status"),
            p("message")
        ),
        "run.vision.refused" => {
            format!("{provider} declined to judge the screen: {}", p("message"))
        }
        "run.vision.badAnswer" => {
            format!(
                "{provider} gave an answer that cannot be read: {}",
                p("answer")
            )
        }
        code => code.to_owned(),
    }
}

pub(crate) fn wait_target(target: &str) -> String {
    match target {
        "screenStable" => "the screen settling".to_owned(),
        image => format!("the wait for {image}"),
    }
}

/// Why a run that passed without looking at the screen isn't a pass, in English.
pub const NOTHING_CHECKED: &str = "Finished, but nothing was checked: the test has no check or wait block that looks at the screen, so it can't catch a wrong result.";

/// JUnit XML for one or more runs, one test case each. Retries use the Surefire elements
/// `flakyFailure` (failed, then passed) and `rerunFailure` (failed again). A failure of a test
/// in quarantine is reported as skipped, with the reason and the failure in its message, and so
/// is a run that passed without checking anything, so CI doesn't count it as a pass.
pub fn junit_xml(reports: &[RunReport]) -> String {
    let reports: Vec<RunReport> = reports.iter().flat_map(RunReport::split_rows).collect();
    let failed = reports
        .iter()
        .filter(|r| r.verdict == Verdict::Failed && !r.is_quarantined_failure())
        .count();
    let skipped = reports.iter().filter(|r| r.is_skipped()).count();
    let seconds = |ms: u64| format!("{:.3}", ms as f64 / 1000.0);
    let total: u64 = reports.iter().map(|r| r.duration_ms).sum();
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let head = format!(
        "name=\"JEVision\" tests=\"{}\" failures=\"{}\" errors=\"0\" skipped=\"{}\" time=\"{}\"",
        reports.len(),
        failed,
        skipped,
        seconds(total)
    );
    let _ = writeln!(out, "<testsuites {head}>");
    let timestamp = reports
        .first()
        .map_or("", |r| r.started_at.trim_end_matches('Z'));
    let _ = writeln!(out, "  <testsuite {head} timestamp=\"{}\">", xml(timestamp));
    for r in reports {
        let _ = write!(
            out,
            "    <testcase name=\"{}\" classname=\"{}\" time=\"{}\"",
            xml(&r.test),
            xml(&r.file),
            seconds(r.duration_ms)
        );
        let failures: Vec<&Failure> = r
            .attempts
            .iter()
            .filter_map(|a| a.failure.as_ref())
            .collect();
        // Which way each `if` of the last attempt went, so a CI log shows the path the test took.
        let paths: Vec<String> = r
            .attempts
            .last()
            .into_iter()
            .flat_map(|a| &a.steps)
            .filter_map(|s| Some(format!("{}: {}", s.block, s.branch_text()?)))
            .collect();
        if r.verdict == Verdict::Passed
            && !r.nothing_checked
            && failures.is_empty()
            && paths.is_empty()
        {
            out.push_str("/>\n");
            continue;
        }
        out.push_str(">\n");
        let element = |out: &mut String, tag: &str, f: &Failure| {
            let _ = writeln!(
                out,
                "      <{tag} message=\"{}\" type=\"{}\"/>",
                xml(&describe(f)),
                xml(&f.code)
            );
        };
        match r.verdict {
            Verdict::Failed if r.quarantined => {
                let failed = failures
                    .last()
                    .map_or_else(|| "The test failed.".to_owned(), |f| describe(f));
                let reason = r
                    .quarantine_reason
                    .as_deref()
                    .map_or_else(String::new, |reason| format!(" ({reason})"));
                let _ = writeln!(
                    out,
                    "      <skipped message=\"{}\"/>",
                    xml(&format!("Quarantined{reason}. {failed}"))
                );
            }
            Verdict::Passed => {
                if r.nothing_checked {
                    let _ = writeln!(out, "      <skipped message=\"{NOTHING_CHECKED}\"/>");
                }
                failures
                    .iter()
                    .for_each(|f| element(&mut out, "flakyFailure", f));
            }
            Verdict::Failed => match failures.split_last() {
                Some((last, earlier)) => {
                    element(&mut out, "failure", last);
                    earlier
                        .iter()
                        .for_each(|f| element(&mut out, "rerunFailure", f));
                }
                None => out.push_str("      <failure message=\"The test failed.\"/>\n"),
            },
            Verdict::Cancelled => {
                out.push_str("      <skipped message=\"The run was cancelled.\"/>\n")
            }
        }
        if !paths.is_empty() {
            let _ = writeln!(
                out,
                "      <system-out>{}</system-out>",
                xml(&paths.join("\n"))
            );
        }
        out.push_str("    </testcase>\n");
    }
    out.push_str("  </testsuite>\n</testsuites>\n");
    out
}

/// A self-contained HTML page for the report, with every image of `run_dir` it refers to
/// embedded. Images that are gone are left out. `video` is the address of the run's video as
/// seen from where the page is saved; videos are too large to embed, so the page links it and
/// every step with a time seeks it.
pub fn html(report: &RunReport, run_dir: &Path, video: Option<&str>) -> String {
    let mut o = String::new();
    let (badge, label) = match (report.verdict, report.flaky) {
        (Verdict::Passed, _) if report.nothing_checked => ("skip", "Nothing checked"),
        (Verdict::Passed, false) => ("pass", "Passed"),
        (Verdict::Passed, true) => ("flaky", "Passed, flaky"),
        (Verdict::Failed, _) if report.quarantined => ("skip", "Failed, quarantined"),
        (Verdict::Failed, false) => ("fail", "Failed"),
        (Verdict::Failed, true) => ("fail", "Failed, flaky"),
        (Verdict::Cancelled, _) => ("skip", "Cancelled"),
    };
    let _ = write!(
        o,
        "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>{title} — {label}</title>\n<style>{CSS}</style>\n</head>\n<body>\n<main>\n\
         <header><h1>{title}</h1><span class=\"badge {badge}\">{label}</span></header>\n\
         <dl class=\"facts\"><dt>Test file</dt><dd>{file}</dd><dt>Application</dt><dd>{app}</dd>{environment}{machine}\
         <dt>Started</dt><dd>{started}</dd><dt>Duration</dt><dd>{duration}</dd>\
         <dt>Attempts</dt><dd>{attempts}</dd></dl>\n",
        title = xml(&report.test),
        file = xml(&report.file),
        app = xml(&report.app),
        environment = report
            .environment
            .as_deref()
            .map_or_else(String::new, |e| format!(
                "<dt>Environment</dt><dd>{}</dd>",
                xml(e)
            )),
        machine = report.machine().map_or_else(String::new, |m| format!(
            "<dt>Computer</dt><dd>{}</dd>",
            xml(&m)
        )),
        started = xml(&report.started_at.replace('T', " ")),
        duration = duration(report.duration_ms),
        attempts = report.attempts.len(),
    );
    if report.nothing_checked {
        let _ = writeln!(o, "<p class=\"note\">{NOTHING_CHECKED}</p>");
    }
    let rows = report.rows();
    if rows.is_empty() {
        if let Some(verdict) = RetryVerdict::of(report.verdict, report.flaky, report.attempts.len())
        {
            let _ = writeln!(o, "<p class=\"note\">{}</p>", xml(&verdict.describe()));
        }
    } else {
        let _ = writeln!(
            o,
            "<p class=\"note\">{}.</p>\n<ol class=\"steps\">",
            xml(&report.rows_summary().unwrap_or_default())
        );
        for r in &rows {
            let (class, status) = match r.verdict {
                Verdict::Passed if r.flaky => ("pass", "Passed, flaky"),
                Verdict::Passed => ("pass", "Passed"),
                Verdict::Failed => ("fail", "Failed"),
                Verdict::Cancelled => ("skip", "Cancelled"),
            };
            let tries = match r.attempts {
                1 => String::new(),
                n => format!("<span class=\"type\">{n} attempts</span>"),
            };
            let _ = writeln!(
                o,
                "<li class=\"{class}\"><div class=\"row\"><span class=\"seq\">{}</span>\
                 <code>{}</code>{tries}<span class=\"status\">{status}</span></div></li>",
                r.row.number,
                xml(&r.row.label)
            );
        }
        o.push_str("</ol>\n");
    }
    if report.quarantined {
        let reason = report
            .quarantine_reason
            .as_deref()
            .map_or_else(String::new, |reason| format!(": {}", xml(reason)));
        let _ = writeln!(
            o,
            "<p class=\"note\">The test is in quarantine{reason}. Its failures don’t fail Run All, the command line or the JUnit report.</p>"
        );
    }
    if !report.baselines_created.is_empty() {
        let _ = writeln!(
            o,
            "<p class=\"note\">New reference images were saved: {}. Review them before relying on this test.</p>",
            xml(&report.baselines_created.join(", "))
        );
    }
    if let Some(src) = video {
        let _ = writeln!(
            o,
            "<video id=\"video\" controls preload=\"metadata\" src=\"{}\"></video>",
            xml(src)
        );
    }
    for a in &report.attempts {
        let _ = writeln!(
            o,
            "<section>\n<h2>{}Attempt {} <small>{}</small></h2>",
            a.row
                .as_ref()
                .map_or_else(String::new, |r| format!("{} · ", xml(&r.title()))),
            a.number,
            duration(a.duration_ms)
        );
        if let Some(f) = &a.failure {
            let _ = writeln!(o, "<p class=\"reason\">{}</p>", xml(&describe(f)));
        }
        o.push_str("<ol class=\"steps\">\n");
        for s in &a.steps {
            let (class, status) = match s.status {
                StepStatus::Passed => ("pass", "Passed"),
                StepStatus::Failed => ("fail", "Failed"),
                StepStatus::BaselineCreated => ("new", "Reference saved"),
                StepStatus::Cancelled => ("skip", "Cancelled"),
            };
            let _ = write!(
                o,
                "<li class=\"{class}\"><div class=\"row\"><span class=\"seq\">{}</span>\
                 <code>{}</code><span class=\"type\">{}</span>",
                s.seq,
                xml(&s.block),
                xml(&s.block_type)
            );
            if s.test != report.file {
                let _ = write!(o, "<span class=\"type\">in {}</span>", xml(&s.test));
            }
            if let Some(b) = s.branch_text() {
                let _ = write!(o, "<span class=\"type\">{}</span>", xml(&b));
            }
            if let Some(sim) = s.similarity {
                let _ = write!(o, "<span class=\"type\">{} alike</span>", percent(sim));
            }
            let _ = write!(
                o,
                "<span class=\"status\">{status}</span><span class=\"time\">{}</span>",
                duration(s.duration_ms)
            );
            if let (Some(_), Some(ms)) = (video, s.video_at_ms) {
                let _ = write!(
                    o,
                    "<button class=\"seek\" data-t=\"{:.3}\" title=\"Show this step in the video\">▶ {}</button>",
                    ms as f64 / 1000.0,
                    clock(ms)
                );
            }
            o.push_str("</div>");
            match (&s.failure, &s.vision) {
                (Some(f), _) => {
                    let _ = write!(o, "<p class=\"reason\">{}</p>", xml(&describe(f)));
                }
                (None, Some(v)) => {
                    let _ = write!(
                        o,
                        "<p class=\"judge\">{} · {}: {}</p>",
                        xml(&v.provider),
                        xml(&v.model),
                        xml(&v.reason)
                    );
                }
                (None, None) => {}
            }
            let sent = if s.sent_masked {
                "Sent to the AI model, masked before sending"
            } else {
                "Sent to the AI model"
            };
            let images: Vec<(&str, &String)> = [
                ("Before", &s.before),
                ("After", &s.after),
                ("Expected", &s.expected),
                ("Differences", &s.diff),
                (sent, &s.sent),
            ]
            .into_iter()
            .filter_map(|(name, path)| Some((name, path.as_ref()?)))
            .collect();
            let figures: String = images
                .iter()
                .filter_map(|(name, path)| {
                    let bytes = std::fs::read(run_dir.join(path)).ok()?;
                    Some(format!(
                        "<figure><img tabindex=\"0\" alt=\"{name}\" \
                         src=\"data:image/png;base64,{}\"><figcaption>{name}</figcaption></figure>",
                        base64(&bytes)
                    ))
                })
                .collect();
            if !figures.is_empty() {
                let _ = write!(o, "<div class=\"shots\">{figures}</div>");
            }
            o.push_str("</li>\n");
        }
        o.push_str("</ol>\n</section>\n");
    }
    o.push_str("</main>\n");
    if video.is_some() {
        o.push_str(SEEK_SCRIPT);
    }
    o.push_str("</body>\n</html>\n");
    o
}

/// Makes the step buttons seek the video.
const SEEK_SCRIPT: &str = "<script>document.addEventListener('click',e=>{\
const b=e.target.closest('[data-t]');if(!b)return;const v=document.getElementById('video');\
v.currentTime=Number(b.dataset.t);v.play();v.scrollIntoView({behavior:'smooth',block:'nearest'})})</script>\n";

const CSS: &str = "\
:root{color-scheme:light dark;--bg:#f6f7f9;--card:#fff;--text:#1d1f23;--muted:#6b7079;--line:#e3e5e9;\
--pass:#1f8a4c;--fail:#c62f3d;--flaky:#b26a00;--new:#2563c9;--skip:#6b7079}\
@media(prefers-color-scheme:dark){:root{--bg:#141518;--card:#1e2024;--text:#eceef1;--muted:#9aa0a9;\
--line:#2d3036;--pass:#4cc27e;--fail:#ff6b78;--flaky:#f0a43a;--new:#6ea2ff;--skip:#9aa0a9}}\
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--text);\
font:15px/1.5 -apple-system,BlinkMacSystemFont,\"Segoe UI\",Roboto,sans-serif}\
main{max-width:1100px;margin:0 auto;padding:32px 24px 64px}\
header{display:flex;align-items:center;gap:14px;flex-wrap:wrap}h1{font-size:26px;margin:0}\
h2{font-size:18px;margin:32px 0 12px}h2 small{color:var(--muted);font-weight:400;margin-left:8px}\
.badge{padding:3px 12px;border-radius:999px;color:#fff;font-weight:600;font-size:13px}\
.badge.pass{background:var(--pass)}.badge.fail{background:var(--fail)}.badge.flaky{background:var(--flaky)}\
.badge.skip{background:var(--skip)}\
.facts{display:grid;grid-template-columns:max-content 1fr;gap:4px 18px;margin:18px 0 0}\
.facts dt{color:var(--muted)}.facts dd{margin:0;overflow-wrap:anywhere}\
.note{background:var(--card);border-left:4px solid var(--new);padding:10px 14px;border-radius:6px}\
.reason{color:var(--fail);font-weight:600;margin:8px 0 0}\
.judge{margin:8px 0 0}\
.steps{list-style:none;margin:0;padding:0;display:grid;gap:10px}\
.steps li{background:var(--card);border:1px solid var(--line);border-left:4px solid var(--skip);\
border-radius:8px;padding:10px 14px}\
.steps li.pass{border-left-color:var(--pass)}.steps li.fail{border-left-color:var(--fail)}\
.steps li.new{border-left-color:var(--new)}\
.row{display:flex;align-items:baseline;gap:10px;flex-wrap:wrap}.seq{color:var(--muted);min-width:2ch}\
.type,.time{color:var(--muted);font-size:13px}.status{margin-left:auto;font-weight:600}\
.shots{display:flex;gap:12px;flex-wrap:wrap;margin-top:10px}\
figure{margin:0;width:240px}figure img{width:100%;border:1px solid var(--line);border-radius:6px;\
display:block;background:repeating-conic-gradient(#ccc 0 25%,#fff 0 50%) 0 0/16px 16px}\
figure img:focus{position:fixed;inset:0;margin:auto;width:auto;max-width:96vw;max-height:96vh;\
z-index:1;box-shadow:0 0 0 100vmax rgba(0,0,0,.6);cursor:zoom-out;outline:none}figure img{cursor:zoom-in}\
figcaption{color:var(--muted);font-size:12px;margin-top:4px}\
video{display:block;width:100%;max-height:70vh;margin-top:20px;border-radius:8px;background:#000}\
.seek{font:inherit;font-size:13px;font-variant-numeric:tabular-nums;color:var(--new);background:none;\
border:1px solid var(--line);border-radius:999px;padding:0 10px;cursor:pointer}";

/// A time in the video, such as `1:05`.
fn clock(ms: u64) -> String {
    let seconds = ms / 1000;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

pub(crate) fn duration(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms} ms")
    } else {
        format!("{:.1} s", ms as f64 / 1000.0)
    }
}

pub(crate) fn percent(share: f64) -> String {
    format!("{:.1}%", share * 100.0)
}

/// Escapes text for XML and HTML content and attribute values.
fn xml(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // Characters XML 1.0 does not allow at all.
            '\u{0}'..='\u{8}' | '\u{b}' | '\u{c}' | '\u{e}'..='\u{1f}' => {}
            c => out.push(c),
        }
    }
    out
}

fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, &b)| n | u32::from(b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ABC[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_from_before_the_way_names_still_say_which_way_an_if_went() {
        let step: Step = serde_json::from_str(
            r#"{"seq":1,"test":"t.flow.json","block":"if","type":"if","status":"passed",
                "durationMs":5,"branch":"then"}"#,
        )
        .unwrap();
        assert_eq!(step.branches, None);
        assert_eq!(step.branch_condition, None);
        assert_eq!(step.branch_text().as_deref(), Some("Took “Yes”"));
        assert_eq!(
            branch_text("elseIf2", None, None).as_deref(),
            Some("Took “Condition 3”")
        );
        assert_eq!(branch_text("next", Some(2), None), None);
    }

    #[test]
    fn base64_matches_rfc_4648() {
        for (input, encoded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(input.as_bytes()), encoded);
        }
    }

    #[test]
    fn xml_escapes_markup_and_drops_control_characters() {
        assert_eq!(
            xml("<a href=\"x\">&'\u{1}"),
            "&lt;a href=&quot;x&quot;&gt;&amp;&apos;"
        );
    }
}
