//! Runs saved in a project: listing them, reading one back, exporting its report and deleting
//! the old ones.

use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use jevision_core::RunRetention;
use jevision_runner::{PruneSummary, Run, StepStatus, Verdict};

use crate::{DataRow, RunReport};

/// A saved run could not be read or its report could not be written.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum ReportError {
    #[error("{reason}")]
    Failed { reason: String },
}

impl From<std::io::Error> for ReportError {
    fn from(e: std::io::Error) -> Self {
        Self::Failed {
            reason: e.to_string(),
        }
    }
}

/// A finished run: its folder and the report saved there.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct SavedRun {
    pub dir: String,
    pub report: RunReport,
}

impl From<Run> for SavedRun {
    fn from(run: Run) -> Self {
        Self {
            dir: run.dir.to_string_lossy().into_owned(),
            report: run.report.into(),
        }
    }
}

/// Runs saved in `<project>/.jevision/runs`, newest first. Runs still going and folders without
/// a readable report are left out.
#[uniffi::export]
pub fn list_runs(project: String) -> Result<Vec<SavedRun>, ReportError> {
    Ok(Run::list(Path::new(&project))?
        .into_iter()
        .map(Into::into)
        .collect())
}

/// Runs saved in `<project>/.jevision/runs`, newest first, like [`list_runs`], but each attempt
/// keeps only the steps that failed and the first step of every test file it ran: enough for
/// lists, health and statistics. [`load_run`] reads a run whole, for its report and the marks on
/// its blocks. Reports are read once per process and again only when one is written anew.
#[uniffi::export]
pub fn list_run_summaries(project: String) -> Result<Vec<SavedRun>, ReportError> {
    Ok(Run::summaries(Path::new(&project))?
        .into_iter()
        .map(Into::into)
        .collect())
}

/// What a project shows from its saved runs at `now_ms` milliseconds since 1970, in one call:
/// the runs as [`list_run_summaries`] gives them, the flaky tests as [`flaky_tests`] tells and
/// the tests unreliable at least `suggest_after` times as [`quarantine_suggestions`] does.
#[uniffi::export]
pub fn project_history(
    project: String,
    now_ms: i64,
    suggest_after: u32,
) -> Result<ProjectHistory, ReportError> {
    let runs = list_run_summaries(project)?;
    let reports: Vec<RunReport> = runs.iter().map(|r| r.report.clone()).collect();
    Ok(ProjectHistory {
        flaky: flaky_tests(reports.clone(), now_ms),
        unreliable: quarantine_suggestions(reports, now_ms, suggest_after),
        runs,
    })
}

/// See [`project_history`].
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ProjectHistory {
    pub runs: Vec<SavedRun>,
    /// By test file relative to the project folder.
    pub flaky: HashMap<String, FlakyEvidence>,
    /// By test file relative to the project folder.
    pub unreliable: HashMap<String, QuarantineSuggestion>,
}

/// What deleting old runs freed, or would free.
#[uniffi::remote(Record)]
pub struct PruneSummary {
    /// Run folders deleted whole.
    pub removed_runs: u64,
    /// Videos and screenshots deleted from runs that stay.
    pub removed_media_files: u64,
    pub freed_bytes: u64,
}

/// Deletes for good the saved runs of the project folder `project`, and the videos and
/// screenshots of others, that `retention` doesn't keep at `now_ms` milliseconds since 1970.
/// The newest run of each test stays, and so does a run still being written.
#[uniffi::export]
pub fn prune_runs(
    project: String,
    retention: RunRetention,
    now_ms: i64,
) -> Result<PruneSummary, ReportError> {
    Ok(jevision_runner::prune_runs(
        Path::new(&project),
        &retention,
        at(now_ms),
    )?)
}

/// What [`prune_runs`] would delete, without deleting anything.
#[uniffi::export]
pub fn preview_prune_runs(
    project: String,
    retention: RunRetention,
    now_ms: i64,
) -> Result<PruneSummary, ReportError> {
    Ok(jevision_runner::preview_prune_runs(
        Path::new(&project),
        &retention,
        at(now_ms),
    )?)
}

/// The time `ms` milliseconds after 1970; earlier times as 1970.
fn at(ms: i64) -> SystemTime {
    UNIX_EPOCH + Duration::from_millis(u64::try_from(ms).unwrap_or(0))
}

/// Reads the run saved in the folder `dir`.
#[uniffi::export]
pub fn load_run(dir: String) -> Result<SavedRun, ReportError> {
    Ok(Run::load(Path::new(&dir))?.into())
}

/// Saves the report of the run in `dir` as one HTML file with its screenshots inside. The run's
/// video, too large to embed, is copied next to the file under the same name ending in `.mp4`.
#[uniffi::export]
pub fn export_html(dir: String, to: String) -> Result<(), ReportError> {
    let run = Run::load(Path::new(&dir))?;
    let to = Path::new(&to);
    let source = run
        .report
        .video
        .as_deref()
        .map(|video| run.dir.join(video))
        .filter(|video| video.is_file());
    let video = match source {
        Some(source) => {
            let copy = to.with_extension("mp4");
            // Copying a file onto itself would empty it.
            let same = std::fs::canonicalize(&copy)
                .is_ok_and(|c| std::fs::canonicalize(&source).is_ok_and(|s| s == c));
            if !same {
                std::fs::copy(&source, &copy)?;
            }
            copy.file_name().map(|name| link(&name.to_string_lossy()))
        }
        None => None,
    };
    std::fs::write(
        to,
        jevision_runner::html(&run.report, &run.dir, video.as_deref()),
    )?;
    Ok(())
}

/// A file name as a relative link: every byte but letters, digits and `-._~` percent-encoded.
fn link(name: &str) -> String {
    name.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                char::from(b).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Saves the reports of the runs in `dirs` as one JUnit XML file, a test case per run, for CI
/// systems. Failures of tests in quarantine are reported as skipped unless
/// `include_quarantined_failures`.
#[uniffi::export]
pub fn export_junit(
    dirs: Vec<String>,
    to: String,
    include_quarantined_failures: bool,
) -> Result<(), ReportError> {
    let mut reports = dirs
        .iter()
        .map(|dir| Run::load(Path::new(dir)).map(|run| run.report))
        .collect::<std::io::Result<Vec<_>>>()?;
    if include_quarantined_failures {
        reports.iter_mut().for_each(|r| r.quarantined = false);
    }
    std::fs::write(to, jevision_runner::junit_xml(&reports))?;
    Ok(())
}

/// How a test has been doing lately, as its saved runs tell. Stopped runs don't count.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct TestHealth {
    /// Runs that passed or failed in the 30 days before now.
    pub runs_30_days: u32,
    /// Share of those runs that passed, in whole percent rounded down; `None` without runs.
    pub pass_percent: Option<u32>,
    /// Of the runs that passed, those that passed only on a retry, after failing: a pass the
    /// percentage counts, but not a clean one.
    pub passed_on_retry: u32,
    /// The last 20 runs that passed or failed, oldest first.
    pub recent: Vec<HealthMark>,
    /// The block the failed runs of the 30 days stopped at most often; of two blocks as
    /// often, the one that failed last.
    pub fails_most_at: Option<FailurePoint>,
    /// Start of the first of the failed runs the test has ended with since it last passed,
    /// RFC 3339; `None` when its latest run passed.
    pub failing_since: Option<String>,
    /// Why the test counts as flaky; `None` when its recent runs agree.
    pub flaky: Option<FlakyEvidence>,
}

/// Runs of the 30 days that both passed and failed with the test file exactly as it is in the
/// latest run, in the same environment and, for a test with a data table, on the same row with
/// the same values: nothing in the test changed, yet the result did.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FlakyEvidence {
    /// Runs that passed on the first try.
    pub passed: u32,
    /// Runs that passed only on a retry, after failing.
    pub passed_on_retry: u32,
    /// Runs that failed every try.
    pub failed: u32,
    /// Environment of the runs; `None` when they used none.
    pub environment: Option<String>,
    /// Start of the first of the runs, RFC 3339.
    pub since: String,
    /// The data row the runs are about, as the latest of them named it; `None` for a test
    /// without a data table.
    #[uniffi(default = None)]
    pub row: Option<DataRow>,
}

/// Why a test is worth putting in quarantine: it was unreliable several times in a week.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct QuarantineSuggestion {
    /// The runs of the test in the 30 days that behave unreliably, in the one environment and
    /// on the one data row with the most unreliable runs in the last 7 days.
    pub evidence: FlakyEvidence,
    /// What made the test unreliable in the last 7 days, oldest first.
    pub events: Vec<FlakyEvent>,
}

/// One time the test was unreliable.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FlakyEvent {
    /// Start of the run, RFC 3339.
    pub started_at: String,
    pub kind: FlakyKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FlakyKind {
    /// The run failed and then passed on a retry.
    PassedOnRetry,
    /// The run failed although the one before it, with nothing changed, passed.
    FailedAfterPass,
}

/// How one run went, for the health bars.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum HealthMark {
    Passed,
    /// Passed after failing first.
    Flaky,
    Failed,
}

/// A block runs failed at.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FailurePoint {
    /// Test file the block belongs to, relative to the project folder: another test's for a
    /// block of a test this one runs.
    pub test: String,
    pub block: String,
    /// Type of the block, such as `waitFor`.
    pub block_type: String,
    /// Failed runs that stopped there.
    pub failures: u32,
}

const HEALTH_WINDOW_MS: i64 = 30 * 86_400_000;
const HEALTH_BARS: usize = 20;
const WEEK_MS: i64 = 7 * 86_400_000;

/// The health of a test from `runs`, its saved runs in any order, at `now_ms` milliseconds
/// since 1970.
#[uniffi::export]
pub fn test_health(runs: Vec<RunReport>, now_ms: i64) -> TestHealth {
    let mut finished: Vec<(i64, &RunReport)> = runs
        .iter()
        .filter(|run| {
            !matches!(
                run.verdict,
                Verdict::Cancelled | Verdict::Blocked | Verdict::Unknown
            )
        })
        .map(|run| (unix_ms(&run.started_at).unwrap_or(i64::MIN), run))
        .collect();
    finished.sort_by_key(|(at, _)| *at);

    let since = now_ms.saturating_sub(HEALTH_WINDOW_MS);
    let month: Vec<&RunReport> = finished
        .iter()
        .filter(|(at, _)| *at >= since)
        .map(|(_, run)| *run)
        .collect();
    let passed = month
        .iter()
        .filter(|run| run.verdict == Verdict::Passed)
        .count();
    let passed_on_retry = month
        .iter()
        .filter(|run| run.verdict == Verdict::Passed && run.flaky)
        .count();

    let mut points: Vec<(FailurePoint, usize)> = Vec::new();
    for (index, step) in month.iter().enumerate().filter_map(|(i, run)| {
        run.attempts
            .last()?
            .steps
            .iter()
            .rev()
            .find(|step| step.status == StepStatus::Failed && step.failure.is_some())
            .map(|step| (i, step))
    }) {
        match points
            .iter_mut()
            .find(|(p, _)| p.test == step.test && p.block == step.block)
        {
            Some((point, last)) => {
                point.failures += 1;
                *last = index;
            }
            None => points.push((
                FailurePoint {
                    test: step.test.clone(),
                    block: step.block.clone(),
                    block_type: step.block_type.clone(),
                    failures: 1,
                },
                index,
            )),
        }
    }

    let recent = finished[finished.len().saturating_sub(HEALTH_BARS)..]
        .iter()
        .map(|(_, run)| match run.verdict {
            Verdict::Passed if run.flaky => HealthMark::Flaky,
            Verdict::Passed => HealthMark::Passed,
            _ => HealthMark::Failed,
        })
        .collect();
    let failing_since = finished
        .iter()
        .rev()
        .take_while(|(_, run)| run.verdict == Verdict::Failed)
        .last()
        .map(|(_, run)| run.started_at.clone());

    TestHealth {
        runs_30_days: u32::try_from(month.len()).unwrap_or(u32::MAX),
        pass_percent: (!month.is_empty())
            .then(|| u32::try_from(passed * 100 / month.len()).unwrap_or(100)),
        passed_on_retry: u32::try_from(passed_on_retry).unwrap_or(u32::MAX),
        recent,
        fails_most_at: points
            .into_iter()
            .max_by_key(|(point, last)| (point.failures, *last))
            .map(|(point, _)| point),
        failing_since,
        flaky: flaky_evidence(&month),
    }
}

/// The tests of `runs` that count as flaky at `now_ms` milliseconds since 1970, by test file
/// relative to the project folder, with the evidence; see [`TestHealth::flaky`].
#[uniffi::export]
pub fn flaky_tests(runs: Vec<RunReport>, now_ms: i64) -> HashMap<String, FlakyEvidence> {
    let mut by_file: HashMap<String, Vec<RunReport>> = HashMap::new();
    for run in runs {
        by_file.entry(run.file.clone()).or_default().push(run);
    }
    by_file
        .into_iter()
        .filter_map(|(file, runs)| Some((file, test_health(runs, now_ms).flaky?)))
        .collect()
}

/// Runs of one test file content in one environment, on one data row.
struct Group {
    evidence: FlakyEvidence,
    events: Vec<(i64, FlakyEvent)>,
    /// The latest run of the group passed.
    passing: bool,
}

/// What `runs`, finished and oldest first, show about each environment and data row of the
/// latest run's test file content, the group run last coming last. Runs saved without the
/// file's hash prove nothing.
fn flaky_groups(runs: &[&RunReport]) -> Vec<Group> {
    let Some(hash) = runs.last().and_then(|r| r.file_hash.as_deref()) else {
        return Vec::new();
    };
    let mut groups: Vec<(Option<String>, Group)> = Vec::new();
    for run in runs.iter().filter(|r| r.file_hash.as_deref() == Some(hash)) {
        let at = unix_ms(&run.started_at).unwrap_or(i64::MIN);
        let seen: Vec<(Option<&DataRow>, Verdict, bool)> = if run.rows.is_empty() {
            vec![(None, run.verdict, run.flaky)]
        } else {
            run.rows
                .iter()
                .map(|r| (Some(&r.row), r.verdict, r.flaky))
                .collect()
        };
        for (row, verdict, retried) in seen {
            let key = row.map(|r| {
                r.hash
                    .clone()
                    .unwrap_or_else(|| format!("{}\u{1f}{}", r.number, r.label))
            });
            let index = match groups
                .iter()
                .position(|(k, g)| *k == key && g.evidence.environment == run.environment)
            {
                Some(index) => index,
                None => {
                    groups.push((
                        key,
                        Group {
                            evidence: FlakyEvidence {
                                passed: 0,
                                passed_on_retry: 0,
                                failed: 0,
                                environment: run.environment.clone(),
                                since: run.started_at.clone(),
                                row: None,
                            },
                            events: Vec::new(),
                            passing: false,
                        },
                    ));
                    groups.len() - 1
                }
            };
            let (_, group) = &mut groups[index];
            group.evidence.row = row.cloned();
            let mut event = |kind| {
                group.events.push((
                    at,
                    FlakyEvent {
                        started_at: run.started_at.clone(),
                        kind,
                    },
                ));
            };
            match verdict {
                Verdict::Passed if retried => {
                    group.evidence.passed_on_retry += 1;
                    event(FlakyKind::PassedOnRetry);
                    group.passing = true;
                }
                Verdict::Passed => {
                    group.evidence.passed += 1;
                    group.passing = true;
                }
                _ => {
                    group.evidence.failed += 1;
                    // A test that broke and stays broken is one event, not one per run.
                    if group.passing {
                        event(FlakyKind::FailedAfterPass);
                    }
                    group.passing = false;
                }
            }
            // The group run last comes last.
            let latest = groups.remove(index);
            groups.push(latest);
        }
    }
    groups.into_iter().map(|(_, g)| g).collect()
}

/// Evidence that the test is flaky from `runs`, finished and oldest first: among the runs of
/// the latest run's test file content, the environment and data row whose runs disagree, the
/// most recently run one if several do.
fn flaky_evidence(runs: &[&RunReport]) -> Option<FlakyEvidence> {
    flaky_groups(runs)
        .into_iter()
        .rev()
        .map(|g| g.evidence)
        .find(|e| e.passed_on_retry > 0 || (e.passed > 0 && e.failed > 0))
}

/// The tests of `runs` unreliable at least `after` times within the 7 days before `now_ms`
/// milliseconds since 1970, by test file relative to the project folder: runs that passed only
/// on a retry, and runs that failed after one that passed with nothing changed. Several failed
/// runs in a row count once, so a test that broke for good isn't suggested.
#[uniffi::export]
pub fn quarantine_suggestions(
    runs: Vec<RunReport>,
    now_ms: i64,
    after: u32,
) -> HashMap<String, QuarantineSuggestion> {
    let mut by_file: HashMap<String, Vec<RunReport>> = HashMap::new();
    for run in runs {
        by_file.entry(run.file.clone()).or_default().push(run);
    }
    let week_start = now_ms.saturating_sub(WEEK_MS);
    by_file
        .into_iter()
        .filter_map(|(file, runs)| {
            let mut finished: Vec<(i64, &RunReport)> = runs
                .iter()
                .filter(|run| {
                    !matches!(
                        run.verdict,
                        Verdict::Cancelled | Verdict::Blocked | Verdict::Unknown
                    )
                })
                .map(|run| (unix_ms(&run.started_at).unwrap_or(i64::MIN), run))
                .filter(|(at, _)| *at >= now_ms.saturating_sub(HEALTH_WINDOW_MS))
                .collect();
            finished.sort_by_key(|(at, _)| *at);
            let finished: Vec<&RunReport> = finished.into_iter().map(|(_, run)| run).collect();
            let best = flaky_groups(&finished)
                .into_iter()
                .map(|g| {
                    let events: Vec<FlakyEvent> = g
                        .events
                        .into_iter()
                        .filter(|(at, _)| *at >= week_start)
                        .map(|(_, event)| event)
                        .collect();
                    (g.evidence, events)
                })
                // The group run last wins a tie.
                .max_by_key(|(_, events)| events.len())?;
            let (evidence, events) = best;
            (events.len() >= after.max(1) as usize)
                .then_some((file, QuarantineSuggestion { evidence, events }))
        })
        .collect()
}

/// Milliseconds since 1970 of an RFC 3339 time, such as `2026-10-18T14:30:05.123Z` or
/// `2026-10-18T16:30:05+02:00`.
pub(crate) fn unix_ms(text: &str) -> Option<i64> {
    let number = |from: usize, to: usize| -> Option<i64> {
        let digits = text.get(from..to)?;
        digits
            .bytes()
            .all(|b| b.is_ascii_digit())
            .then(|| digits.parse().ok())?
    };
    let separators = [(4, b'-'), (7, b'-'), (13, b':'), (16, b':')];
    let bytes = text.as_bytes();
    if bytes.len() < 20
        || !separators.iter().all(|&(i, c)| bytes[i] == c)
        || !matches!(bytes[10], b'T' | b't' | b' ')
    {
        return None;
    }
    let (year, month, day) = (number(0, 4)?, number(5, 7)?, number(8, 10)?);
    let (hour, minute, second) = (number(11, 13)?, number(14, 16)?, number(17, 19)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 {
        return None;
    }
    if second > 60 {
        return None;
    }
    let mut rest = &text[19..];
    let mut ms = 0;
    if let Some(fraction) = rest.strip_prefix('.') {
        let digits = fraction.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            return None;
        }
        ms = format!("{:0<3}", &fraction[..digits.min(3)]).parse().ok()?;
        rest = &fraction[digits..];
    }
    let offset = match rest {
        "Z" | "z" => 0,
        _ => {
            let sign = match rest.as_bytes().first()? {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let parts = rest.get(1..)?;
            let digits = |range: std::ops::Range<usize>| -> Option<i64> {
                let part = parts.get(range)?;
                part.bytes()
                    .all(|b| b.is_ascii_digit())
                    .then(|| part.parse().ok())?
            };
            if parts.len() != 5 || parts.as_bytes()[2] != b':' {
                return None;
            }
            sign * (digits(0..2)? * 60 + digits(3..5)?)
        }
    };
    // Civil date to days, after Howard Hinnant's `days_from_civil`.
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let seconds = days * 86_400 + hour * 3600 + minute * 60 + second - offset * 60;
    Some(seconds * 1000 + ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct Folder(PathBuf);

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn report(test: &str, started: &str, verdict: &str) -> String {
        format!(
            r#"{{"format":1,"test":"{test}","file":"{test}.flow.json","app":"com.example.App",
            "startedAt":"{started}","durationMs":1200,"verdict":"{verdict}","flaky":false,
            "attempts":[{{"number":1,"verdict":"{verdict}","durationMs":1200,"steps":[
              {{"seq":1,"test":"{test}.flow.json","block":"check","type":"check","status":"{verdict}",
                "durationMs":300,"after":"shots/1-001-check-after.png","reference":"refs/home.png",
                "similarity":0.5}}]}}]}}"#
        )
    }

    fn saved(test: &str, started: &str, verdict: &str) -> RunReport {
        serde_json::from_str::<jevision_runner::RunReport>(&report(test, started, verdict))
            .unwrap()
            .into()
    }

    #[test]
    fn flaky_when_the_same_test_both_passes_and_fails() {
        let now = unix_ms("2026-10-18T12:00:00Z").unwrap();
        let run = |at: &str, verdict: &str, hash: &str, env: Option<&str>| {
            let mut run = saved("t", &format!("2026-10-18T{at}:00.000Z"), verdict);
            run.file_hash = Some(hash.into());
            run.environment = env.map(Into::into);
            run
        };
        let flaky = |runs: Vec<RunReport>| test_health(runs, now).flaky;

        // A failure before the test was changed is not flakiness.
        assert_eq!(
            flaky(vec![
                run("08:00", "failed", "old", None),
                run("09:00", "passed", "new", None)
            ]),
            None
        );
        // Failing in one environment and passing in another is not either.
        assert_eq!(
            flaky(vec![
                run("08:00", "failed", "a", Some("staging")),
                run("09:00", "passed", "a", Some("production")),
            ]),
            None
        );
        // Old reports without a hash prove nothing.
        let mut unhashed = run("08:00", "failed", "a", None);
        unhashed.file_hash = None;
        assert_eq!(
            flaky(vec![unhashed, run("09:00", "passed", "a", None)]),
            None
        );

        let mut retried = run("10:00", "passed", "a", None);
        retried.flaky = true;
        assert_eq!(
            flaky(vec![
                run("07:00", "failed", "old", None),
                run("08:00", "failed", "a", None),
                run("09:00", "passed", "a", None),
                retried,
                run("11:00", "passed", "a", None),
            ]),
            Some(FlakyEvidence {
                passed: 2,
                passed_on_retry: 1,
                failed: 1,
                environment: None,
                since: "2026-10-18T08:00:00.000Z".into(),
                row: None,
            })
        );
        // A pass after failing within one run is evidence on its own.
        let mut retried = run("10:00", "passed", "a", None);
        retried.flaky = true;
        assert_eq!(flaky(vec![retried]).map(|e| e.passed_on_retry), Some(1));
        let mut other = run("09:00", "passed", "b", None);
        other.file = "other.flow.json".into();
        let found = flaky_tests(
            vec![
                run("08:00", "failed", "a", None),
                run("09:00", "passed", "a", None),
                other,
            ],
            now,
        );
        assert_eq!(found.keys().collect::<Vec<_>>(), ["t.flow.json"]);
        // Runs older than 30 days are forgotten.
        let mut old = run("08:00", "failed", "a", None);
        old.started_at = "2026-08-01T08:00:00.000Z".into();
        assert_eq!(flaky(vec![old, run("09:00", "passed", "a", None)]), None);
    }

    /// A saved run of a test with a data table: `rows` are (hash, verdict of the row, the row
    /// passed only on a retry).
    fn row_run(started: &str, rows: &[(&str, &str, bool)]) -> RunReport {
        let attempts: Vec<String> = rows
            .iter()
            .enumerate()
            .flat_map(|(i, (hash, verdict, retried))| {
                let row = format!(
                    r#""row":{{"number":{},"label":"r{i}","hash":"{hash}"}}"#,
                    i + 1
                );
                let mut attempts = Vec::new();
                if *retried {
                    attempts.push(format!(
                        r#"{{"number":1,"verdict":"failed","durationMs":1,"steps":[],{row},
                        "failure":{{"code":"run.check.mismatch","params":{{}}}}}}"#
                    ));
                }
                let number = attempts.len() + 1;
                let failure = if *verdict == "failed" {
                    r#","failure":{"code":"run.check.mismatch","params":{}}"#
                } else {
                    ""
                };
                attempts.push(format!(
                    r#"{{"number":{number},"verdict":"{verdict}","durationMs":1,"steps":[],{row}{failure}}}"#
                ));
                attempts
            })
            .collect();
        let verdict = if rows.iter().all(|(_, v, _)| *v == "passed") {
            "passed"
        } else {
            "failed"
        };
        let mut run: RunReport = serde_json::from_str::<jevision_runner::RunReport>(&format!(
            r#"{{"format":1,"test":"t","file":"t.flow.json","app":"com.example.App",
            "startedAt":"{started}","durationMs":1,"verdict":"{verdict}","flaky":false,
            "attempts":[{}]}}"#,
            attempts.join(",")
        ))
        .unwrap()
        .into();
        run.file_hash = Some("a".into());
        run
    }

    #[test]
    fn flakiness_is_told_per_data_row() {
        let now = unix_ms("2026-10-18T12:00:00Z").unwrap();
        let at = |hour: u32| format!("2026-10-18T{hour:02}:00:00.000Z");
        // Row 1 always passes; row 2 passes on a retry once, row 3 fails and passes.
        let runs = vec![
            row_run(
                &at(7),
                &[
                    ("h1", "passed", false),
                    ("h2", "passed", false),
                    ("h3", "passed", false),
                ],
            ),
            row_run(
                &at(8),
                &[
                    ("h1", "passed", false),
                    ("h2", "passed", true),
                    ("h3", "failed", false),
                ],
            ),
            row_run(
                &at(9),
                &[
                    ("h1", "passed", false),
                    ("h2", "passed", false),
                    ("h3", "passed", false),
                ],
            ),
        ];
        let evidence = test_health(runs, now).flaky.unwrap();
        let row = evidence.row.as_ref().unwrap();
        assert_eq!((row.number, row.hash.as_deref()), (3, Some("h3")));
        assert_eq!(
            (evidence.passed, evidence.passed_on_retry, evidence.failed),
            (2, 0, 1)
        );

        // A row that fails everywhere is not flaky because another row passes.
        let runs = vec![
            row_run(&at(8), &[("h1", "passed", false), ("h2", "failed", false)]),
            row_run(&at(9), &[("h1", "passed", false), ("h2", "failed", false)]),
        ];
        assert_eq!(test_health(runs, now).flaky, None);

        // Editing a row's values starts its history over.
        let runs = vec![
            row_run(&at(8), &[("old", "failed", false)]),
            row_run(&at(9), &[("new", "passed", false)]),
        ];
        assert_eq!(test_health(runs, now).flaky, None);
    }

    #[test]
    fn quarantine_is_suggested_after_enough_unreliable_runs_in_a_week() {
        let now = unix_ms("2026-10-18T12:00:00Z").unwrap();
        let run = |day: u32, hour: u32, verdict: &str, retried: bool| {
            row_run(
                &format!("2026-10-{day:02}T{hour:02}:00:00.000Z"),
                &[("h", verdict, retried)],
            )
        };
        let suggested = |runs: Vec<RunReport>, after| quarantine_suggestions(runs, now, after);

        // Three runs that needed a retry in a week.
        let runs = vec![
            run(14, 9, "passed", true),
            run(15, 9, "passed", false),
            run(16, 9, "passed", true),
            run(17, 9, "passed", true),
        ];
        let found = suggested(runs.clone(), 3);
        let suggestion = &found["t.flow.json"];
        assert_eq!(suggestion.events.len(), 3);
        assert!(
            suggestion
                .events
                .iter()
                .all(|e| e.kind == FlakyKind::PassedOnRetry)
        );
        assert_eq!(suggestion.events[0].started_at, "2026-10-14T09:00:00.000Z");
        assert_eq!(suggestion.evidence.row.as_ref().unwrap().number, 1);
        assert!(suggested(runs.clone(), 4).is_empty());

        // Runs older than 7 days still count for health but not for the suggestion.
        let mut old = runs.clone();
        old[0].started_at = "2026-10-01T09:00:00.000Z".into();
        assert!(suggested(old, 3).is_empty());

        // Pass, fail, pass, fail, pass, fail: failing after a pass is unreliable too.
        let swing = vec![
            run(12, 9, "passed", false),
            run(13, 9, "failed", false),
            run(14, 9, "passed", false),
            run(15, 9, "failed", false),
            run(16, 9, "passed", false),
            run(17, 9, "failed", false),
        ];
        let found = suggested(swing, 3);
        let kinds: Vec<_> = found["t.flow.json"].events.iter().map(|e| e.kind).collect();
        assert_eq!(kinds, [FlakyKind::FailedAfterPass; 3]);

        // A test that broke for good is one event, however many runs failed since.
        let broken = vec![
            run(13, 9, "passed", false),
            run(14, 9, "failed", false),
            run(15, 9, "failed", false),
            run(16, 9, "failed", false),
            run(17, 9, "failed", false),
        ];
        assert!(suggested(broken, 2).is_empty());
    }

    #[test]
    fn reads_rfc3339_times() {
        assert_eq!(unix_ms("2023-11-14T22:13:20.123Z"), Some(1_700_000_000_123));
        assert_eq!(
            unix_ms("2023-11-15T00:13:20.1+02:00"),
            Some(1_700_000_000_100)
        );
        assert_eq!(
            unix_ms("2023-11-14T20:13:20-02:00"),
            Some(1_700_000_000_000)
        );
        assert_eq!(unix_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(unix_ms("2000-02-29T00:00:00.000Z"), Some(951_782_400_000));
        assert_eq!(unix_ms("1969-12-31T23:59:59Z"), Some(-1000));
        for bad in [
            "",
            "2023-11-14",
            "2023-11-14T22:13:20",
            "2023-13-14T22:13:20Z",
            "2023-11-14T22:13:20.Z",
            "2023-11-14T22:13:20+0200",
            "2023-11-1xT22:13:20Z",
            "2023-11-14T22:13:2\u{e9}Z",
        ] {
            assert_eq!(unix_ms(bad), None, "{bad}");
        }
    }

    #[test]
    fn health_of_a_test() {
        let now = unix_ms("2026-10-18T15:00:00.000Z").unwrap();
        let empty = test_health(vec![], now);
        assert_eq!(empty.runs_30_days, 0);
        assert_eq!(empty.pass_percent, None);
        assert!(empty.recent.is_empty());
        assert_eq!(empty.fails_most_at, None);
        assert_eq!(empty.failing_since, None);

        let at_block = |run: RunReport, test: &str, block: &str| {
            let mut run = run;
            let step = &mut run.attempts[0].steps[0];
            step.test = test.into();
            step.block = block.into();
            step.failure = Some(crate::Failure {
                code: "run.check.mismatch".into(),
                params: Default::default(),
            });
            run
        };
        let mut runs = vec![
            // Older than 30 days: a bar, but not in the percentage or the failure count.
            at_block(
                saved("t", "2026-09-01T10:00:00.000Z", "failed"),
                "t.flow.json",
                "old",
            ),
            saved("t", "2026-10-10T10:00:00.000Z", "passed"),
            at_block(
                saved("t", "2026-10-11T10:00:00.000Z", "failed"),
                "t.flow.json",
                "wait",
            ),
            at_block(
                saved("t", "2026-10-12T10:00:00.000Z", "failed"),
                "shared/login.flow.json",
                "wait",
            ),
            at_block(
                saved("t", "2026-10-13T10:00:00.000Z", "failed"),
                "shared/login.flow.json",
                "wait",
            ),
            saved("t", "2026-10-17T10:00:00.000Z", "cancelled"),
            saved("t", "2026-10-17T11:00:00.000Z", "blocked"),
            // Saved by a newer version: neither passed nor failed as far as this one knows.
            saved("t", "2026-10-17T12:00:00.000Z", "skipped"),
        ];
        let mut flaky = saved("t", "2026-10-18T09:00:00.000Z", "passed");
        flaky.flaky = true;
        runs.push(flaky);
        runs.push(at_block(
            saved("t", "2026-10-18T14:28:00.000Z", "failed"),
            "t.flow.json",
            "wait",
        ));
        runs.push(at_block(
            saved("t", "2026-10-18T14:40:00.000Z", "failed"),
            "t.flow.json",
            "check",
        ));
        runs.reverse();

        let health = test_health(runs, now);
        assert_eq!(health.runs_30_days, 7);
        assert_eq!(health.pass_percent, Some(28));
        assert_eq!(health.passed_on_retry, 1);
        assert_eq!(
            health.recent,
            [
                HealthMark::Failed,
                HealthMark::Passed,
                HealthMark::Failed,
                HealthMark::Failed,
                HealthMark::Failed,
                HealthMark::Flaky,
                HealthMark::Failed,
                HealthMark::Failed,
            ]
        );
        assert_eq!(
            health.fails_most_at,
            Some(FailurePoint {
                test: "t.flow.json".into(),
                block: "wait".into(),
                block_type: "check".into(),
                failures: 2,
            }),
            "a tie goes to the block that failed last"
        );
        assert_eq!(
            health.failing_since.as_deref(),
            Some("2026-10-18T14:28:00.000Z")
        );

        let many: Vec<RunReport> = (0..25)
            .map(|i| saved("t", &format!("2026-10-18T10:{i:02}:00.000Z"), "passed"))
            .collect();
        let health = test_health(many, now);
        assert_eq!(health.recent.len(), 20);
        assert_eq!(health.pass_percent, Some(100));
        assert_eq!(health.failing_since, None);
    }

    #[test]
    fn project_history_tells_what_the_lists_do_and_old_runs_go() {
        let root = std::env::temp_dir().join(format!(
            "jevision-ffi-project-history-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let project = Folder(root);
        let path = project.0.to_string_lossy().into_owned();
        let save = |name: &str, started: &str, verdict: &str| {
            let dir = project.0.join(".jevision/runs").join(name);
            std::fs::create_dir_all(&dir).unwrap();
            let json = report("login", started, verdict).replacen(
                r#""flaky":false"#,
                r#""flaky":false,"fileHash":"a""#,
                1,
            );
            std::fs::write(dir.join("report.json"), json).unwrap();
        };
        save("1", "2026-10-17T09:00:00.000Z", "passed");
        save("2", "2026-10-18T10:15:00.000Z", "failed");
        save("3", "2026-04-01T08:00:00.000Z", "passed");
        let now = unix_ms("2026-10-18T12:00:00Z").unwrap();

        let history = project_history(path.clone(), now, 1).unwrap();
        let listed = list_runs(path.clone()).unwrap();
        let reports: Vec<RunReport> = listed.iter().map(|r| r.report.clone()).collect();
        assert_eq!(history.runs, listed);
        assert_eq!(history.flaky, flaky_tests(reports.clone(), now));
        assert!(history.flaky.contains_key("login.flow.json"));
        assert_eq!(history.unreliable, quarantine_suggestions(reports, now, 1));
        assert!(history.unreliable.contains_key("login.flow.json"));

        let keep = RunRetention {
            max_age_days: Some(180),
            ..RunRetention::default()
        };
        let preview = preview_prune_runs(path.clone(), keep, now).unwrap();
        assert_eq!(preview.removed_runs, 1);
        assert_eq!(list_runs(path.clone()).unwrap().len(), 3);
        assert_eq!(prune_runs(path.clone(), keep, now).unwrap(), preview);
        assert_eq!(list_run_summaries(path).unwrap().len(), 2);
    }

    #[test]
    fn lists_and_loads_saved_runs() {
        let root =
            std::env::temp_dir().join(format!("jevision-ffi-history-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let project = Folder(root);
        let runs = project.0.join(".jevision/runs");
        let save = |name: &str, json: &str| {
            std::fs::create_dir_all(runs.join(name)).unwrap();
            std::fs::write(runs.join(name).join("report.json"), json).unwrap();
        };
        assert_eq!(list_runs(project.0.to_string_lossy().into()), Ok(vec![]));

        save(
            "20261017-090000-login",
            &report("login", "2026-10-17T09:00:00.000Z", "passed"),
        );
        save(
            "20261018-101500-login",
            &report("login", "2026-10-18T10:15:00.000Z", "failed"),
        );
        save(
            "20261018-080000-home",
            &report("home", "2026-10-18T08:00:00.000Z", "cancelled"),
        );
        save("20261018-110000-broken", "{ not json");
        save(
            "20261018-120000-future",
            &report("future", "2026-10-18T12:00:00.000Z", "passed").replacen(
                r#""format":1"#,
                r#""format":99"#,
                1,
            ),
        );
        // Still running: no report yet.
        std::fs::create_dir_all(runs.join("20261018-130000-busy/shots")).unwrap();

        let listed = list_runs(project.0.to_string_lossy().into()).unwrap();
        let order: Vec<_> = listed
            .iter()
            .map(|r| (r.report.test.as_str(), r.report.verdict))
            .collect();
        assert_eq!(
            order,
            [
                ("login", Verdict::Failed),
                ("home", Verdict::Cancelled),
                ("login", Verdict::Passed)
            ]
        );
        assert!(listed[0].dir.ends_with("20261018-101500-login"));

        let loaded = load_run(listed[0].dir.clone()).unwrap();
        assert_eq!(loaded, listed[0]);
        let step = &loaded.report.attempts[0].steps[0];
        assert_eq!(step.status, StepStatus::Failed);
        assert_eq!(step.reference.as_deref(), Some("refs/home.png"));

        assert!(load_run(runs.join("20261018-110000-broken").to_string_lossy().into()).is_err());
        let Err(ReportError::Failed { reason }) =
            load_run(runs.join("20261018-120000-future").to_string_lossy().into())
        else {
            panic!("a newer format must not load");
        };
        assert!(reason.contains("99"));

        let html = project.0.join("report.html");
        let junit = project.0.join("junit.xml");
        export_html(listed[0].dir.clone(), html.to_string_lossy().into()).unwrap();
        export_junit(
            vec![listed[0].dir.clone()],
            junit.to_string_lossy().into(),
            false,
        )
        .unwrap();
        assert!(std::fs::read_to_string(&html).unwrap().contains("login"));
        assert!(!std::fs::read_to_string(&html).unwrap().contains("<video"));
        assert!(
            std::fs::read_to_string(&junit)
                .unwrap()
                .contains("<failure")
        );
        let all = listed.iter().map(|r| r.dir.clone()).collect();
        export_junit(all, junit.to_string_lossy().into(), false).unwrap();
        let xml = std::fs::read_to_string(&junit).unwrap();
        assert!(
            xml.contains(r#"tests="3" failures="1" errors="0" skipped="1""#),
            "{xml}"
        );
        assert!(
            export_junit(
                vec![runs.join("20261018-110000-broken").to_string_lossy().into()],
                junit.to_string_lossy().into(),
                false,
            )
            .is_err()
        );
        assert!(
            export_html(
                listed[0].dir.clone(),
                project.0.join("missing/x.html").to_string_lossy().into()
            )
            .is_err()
        );
    }

    #[test]
    fn exported_html_brings_the_video_along() {
        let root = std::env::temp_dir().join(format!("jevision-ffi-video-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let project = Folder(root);
        let dir = project.0.join(".jevision/runs/20261018-101500-login");
        std::fs::create_dir_all(&dir).unwrap();
        let json = report("login", "2026-10-18T10:15:00.000Z", "failed")
            .replacen(
                r#""flaky":false,"#,
                r#""flaky":false,"video":"video.mp4","#,
                1,
            )
            .replacen(
                r#""similarity":0.5"#,
                r#""similarity":0.5,"videoAtMs":2500"#,
                1,
            );
        std::fs::write(dir.join("report.json"), json).unwrap();
        std::fs::write(dir.join("video.mp4"), b"movie").unwrap();

        let run = load_run(dir.to_string_lossy().into()).unwrap();
        assert_eq!(run.report.video.as_deref(), Some("video.mp4"));
        assert_eq!(run.report.attempts[0].steps[0].video_at_ms, Some(2500));

        let html = project.0.join("Sign in #1.html");
        export_html(dir.to_string_lossy().into(), html.to_string_lossy().into()).unwrap();
        assert_eq!(
            std::fs::read(project.0.join("Sign in #1.mp4")).unwrap(),
            b"movie"
        );
        let page = std::fs::read_to_string(&html).unwrap();
        assert!(page.contains(r#"src="Sign%20in%20%231.mp4""#), "{page}");
        assert!(page.contains(r#"data-t="2.500""#), "{page}");

        // Saving the page into the run folder as `video.html` must not empty the video.
        let inside = dir.join("video.html");
        export_html(
            dir.to_string_lossy().into(),
            inside.to_string_lossy().into(),
        )
        .unwrap();
        assert_eq!(std::fs::read(dir.join("video.mp4")).unwrap(), b"movie");

        std::fs::remove_file(dir.join("video.mp4")).unwrap();
        export_html(dir.to_string_lossy().into(), html.to_string_lossy().into()).unwrap();
        assert!(!std::fs::read_to_string(&html).unwrap().contains("<video"));
    }
}
