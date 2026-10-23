//! What a run leaves behind: the report model and its HTML and JUnit XML exports.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use jevision_core::Blocker;
use jevision_l10n::{decimal, tr_in};
use serde::{Deserialize, Serialize};

use crate::CiOrigin;
use crate::words::describe_in;

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
    /// The CI job that started the run; absent for a run started anywhere else.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ci: Option<CiOrigin>,
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
    /// Why the test didn't run: a test it depends on didn't pass. A blocked run has no
    /// attempts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked: Option<Blocker>,
    /// The test wasn't chosen: it ran because a chosen test depends on it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub added_as_dependency: bool,
    /// Block packages whose steps ran, each once, in the order they first ran.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub packages: Vec<UsedPackage>,
    pub attempts: Vec<Attempt>,
}

/// A block package a run used, as its `block.json` described it when the run ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsedPackage {
    /// Like `com.acme.sign-in`.
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    /// Steps file of the package, relative to the project folder, as the steps name it.
    pub file: String,
}

impl RunReport {
    /// When the run ended, in seconds since 1970 in UTC; `None` when the start time can't be
    /// read.
    pub fn finished_at(&self) -> Option<i64> {
        let started = chrono::DateTime::parse_from_rfc3339(&self.started_at).ok()?;
        let ms = started.timestamp_millis() + i64::try_from(self.duration_ms).ok()?;
        Some(ms.div_euclid(1000))
    }

    /// The run failed while the test was in quarantine, so it doesn't fail a batch of runs.
    pub fn is_quarantined_failure(&self) -> bool {
        self.quarantined && self.verdict == Verdict::Failed
    }

    /// Reported as skipped rather than passed: the run was cancelled, failed in quarantine or
    /// passed without checking anything.
    fn is_skipped(&self) -> bool {
        matches!(
            self.verdict,
            Verdict::Cancelled | Verdict::Blocked | Verdict::Unknown
        ) || self.is_quarantined_failure()
            || self.nothing_checked
    }

    /// Where the run ran, for the report's header: `Virtual machine` or `Physical Mac`, then the
    /// display, such as `Retina display (2×)`; `None` when the report doesn't say. In the
    /// language set with `jevision_l10n::set_language`.
    pub fn machine(&self) -> Option<String> {
        self.machine_in(&jevision_l10n::language())
    }

    /// [`RunReport::machine`] in `lang`.
    pub fn machine_in(&self, lang: &str) -> Option<String> {
        let computer = self.virtual_machine.map(|v| {
            if v {
                tr_in!(lang, "report.machine.virtual")
            } else {
                tr_in!(lang, "report.machine.physical")
            }
        });
        let display = self.display_scale.map(|scale| {
            let written = if scale.fract() == 0.0 {
                format!("{scale:.0}")
            } else {
                decimal(lang, scale, 1)
            };
            if scale >= 2.0 {
                tr_in!(lang, "report.machine.retina", written)
            } else {
                tr_in!(lang, "report.machine.standard", written)
            }
        });
        match (computer, display) {
            (None, None) => None,
            (computer, display) => Some(
                [computer, display]
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
            Verdict::Cancelled | Verdict::Blocked | Verdict::Unknown => None,
        }
    }

    /// The verdict in one sentence, for the HTML report and the command line, in the language set
    /// with `jevision_l10n::set_language`.
    pub fn describe(self) -> String {
        self.describe_in(&jevision_l10n::language())
    }

    /// [`RetryVerdict::describe`] in `lang`.
    pub fn describe_in(self, lang: &str) -> String {
        match self {
            Self::PassedOnRetry { retry } => tr_in!(lang, "retry.passedOnRetry", retry),
            // Every attempt failed: "failed 2 of 2 times".
            Self::FailedDifferently { attempts } => {
                tr_in!(lang, "retry.failedDifferently", attempts, attempts)
            }
            Self::FailedEveryTime { attempts } => tr_in!(lang, "retry.failedEveryTime", attempts),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Verdict {
    Passed,
    Failed,
    Cancelled,
    /// Not run: a test it depends on didn't pass. Only a whole run is blocked, never an
    /// attempt.
    Blocked,
    /// A verdict this version doesn't know, written by a newer one. Never produced here.
    #[serde(other)]
    Unknown,
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
    /// The row's value in its `name` column, or else in its first, with secrets masked; may be
    /// empty.
    pub label: String,
    /// First 16 hex digits of the SHA-256 of the row's columns and values, so a row whose
    /// values were edited is told from the same row before the edit without keeping the
    /// values; absent in reports written before it was kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
}

impl DataRow {
    /// `Row 2: anna@example.com`, or `Row 2` when the label is empty, in the language set with
    /// `jevision_l10n::set_language`.
    pub fn title(&self) -> String {
        self.title_in(&jevision_l10n::language())
    }

    /// [`DataRow::title`] in `lang`.
    pub fn title_in(&self, lang: &str) -> String {
        if self.label.is_empty() {
            tr_in!(lang, "report.row.titleBare", self.number)
        } else {
            tr_in!(lang, "report.row.title", self.number, &self.label)
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

    /// `4 of 5 rows passed` for a run with a data table, in the language set with
    /// `jevision_l10n::set_language`.
    pub fn rows_summary(&self) -> Option<String> {
        self.rows_summary_in(&jevision_l10n::language())
    }

    /// [`RunReport::rows_summary`] in `lang`.
    pub fn rows_summary_in(&self, lang: &str) -> Option<String> {
        let rows = self.rows();
        if rows.is_empty() {
            return None;
        }
        let passed = rows.iter().filter(|r| r.verdict == Verdict::Passed).count();
        Some(tr_in!(lang, "report.rowsSummary", rows.len(), passed))
    }

    /// The report of each data row on its own, named like `Login [Row 2: anna]`; just the
    /// report for a run without a data table. In the language set with
    /// `jevision_l10n::set_language`.
    pub fn split_rows(&self) -> Vec<RunReport> {
        self.split_rows_in(&jevision_l10n::language())
    }

    /// [`RunReport::split_rows`] in `lang`.
    pub fn split_rows_in(&self, lang: &str) -> Vec<RunReport> {
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
                    test: format!("{} [{}]", self.test, r.row.title_in(lang)),
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
    /// Where the vision model found the elements the block describes in words, in fractions
    /// (0–1) of [`sent`](Self::sent).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub found: Vec<jevision_core::Rect>,
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
    /// Values the block saved for the blocks after it, read as `${var.NAME}`. Secrets are masked.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub outputs: BTreeMap<String, String>,
    /// Why the step failed. Absent on a loop or `runTest` step whose inner step failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<Failure>,
    /// When the block started, in milliseconds from the start of the run's video.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video_at_ms: Option<u64>,
}

/// A vision model's verdict on a screen, as kept in the report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisionAnswer {
    /// Provider name, such as `Anthropic`.
    pub provider: String,
    pub model: String,
    pub passed: bool,
    /// The model's explanation, in a sentence.
    pub reason: String,
    /// Tokens the call used, as the provider counted them.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub input_tokens: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub output_tokens: u64,
    /// What the call cost in US dollars, when the model has a price.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
    /// A spending limit of the provider is close or used up: `run.vision.limitNear` or
    /// `run.vision.limitReached`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<Failure>,
    /// Why the fallback provider answered instead of the main one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<Failure>,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StepStatus {
    Passed,
    Failed,
    /// There was no reference yet; the screenshot became the reference.
    BaselineCreated,
    Cancelled,
    /// A status this version doesn't know, written by a newer one. Never produced here.
    #[serde(other)]
    Unknown,
}

/// Why a run failed: a localization key and the values its text refers to, e.g.
/// `run.waitFor.timeout` with `block`, `target` and `seconds`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Failure {
    pub code: String,
    pub params: BTreeMap<String, String>,
}

impl Step {
    /// Which way an `if` went: `Took “Condition 2: Inbox is empty”`, `Took “Otherwise”`. `None`
    /// for other blocks. In the language set with `jevision_l10n::set_language`.
    pub fn branch_text(&self) -> Option<String> {
        self.branch_text_in(&jevision_l10n::language())
    }

    /// [`Step::branch_text`] in `lang`.
    pub fn branch_text_in(&self, lang: &str) -> Option<String> {
        branch_text_in(
            lang,
            self.branch.as_deref()?,
            self.branches,
            self.branch_condition.as_deref(),
        )
    }
}

/// [`Step::branch_text`] from its parts, named as the editor names the ways out of an `if`.
pub fn branch_text(branch: &str, branches: Option<u32>, condition: Option<&str>) -> Option<String> {
    branch_text_in(&jevision_l10n::language(), branch, branches, condition)
}

/// [`branch_text`] in `lang`.
pub fn branch_text_in(
    lang: &str,
    branch: &str,
    branches: Option<u32>,
    condition: Option<&str>,
) -> Option<String> {
    let index = jevision_core::else_if_index(branch);
    let plain = branches.map_or(index.is_none(), |n| n <= 2);
    let name = match (branch, index) {
        (jevision_core::THEN, _) if plain => tr_in!(lang, "report.branch.yes"),
        (jevision_core::THEN, _) => tr_in!(lang, "report.branch.condition", 1),
        (jevision_core::ELSE, _) if plain => tr_in!(lang, "report.branch.no"),
        (jevision_core::ELSE, _) => tr_in!(lang, "report.branch.otherwise"),
        (_, Some(n)) => tr_in!(lang, "report.branch.condition", n + 2),
        _ => return None,
    };
    Some(match condition {
        Some(c) => tr_in!(lang, "report.branch.tookWithCondition", name, c),
        None => tr_in!(lang, "report.branch.took", name),
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

/// Why a run that passed without looking at the screen isn't a pass.
pub fn nothing_checked_in(lang: &str) -> String {
    tr_in!(lang, "report.nothingChecked")
}

/// JUnit XML for one or more runs, one test case each. Retries use the Surefire elements
/// `flakyFailure` (failed, then passed) and `rerunFailure` (failed again). A failure of a test
/// in quarantine is reported as skipped, with the reason and the failure in its message, and so
/// is a run that passed without checking anything, so CI doesn't count it as a pass.
pub fn junit_xml(reports: &[RunReport]) -> String {
    junit_xml_in(&jevision_l10n::language(), reports)
}

/// [`junit_xml`] with its messages in `lang`.
pub fn junit_xml_in(lang: &str, reports: &[RunReport]) -> String {
    let reports: Vec<RunReport> = reports.iter().flat_map(|r| r.split_rows_in(lang)).collect();
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
        // Which way each `if` of the last attempt went and what its blocks saved, so a CI log
        // shows the path the test took.
        let paths: Vec<String> = r
            .attempts
            .last()
            .into_iter()
            .flat_map(|a| &a.steps)
            .flat_map(|s| {
                let branch = s.branch_text_in(lang).map(|b| format!("{}: {b}", s.block));
                let saved = s
                    .outputs
                    .iter()
                    .map(|(n, v)| format!("{}: {n} = {v}", s.block));
                branch.into_iter().chain(saved)
            })
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
                xml(&describe_in(lang, f)),
                xml(&f.code)
            );
        };
        match r.verdict {
            Verdict::Failed if r.quarantined => {
                let failed = failures.last().map_or_else(
                    || tr_in!(lang, "report.junit.testFailed"),
                    |f| describe_in(lang, f),
                );
                let message = match r.quarantine_reason.as_deref() {
                    Some(reason) => {
                        tr_in!(lang, "report.junit.quarantinedBecause", reason, failed)
                    }
                    None => tr_in!(lang, "report.junit.quarantined", failed),
                };
                let _ = writeln!(out, "      <skipped message=\"{}\"/>", xml(&message));
            }
            Verdict::Passed => {
                if r.nothing_checked {
                    let _ = writeln!(
                        out,
                        "      <skipped message=\"{}\"/>",
                        xml(&nothing_checked_in(lang))
                    );
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
                None => {
                    let _ = writeln!(
                        out,
                        "      <failure message=\"{}\"/>",
                        xml(&tr_in!(lang, "report.junit.testFailed"))
                    );
                }
            },
            Verdict::Cancelled => {
                let _ = writeln!(
                    out,
                    "      <skipped message=\"{}\"/>",
                    xml(&tr_in!(lang, "report.junit.cancelled"))
                );
            }
            Verdict::Blocked => {
                let reason = r.blocked.as_ref().map_or_else(
                    || tr_in!(lang, "report.junit.blocked"),
                    |b| format!("{}.", b.describe_in(lang)),
                );
                let _ = writeln!(out, "      <skipped message=\"{}\"/>", xml(&reason));
            }
            Verdict::Unknown => {
                let _ = writeln!(
                    out,
                    "      <skipped message=\"{}\"/>",
                    xml(&tr_in!(lang, "report.junit.unknown"))
                );
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
    html_in(&jevision_l10n::language(), report, run_dir, video)
}

/// [`html`] written in `lang`.
pub fn html_in(lang: &str, report: &RunReport, run_dir: &Path, video: Option<&str>) -> String {
    let mut o = String::new();
    let (badge, label) = match (report.verdict, report.flaky) {
        (Verdict::Passed, _) if report.nothing_checked => {
            ("skip", tr_in!(lang, "report.status.nothingChecked"))
        }
        (Verdict::Passed, false) => ("pass", tr_in!(lang, "report.status.passed")),
        (Verdict::Passed, true) => ("flaky", tr_in!(lang, "report.status.passedFlaky")),
        (Verdict::Failed, _) if report.quarantined => {
            ("skip", tr_in!(lang, "report.status.failedQuarantined"))
        }
        (Verdict::Failed, false) => ("fail", tr_in!(lang, "report.status.failed")),
        (Verdict::Failed, true) => ("fail", tr_in!(lang, "report.status.failedFlaky")),
        (Verdict::Cancelled, _) => ("skip", tr_in!(lang, "report.status.cancelled")),
        (Verdict::Blocked, _) => ("skip", tr_in!(lang, "report.status.blocked")),
        (Verdict::Unknown, _) => ("skip", tr_in!(lang, "report.status.unknown")),
    };
    let language = jevision_l10n::base_language(lang);
    let _ = write!(
        o,
        "<!DOCTYPE html>\n<html lang=\"{language}\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>{title} — {label}</title>\n<style>{CSS}</style>\n</head>\n<body>\n<main>\n\
         <header><h1>{title}</h1><span class=\"badge {badge}\">{label}</span></header>\n\
         <dl class=\"facts\"><dt>{test_file}</dt><dd>{file}</dd><dt>{application}</dt><dd>{app}</dd>{environment}{packages}{machine}\
         <dt>{started_label}</dt><dd>{started}</dd><dt>{duration_label}</dt><dd>{duration}</dd>\
         <dt>{attempts_label}</dt><dd>{attempts}</dd></dl>\n",
        test_file = xml(&tr_in!(lang, "report.fact.testFile")),
        application = xml(&tr_in!(lang, "report.fact.application")),
        started_label = xml(&tr_in!(lang, "report.fact.started")),
        duration_label = xml(&tr_in!(lang, "report.fact.duration")),
        attempts_label = xml(&tr_in!(lang, "report.fact.attempts")),
        title = xml(&report.test),
        file = xml(&report.file),
        app = xml(&report.app),
        environment = report
            .environment
            .as_deref()
            .map_or_else(String::new, |e| format!(
                "<dt>{}</dt><dd>{}</dd>",
                xml(&tr_in!(lang, "report.fact.environment")),
                xml(e)
            )),
        packages = if report.packages.is_empty() {
            String::new()
        } else {
            let list = report
                .packages
                .iter()
                .map(|p| {
                    tr_in!(
                        lang,
                        "report.html.package",
                        &p.name,
                        &p.version,
                        &p.author,
                        &p.id
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "<dt>{}</dt><dd>{}</dd>",
                xml(&tr_in!(lang, "report.fact.communityBlocks")),
                xml(&list)
            )
        },
        machine = report
            .machine_in(lang)
            .map_or_else(String::new, |m| format!(
                "<dt>{}</dt><dd>{}</dd>",
                xml(&tr_in!(lang, "report.fact.computer")),
                xml(&m)
            )),
        started = xml(&report.started_at.replace('T', " ")),
        duration = duration(lang, report.duration_ms),
        attempts = report.attempts.len(),
    );
    if report.nothing_checked {
        let _ = writeln!(
            o,
            "<p class=\"note\">{}</p>",
            xml(&nothing_checked_in(lang))
        );
    }
    let rows = report.rows();
    if rows.is_empty() {
        if let Some(verdict) = RetryVerdict::of(report.verdict, report.flaky, report.attempts.len())
        {
            let _ = writeln!(
                o,
                "<p class=\"note\">{}</p>",
                xml(&verdict.describe_in(lang))
            );
        }
    } else {
        let _ = writeln!(
            o,
            "<p class=\"note\">{}.</p>\n<ol class=\"steps\">",
            xml(&report.rows_summary_in(lang).unwrap_or_default())
        );
        for r in &rows {
            let (class, status) = match r.verdict {
                Verdict::Passed if r.flaky => ("pass", tr_in!(lang, "report.status.passedFlaky")),
                Verdict::Passed => ("pass", tr_in!(lang, "report.status.passed")),
                Verdict::Failed => ("fail", tr_in!(lang, "report.status.failed")),
                Verdict::Cancelled | Verdict::Blocked => {
                    ("skip", tr_in!(lang, "report.status.cancelled"))
                }
                Verdict::Unknown => ("skip", tr_in!(lang, "report.status.unknown")),
            };
            let tries = match r.attempts {
                1 => String::new(),
                n => format!(
                    "<span class=\"type\">{}</span>",
                    xml(&tr_in!(lang, "report.html.rowAttempts", n))
                ),
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
    if let Some(blocked) = &report.blocked {
        let _ = writeln!(
            o,
            "<p class=\"reason\">{}</p>",
            xml(&tr_in!(
                lang,
                "report.html.notRun",
                blocked.describe_in(lang)
            ))
        );
    }
    if report.added_as_dependency {
        let _ = writeln!(
            o,
            "<p class=\"note\">{}</p>",
            xml(&tr_in!(lang, "report.html.addedAsDependency"))
        );
    }
    if report.quarantined {
        let note = match report.quarantine_reason.as_deref() {
            Some(reason) => tr_in!(lang, "report.html.quarantineBecause", reason),
            None => tr_in!(lang, "report.html.quarantine"),
        };
        let _ = writeln!(o, "<p class=\"note\">{}</p>", xml(&note));
    }
    if !report.baselines_created.is_empty() {
        let _ = writeln!(
            o,
            "<p class=\"note\">{}</p>",
            xml(&tr_in!(
                lang,
                "report.html.newBaselines",
                report.baselines_created.join(", ")
            ))
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
            "<section>\n<h2>{}{} <small>{}</small></h2>",
            a.row
                .as_ref()
                .map_or_else(String::new, |r| format!("{} · ", xml(&r.title_in(lang)))),
            xml(&tr_in!(lang, "report.html.attempt", a.number)),
            duration(lang, a.duration_ms)
        );
        if let Some(f) = &a.failure {
            let _ = writeln!(o, "<p class=\"reason\">{}</p>", xml(&describe_in(lang, f)));
        }
        o.push_str("<ol class=\"steps\">\n");
        for s in &a.steps {
            let (class, status) = match s.status {
                StepStatus::Passed => ("pass", tr_in!(lang, "report.status.passed")),
                StepStatus::Failed => ("fail", tr_in!(lang, "report.status.failed")),
                StepStatus::BaselineCreated => {
                    ("new", tr_in!(lang, "report.status.baselineCreated"))
                }
                StepStatus::Cancelled => ("skip", tr_in!(lang, "report.status.cancelled")),
                StepStatus::Unknown => ("skip", tr_in!(lang, "report.status.unknown")),
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
                let _ = write!(
                    o,
                    "<span class=\"type\">{}</span>",
                    xml(&tr_in!(lang, "report.html.inTest", &s.test))
                );
            }
            if let Some(b) = s.branch_text_in(lang) {
                let _ = write!(o, "<span class=\"type\">{}</span>", xml(&b));
            }
            for (name, value) in &s.outputs {
                let _ = write!(
                    o,
                    "<span class=\"type\">{} = {}</span>",
                    xml(name),
                    xml(value)
                );
            }
            if let Some(sim) = s.similarity {
                let _ = write!(
                    o,
                    "<span class=\"type\">{}</span>",
                    xml(&tr_in!(lang, "report.html.alike", percent(lang, sim)))
                );
            }
            let _ = write!(
                o,
                "<span class=\"status\">{status}</span><span class=\"time\">{}</span>",
                duration(lang, s.duration_ms)
            );
            if let (Some(_), Some(ms)) = (video, s.video_at_ms) {
                let _ = write!(
                    o,
                    "<button class=\"seek\" data-t=\"{:.3}\" title=\"{}\">▶ {}</button>",
                    ms as f64 / 1000.0,
                    xml(&tr_in!(lang, "report.html.showInVideo")),
                    clock(ms)
                );
            }
            o.push_str("</div>");
            match (&s.failure, &s.vision) {
                (Some(f), _) => {
                    let _ = write!(o, "<p class=\"reason\">{}</p>", xml(&describe_in(lang, f)));
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
                tr_in!(lang, "report.html.sentMasked")
            } else {
                tr_in!(lang, "report.html.sent")
            };
            let images: Vec<(String, &String, &[jevision_core::Rect])> = [
                (tr_in!(lang, "report.html.before"), &s.before, &[][..]),
                (tr_in!(lang, "report.html.after"), &s.after, &[]),
                (tr_in!(lang, "report.html.expected"), &s.expected, &[]),
                (tr_in!(lang, "report.html.differences"), &s.diff, &[]),
                (sent, &s.sent, &s.found),
            ]
            .into_iter()
            .filter_map(|(name, path, found)| Some((name, path.as_ref()?, found)))
            .collect();
            let figures: String = images
                .iter()
                .filter_map(|(name, path, found)| {
                    let bytes = std::fs::read(run_dir.join(path)).ok()?;
                    Some(format!(
                        "<figure>{}<figcaption>{}</figcaption></figure>",
                        picture(&xml(name), &bytes, found),
                        xml(name)
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

/// A PNG for the HTML report, with `found` outlined: boxes in fractions of the picture.
fn picture(name: &str, png: &[u8], found: &[jevision_core::Rect]) -> String {
    let data = base64(png);
    // Width and height from the PNG header.
    let side = |at: usize| Some(u32::from_be_bytes(png.get(at..at + 4)?.try_into().ok()?));
    let (Some(w), Some(h), false) = (side(16), side(20), found.is_empty()) else {
        return format!("<img tabindex=\"0\" alt=\"{name}\" src=\"data:image/png;base64,{data}\">");
    };
    let boxes: String = found
        .iter()
        .map(|r| {
            format!(
                "<rect class=\"found\" x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\"/>",
                r.x * f64::from(w),
                r.y * f64::from(h),
                r.width * f64::from(w),
                r.height * f64::from(h)
            )
        })
        .collect();
    format!(
        "<svg tabindex=\"0\" role=\"img\" aria-label=\"{name}\" width=\"{w}\" height=\"{h}\" \
         viewBox=\"0 0 {w} {h}\"><image width=\"{w}\" height=\"{h}\" \
         href=\"data:image/png;base64,{data}\"/>{boxes}</svg>"
    )
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
figure svg{width:100%;height:auto;display:block;border:1px solid var(--line);border-radius:6px}\
figure img:focus,figure svg:focus{position:fixed;inset:0;margin:auto;width:auto;max-width:96vw;\
max-height:96vh;z-index:1;box-shadow:0 0 0 100vmax rgba(0,0,0,.6);cursor:zoom-out;outline:none}\
figure img,figure svg{cursor:zoom-in}\
.found{fill:none;stroke:#ff3b30;stroke-width:3;vector-effect:non-scaling-stroke}\
figcaption{color:var(--muted);font-size:12px;margin-top:4px}\
video{display:block;width:100%;max-height:70vh;margin-top:20px;border-radius:8px;background:#000}\
.seek{font:inherit;font-size:13px;font-variant-numeric:tabular-nums;color:var(--new);background:none;\
border:1px solid var(--line);border-radius:999px;padding:0 10px;cursor:pointer}";

/// A time in the video, such as `1:05`.
fn clock(ms: u64) -> String {
    let seconds = ms / 1000;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// A length of time for a person: `340 ms`, `2.5 s`.
pub(crate) fn duration(lang: &str, ms: u64) -> String {
    if ms < 1000 {
        tr_in!(lang, "report.duration.ms", ms)
    } else {
        tr_in!(
            lang,
            "report.duration.s",
            decimal(lang, ms as f64 / 1000.0, 1)
        )
    }
}

/// A share the way a saved report keeps it, the same in every language: `87.5%`.
pub(crate) fn kept_percent(share: f64) -> String {
    format!("{:.1}%", share * 100.0)
}

/// A share as a percentage in `lang`: `87.5%`.
pub(crate) fn percent(lang: &str, share: f64) -> String {
    tr_in!(lang, "report.percent", decimal(lang, share * 100.0, 1))
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
        assert_eq!(step.branch_text_in("en").as_deref(), Some("Took “Yes”"));
        assert_eq!(
            branch_text_in("en", "elseIf2", None, None).as_deref(),
            Some("Took “Condition 3”")
        );
        assert_eq!(branch_text_in("en", "next", Some(2), None), None);
    }

    #[test]
    fn branches_and_retries_are_told_in_the_language_asked_for() {
        assert_eq!(
            branch_text_in("ru", "then", None, None).as_deref(),
            Some("Выбран путь «Да»")
        );
        assert_eq!(
            branch_text_in("ru", "elseIf2", None, None).as_deref(),
            Some("Выбран путь «Условие 3»")
        );
        assert_eq!(
            branch_text_in("en", "then", Some(2), Some("a > b")).as_deref(),
            Some("Took “Yes: a > b”")
        );
        // The count picks the form: 1, 2, 5, 21 and 11 in Russian.
        for (n, en, ru) in [
            (1, "Failed 1 time in a row", "Упал 1 раз подряд"),
            (2, "Failed 2 times in a row", "Упал 2 раза подряд"),
            (5, "Failed 5 times in a row", "Упал 5 раз подряд"),
            (21, "Failed 21 times in a row", "Упал 21 раз подряд"),
            (11, "Failed 11 times in a row", "Упал 11 раз подряд"),
        ] {
            let verdict = RetryVerdict::FailedEveryTime { attempts: n };
            assert!(verdict.describe_in("en").starts_with(en), "{n}");
            assert!(verdict.describe_in("ru").starts_with(ru), "{n}");
        }
        assert_eq!(
            RetryVerdict::PassedOnRetry { retry: 2 }.describe_in("ru"),
            "Прошёл с повтора 2 после сбоя — тест нестабилен."
        );
    }

    #[test]
    fn durations_and_shares_use_the_decimal_mark_of_the_language() {
        assert_eq!(duration("en", 340), "340 ms");
        assert_eq!(duration("ru", 340), "340 мс");
        assert_eq!(duration("en", 2500), "2.5 s");
        assert_eq!(duration("ru", 2500), "2,5 с");
        assert_eq!(percent("en", 0.875), "87.5%");
        assert_eq!(percent("ru", 0.875), "87,5\u{a0}%");
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
