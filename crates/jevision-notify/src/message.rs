//! What a message says about one run or a whole batch of them: the counts, the failures with a
//! sentence each, what changed since the runs before, and where the reports are.

use std::fs;
use std::path::{Component, Path};

use jevision_core::BlockReason;
use jevision_l10n::{decimal, language, tr_in};
use jevision_runner::{CiOrigin, Run, RunReport, StepStatus, Verdict, describe_in};

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
    /// The language the message is written in, like `en`.
    pub lang: String,
}

impl Message {
    /// A message with the plain text made from `details`, in the process language.
    pub fn new(title: String, details: Details) -> Self {
        Self::new_in(&language(), title, details)
    }

    /// [`Message::new`] in `lang`.
    pub fn new_in(lang: &str, title: String, details: Details) -> Self {
        Self {
            title,
            body: details.text(lang),
            details,
            lang: lang.to_owned(),
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
    fn text(&self, lang: &str) -> String {
        let mut lines: Vec<String> = self.lead.iter().cloned().collect();
        lines.extend(self.failures.iter().map(|f| match f.reason.as_str() {
            "" => format!("• {}", f.test),
            reason => format!("• {}: {reason}", f.test),
        }));
        if self.unlisted > 0 {
            lines.push(tr_in!(lang, "and %lld more", self.unlisted));
        }
        if !self.fixed.is_empty() {
            let fixed = tr_in!(lang, "notify.fixed");
            lines.push(format!("{fixed}: {}", self.fixed.join(", ")));
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
    /// The CI system whose page `target` is, like `GitHub Actions`; empty for a path.
    pub system: String,
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

    /// Blocked by a dependency that is missing or in a circle. No test failed to say so, unlike
    /// one blocked by a failed dependency, which has its own message.
    fn stuck(&self) -> bool {
        self.report().verdict == Verdict::Blocked
            && self.report().blocked.as_ref().is_some_and(|b| {
                matches!(b.reason, BlockReason::Missing | BlockReason::Cycle { .. })
            })
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
            Self::Failed => runs.iter().any(|r| r.failed() || r.stuck()),
            Self::FailedOrFixed => runs.iter().any(|r| r.failed() || r.fixed() || r.stuck()),
            Self::Changed => runs.iter().any(|r| r.broke() || r.fixed() || r.stuck()),
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
/// batch with one, sends nothing; nor does a verdict only a newer version knows, which is never
/// this version's own run.
pub fn notice_for(
    settings: &NotificationSettings,
    project: &str,
    runs: &[Finished],
    unattended: bool,
) -> Option<Notice> {
    notice_for_in(&language(), settings, project, runs, unattended)
}

/// [`notice_for`] with the message in `lang`.
pub fn notice_for_in(
    lang: &str,
    settings: &NotificationSettings,
    project: &str,
    runs: &[Finished],
    unattended: bool,
) -> Option<Notice> {
    if runs.is_empty()
        || (settings.unattended_only && !unattended)
        || runs
            .iter()
            .any(|r| matches!(r.report().verdict, Verdict::Cancelled | Verdict::Unknown))
    {
        return None;
    }
    let channels: Vec<NotifyChannel> = settings
        .channels()
        .into_iter()
        .filter(|c| settings.when(*c).is_some_and(|when| when.due(runs)))
        .collect();
    (!channels.is_empty()).then(|| Notice {
        message: message_in(lang, project, runs),
        channels,
    })
}

/// The message about `runs`: for one, what happened, why, how long it took and where the
/// report is; for several, how many passed, failed, were skipped or blocked, and which failed.
pub fn message(project: &str, runs: &[Finished]) -> Message {
    message_in(&language(), project, runs)
}

/// [`message`] in `lang`.
pub fn message_in(lang: &str, project: &str, runs: &[Finished]) -> Message {
    match runs {
        [one] => single(lang, one),
        _ => summary(lang, project, runs),
    }
}

fn single(lang: &str, finished: &Finished) -> Message {
    let run = &finished.run;
    let report = &run.report;
    let name = finished.name();
    let (title, tone) = match report.verdict {
        Verdict::Failed => (tr_in!(lang, "notify.single.failed", &name), Tone::Bad),
        Verdict::Passed if finished.fixed() => {
            (tr_in!(lang, "notify.single.fixed", &name), Tone::Good)
        }
        Verdict::Passed => (tr_in!(lang, "notify.single.passed", &name), Tone::Good),
        Verdict::Cancelled => (tr_in!(lang, "notify.single.stopped", &name), Tone::Neutral),
        Verdict::Blocked => (tr_in!(lang, "notify.single.blocked", &name), Tone::Neutral),
        Verdict::Unknown => (tr_in!(lang, "notify.single.unknown", &name), Tone::Neutral),
    };
    let mut facts = vec![
        (
            tr_in!(lang, "notify.fact.took"),
            duration(lang, report.duration_ms),
        ),
        (tr_in!(lang, "File"), report.file.clone()),
    ];
    facts.extend(context(lang, std::slice::from_ref(report)));
    Message::new_in(
        lang,
        title,
        Details {
            tone,
            lead: reason(lang, report),
            facts,
            report: Some(report_link(
                lang,
                &run.dir.join("report.html"),
                tr_in!(lang, "notify.report.onMac"),
            )),
            ..Details::default()
        },
    )
}

fn summary(lang: &str, project: &str, runs: &[Finished]) -> Message {
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
        (
            tr_in!(lang, "notify.summary.failed", total, failed),
            Tone::Bad,
        )
    } else if blocked > 0 {
        (
            tr_in!(lang, "notify.summary.blocked", total, blocked),
            Tone::Neutral,
        )
    } else if !fixed.is_empty() {
        (
            tr_in!(lang, "notify.summary.fixed", fixed.len()),
            Tone::Good,
        )
    } else {
        (tr_in!(lang, "notify.summary.passed", total), Tone::Good)
    };

    let mut facts = vec![
        (tr_in!(lang, "notify.fact.passed"), passed.to_string()),
        (tr_in!(lang, "notify.fact.failed"), failed.to_string()),
    ];
    if skipped > 0 {
        facts.push((tr_in!(lang, "notify.fact.skipped"), skipped.to_string()));
    }
    if blocked > 0 {
        facts.push((tr_in!(lang, "notify.fact.blocked"), blocked.to_string()));
    }
    let took = runs.iter().map(|r| r.report().duration_ms).sum();
    facts.push((tr_in!(lang, "notify.fact.took"), duration(lang, took)));
    if !project.is_empty() {
        facts.push((tr_in!(lang, "notify.fact.project"), project.to_owned()));
    }
    let reports: Vec<RunReport> = runs.iter().map(|r| r.report().clone()).collect();
    facts.extend(context(lang, &reports));

    let runs_folder = runs
        .first()
        .and_then(|r| r.run.dir.parent())
        .unwrap_or(Path::new(""));
    Message::new_in(
        lang,
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
                    reason: clip(&reason(lang, f.report()).unwrap_or_default(), MAX_REASON),
                })
                .collect(),
            unlisted: failures.len().saturating_sub(MAX_LISTED),
            fixed,
            report: Some(report_link(
                lang,
                runs_folder,
                tr_in!(lang, "notify.report.onMacMany"),
            )),
            picture: None,
        },
    )
}

/// The schedule and environment the runs went by, as facts.
fn context(lang: &str, reports: &[RunReport]) -> Vec<(String, String)> {
    let first =
        |pick: fn(&RunReport) -> &Option<String>| reports.iter().find_map(|r| pick(r).clone());
    [
        (tr_in!(lang, "Schedule"), first(|r| &r.schedule)),
        (
            tr_in!(lang, "report.fact.environment"),
            first(|r| &r.environment),
        ),
    ]
    .into_iter()
    .filter_map(|(label, value)| Some((label, value?)))
    .collect()
}

/// The page of the CI run when the runs are part of one; otherwise `path` on this Mac.
fn report_link(lang: &str, path: &Path, on_this_mac: String) -> ReportLink {
    match CiOrigin::from_env().and_then(|ci| Some((ci.run_url?, ci.system))) {
        Some((url, system)) => ReportLink {
            label: tr_in!(lang, "notify.report.label"),
            is_link: url.starts_with("https://") || url.starts_with("http://"),
            target: url,
            system,
        },
        None => ReportLink {
            label: on_this_mac,
            target: path.display().to_string(),
            is_link: false,
            system: String::new(),
        },
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
fn reason(lang: &str, report: &RunReport) -> Option<String> {
    if report.verdict == Verdict::Passed {
        return jevision_runner::RetryVerdict::of(
            report.verdict,
            report.flaky,
            report.attempts.len(),
        )
        .map(|verdict| verdict.describe_in(lang));
    }
    if let Some(blocker) = &report.blocked {
        return Some(blocker.describe_in(lang));
    }
    let rows = report.rows();
    if let Some(row) = rows.iter().find(|r| r.verdict == Verdict::Failed) {
        let why = row
            .failure
            .as_ref()
            .map(|f| describe_in(lang, f))
            .unwrap_or_default();
        let total = rows.len();
        let failed = rows.iter().filter(|r| r.verdict == Verdict::Failed).count();
        let more = match failed {
            1 => String::new(),
            n => format!(" {}", tr_in!(lang, "notify.rowsFailed", total, n)),
        };
        return Some(
            format!("{}: {why}{more}", row.row.title_in(lang))
                .trim()
                .to_owned(),
        );
    }
    report
        .attempts
        .last()
        .and_then(|a| a.failure.as_ref())
        .map(|f| describe_in(lang, f))
}

/// `850 ms`, `12.3 s` or `2 min 5 s`.
pub(crate) fn duration(lang: &str, ms: u64) -> String {
    match ms {
        0..1_000 => tr_in!(lang, "report.duration.ms", ms),
        1_000..60_000 => tr_in!(
            lang,
            "report.duration.s",
            decimal(lang, ms as f64 / 1000.0, 1)
        ),
        _ => tr_in!(
            lang,
            "notify.duration.minutes",
            ms / 60_000,
            ms % 60_000 / 1000
        ),
    }
}

/// The screenshot of the step where the first failed test of `runs` failed, as that run's
/// report keeps it. The report only ever holds pictures with the areas the project hides, and
/// password fields, already covered, so nothing private leaves in a mail.
pub fn failure_picture(runs: &[Finished]) -> Option<Picture> {
    failure_picture_in(&language(), runs)
}

/// [`failure_picture`] with the caption in `lang`.
pub fn failure_picture_in(lang: &str, runs: &[Finished]) -> Option<Picture> {
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
        caption: tr_in!(lang, "notify.picture.caption", failed.name()),
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
