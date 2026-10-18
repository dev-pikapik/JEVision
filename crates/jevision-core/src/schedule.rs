//! Scheduled runs of a project: which tests run, when, in which environment and whether a
//! video is kept. Schedules live in `jevision.schedule.json` in the project folder. What came
//! of each scheduled run is kept on this computer in `.jevision/schedule-history.json`.
//!
//! Times are local wall-clock minutes since 1970-01-01 00:00: the caller adds the offset of
//! its time zone, so "every day at 09:00" stays at nine o'clock across daylight saving time.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::Path;
use std::{fs, io};

use serde::{Deserialize, Serialize};

use crate::env::is_environment_name;

/// Name of the schedule file in the project folder.
pub const SCHEDULE_FILE: &str = "jevision.schedule.json";
/// Format of the schedule file written by this build.
pub const SCHEDULE_FORMAT: u32 = 1;
/// What came of scheduled runs, relative to the project folder; not committed.
pub const SCHEDULE_HISTORY_FILE: &str = ".jevision/schedule-history.json";
/// Scheduled runs the history keeps, newest ones.
pub const SCHEDULE_HISTORY_KEPT: usize = 200;
/// A run missed while the computer slept or was off is made up once it wakes, unless it is
/// later than this, in minutes; then it waits for its next time.
pub const MAX_LATE_MINUTES: i64 = 12 * 60;
/// Hours between runs `everyHours` may have: the ones that divide a day evenly.
pub const EVERY_HOURS: [u8; 7] = [1, 2, 3, 4, 6, 8, 12];
/// Video choices of a schedule, as on the command line.
pub const SCHEDULE_VIDEO_MODES: [&str; 3] = ["off", "on-failure", "always"];
const MAX_NAME_LEN: usize = 64;
const DAY: i64 = 24 * 60;

/// One schedule: these tests, at these times.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Schedule {
    /// Unique within the project; names the schedule in run history.
    pub name: String,
    pub tests: ScheduleTests,
    pub repeat: Repeat,
    /// Time of day of the run; for `everyHours`, the run the others count from.
    pub at: TimeOfDay,
    /// Environment of the project's environments file; absent uses its default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    /// `off`, `on-failure` or `always`; absent follows the app's setting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video: Option<String>,
}

/// Which tests a schedule runs. Paths are relative to the project folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum ScheduleTests {
    /// Every test of the project.
    All,
    /// Every test in a folder and its subfolders.
    Folder(String),
    /// These test files.
    Files(Vec<String>),
}

/// On which days and how often a schedule runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum Repeat {
    /// Every day at `at`.
    Daily,
    /// These days of the week at `at`.
    Days(Vec<Weekday>),
    /// Every so many hours, around the clock, in step with `at`.
    EveryHours(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Weekday {
    Sun,
    Mon,
    Tue,
    Wed,
    Thu,
    Fri,
    Sat,
}

impl Weekday {
    /// Sunday first, as launchd counts them.
    pub const ALL: [Weekday; 7] = [
        Self::Sun,
        Self::Mon,
        Self::Tue,
        Self::Wed,
        Self::Thu,
        Self::Fri,
        Self::Sat,
    ];

    /// 0 for Sunday to 6 for Saturday.
    pub fn number(self) -> u8 {
        self as u8
    }

    pub fn from_number(number: u8) -> Option<Self> {
        Self::ALL.get(usize::from(number)).copied()
    }

    /// The day of the week of a day counted from 1970-01-01, a Thursday.
    fn of(day: i64) -> Self {
        Self::ALL[(day + 4).rem_euclid(7) as usize]
    }
}

/// A time of day, written `HH:MM`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct TimeOfDay {
    pub hour: u8,
    pub minute: u8,
}

impl TimeOfDay {
    pub fn new(hour: u8, minute: u8) -> Option<Self> {
        (hour < 24 && minute < 60).then_some(Self { hour, minute })
    }

    fn minutes(self) -> i64 {
        i64::from(self.hour) * 60 + i64::from(self.minute)
    }
}

impl TryFrom<String> for TimeOfDay {
    type Error = String;

    fn try_from(text: String) -> Result<Self, String> {
        let bad = || format!("\"{text}\" isn't a time of day; write it as HH:MM, such as 09:30");
        let (hour, minute) = text.split_once(':').ok_or_else(bad)?;
        let number = |part: &str| {
            (part.len() == 2 && part.bytes().all(|b| b.is_ascii_digit()))
                .then(|| part.parse::<u8>().ok())
                .flatten()
        };
        number(hour)
            .zip(number(minute))
            .and_then(|(hour, minute)| Self::new(hour, minute))
            .ok_or_else(bad)
    }
}

impl From<TimeOfDay> for String {
    fn from(time: TimeOfDay) -> Self {
        format!("{:02}:{:02}", time.hour, time.minute)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    format: u32,
    schedules: Vec<Schedule>,
}

impl Schedule {
    /// What is wrong with the schedule, if anything.
    pub fn problem(&self) -> Option<String> {
        let name = &self.name;
        if name.trim().is_empty() {
            return Some("a schedule has no name".to_owned());
        }
        if name.trim() != name
            || name.chars().count() > MAX_NAME_LEN
            || name.chars().any(char::is_control)
        {
            return Some(format!(
                "\"{name}\" isn't a valid schedule name; use up to {MAX_NAME_LEN} characters without spaces around them"
            ));
        }
        let problem = |what: String| Some(format!("schedule \"{name}\": {what}"));
        match &self.tests {
            ScheduleTests::All => {}
            ScheduleTests::Folder(folder) => {
                if !is_relative_path(folder) {
                    return problem(format!(
                        "\"{folder}\" isn't a folder inside the project; write it relative to the project folder"
                    ));
                }
            }
            ScheduleTests::Files(files) => {
                if files.is_empty() {
                    return problem("\"files\" names no tests".to_owned());
                }
                if let Some(file) = files
                    .iter()
                    .find(|f| !is_relative_path(f) || !f.ends_with(".flow.json"))
                {
                    return problem(format!(
                        "\"{file}\" isn't a test file (*.flow.json) inside the project; write it relative to the project folder"
                    ));
                }
                if files.iter().collect::<BTreeSet<_>>().len() != files.len() {
                    return problem("a test is listed twice".to_owned());
                }
            }
        }
        match &self.repeat {
            Repeat::Daily => {}
            Repeat::Days(days) => {
                if days.is_empty() {
                    return problem("\"days\" names no days".to_owned());
                }
                if days.iter().collect::<BTreeSet<_>>().len() != days.len() {
                    return problem("a day is listed twice".to_owned());
                }
            }
            Repeat::EveryHours(hours) => {
                if !EVERY_HOURS.contains(hours) {
                    let allowed: Vec<_> = EVERY_HOURS.iter().map(u8::to_string).collect();
                    return problem(format!(
                        "\"everyHours\" must be one of {}, not {hours}",
                        allowed.join(", ")
                    ));
                }
            }
        }
        if let Some(environment) = self
            .environment
            .as_deref()
            .filter(|e| !is_environment_name(e))
        {
            return problem(format!(
                "\"{environment}\" isn't a valid environment name; use letters, digits, - and _"
            ));
        }
        if let Some(video) = self
            .video
            .as_deref()
            .filter(|v| !SCHEDULE_VIDEO_MODES.contains(v))
        {
            return problem(format!(
                "\"video\" must be {}, not \"{video}\"",
                SCHEDULE_VIDEO_MODES.join(", ")
            ));
        }
        None
    }

    /// Minutes after midnight of the runs of a day it runs on, ascending.
    fn times(&self) -> Vec<i64> {
        let at = self.at.minutes();
        match self.repeat {
            Repeat::EveryHours(hours) => {
                let step = i64::from(hours.max(1)) * 60;
                (at % step..DAY).step_by(step as usize).collect()
            }
            Repeat::Daily | Repeat::Days(_) => vec![at],
        }
    }

    fn runs_on(&self, day: i64) -> bool {
        match &self.repeat {
            Repeat::Days(days) => days.contains(&Weekday::of(day)),
            Repeat::Daily | Repeat::EveryHours(_) => true,
        }
    }

    /// Every run within a week of `day`, in either direction, in time order.
    fn runs_around(&self, day: i64) -> impl DoubleEndedIterator<Item = i64> {
        let times = self.times();
        (day - 7..=day + 7)
            .filter(|d| self.runs_on(*d))
            .flat_map(move |d| times.clone().into_iter().map(move |t| d * DAY + t))
    }

    /// Local minute of the first run after `after`; `None` when it never runs.
    pub fn next_run(&self, after: i64) -> Option<i64> {
        self.runs_around(after.div_euclid(DAY))
            .find(|run| *run > after)
    }

    /// Local minute of the last run at or before `now`; `None` when it never runs.
    pub fn last_run(&self, now: i64) -> Option<i64> {
        self.runs_around(now.div_euclid(DAY))
            .rev()
            .find(|run| *run <= now)
    }
}

/// Whether `path` names something inside the project folder: relative, without `..`.
fn is_relative_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

/// Problems shared by the schedules of a project: names must differ.
fn check(schedules: &[Schedule]) -> Result<(), String> {
    if let Some(problem) = schedules.iter().find_map(Schedule::problem) {
        return Err(problem);
    }
    let mut names = BTreeSet::new();
    if let Some(twice) = schedules
        .iter()
        .find(|s| !names.insert(s.name.to_lowercase()))
    {
        return Err(format!(
            "two schedules are named \"{}\"; give each its own name",
            twice.name
        ));
    }
    Ok(())
}

/// Reads the text of a schedule file; `Err` says what is wrong with it.
pub fn parse_schedules(text: &str) -> Result<Vec<Schedule>, String> {
    let file: File = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if file.format > SCHEDULE_FORMAT {
        return Err(format!(
            "the file has format {}, this version reads up to {SCHEDULE_FORMAT}",
            file.format
        ));
    }
    if file.format < 1 {
        return Err(format!("\"format\" must be {SCHEDULE_FORMAT}"));
    }
    check(&file.schedules)?;
    Ok(file.schedules)
}

/// The schedules of the project folder `project`; a project without a schedule file has none.
pub fn load_schedules(project: &Path) -> Result<Vec<Schedule>, String> {
    match fs::read_to_string(project.join(SCHEDULE_FILE)) {
        Ok(text) => parse_schedules(&text),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e.to_string()),
    }
}

/// The schedule file for `schedules`: days in week order, two-space indentation and a
/// trailing newline, so edits make small diffs.
pub fn schedules_to_json(schedules: &[Schedule]) -> Result<String, String> {
    check(schedules)?;
    let mut schedules = schedules.to_vec();
    for schedule in &mut schedules {
        if let Repeat::Days(days) = &mut schedule.repeat {
            days.sort();
        }
    }
    let file = File {
        format: SCHEDULE_FORMAT,
        schedules,
    };
    let mut text = serde_json::to_string_pretty(&file).map_err(|e| e.to_string())?;
    text.push('\n');
    Ok(text)
}

/// Writes the schedule file of `project`; without schedules the file goes away.
pub fn save_schedules(project: &Path, schedules: &[Schedule]) -> Result<(), String> {
    let file = project.join(SCHEDULE_FILE);
    if schedules.is_empty() {
        return match fs::remove_file(&file) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        };
    }
    write_atomically(&file, &schedules_to_json(schedules)?)
}

fn write_atomically(file: &Path, text: &str) -> Result<(), String> {
    let temporary = file.with_extension("json.tmp");
    fs::write(&temporary, text)
        .and_then(|()| fs::rename(&temporary, file))
        .map_err(|e| e.to_string())
}

/// The schedules with a run in `(since, now]`, not counting runs later than
/// [`MAX_LATE_MINUTES`]. Each one is due once, however many of its runs were missed.
pub fn due_schedules(schedules: &[Schedule], since: i64, now: i64) -> Vec<Schedule> {
    let from = since.max(now - MAX_LATE_MINUTES);
    schedules
        .iter()
        .filter(|s| s.last_run(now).is_some_and(|run| run > from))
        .cloned()
        .collect()
}

/// A time launchd starts the job at; without a weekday, every day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CalendarInterval {
    pub weekday: Option<Weekday>,
    pub hour: u8,
    pub minute: u8,
}

/// Every time any of `schedules` runs, without repeats.
pub fn calendar_intervals(schedules: &[Schedule]) -> Vec<CalendarInterval> {
    let mut all = BTreeSet::new();
    for schedule in schedules {
        let days: Vec<Option<Weekday>> = match &schedule.repeat {
            Repeat::Days(days) => days.iter().copied().map(Some).collect(),
            Repeat::Daily | Repeat::EveryHours(_) => vec![None],
        };
        for time in schedule.times() {
            for &weekday in &days {
                all.insert(CalendarInterval {
                    weekday,
                    hour: (time / 60) as u8,
                    minute: (time % 60) as u8,
                });
            }
        }
    }
    let daily: BTreeSet<(u8, u8)> = all
        .iter()
        .filter(|i| i.weekday.is_none())
        .map(|i| (i.hour, i.minute))
        .collect();
    all.into_iter()
        .filter(|i| i.weekday.is_none() || !daily.contains(&(i.hour, i.minute)))
        .collect()
}

/// Label of the launchd job that runs the schedules of `project` for the app `bundle_id`.
pub fn schedule_agent_label(bundle_id: &str, project: &str) -> String {
    // FNV-1a: the same project gives the same label in every build.
    let hash = project
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        });
    format!("{bundle_id}.schedule.{hash:016x}")
}

/// The property list of a per-user launchd job that starts `program` (the program and its
/// arguments) at every time of `schedules`.
pub fn launch_agent_plist(
    label: &str,
    bundle_id: &str,
    program: &[String],
    schedules: &[Schedule],
) -> String {
    let string = |text: &str| format!("<string>{}</string>", xml_escape(text));
    let mut plist = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n<dict>\n",
    );
    let _ = writeln!(plist, "\t<key>Label</key>\n\t{}", string(label));
    let _ = writeln!(
        plist,
        "\t<key>AssociatedBundleIdentifiers</key>\n\t<array>\n\t\t{}\n\t</array>",
        string(bundle_id)
    );
    plist.push_str("\t<key>ProgramArguments</key>\n\t<array>\n");
    for argument in program {
        let _ = writeln!(plist, "\t\t{}", string(argument));
    }
    plist.push_str("\t</array>\n\t<key>StartCalendarInterval</key>\n\t<array>\n");
    for interval in calendar_intervals(schedules) {
        plist.push_str("\t\t<dict>\n");
        let _ = writeln!(
            plist,
            "\t\t\t<key>Hour</key>\n\t\t\t<integer>{}</integer>\n\t\t\t<key>Minute</key>\n\t\t\t<integer>{}</integer>",
            interval.hour, interval.minute
        );
        if let Some(weekday) = interval.weekday {
            let _ = writeln!(
                plist,
                "\t\t\t<key>Weekday</key>\n\t\t\t<integer>{}</integer>",
                weekday.number()
            );
        }
        plist.push_str("\t\t</dict>\n");
    }
    // Tests drive the user interface: launchd must not slow them down like a background job.
    plist.push_str(
        "\t</array>\n\t<key>ProcessType</key>\n\t<string>Interactive</string>\n</dict>\n</plist>\n",
    );
    plist
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// What came of one scheduled run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledRun {
    /// Name of the schedule.
    pub schedule: String,
    /// Milliseconds since 1970, UTC.
    pub started_at_ms: i64,
    pub duration_ms: u64,
    pub outcome: ScheduledOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "camelCase")]
pub enum ScheduledOutcome {
    /// The tests ran; `failed` counts every test that didn't pass.
    Finished { passed: u32, failed: u32 },
    /// Nothing ran, so nothing touched the computer.
    Skipped { reason: SkipReason },
    /// The tests couldn't start, such as with an environment the project doesn't have.
    Error { message: String },
}

/// Why a scheduled run didn't start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SkipReason {
    /// The screen was locked: input would go to the lock screen.
    ScreenLocked,
    /// Nobody was logged in with a screen.
    NoUser,
    /// macOS hasn't allowed the tool to record the screen or control the computer.
    NotAllowed,
}

/// Scheduled runs of a project on this computer, and up to when schedules were checked.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleHistory {
    /// Local minute schedules were last checked at; runs up to it are done or missed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked_at: Option<i64>,
    /// Oldest first.
    #[serde(default)]
    pub runs: Vec<ScheduledRun>,
}

impl ScheduleHistory {
    /// The history of `project`. It is only a record of this computer, so one that can't be
    /// read as JSON starts over instead of stopping the schedules.
    pub fn load(project: &Path) -> Result<Self, String> {
        match fs::read_to_string(project.join(SCHEDULE_HISTORY_FILE)) {
            Ok(text) => Ok(serde_json::from_str(&text).unwrap_or_default()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn save(&self, project: &Path) -> Result<(), String> {
        let file = project.join(SCHEDULE_HISTORY_FILE);
        if let Some(folder) = file.parent() {
            fs::create_dir_all(folder).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        write_atomically(&file, &text)
    }
}

/// The schedules of `project` due at the local minute `now`, marking them as handled: the
/// next check only finds runs after `now`. The first check looks a few minutes back, for the
/// launchd job that started it.
pub fn begin_schedule_check(project: &Path, now: i64) -> Result<Vec<Schedule>, String> {
    let schedules = load_schedules(project)?;
    let mut history = ScheduleHistory::load(project)?;
    let since = history.checked_at.unwrap_or(now - 5).min(now);
    history.checked_at = Some(now);
    history.save(project)?;
    Ok(due_schedules(&schedules, since, now))
}

/// Adds a scheduled run to the history of `project`, forgetting the oldest beyond
/// [`SCHEDULE_HISTORY_KEPT`].
pub fn record_scheduled_run(project: &Path, run: ScheduledRun) -> Result<(), String> {
    let mut history = ScheduleHistory::load(project)?;
    history.runs.push(run);
    let extra = history.runs.len().saturating_sub(SCHEDULE_HISTORY_KEPT);
    history.runs.drain(..extra);
    history.save(project)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Local minute of a date and time, after Howard Hinnant's `days_from_civil`.
    fn at(year: i64, month: i64, day: i64, hour: i64, minute: i64) -> i64 {
        let y = if month <= 2 { year - 1 } else { year };
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        (era * 146_097 + doe - 719_468) * DAY + hour * 60 + minute
    }

    fn schedule(repeat: Repeat, hour: u8, minute: u8) -> Schedule {
        Schedule {
            name: "Nightly".to_owned(),
            tests: ScheduleTests::All,
            repeat,
            at: TimeOfDay::new(hour, minute).unwrap(),
            environment: None,
            video: None,
        }
    }

    #[test]
    fn reads_and_writes_the_file() {
        let text = r#"{
  "format": 1,
  "schedules": [
    {
      "name": "Smoke",
      "tests": {
        "files": [
          "login.flow.json",
          "checkout/pay.flow.json"
        ]
      },
      "repeat": {
        "days": [
          "mon",
          "fri"
        ]
      },
      "at": "09:05",
      "environment": "staging",
      "video": "always"
    },
    {
      "name": "Nightly",
      "tests": "all",
      "repeat": "daily",
      "at": "03:00"
    },
    {
      "name": "Checkout",
      "tests": {
        "folder": "checkout"
      },
      "repeat": {
        "everyHours": 6
      },
      "at": "01:30"
    }
  ]
}
"#;
        let schedules = parse_schedules(text).unwrap();
        assert_eq!(schedules.len(), 3);
        assert_eq!(
            schedules[0].tests,
            ScheduleTests::Files(vec![
                "login.flow.json".into(),
                "checkout/pay.flow.json".into()
            ])
        );
        assert_eq!(
            schedules[0].repeat,
            Repeat::Days(vec![Weekday::Mon, Weekday::Fri])
        );
        assert_eq!(schedules[0].at, TimeOfDay::new(9, 5).unwrap());
        assert_eq!(schedules[2].tests, ScheduleTests::Folder("checkout".into()));
        assert_eq!(schedules_to_json(&schedules).unwrap(), text);

        let mut unsorted = schedules.clone();
        unsorted[0].repeat = Repeat::Days(vec![Weekday::Fri, Weekday::Mon]);
        assert_eq!(schedules_to_json(&unsorted).unwrap(), text);
    }

    #[test]
    fn rejects_what_it_cannot_run() {
        let file = |schedule: &str| {
            parse_schedules(&format!(r#"{{"format": 1, "schedules": [{schedule}]}}"#)).unwrap_err()
        };
        let base = r#""name": "A", "tests": "all", "repeat": "daily""#;
        assert!(file(&format!(r#"{{{base}, "at": "9:00"}}"#)).contains("HH:MM"));
        assert!(file(&format!(r#"{{{base}, "at": "24:00"}}"#)).contains("HH:MM"));
        assert!(file(&format!(r#"{{{base}, "at": "09:00", "when": 1}}"#)).contains("unknown"));
        assert!(
            file(r#"{"name": "A", "tests": "all", "repeat": {"everyHours": 5}, "at": "00:00"}"#)
                .contains("one of 1, 2, 3, 4, 6, 8, 12")
        );
        assert!(
            file(r#"{"name": "A", "tests": "all", "repeat": {"days": []}, "at": "00:00"}"#)
                .contains("no days")
        );
        assert!(
            file(r#"{"name": "A", "tests": {"folder": "../x"}, "repeat": "daily", "at": "00:00"}"#)
                .contains("inside the project")
        );
        assert!(
            file(
                r#"{"name": "A", "tests": {"files": ["a.json"]}, "repeat": "daily", "at": "00:00"}"#
            )
            .contains("test file")
        );
        assert!(file(&format!(r#"{{{base}, "at": "09:00", "video": "never"}}"#)).contains("video"));
        assert!(
            file(&format!(
                r#"{{{base}, "at": "09:00", "environment": "a b"}}"#
            ))
            .contains("environment")
        );
        assert!(
            file(r#"{"name": " ", "tests": "all", "repeat": "daily", "at": "00:00"}"#)
                .contains("no name")
        );
        let twice = format!(
            r#"{{{base}, "at": "09:00"}}, {{"name": "a", "tests": "all", "repeat": "daily", "at": "10:00"}}"#
        );
        assert!(file(&twice).contains("two schedules"));
        assert!(
            parse_schedules(r#"{"format": 2, "schedules": []}"#)
                .unwrap_err()
                .contains("format 2")
        );
    }

    #[test]
    fn finds_the_next_and_the_last_run() {
        // 2026-10-19 is a Monday.
        let monday_nine = at(2026, 10, 19, 9, 0);
        let weekdays = schedule(
            Repeat::Days(vec![
                Weekday::Mon,
                Weekday::Tue,
                Weekday::Wed,
                Weekday::Thu,
                Weekday::Fri,
            ]),
            9,
            0,
        );
        assert_eq!(weekdays.next_run(monday_nine - 1), Some(monday_nine));
        assert_eq!(weekdays.next_run(monday_nine), Some(at(2026, 10, 20, 9, 0)));
        let friday = at(2026, 10, 23, 12, 0);
        assert_eq!(weekdays.next_run(friday), Some(at(2026, 10, 26, 9, 0)));
        assert_eq!(
            weekdays.last_run(at(2026, 10, 25, 23, 0)),
            Some(at(2026, 10, 23, 9, 0))
        );

        let hourly = schedule(Repeat::EveryHours(6), 1, 30);
        assert_eq!(
            hourly.next_run(at(2026, 10, 19, 1, 30)),
            Some(at(2026, 10, 19, 7, 30))
        );
        assert_eq!(
            hourly.next_run(at(2026, 10, 19, 20, 0)),
            Some(at(2026, 10, 20, 1, 30))
        );
        assert_eq!(
            hourly.last_run(at(2026, 10, 19, 1, 0)),
            Some(at(2026, 10, 18, 19, 30))
        );

        let daily = schedule(Repeat::Daily, 0, 0);
        assert_eq!(
            daily.next_run(at(2026, 12, 31, 23, 59)),
            Some(at(2027, 1, 1, 0, 0))
        );
        // Before 1970 too.
        assert_eq!(daily.last_run(-1), Some(-DAY));
    }

    #[test]
    fn a_schedule_is_due_once_and_not_too_late() {
        let nine = at(2026, 10, 19, 9, 0);
        let daily = schedule(Repeat::Daily, 9, 0);
        let all = [daily];
        assert_eq!(due_schedules(&all, nine - 5, nine).len(), 1);
        assert!(due_schedules(&all, nine, nine + 60).is_empty());
        assert!(due_schedules(&all, nine - 5, nine - 1).is_empty());
        // Woke up three hours later: made up once.
        assert_eq!(due_schedules(&all, nine - 600, nine + 180).len(), 1);
        // Off for two days: too late, waits for tomorrow.
        assert!(due_schedules(&all, nine - 2 * DAY, nine + 13 * 60).is_empty());
    }

    #[test]
    fn launchd_starts_the_job_at_every_run() {
        let mut weekly = schedule(Repeat::Days(vec![Weekday::Sun, Weekday::Mon]), 9, 0);
        weekly.name = "Weekly".into();
        let mut other = schedule(Repeat::Days(vec![Weekday::Sun]), 10, 15);
        other.name = "Other".into();
        let all = [schedule(Repeat::EveryHours(12), 9, 0), weekly, other];
        let at = |weekday, hour, minute| CalendarInterval {
            weekday,
            hour,
            minute,
        };
        assert_eq!(
            calendar_intervals(&all),
            vec![
                at(None, 9, 0),
                at(None, 21, 0),
                at(Some(Weekday::Sun), 10, 15),
            ]
        );

        let label = schedule_agent_label("io.example.app", "/Users/a/Tests & <More>");
        assert_eq!(
            label,
            schedule_agent_label("io.example.app", "/Users/a/Tests & <More>")
        );
        assert_ne!(
            label,
            schedule_agent_label("io.example.app", "/Users/a/Other")
        );
        assert!(label.starts_with("io.example.app.schedule."));
        let plist = launch_agent_plist(
            &label,
            "io.example.app",
            &[
                "/App/jevision".into(),
                "schedule".into(),
                "run".into(),
                "/Users/a/Tests & <More>".into(),
            ],
            &all,
        );
        assert!(plist.contains("<string>/Users/a/Tests &amp; &lt;More&gt;</string>"));
        assert!(plist.contains(&format!("<key>Label</key>\n\t<string>{label}</string>")));
        assert!(plist.contains("<key>Weekday</key>\n\t\t\t<integer>0</integer>"));
        assert_eq!(plist.matches("<key>Hour</key>").count(), 3);
    }

    #[test]
    fn keeps_the_history_of_scheduled_runs() {
        let project =
            std::env::temp_dir().join(format!("jevision-schedule-{}", std::process::id()));
        fs::create_dir_all(&project).unwrap();
        let nine = at(2026, 10, 19, 9, 0);
        save_schedules(&project, &[schedule(Repeat::Daily, 9, 0)]).unwrap();
        assert_eq!(begin_schedule_check(&project, nine).unwrap().len(), 1);
        assert!(begin_schedule_check(&project, nine + 1).unwrap().is_empty());
        assert_eq!(
            ScheduleHistory::load(&project).unwrap().checked_at,
            Some(nine + 1)
        );

        for i in 0..SCHEDULE_HISTORY_KEPT + 2 {
            let outcome = if i == 0 {
                ScheduledOutcome::Skipped {
                    reason: SkipReason::ScreenLocked,
                }
            } else {
                ScheduledOutcome::Finished {
                    passed: 2,
                    failed: 1,
                }
            };
            record_scheduled_run(
                &project,
                ScheduledRun {
                    schedule: "Nightly".into(),
                    started_at_ms: i as i64,
                    duration_ms: 10,
                    outcome,
                },
            )
            .unwrap();
        }
        let history = ScheduleHistory::load(&project).unwrap();
        assert_eq!(history.runs.len(), SCHEDULE_HISTORY_KEPT);
        assert_eq!(history.runs[0].started_at_ms, 2);

        fs::write(project.join(SCHEDULE_HISTORY_FILE), "{").unwrap();
        assert_eq!(
            ScheduleHistory::load(&project).unwrap(),
            ScheduleHistory::default()
        );

        save_schedules(&project, &[]).unwrap();
        assert!(!project.join(SCHEDULE_FILE).exists());
        assert!(load_schedules(&project).unwrap().is_empty());
        fs::remove_dir_all(&project).unwrap();
    }
}
