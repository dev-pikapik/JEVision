//! Scheduled runs of a project: the schedule file, when schedules run, the launchd job that
//! starts them and what came of them.

use std::path::Path;

use jevision_l10n::tr;

use jevision_core::{
    Repeat, Schedule, ScheduleTests, ScheduledOutcome as CoreOutcome, TimeOfDay, Weekday,
};

use jevision_runner::VideoMode;

/// The schedule file can't be read or written, or the schedules in it can't run.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum ScheduleError {
    #[error("{}", tr!("project.file.unreadable", .file, .reason))]
    Unreadable { file: String, reason: String },
    /// A schedule can't run as it is; `reason` says why.
    #[error("{reason}")]
    Invalid { reason: String },
    #[error("{reason}")]
    Unwritable { reason: String },
}

/// Which tests a schedule runs. Paths are relative to the project folder.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum ScheduledTests {
    All,
    Folder { path: String },
    Files { paths: Vec<String> },
}

/// On which days and how often a schedule runs.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum ScheduleRepeat {
    Daily,
    /// Days of the week, 0 for Sunday to 6 for Saturday.
    Days {
        days: Vec<u8>,
    },
    /// Around the clock, in step with the schedule's time; one of [`schedule_every_hours`].
    EveryHours {
        hours: u8,
    },
}

/// One schedule of a project.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ProjectSchedule {
    /// Unique within the project.
    pub name: String,
    pub tests: ScheduledTests,
    pub repeats: ScheduleRepeat,
    /// Time of day of the run; for every few hours, the run the others count from.
    pub hour: u8,
    pub minute: u8,
    /// `None` runs in the default environment of the project.
    pub environment: Option<String>,
    /// `None` follows the app's setting.
    pub video: Option<VideoMode>,
}

/// What came of a scheduled run.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum ScheduledOutcome {
    /// `failed` counts every test that didn't pass.
    Finished { passed: u32, failed: u32 },
    /// Nothing ran, so nothing touched the computer.
    Skipped { reason: SkipReason },
    /// The tests couldn't start.
    Error { message: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SkipReason {
    ScreenLocked,
    NoUser,
    NotAllowed,
}

/// One scheduled run.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ScheduledRun {
    pub schedule: String,
    /// Milliseconds since 1970, UTC.
    pub started_at_ms: i64,
    pub duration_ms: u64,
    pub outcome: ScheduledOutcome,
}

/// The launchd job that runs the schedules of a project.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LaunchAgent {
    pub label: String,
    /// The property list to save as `~/Library/LaunchAgents/<label>.plist`.
    pub plist: String,
}

fn invalid(reason: String) -> ScheduleError {
    ScheduleError::Invalid { reason }
}

impl TryFrom<ProjectSchedule> for Schedule {
    type Error = ScheduleError;

    fn try_from(s: ProjectSchedule) -> Result<Self, ScheduleError> {
        let at = TimeOfDay::new(s.hour, s.minute).ok_or_else(|| {
            let time = format!("{}:{:02}", s.hour, s.minute);
            invalid(tr!(
                "schedule.problem",
                &s.name,
                tr!("schedule.hourMinute.invalid", time)
            ))
        })?;
        let repeat = match s.repeats {
            ScheduleRepeat::Daily => Repeat::Daily,
            ScheduleRepeat::Days { days } => Repeat::Days(
                days.into_iter()
                    .map(|day| {
                        Weekday::from_number(day).ok_or_else(|| {
                            invalid(tr!(
                                "schedule.problem",
                                &s.name,
                                tr!("schedule.day.invalid", day)
                            ))
                        })
                    })
                    .collect::<Result<_, _>>()?,
            ),
            ScheduleRepeat::EveryHours { hours } => Repeat::EveryHours(hours),
        };
        Ok(Schedule {
            name: s.name,
            tests: match s.tests {
                ScheduledTests::All => ScheduleTests::All,
                ScheduledTests::Folder { path } => ScheduleTests::Folder(path),
                ScheduledTests::Files { paths } => ScheduleTests::Files(paths),
            },
            repeat,
            at,
            environment: s.environment,
            video: s.video.map(|mode| video_name(mode).to_owned()),
        })
    }
}

impl From<Schedule> for ProjectSchedule {
    fn from(s: Schedule) -> Self {
        Self {
            name: s.name,
            tests: match s.tests {
                ScheduleTests::All => ScheduledTests::All,
                ScheduleTests::Folder(path) => ScheduledTests::Folder { path },
                ScheduleTests::Files(paths) => ScheduledTests::Files { paths },
            },
            repeats: match s.repeat {
                Repeat::Daily => ScheduleRepeat::Daily,
                Repeat::Days(days) => ScheduleRepeat::Days {
                    days: days.into_iter().map(Weekday::number).collect(),
                },
                Repeat::EveryHours(hours) => ScheduleRepeat::EveryHours { hours },
            },
            hour: s.at.hour,
            minute: s.at.minute,
            environment: s.environment,
            video: s.video.as_deref().and_then(video_mode),
        }
    }
}

fn video_name(mode: VideoMode) -> &'static str {
    match mode {
        VideoMode::Off => "off",
        VideoMode::OnFailure => "on-failure",
        VideoMode::Always => "always",
    }
}

fn video_mode(name: &str) -> Option<VideoMode> {
    [VideoMode::Off, VideoMode::OnFailure, VideoMode::Always]
        .into_iter()
        .find(|mode| video_name(*mode) == name)
}

fn to_core(schedules: Vec<ProjectSchedule>) -> Result<Vec<Schedule>, ScheduleError> {
    schedules.into_iter().map(Schedule::try_from).collect()
}

fn unreadable(reason: String) -> ScheduleError {
    ScheduleError::Unreadable {
        file: jevision_core::SCHEDULE_FILE.to_owned(),
        reason,
    }
}

/// Name of the schedule file in a project folder.
#[uniffi::export]
pub fn schedule_file_name() -> String {
    jevision_core::SCHEDULE_FILE.to_owned()
}

/// Hours between runs a schedule that repeats every few hours may have.
#[uniffi::export]
pub fn schedule_every_hours() -> Vec<u8> {
    jevision_core::EVERY_HOURS.to_vec()
}

/// The schedules of the project folder `project`, in the file's order; none without a file.
#[uniffi::export]
pub fn project_schedules(project: String) -> Result<Vec<ProjectSchedule>, ScheduleError> {
    jevision_core::load_schedules(Path::new(&project))
        .map(|all| all.into_iter().map(Into::into).collect())
        .map_err(unreadable)
}

/// Checks `schedules` and saves them as the schedule file of `project`; with none, the file
/// goes away.
#[uniffi::export]
pub fn save_project_schedules(
    project: String,
    schedules: Vec<ProjectSchedule>,
) -> Result<(), ScheduleError> {
    let schedules = to_core(schedules)?;
    jevision_core::schedules_to_json(&schedules).map_err(invalid)?;
    jevision_core::save_schedules(Path::new(&project), &schedules)
        .map_err(|reason| ScheduleError::Unwritable { reason })
}

/// What keeps `schedule` from running, if anything; `others` are the project's other schedules.
#[uniffi::export]
pub fn schedule_problem(schedule: ProjectSchedule, others: Vec<ProjectSchedule>) -> Option<String> {
    let mut all = match to_core(others) {
        Ok(all) => all,
        Err(e) => return Some(e.to_string()),
    };
    match Schedule::try_from(schedule) {
        Ok(schedule) => all.push(schedule),
        Err(e) => return Some(e.to_string()),
    }
    jevision_core::schedules_to_json(&all).err()
}

/// Local minute (minutes since 1970 in the local time zone) of the first run of `schedule`
/// after `after`; `None` when it never runs.
#[uniffi::export]
pub fn schedule_next_run(schedule: ProjectSchedule, after: i64) -> Option<i64> {
    Schedule::try_from(schedule).ok()?.next_run(after)
}

/// The schedules of `project` due at the local minute `now`, marked as handled so the next
/// check only finds later runs.
#[uniffi::export]
pub fn begin_schedule_check(
    project: String,
    now: i64,
) -> Result<Vec<ProjectSchedule>, ScheduleError> {
    jevision_core::begin_schedule_check(Path::new(&project), now)
        .map(|due| due.into_iter().map(Into::into).collect())
        .map_err(unreadable)
}

/// Adds a scheduled run to the history of `project`.
#[uniffi::export]
pub fn record_scheduled_run(project: String, run: ScheduledRun) -> Result<(), ScheduleError> {
    let outcome = match run.outcome {
        ScheduledOutcome::Finished { passed, failed } => CoreOutcome::Finished { passed, failed },
        ScheduledOutcome::Skipped { reason } => CoreOutcome::Skipped {
            reason: match reason {
                SkipReason::ScreenLocked => jevision_core::SkipReason::ScreenLocked,
                SkipReason::NoUser => jevision_core::SkipReason::NoUser,
                SkipReason::NotAllowed => jevision_core::SkipReason::NotAllowed,
            },
        },
        ScheduledOutcome::Error { message } => CoreOutcome::Error { message },
    };
    jevision_core::record_scheduled_run(
        Path::new(&project),
        jevision_core::ScheduledRun {
            schedule: run.schedule,
            started_at_ms: run.started_at_ms,
            duration_ms: run.duration_ms,
            outcome,
        },
    )
    .map_err(|reason| ScheduleError::Unwritable { reason })
}

/// Scheduled runs of `project` on this computer, newest first.
#[uniffi::export]
pub fn scheduled_runs(project: String) -> Result<Vec<ScheduledRun>, ScheduleError> {
    let history = jevision_core::ScheduleHistory::load(Path::new(&project)).map_err(unreadable)?;
    Ok(history
        .runs
        .into_iter()
        .rev()
        .map(|run| ScheduledRun {
            schedule: run.schedule,
            started_at_ms: run.started_at_ms,
            duration_ms: run.duration_ms,
            outcome: match run.outcome {
                CoreOutcome::Finished { passed, failed } => {
                    ScheduledOutcome::Finished { passed, failed }
                }
                CoreOutcome::Skipped { reason } => ScheduledOutcome::Skipped {
                    reason: match reason {
                        jevision_core::SkipReason::ScreenLocked => SkipReason::ScreenLocked,
                        jevision_core::SkipReason::NoUser => SkipReason::NoUser,
                        jevision_core::SkipReason::NotAllowed => SkipReason::NotAllowed,
                    },
                },
                CoreOutcome::Error { message } => ScheduledOutcome::Error { message },
            },
        })
        .collect())
}

/// Label of the launchd job that runs the schedules of `project` for the app `bundle_id`.
#[uniffi::export]
pub fn schedule_agent_label(bundle_id: String, project: String) -> String {
    jevision_core::schedule_agent_label(&bundle_id, &project)
}

/// The launchd job of the app `bundle_id` that runs `tool schedule run <project>` at every
/// time of `schedules`.
#[uniffi::export]
pub fn schedule_launch_agent(
    bundle_id: String,
    project: String,
    tool: String,
    schedules: Vec<ProjectSchedule>,
) -> Result<LaunchAgent, ScheduleError> {
    let schedules = to_core(schedules)?;
    let label = jevision_core::schedule_agent_label(&bundle_id, &project);
    let program = [tool, "schedule".to_owned(), "run".to_owned(), project];
    let plist = jevision_core::launch_agent_plist(&label, &bundle_id, &program, &schedules);
    Ok(LaunchAgent { label, plist })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nightly() -> ProjectSchedule {
        ProjectSchedule {
            name: "Nightly".into(),
            tests: ScheduledTests::Files {
                paths: vec!["login.flow.json".into()],
            },
            repeats: ScheduleRepeat::Days { days: vec![5, 1] },
            hour: 3,
            minute: 0,
            environment: Some("staging".into()),
            video: Some(VideoMode::Always),
        }
    }

    #[test]
    fn saves_reads_and_checks_schedules() {
        let project =
            std::env::temp_dir().join(format!("jevision-ffi-schedule-{}", std::process::id()));
        std::fs::create_dir_all(&project).unwrap();
        let path = project.to_string_lossy().to_string();
        save_project_schedules(path.clone(), vec![nightly()]).unwrap();
        let mut read = nightly();
        read.repeats = ScheduleRepeat::Days { days: vec![1, 5] };
        assert_eq!(project_schedules(path.clone()).unwrap(), vec![read]);

        let mut bad = nightly();
        bad.repeats = ScheduleRepeat::EveryHours { hours: 5 };
        assert!(matches!(
            save_project_schedules(path.clone(), vec![bad.clone()]),
            Err(ScheduleError::Invalid { .. })
        ));
        assert!(schedule_problem(bad, vec![]).is_some());
        assert!(
            schedule_problem(nightly(), vec![nightly()])
                .unwrap()
                .contains("two schedules")
        );
        assert_eq!(schedule_problem(nightly(), vec![]), None);

        // Monday 2026-10-19 03:00 local is due; then the history says so.
        let monday = 20_745 * 24 * 60 + 3 * 60;
        assert_eq!(schedule_next_run(nightly(), monday - 1), Some(monday));
        assert_eq!(begin_schedule_check(path.clone(), monday).unwrap().len(), 1);
        record_scheduled_run(
            path.clone(),
            ScheduledRun {
                schedule: "Nightly".into(),
                started_at_ms: 1,
                duration_ms: 0,
                outcome: ScheduledOutcome::Skipped {
                    reason: SkipReason::ScreenLocked,
                },
            },
        )
        .unwrap();
        assert_eq!(
            scheduled_runs(path.clone()).unwrap()[0].outcome,
            ScheduledOutcome::Skipped {
                reason: SkipReason::ScreenLocked
            }
        );

        let agent = schedule_launch_agent(
            "io.example".into(),
            path.clone(),
            "/jevision".into(),
            vec![nightly()],
        )
        .unwrap();
        assert_eq!(
            agent.label,
            schedule_agent_label("io.example".into(), path.clone())
        );
        assert!(agent.plist.contains("<string>schedule</string>"));
        std::fs::remove_dir_all(&project).unwrap();
    }
}
