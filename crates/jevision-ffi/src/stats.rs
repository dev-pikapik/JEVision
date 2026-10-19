//! Statistics over the whole life of a test, and of the project: how often tests pass, how
//! long they take, where they fail and how much they can be trusted.

use std::collections::HashMap;

use jevision_runner::{StepStatus, Verdict};

use crate::history::{HealthMark, unix_ms};
use crate::{Failure, RunReport};

/// How a test has done in every saved run since it was made. Stopped runs don't count.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct TestStatistics {
    /// Runs that passed or failed.
    pub runs: u32,
    /// Runs that passed on the first try.
    pub passed: u32,
    /// Runs that passed only on a retry, after failing.
    pub passed_on_retry: u32,
    pub failed: u32,
    /// Share of the runs that passed, retries included, 0–1; `None` without runs.
    pub pass_rate: Option<f64>,
    /// Start of the first and of the latest run, RFC 3339.
    pub first_run: Option<String>,
    pub last_run: Option<String>,
    pub duration: Option<Durations>,
    /// The span each point of `trend` covers, as long as the history needs.
    pub period: StatisticsPeriod,
    /// Runs by period, oldest first; periods without runs are left out.
    pub trend: Vec<TrendPoint>,
    /// The blocks failed runs stopped at, the most failures first, at most five.
    pub failing_blocks: Vec<BlockFailures>,
    pub stability: Stability,
    /// Start of the first of the failed runs the test has ended with since it last passed,
    /// RFC 3339; `None` when its latest run passed.
    pub failing_since: Option<String>,
    /// The latest runs, at most [`RECENT_RUNS`], oldest first.
    pub recent: Vec<RecentRun>,
}

/// How long runs took, in milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct Durations {
    pub average_ms: u64,
    pub median_ms: u64,
    /// 95 runs in 100 took this long or less.
    pub p95_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum StatisticsPeriod {
    Day,
    Week,
    Month,
}

/// The runs of one period.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct TrendPoint {
    /// Start of the period in the caller's time zone, in milliseconds since 1970.
    pub start_ms: i64,
    pub passed: u32,
    pub passed_on_retry: u32,
    pub failed: u32,
    pub average_ms: u64,
}

/// A block runs failed at, and why.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct BlockFailures {
    /// Test file the block belongs to, relative to the project folder: another test's for a
    /// block of a test this one runs.
    pub test: String,
    pub block: String,
    /// Type of the block, such as `waitFor`.
    pub block_type: String,
    pub failures: u32,
    /// Start of the latest run that failed there, RFC 3339.
    pub last_failed: String,
    /// The reasons, the most frequent first, at most three; one failure of each.
    pub reasons: Vec<FailureReason>,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct FailureReason {
    /// The latest failure with this code.
    pub failure: Failure,
    pub count: u32,
}

/// Whether the result of a test can be trusted: how often it changed while the test file and
/// the environment stayed the same.
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct Stability {
    pub rating: StabilityRating,
    /// Share of the comparable runs whose result disagreed, 0–1; `None` with too few runs.
    pub flakiness: Option<f64>,
    /// Times a run's result differed from the run before it of the same test file content in
    /// the same environment.
    pub changes: u32,
    /// Runs that had a run before them of the same content and environment.
    pub comparisons: u32,
    /// Runs that passed only on a retry: each one disagreed with itself.
    pub passed_on_retry: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum StabilityRating {
    /// Too few runs of the same content to tell.
    Unknown,
    Stable,
    /// Disagreed in fewer than one run in ten.
    SometimesFlaky,
    Flaky,
}

/// One of the latest runs.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct RecentRun {
    /// Position of the run in the list given.
    pub index: u32,
    pub mark: HealthMark,
    pub started_at: String,
    pub duration_ms: u64,
}

/// The project's tests over the 30 days before now.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ProjectStatistics {
    /// Runs that passed or failed.
    pub runs: u32,
    /// Share of them that passed, 0–1; `None` without runs.
    pub pass_rate: Option<f64>,
    /// Tests that ran.
    pub tests: u32,
    /// Tests that failed at least once, the most failures first, at most ten.
    pub most_failing: Vec<TestSummary>,
    /// Tests by how long they take, the slowest first, at most ten.
    pub slowest: Vec<TestSummary>,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct TestSummary {
    /// Test file, relative to the project folder.
    pub file: String,
    /// Name of the test in its latest run.
    pub name: String,
    pub runs: u32,
    pub failed: u32,
    pub pass_rate: f64,
    pub duration: Durations,
    pub last: HealthMark,
}

/// Most runs [`TestStatistics::recent`] holds.
pub const RECENT_RUNS: usize = 50;
const MAX_BLOCKS: usize = 5;
const MAX_REASONS: usize = 3;
const MAX_TESTS: usize = 10;
const DAY_MS: i64 = 86_400_000;
const PROJECT_WINDOW_MS: i64 = 30 * DAY_MS;

/// Statistics of a test from `runs`, its saved runs in any order, with periods starting at
/// midnight `utc_offset_s` seconds east of UTC.
#[uniffi::export]
pub fn test_statistics(runs: Vec<RunReport>, utc_offset_s: i32) -> TestStatistics {
    let finished = finished(&runs);
    let count = |mark: HealthMark| finished.iter().filter(|r| r.mark == mark).count();
    let (passed, passed_on_retry, failed) = (
        count(HealthMark::Passed),
        count(HealthMark::Flaky),
        count(HealthMark::Failed),
    );
    let (period, trend) = trend(&finished, i64::from(utc_offset_s) * 1000);
    TestStatistics {
        runs: number(finished.len()),
        passed: number(passed),
        passed_on_retry: number(passed_on_retry),
        failed: number(failed),
        pass_rate: rate(passed + passed_on_retry, finished.len()),
        first_run: finished.first().map(|r| r.report.started_at.clone()),
        last_run: finished.last().map(|r| r.report.started_at.clone()),
        duration: durations(finished.iter().map(|r| r.report.duration_ms).collect()),
        period,
        trend,
        failing_blocks: failing_blocks(&finished),
        stability: stability(&finished),
        failing_since: finished
            .iter()
            .rev()
            .take_while(|r| r.mark == HealthMark::Failed)
            .last()
            .map(|r| r.report.started_at.clone()),
        recent: finished[finished.len().saturating_sub(RECENT_RUNS)..]
            .iter()
            .map(|r| RecentRun {
                index: number(r.index),
                mark: r.mark,
                started_at: r.report.started_at.clone(),
                duration_ms: r.report.duration_ms,
            })
            .collect(),
    }
}

/// The project's statistics from `runs`, its saved runs in any order, over the 30 days
/// before `now_ms` milliseconds since 1970.
#[uniffi::export]
pub fn project_statistics(runs: Vec<RunReport>, now_ms: i64) -> ProjectStatistics {
    let since = now_ms.saturating_sub(PROJECT_WINDOW_MS);
    let month: Vec<Finished> = finished(&runs)
        .into_iter()
        .filter(|r| r.at >= since)
        .collect();
    let mut by_file: HashMap<&str, Vec<&Finished>> = HashMap::new();
    for run in &month {
        by_file.entry(&run.report.file).or_default().push(run);
    }
    let summaries: Vec<TestSummary> = by_file
        .into_iter()
        .filter_map(|(file, runs)| {
            let latest = runs.last()?;
            let failed = runs.iter().filter(|r| r.mark == HealthMark::Failed).count();
            Some(TestSummary {
                file: file.to_owned(),
                name: latest.report.test.clone(),
                runs: number(runs.len()),
                failed: number(failed),
                pass_rate: rate(runs.len() - failed, runs.len())?,
                duration: durations(runs.iter().map(|r| r.report.duration_ms).collect())?,
                last: latest.mark,
            })
        })
        .collect();

    let mut most_failing: Vec<TestSummary> =
        summaries.iter().filter(|s| s.failed > 0).cloned().collect();
    most_failing.sort_by(|a, b| {
        b.failed
            .cmp(&a.failed)
            .then(a.pass_rate.total_cmp(&b.pass_rate))
            .then_with(|| a.file.cmp(&b.file))
    });
    most_failing.truncate(MAX_TESTS);
    let mut slowest = summaries.clone();
    slowest.sort_by(|a, b| {
        b.duration
            .median_ms
            .cmp(&a.duration.median_ms)
            .then_with(|| a.file.cmp(&b.file))
    });
    slowest.truncate(MAX_TESTS);

    let passed = month
        .iter()
        .filter(|r| r.mark != HealthMark::Failed)
        .count();
    ProjectStatistics {
        runs: number(month.len()),
        pass_rate: rate(passed, month.len()),
        tests: number(summaries.len()),
        most_failing,
        slowest,
    }
}

/// A run that passed or failed, with when it started.
struct Finished<'a> {
    index: usize,
    at: i64,
    mark: HealthMark,
    report: &'a RunReport,
}

/// The runs that passed or failed, oldest first.
fn finished(runs: &[RunReport]) -> Vec<Finished<'_>> {
    let mut finished: Vec<Finished> = runs
        .iter()
        .enumerate()
        .filter(|(_, run)| run.verdict != Verdict::Cancelled)
        .map(|(index, report)| Finished {
            index,
            at: unix_ms(&report.started_at).unwrap_or(i64::MIN),
            mark: match report.verdict {
                Verdict::Passed if report.flaky => HealthMark::Flaky,
                Verdict::Passed => HealthMark::Passed,
                _ => HealthMark::Failed,
            },
            report,
        })
        .collect();
    finished.sort_by_key(|r| r.at);
    finished
}

fn durations(mut ms: Vec<u64>) -> Option<Durations> {
    if ms.is_empty() {
        return None;
    }
    ms.sort_unstable();
    let n = ms.len();
    // Nearest rank: the smallest value at least `share` of the runs don't exceed.
    let rank = |share: usize| ms[(n * share).div_ceil(100).max(1) - 1];
    Some(Durations {
        average_ms: ms.iter().sum::<u64>() / n as u64,
        median_ms: rank(50),
        p95_ms: rank(95),
    })
}

/// Runs by day while the history is up to two months long, by week up to two years, and by
/// month beyond.
fn trend(runs: &[Finished], offset_ms: i64) -> (StatisticsPeriod, Vec<TrendPoint>) {
    let known: Vec<&Finished> = runs.iter().filter(|r| r.at != i64::MIN).collect();
    let span = match (known.first(), known.last()) {
        (Some(first), Some(last)) => last.at - first.at,
        _ => 0,
    };
    let period = match span {
        s if s <= 62 * DAY_MS => StatisticsPeriod::Day,
        s if s <= 730 * DAY_MS => StatisticsPeriod::Week,
        _ => StatisticsPeriod::Month,
    };
    let mut points: Vec<(TrendPoint, u64)> = Vec::new();
    for run in known {
        let start = period_start(run.at + offset_ms, period) - offset_ms;
        if points.last().is_none_or(|(p, _)| p.start_ms != start) {
            points.push((
                TrendPoint {
                    start_ms: start,
                    passed: 0,
                    passed_on_retry: 0,
                    failed: 0,
                    average_ms: 0,
                },
                0,
            ));
        }
        let (point, total) = points.last_mut().expect("pushed above");
        match run.mark {
            HealthMark::Passed => point.passed += 1,
            HealthMark::Flaky => point.passed_on_retry += 1,
            HealthMark::Failed => point.failed += 1,
        }
        *total += run.report.duration_ms;
    }
    let points = points
        .into_iter()
        .map(|(mut point, total)| {
            point.average_ms =
                total / u64::from(point.passed + point.passed_on_retry + point.failed);
            point
        })
        .collect();
    (period, points)
}

/// Start of the period `local_ms` is in, both in local milliseconds since 1970. Weeks start on
/// Monday.
fn period_start(local_ms: i64, period: StatisticsPeriod) -> i64 {
    let days = local_ms.div_euclid(DAY_MS);
    let first = match period {
        StatisticsPeriod::Day => days,
        // 1970-01-01 was a Thursday.
        StatisticsPeriod::Week => days - (days + 3).rem_euclid(7),
        StatisticsPeriod::Month => {
            let (_, _, day) = civil(days);
            days - (day - 1)
        }
    };
    first * DAY_MS
}

/// Year, month and day of a day count since 1970, after Howard Hinnant's `civil_from_days`.
fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

fn failing_blocks(runs: &[Finished]) -> Vec<BlockFailures> {
    let mut blocks: Vec<(BlockFailures, i64)> = Vec::new();
    for run in runs.iter().filter(|r| r.mark == HealthMark::Failed) {
        let Some(step) = crate::run::deciding_attempt(run.report).and_then(|attempt| {
            attempt
                .steps
                .into_iter()
                .rev()
                .find(|s| s.status == StepStatus::Failed && s.failure.is_some())
        }) else {
            continue;
        };
        let index = match blocks
            .iter()
            .position(|(b, _)| b.test == step.test && b.block == step.block)
        {
            Some(index) => index,
            None => {
                blocks.push((
                    BlockFailures {
                        test: step.test.clone(),
                        block: step.block.clone(),
                        block_type: step.block_type.clone(),
                        failures: 0,
                        last_failed: String::new(),
                        reasons: Vec::new(),
                    },
                    0,
                ));
                blocks.len() - 1
            }
        };
        let (block, last) = &mut blocks[index];
        block.failures += 1;
        block.last_failed.clone_from(&run.report.started_at);
        *last = run.at;
        let failure = step.failure.expect("found with a failure");
        match block
            .reasons
            .iter_mut()
            .find(|r| r.failure.code == failure.code)
        {
            Some(reason) => {
                reason.failure = failure;
                reason.count += 1;
            }
            None => block.reasons.push(FailureReason { failure, count: 1 }),
        }
    }
    blocks.sort_by(|(a, a_last), (b, b_last)| b.failures.cmp(&a.failures).then(b_last.cmp(a_last)));
    blocks.truncate(MAX_BLOCKS);
    blocks
        .into_iter()
        .map(|(mut block, _)| {
            block.reasons.sort_by_key(|r| std::cmp::Reverse(r.count));
            block.reasons.truncate(MAX_REASONS);
            block
        })
        .collect()
}

/// Compares each run with the run before it of the same test file content in the same
/// environment. Runs saved without the file's hash can't be compared.
fn stability(runs: &[Finished]) -> Stability {
    let mut previous: HashMap<(&str, Option<&str>), bool> = HashMap::new();
    let (mut changes, mut comparisons) = (0, 0);
    for run in runs {
        let Some(hash) = run.report.file_hash.as_deref() else {
            continue;
        };
        let passed = run.mark != HealthMark::Failed;
        if let Some(before) = previous.insert((hash, run.report.environment.as_deref()), passed) {
            comparisons += 1;
            changes += u32::from(before != passed);
        }
    }
    let passed_on_retry = number(runs.iter().filter(|r| r.mark == HealthMark::Flaky).count());
    let disagreed = changes + passed_on_retry;
    let compared = comparisons + passed_on_retry;
    let flakiness = (compared >= 2).then(|| f64::from(disagreed) / f64::from(compared));
    Stability {
        rating: match flakiness {
            None => StabilityRating::Unknown,
            Some(_) if disagreed == 0 => StabilityRating::Stable,
            Some(f) if f < 0.1 => StabilityRating::SometimesFlaky,
            Some(_) => StabilityRating::Flaky,
        },
        flakiness,
        changes,
        comparisons,
        passed_on_retry,
    }
}

fn rate(part: usize, whole: usize) -> Option<f64> {
    (whole > 0).then(|| part as f64 / whole as f64)
}

fn number(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A run of `file` started `day` days and `hour` hours after 2026-01-01, that failed at
    /// `failed_at` with `code` when given.
    fn run(file: &str, day: i64, hour: i64, ms: u64, failed_at: Option<(&str, &str)>) -> RunReport {
        let at = unix_ms("2026-01-01T00:00:00Z").unwrap() + day * DAY_MS + hour * 3_600_000;
        let (days, rest) = (at.div_euclid(DAY_MS), at.rem_euclid(DAY_MS));
        let (y, m, d) = civil(days);
        let started = format!("{y:04}-{m:02}-{d:02}T{:02}:00:00Z", rest / 3_600_000);
        let verdict = if failed_at.is_some() {
            "failed"
        } else {
            "passed"
        };
        let step = match failed_at {
            Some((block, code)) => format!(
                r#"{{"seq":2,"test":"{file}","block":"{block}","type":"check","status":"failed","durationMs":5,
                "failure":{{"code":"{code}","params":{{"block":"{block}"}}}}}}"#
            ),
            None => r#"{"seq":2,"test":"x","block":"end","type":"check","status":"passed","durationMs":5}"#.into(),
        };
        let json = format!(
            r#"{{"format":1,"test":"Sign in","file":"{file}","app":"com.example.App","startedAt":"{started}",
            "durationMs":{ms},"verdict":"{verdict}","flaky":false,"fileHash":"h",
            "attempts":[{{"number":1,"verdict":"{verdict}","durationMs":{ms},"steps":[{step}]}}]}}"#
        );
        serde_json::from_str::<jevision_runner::RunReport>(&json)
            .unwrap()
            .into()
    }

    #[test]
    fn statistics_cover_the_whole_life_of_a_test() {
        let empty = test_statistics(vec![], 0);
        assert_eq!(
            (empty.runs, empty.pass_rate, empty.duration),
            (0, None, None)
        );
        assert_eq!(empty.stability.rating, StabilityRating::Unknown);

        let mut runs = vec![
            run("a.flow.json", 0, 9, 1000, None),
            run(
                "a.flow.json",
                0,
                10,
                2000,
                Some(("check", "run.check.mismatch")),
            ),
            run("a.flow.json", 1, 9, 3000, None),
            run(
                "a.flow.json",
                2,
                9,
                4000,
                Some(("check", "run.check.mismatch")),
            ),
            run(
                "a.flow.json",
                3,
                9,
                10_000,
                Some(("wait", "run.waitFor.timeout")),
            ),
            run(
                "a.flow.json",
                3,
                10,
                5000,
                Some(("check", "run.target.notFound")),
            ),
        ];
        let mut retried = run("a.flow.json", 1, 10, 2000, None);
        retried.flaky = true;
        runs.insert(0, retried);
        let mut stopped = run("a.flow.json", 4, 0, 1, None);
        stopped.verdict = Verdict::Cancelled;
        runs.push(stopped);

        let stats = test_statistics(runs, 0);
        assert_eq!(
            (
                stats.runs,
                stats.passed,
                stats.passed_on_retry,
                stats.failed
            ),
            (7, 2, 1, 4)
        );
        assert_eq!(stats.pass_rate, Some(3.0 / 7.0));
        assert_eq!(stats.first_run.as_deref(), Some("2026-01-01T09:00:00Z"));
        assert_eq!(stats.last_run.as_deref(), Some("2026-01-04T10:00:00Z"));
        assert_eq!(
            stats.duration,
            Some(Durations {
                average_ms: 27_000 / 7,
                median_ms: 3000,
                p95_ms: 10_000
            })
        );
        assert_eq!(stats.period, StatisticsPeriod::Day);
        let days: Vec<(u32, u32, u32)> = stats
            .trend
            .iter()
            .map(|p| (p.passed, p.passed_on_retry, p.failed))
            .collect();
        assert_eq!(days, [(1, 0, 1), (1, 1, 0), (0, 0, 1), (0, 0, 2)]);
        assert_eq!(stats.trend[1].start_ms - stats.trend[0].start_ms, DAY_MS);
        assert_eq!(stats.trend[0].average_ms, 1500);

        let check = &stats.failing_blocks[0];
        assert_eq!((check.block.as_str(), check.failures), ("check", 3));
        assert_eq!(check.last_failed, "2026-01-04T10:00:00Z");
        let reasons: Vec<(&str, u32)> = check
            .reasons
            .iter()
            .map(|r| (r.failure.code.as_str(), r.count))
            .collect();
        assert_eq!(
            reasons,
            [("run.check.mismatch", 2), ("run.target.notFound", 1)]
        );
        assert_eq!(stats.failing_blocks[1].block, "wait");

        // Six comparisons, three changes, one pass on retry: four disagreements in seven.
        assert_eq!(
            (stats.stability.changes, stats.stability.comparisons),
            (3, 6)
        );
        assert_eq!(stats.stability.flakiness, Some(4.0 / 7.0));
        assert_eq!(stats.stability.rating, StabilityRating::Flaky);
        assert_eq!(stats.failing_since.as_deref(), Some("2026-01-03T09:00:00Z"));
        assert_eq!(stats.recent.len(), 7);
        // The retried run was given first, and it ran fourth.
        assert_eq!(stats.recent[3].index, 0);
        assert_eq!(stats.recent[3].mark, HealthMark::Flaky);
    }

    #[test]
    fn stability_compares_only_runs_of_the_same_content() {
        let mut runs: Vec<RunReport> = (0..20).map(|d| run("a.flow.json", d, 9, 1, None)).collect();
        assert_eq!(
            test_statistics(runs.clone(), 0).stability.rating,
            StabilityRating::Stable
        );
        let mut changed = run(
            "a.flow.json",
            20,
            9,
            1,
            Some(("check", "run.check.mismatch")),
        );
        changed.file_hash = Some("new".into());
        runs.push(changed);
        assert_eq!(
            test_statistics(runs.clone(), 0).stability.rating,
            StabilityRating::Stable
        );
        runs.push(run(
            "a.flow.json",
            21,
            9,
            1,
            Some(("check", "run.check.mismatch")),
        ));
        let stability = test_statistics(runs, 0).stability;
        assert_eq!((stability.changes, stability.comparisons), (1, 20));
        assert_eq!(stability.rating, StabilityRating::SometimesFlaky);
    }

    #[test]
    fn long_histories_are_counted_by_week_or_month_in_local_time() {
        let runs = vec![run("a", 0, 1, 1, None), run("a", 100, 1, 1, None)];
        let stats = test_statistics(runs.clone(), 0);
        assert_eq!(stats.period, StatisticsPeriod::Week);
        // 2026-01-01 was a Thursday; its week started on Monday 2025-12-29.
        assert_eq!(
            stats.trend[0].start_ms,
            unix_ms("2025-12-29T00:00:00Z").unwrap()
        );
        // At 01:00 UTC it is still 2025-12-31 in New York, a Wednesday of the same week.
        let west = test_statistics(runs, -5 * 3600);
        assert_eq!(
            west.trend[0].start_ms,
            unix_ms("2025-12-29T05:00:00Z").unwrap()
        );
        let runs = vec![run("a", 0, 12, 1, None), run("a", 800, 12, 1, None)];
        let stats = test_statistics(runs, 0);
        assert_eq!(stats.period, StatisticsPeriod::Month);
        assert_eq!(
            stats.trend[1].start_ms,
            unix_ms("2028-03-01T00:00:00Z").unwrap()
        );
    }

    #[test]
    fn the_project_names_its_most_failing_and_slowest_tests() {
        let now = unix_ms("2026-01-31T00:00:00Z").unwrap();
        let fail = Some(("check", "run.check.mismatch"));
        let runs = vec![
            run("fast.flow.json", 20, 0, 100, None),
            run("slow.flow.json", 20, 0, 9000, fail),
            run("slow.flow.json", 21, 0, 7000, None),
            run("flaky.flow.json", 20, 0, 500, fail),
            run("flaky.flow.json", 21, 0, 500, fail),
            // Older than 30 days.
            run("fast.flow.json", -5, 0, 100, fail),
        ];
        let stats = project_statistics(runs, now);
        assert_eq!((stats.runs, stats.tests), (5, 3));
        assert_eq!(stats.pass_rate, Some(0.4));
        let files = |list: &[TestSummary]| list.iter().map(|t| t.file.clone()).collect::<Vec<_>>();
        assert_eq!(
            files(&stats.most_failing),
            ["flaky.flow.json", "slow.flow.json"]
        );
        assert_eq!(
            files(&stats.slowest),
            ["slow.flow.json", "flaky.flow.json", "fast.flow.json"]
        );
        assert_eq!(stats.slowest[0].duration.median_ms, 7000);
        assert_eq!(stats.most_failing[0].last, HealthMark::Failed);
    }

    #[test]
    fn thousands_of_runs_are_quick() {
        let runs: Vec<RunReport> = (0..5000)
            .map(|i| {
                let fail = (i % 7 == 0).then_some(("check", "run.check.mismatch"));
                run(
                    &format!("t{}.flow.json", i % 50),
                    i / 10,
                    i % 10,
                    1000 + i as u64,
                    fail,
                )
            })
            .collect();
        let started = std::time::Instant::now();
        let stats = test_statistics(runs.clone(), 3600);
        let project = project_statistics(runs, unix_ms("2027-05-01T00:00:00Z").unwrap());
        assert_eq!(stats.runs, 5000);
        assert!(!project.slowest.is_empty());
        assert!(
            started.elapsed() < std::time::Duration::from_secs(1),
            "{:?}",
            started.elapsed()
        );
    }
}
