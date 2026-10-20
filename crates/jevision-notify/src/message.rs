//! What a message says about one run or a whole batch of them: the counts, the failures with a
//! sentence each, what changed since the runs before, and where the reports are.

use std::fs;
use std::path::{Component, Path};

use jevision_runner::{Run, RunReport, StepStatus, Verdict, describe};

use crate::{NotificationSettings, NotifyChannel, NotifyWhen};

/// Most failed tests a summary lists by name.
const MAX_LISTED: usize = 10;
/// Longest reason a summary gives for one test, in characters.
const MAX_REASON: usize = 240;
/// Biggest screenshot a mail carries, in bytes.
const MAX_PICTURE: u64 = 5 * 1024 * 1024;
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";

/// What a message says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// The first line, also the mail's subject, like `Failed: Sign in`.
    pub title: String,
    /// The lines after the title, as plain text.
    pub body: String,
    /// The same things in parts, for Slack and mail to lay out their own way.
    pub details: Details,
}

impl Message {
    /// A message with the plain text made from `details`.
    pub fn new(title: String, details: Details) -> Self {
        Self {
            title,
            body: details.text(),
            details,
        }
    }

    pub(crate) fn text(&self) -> String {
        format!("{}\n{}", self.title, self.body)
    }
}

/// How a message feels, for the colour of its heading.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Tone {
    /// Something failed.
    Bad,
    /// Everything passed, or a failure is fixed.
    Good,
    #[default]
    Neutral,
}

/// The parts of a message.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Details {
    pub tone: Tone,
    /// A sentence above the rest: why a single run failed.
    pub lead: Option<String>,
    /// Label and value, like `Took` and `12.3 s`.
    pub facts: Vec<(String, String)>,
    /// The tests that failed, as many as are listed by name.
    pub failures: Vec<FailedTest>,
    /// Failed tests left off `failures`.
    pub unlisted: usize,
    /// Tests that passed again after failing.
    pub fixed: Vec<String>,
    pub report: Option<ReportLink>,
    /// The step where a test failed, for mail.
    pub picture: Option<Picture>,
}

impl Details {
    fn text(&self) -> String {
        let mut lines: Vec<String> = self.lead.iter().cloned().collect();
        lines.extend(self.failures.iter().map(|f| match f.reason.as_str() {
            "" => format!("• {}", f.test),
            reason => format!("• {}: {reason}", f.test),
        }));
        if self.unlisted > 0 {
            lines.push(format!("and {} more", self.unlisted));
        }
        if !self.fixed.is_empty() {
            lines.push(format!("Fixed: {}", self.fixed.join(", ")));
        }
        if !self.facts.is_empty() {
            let facts: Vec<String> = self
                .facts
                .iter()
                .map(|(l, v)| format!("{l}: {v}"))
                .collect();
            lines.push(facts.join(" · "));
        }
        if let Some(report) = &self.report {
            lines.push(format!("{}: {}", report.label, report.target));
        }
        lines.join("\n")
    }
}

/// A test that failed and why, in a sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedTest {
    pub test: String,
    /// Empty when the report doesn't say.
    pub reason: String,
}

/// Where the report is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportLink {
    /// `Report` or `Report on the Mac that ran it`.
    pub label: String,
    /// The page of the CI run, or a path on the Mac that ran the tests.
    pub target: String,
    /// `target` is a web address.
    pub is_link: bool,
}

/// A screenshot as the report keeps it: private areas already covered.
#[derive(Clone, PartialEq, Eq)]
pub struct Picture {
    pub png: Vec<u8>,
    /// What it shows, for people who can't see it.
    pub caption: String,
}

impl std::fmt::Debug for Picture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Picture({} bytes, {:?})", self.png.len(), self.caption)
    }
}

/// A finished run and the run of the same test before it, to tell what changed.
#[derive(Debug, Clone)]
pub struct Finished {
    pub run: Run,
    pub previous: Option<RunReport>,
}

impl Finished {
    fn report(&self) -> &RunReport {
        &self.run.report
    }

    /// Failed, and not in quarantine, which only keeps its failures from failing a batch.
    fn failed(&self) -> bool {
        self.report().verdict == Verdict::Failed && !self.report().quarantined
    }

    fn was_failing(&self) -> bool {
        self.previous
            .as_ref()
            .is_some_and(|p| p.verdict == Verdict::Failed)
    }

    /// Passed after the run before failed.
    fn fixed(&self) -> bool {
        self.report().verdict == Verdict::Passed && self.was_failing()
    }

    /// Went from not failing to failing.
    fn broke(&self) -> bool {
        self.failed() && !self.was_failing()
    }

    fn name(&self) -> String {
        let report = self.report();
        if report.test.is_empty() {
            report.file.clone()
        } else {
            report.test.clone()
        }
    }
}

impl NotifyWhen {
    /// Whether `runs`, finished and not stopped, are worth a message.
    fn due(self, runs: &[Finished]) -> bool {
        match self {
            Self::Every => true,
            Self::Failed => runs.iter().any(Finished::failed),
            Self::FailedOrFixed => runs.iter().any(|r| r.failed() || r.fixed()),
            Self::Changed => runs.iter().any(|r| r.broke() || r.fixed()),
        }
    }
}

/// A message and the channels it goes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub message: Message,
    pub channels: Vec<NotifyChannel>,
}

/// The message about `runs`, one batch of a project called `project`, when some channel of the
/// settings asks for one. `unattended` says a schedule or CI started them. A stopped run, or a
/// batch with one, sends nothing.
pub fn notice_for(
    settings: &NotificationSettings,
    project: &str,
    runs: &[Finished],
    unattended: bool,
) -> Option<Notice> {
    if runs.is_empty()
        || (settings.unattended_only && !unattended)
        || runs
            .iter()
            .any(|r| r.report().verdict == Verdict::Cancelled)
    {
        return None;
    }
    let channels: Vec<NotifyChannel> = settings
        .channels()
        .into_iter()
        .filter(|c| settings.when(*c).is_some_and(|when| when.due(runs)))
        .collect();
    (!channels.is_empty()).then(|| Notice {
        message: message(project, runs),
        channels,
    })
}

/// The message about `runs`: for one, what happened, why, how long it took and where the
/// report is; for several, how many passed, failed, were skipped or blocked, and which failed.
pub fn message(project: &str, runs: &[Finished]) -> Message {
    match runs {
        [one] => single(one),
        _ => summary(project, runs),
    }
}

fn single(finished: &Finished) -> Message {
    let run = &finished.run;
    let report = &run.report;
    let (outcome, tone) = match report.verdict {
        Verdict::Failed => ("Failed", Tone::Bad),
        Verdict::Passed if finished.fixed() => ("Fixed", Tone::Good),
        Verdict::Passed => ("Passed", Tone::Good),
        Verdict::Cancelled => ("Stopped", Tone::Neutral),
        Verdict::Blocked => ("Blocked", Tone::Neutral),
    };
    let mut facts = vec![
        ("Took".to_owned(), duration(report.duration_ms)),
        ("File".to_owned(), report.file.clone()),
    ];
    facts.extend(context(std::slice::from_ref(report)));
    Message::new(
        format!("{outcome}: {}", finished.name()),
        Details {
            tone,
            lead: reason(report),
            facts,
            report: Some(report_link(
                &run.dir.join("report.html"),
                "Report on the Mac that ran it",
            )),
            ..Details::default()
        },
    )
}

fn summary(project: &str, runs: &[Finished]) -> Message {
    let total = runs.len();
    let (mut passed, mut failed, mut skipped, mut blocked) = (0, 0, 0, 0);
    for run in runs {
        let report = run.report();
        match report.verdict {
            Verdict::Blocked => blocked += 1,
            Verdict::Failed if !report.quarantined => failed += 1,
            Verdict::Passed if !report.nothing_checked => passed += 1,
            _ => skipped += 1,
        }
    }
    let fixed: Vec<String> = runs
        .iter()
        .filter(|r| r.fixed())
        .map(Finished::name)
        .collect();
    let failures: Vec<&Finished> = runs.iter().filter(|r| r.failed()).collect();

    let (title, tone) = if failed > 0 {
        (format!("Failed: {failed} of {}", tests(total)), Tone::Bad)
    } else if blocked > 0 {
        (
            format!("Blocked: {blocked} of {}", tests(total)),
            Tone::Neutral,
        )
    } else if !fixed.is_empty() {
        (
            format!("Fixed: {} passing again", tests(fixed.len())),
            Tone::Good,
        )
    } else {
        (format!("Passed: {}", tests(total)), Tone::Good)
    };

    let mut facts = vec![
        ("Passed".to_owned(), passed.to_string()),
        ("Failed".to_owned(), failed.to_string()),
    ];
    if skipped > 0 {
        facts.push(("Skipped".to_owned(), skipped.to_string()));
    }
    if blocked > 0 {
        facts.push(("Blocked".to_owned(), blocked.to_string()));
    }
    let took = runs.iter().map(|r| r.report().duration_ms).sum();
    facts.push(("Took".to_owned(), duration(took)));
    if !project.is_empty() {
        facts.push(("Project".to_owned(), project.to_owned()));
    }
    let reports: Vec<RunReport> = runs.iter().map(|r| r.report().clone()).collect();
    facts.extend(context(&reports));

    let runs_folder = runs
        .first()
        .and_then(|r| r.run.dir.parent())
        .unwrap_or(Path::new(""));
    Message::new(
        title,
        Details {
            tone,
            lead: None,
            facts,
            failures: failures
                .iter()
                .take(MAX_LISTED)
                .map(|f| FailedTest {
                    test: f.name(),
                    reason: clip(&reason(f.report()).unwrap_or_default(), MAX_REASON),
                })
                .collect(),
            unlisted: failures.len().saturating_sub(MAX_LISTED),
            fixed,
            report: Some(report_link(runs_folder, "Reports on the Mac that ran them")),
            picture: None,
        },
    )
}

/// The schedule and environment the runs went by, as facts.
fn context(reports: &[RunReport]) -> Vec<(String, String)> {
    let first =
        |pick: fn(&RunReport) -> &Option<String>| reports.iter().find_map(|r| pick(r).clone());
    [
        ("Schedule", first(|r| &r.schedule)),
        ("Environment", first(|r| &r.environment)),
    ]
    .into_iter()
    .filter_map(|(label, value)| Some((label.to_owned(), value?)))
    .collect()
}

/// The CI run's page when the runs are part of one; otherwise `path` on this Mac.
fn report_link(path: &Path, on_this_mac: &str) -> ReportLink {
    match ci_run_url() {
        Some(url) => ReportLink {
            label: "Report".into(),
            is_link: url.starts_with("https://") || url.starts_with("http://"),
            target: url,
        },
        None => ReportLink {
            label: on_this_mac.into(),
            target: path.display().to_string(),
            is_link: false,
        },
    }
}

/// `1 test` or `3 tests`.
fn tests(n: usize) -> String {
    match n {
        1 => "1 test".into(),
        n => format!("{n} tests"),
    }
}

/// `text` cut to `max` characters, with an ellipsis where it was cut.
pub(crate) fn clip(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((end, _)) => format!("{}…", text[..end].trim_end()),
        None => text.to_owned(),
    }
}

/// Why the run failed or didn't run, or that it passed only on a retry, in one sentence.
pub(crate) fn reason(report: &RunReport) -> Option<String> {
    if report.verdict == Verdict::Passed {
        return jevision_runner::RetryVerdict::of(
            report.verdict,
            report.flaky,
            report.attempts.len(),
        )
        .map(jevision_runner::RetryVerdict::describe);
    }
    if let Some(blocker) = &report.blocked {
        return Some(blocker.describe());
    }
    let rows = report.rows();
    if let Some(row) = rows.iter().find(|r| r.verdict == Verdict::Failed) {
        let why = row.failure.as_ref().map(describe).unwrap_or_default();
        let total = rows.len();
        let failed = rows.iter().filter(|r| r.verdict == Verdict::Failed).count();
        let more = match failed {
            1 => String::new(),
            n => format!(" ({n} of {total} rows failed)"),
        };
        return Some(
            format!("{}: {why}{more}", row.row.title())
                .trim()
                .to_owned(),
        );
    }
    report
        .attempts
        .last()
        .and_then(|a| a.failure.as_ref())
        .map(describe)
}

/// `850 ms`, `12.3 s` or `2 min 5 s`.
pub(crate) fn duration(ms: u64) -> String {
    match ms {
        0..1_000 => format!("{ms} ms"),
        1_000..60_000 => format!("{:.1} s", ms as f64 / 1000.0),
        _ => format!("{} min {} s", ms / 60_000, ms % 60_000 / 1000),
    }
}

/// The page of the GitHub Actions run this is part of, where its reports are kept.
pub(crate) fn ci_run_url() -> Option<String> {
    let var = |name| std::env::var(name).ok().filter(|v| !v.is_empty());
    Some(format!(
        "{}/{}/actions/runs/{}",
        var("GITHUB_SERVER_URL")?,
        var("GITHUB_REPOSITORY")?,
        var("GITHUB_RUN_ID")?
    ))
}

/// The screenshot of the step where the first failed test of `runs` failed, as that run's
/// report keeps it. The report only ever holds pictures with the areas the project hides, and
/// password fields, already covered, so nothing private leaves in a mail.
pub fn failure_picture(runs: &[Finished]) -> Option<Picture> {
    let failed = runs.iter().find(|r| r.failed())?;
    let report = failed.report();
    let attempt = report.deciding_attempt()?;
    let step = attempt
        .steps
        .iter()
        .rev()
        .find(|s| s.status == StepStatus::Failed && s.failure.is_some())?;
    let name = step.after.as_ref().or(step.before.as_ref())?;
    let png = read_inside(&failed.run.dir, name)?;
    Some(Picture {
        png,
        caption: format!(
            "{}: the step where it failed, with private areas covered",
            failed.name()
        ),
    })
}

/// The PNG `name` of the run folder `dir`; nothing for a file that isn't a picture, is too big
/// or lies outside the folder.
fn read_inside(dir: &Path, name: &str) -> Option<Vec<u8>> {
    if !Path::new(name)
        .components()
        .all(|c| matches!(c, Component::Normal(_)))
    {
        return None;
    }
    let dir = fs::canonicalize(dir).ok()?;
    let path = fs::canonicalize(dir.join(name)).ok()?;
    if !path.starts_with(&dir) || fs::metadata(&path).ok()?.len() > MAX_PICTURE {
        return None;
    }
    let png = fs::read(path).ok()?;
    png.starts_with(PNG).then_some(png)
}
