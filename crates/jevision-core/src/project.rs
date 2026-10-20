//! Settings a project shares with everyone who runs its tests, kept in `jevision.project.json`
//! in the project folder and committed with the tests: how many times a failed test is retried
//! when its file doesn't say, and the tests in quarantine, each until a date if it has one. A
//! quarantined test still runs, but its failure doesn't fail Run All, the command line or the
//! JUnit report; once the date is past the test counts as any other. The file also says
//! how private the project's screens are: areas covered in every screenshot before it is saved
//! or sent anywhere, and whether screenshots may go to AI models outside this computer, and
//! which folders run their tests side by side rather than one after another.

use std::path::{Component, Path};
use std::{fs, io};

use chrono::{Days, NaiveDate};
use serde::{Deserialize, Serialize};

use crate::Rect;
use crate::batch::{DEFAULT_PARALLEL, MAX_PARALLEL, RunMode};
use crate::model::TEST_SUFFIX;
use crate::validate::{MAX_RETRIES, rect_fits};

/// Name of the project settings file in the project folder.
pub const PROJECT_FILE: &str = "jevision.project.json";
/// Format of the project settings file written by this build.
pub const PROJECT_FORMAT: u32 = 1;
/// Days a new quarantine lasts unless someone picks another end.
pub const QUARANTINE_DAYS: u64 = 14;
/// Flaky runs of a test in a week after which its quarantine is suggested, unless the project
/// says otherwise.
pub const DEFAULT_QUARANTINE_AFTER: u32 = 3;
/// Most flaky runs a project can ask for before quarantine is suggested.
pub const MAX_QUARANTINE_AFTER: u32 = 10;

/// The settings of a project.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProjectSettings {
    /// How many times a failed test is retried when its file doesn't say; `None` retries none.
    pub retries: Option<u32>,
    /// Tests in quarantine, sorted by file; entries whose end date is past are kept until
    /// someone releases them.
    pub quarantine: Vec<Quarantined>,
    /// Flaky runs of a test within 7 days after which quarantine is suggested; `None` for
    /// [`DEFAULT_QUARANTINE_AFTER`].
    pub suggest_quarantine_after: Option<u32>,
    /// Screenshots go only to AI models running on this computer; checks that would need a
    /// cloud service fail instead.
    pub local_only: bool,
    /// Areas of the window, in fractions from its top-left corner, covered in every screenshot
    /// before it is saved or sent to an AI model.
    pub hide: Vec<Rect>,
    /// How the tests of folders run, set explicitly, sorted by folder. A folder without an
    /// entry runs like the nearest folder above it that has one; the project folder, without
    /// one, runs one test after another.
    pub run_modes: Vec<FolderRunMode>,
}

/// How the tests of a folder run when the folder, or a folder above it, is run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderRunMode {
    /// Relative to the project folder, with `/` between folders; `.` is the project folder.
    pub folder: String,
    pub mode: RunMode,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RunModeEntry {
    folder: String,
    run: RunKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    limit: Option<u32>,
}

#[derive(Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
enum RunKind {
    Sequential,
    Parallel,
}

/// A test in quarantine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Quarantined {
    /// Test file, relative to the project folder, with `/` between folders.
    pub test: String,
    /// Why the test is in quarantine, such as a link to the bug that makes it fail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Last day the test is in quarantine, as `YYYY-MM-DD`; absent for a quarantine without an
    /// end.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<String>,
}

impl Quarantined {
    /// Whether the quarantine is in force on `today`, as `YYYY-MM-DD`: it has no end, or its
    /// last day isn't past.
    pub fn is_active(&self, today: &str) -> bool {
        match (self.until.as_deref().and_then(date), date(today)) {
            (Some(until), Some(today)) => until >= today,
            _ => true,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    format: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    retries: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    quarantine: Vec<Quarantined>,
    #[serde(
        default,
        rename = "suggestQuarantineAfter",
        skip_serializing_if = "Option::is_none"
    )]
    suggest_quarantine_after: Option<u32>,
    #[serde(
        default,
        rename = "localOnly",
        skip_serializing_if = "std::ops::Not::not"
    )]
    local_only: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    hide: Vec<Rect>,
    #[serde(default, rename = "runModes", skip_serializing_if = "Vec::is_empty")]
    run_modes: Vec<RunModeEntry>,
}

impl ProjectSettings {
    /// Reads the text of a project settings file; `Err` says what is wrong with it.
    pub fn parse(text: &str) -> Result<Self, String> {
        let file: File = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if file.format > PROJECT_FORMAT {
            return Err(format!(
                "the file has format {}, this version reads up to {PROJECT_FORMAT}",
                file.format
            ));
        }
        if file.format < 1 {
            return Err(format!("\"format\" must be {PROJECT_FORMAT}"));
        }
        if let Some(retries) = file.retries.filter(|r| *r > MAX_RETRIES) {
            return Err(format!(
                "\"retries\" is {retries}; it must be from 0 to {MAX_RETRIES}"
            ));
        }
        if let Some(after) = file
            .suggest_quarantine_after
            .filter(|n| !(1..=MAX_QUARANTINE_AFTER).contains(n))
        {
            return Err(format!(
                "\"suggestQuarantineAfter\" is {after}; it must be from 1 to {MAX_QUARANTINE_AFTER}"
            ));
        }
        let mut quarantine = file.quarantine;
        for entry in &quarantine {
            if !is_test_path(&entry.test) {
                return Err(format!(
                    "\"{}\" in \"quarantine\" isn't a test file of the project; write its path \
                     from the project folder, such as login/sign-in{TEST_SUFFIX}",
                    entry.test
                ));
            }
            if entry.reason.as_deref().is_some_and(|r| r.trim().is_empty()) {
                return Err(format!(
                    "the reason for \"{}\" in \"quarantine\" is empty; write why or leave it out",
                    entry.test
                ));
            }
            if let Some(until) = entry.until.as_deref().filter(|u| date(u).is_none()) {
                return Err(format!(
                    "\"until\" of \"{}\" in \"quarantine\" is \"{until}\"; write a date such as \
                     2026-11-03",
                    entry.test
                ));
            }
        }
        quarantine.sort_by(|a, b| a.test.cmp(&b.test));
        if let Some(twice) = quarantine.windows(2).find(|w| w[0].test == w[1].test) {
            return Err(format!(
                "\"{}\" is in \"quarantine\" more than once",
                twice[0].test
            ));
        }
        check_hide(&file.hide)?;
        let mut run_modes = Vec::with_capacity(file.run_modes.len());
        for entry in file.run_modes {
            run_modes.push(run_mode(entry)?);
        }
        run_modes.sort_by(|a, b| a.folder.cmp(&b.folder));
        if let Some(twice) = run_modes.windows(2).find(|w| w[0].folder == w[1].folder) {
            return Err(format!(
                "the folder \"{}\" is in \"runModes\" more than once",
                twice[0].folder
            ));
        }
        Ok(Self {
            retries: file.retries,
            quarantine,
            suggest_quarantine_after: file.suggest_quarantine_after,
            local_only: file.local_only,
            hide: file.hide,
            run_modes,
        })
    }

    /// Reads the settings file of the project folder `project`; a project without one has the
    /// default settings.
    pub fn load(project: &Path) -> Result<Self, String> {
        match fs::read_to_string(project.join(PROJECT_FILE)) {
            Ok(text) => Self::parse(&text),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.to_string()),
        }
    }

    /// The settings as the text of the file: fixed key order, defaults left out, two-space
    /// indentation and a trailing newline.
    pub fn to_json(&self) -> String {
        let file = File {
            format: PROJECT_FORMAT,
            retries: self.retries,
            quarantine: self.quarantine.clone(),
            suggest_quarantine_after: self.suggest_quarantine_after,
            local_only: self.local_only,
            hide: self.hide.clone(),
            run_modes: self
                .run_modes
                .iter()
                .map(|m| match m.mode {
                    RunMode::Sequential => RunModeEntry {
                        folder: m.folder.clone(),
                        run: RunKind::Sequential,
                        limit: None,
                    },
                    RunMode::Parallel { limit } => RunModeEntry {
                        folder: m.folder.clone(),
                        run: RunKind::Parallel,
                        limit: Some(limit),
                    },
                })
                .collect(),
        };
        let mut text = serde_json::to_string_pretty(&file).expect("settings always serialize");
        text.push('\n');
        text
    }

    /// Writes the settings to the project folder `project`.
    pub fn save(&self, project: &Path) -> io::Result<()> {
        fs::write(project.join(PROJECT_FILE), self.to_json())
    }

    /// The quarantine of the test file `test`, relative to the project folder, if it is in force
    /// on `today`, as `YYYY-MM-DD`; a test whose quarantine ended counts as any other.
    pub fn quarantined(&self, test: &str, today: &str) -> Option<&Quarantined> {
        self.quarantine
            .binary_search_by(|q| q.test.as_str().cmp(test))
            .ok()
            .map(|i| &self.quarantine[i])
            .filter(|q| q.is_active(today))
    }

    /// Puts the test file `test` in quarantine until the end of the day `until`, as
    /// `YYYY-MM-DD`, or without an end, or changes the reason and end of one already there. A
    /// blank reason counts as none; `Err` when `test` isn't a test file path or `until` isn't a
    /// date.
    pub fn quarantine(
        &mut self,
        test: &str,
        reason: Option<&str>,
        until: Option<&str>,
    ) -> Result<(), String> {
        if !is_test_path(test) {
            return Err(format!("\"{test}\" isn't a test file of the project"));
        }
        if let Some(until) = until.filter(|u| date(u).is_none()) {
            return Err(format!(
                "\"{until}\" isn't a date; write one such as 2026-11-03"
            ));
        }
        let entry = Quarantined {
            test: test.to_owned(),
            reason: reason
                .map(str::trim)
                .filter(|r| !r.is_empty())
                .map(str::to_owned),
            until: until.map(str::to_owned),
        };
        match self
            .quarantine
            .binary_search_by(|q| q.test.as_str().cmp(test))
        {
            Ok(i) => self.quarantine[i] = entry,
            Err(i) => self.quarantine.insert(i, entry),
        }
        Ok(())
    }

    /// How many flaky runs of a test in a week make its quarantine worth suggesting.
    pub fn quarantine_threshold(&self) -> u32 {
        self.suggest_quarantine_after
            .unwrap_or(DEFAULT_QUARANTINE_AFTER)
    }

    /// Replaces the areas hidden in every screenshot; `Err` names the first one that doesn't
    /// fit the window.
    pub fn set_hide(&mut self, hide: Vec<Rect>) -> Result<(), String> {
        check_hide(&hide)?;
        self.hide = hide;
        Ok(())
    }

    /// How the tests of `folder`, relative to the project folder (`.` for the project folder),
    /// run: as set for it or for the nearest folder above it, otherwise one after another.
    pub fn run_mode(&self, folder: &str) -> RunMode {
        let mut folder = folder.trim_matches('/');
        loop {
            let key = if folder.is_empty() { "." } else { folder };
            if let Some(m) = self.run_modes.iter().find(|m| m.folder == key) {
                return m.mode;
            }
            if folder.is_empty() || folder == "." {
                return RunMode::Sequential;
            }
            folder = folder.rfind('/').map_or("", |at| &folder[..at]);
        }
    }

    /// Sets how the tests of `folder` run; `Err` when the folder or the limit isn't allowed.
    pub fn set_run_mode(&mut self, folder: &str, mode: RunMode) -> Result<(), String> {
        let entry = run_mode(RunModeEntry {
            folder: folder.to_owned(),
            run: match mode {
                RunMode::Sequential => RunKind::Sequential,
                RunMode::Parallel { .. } => RunKind::Parallel,
            },
            limit: match mode {
                RunMode::Sequential => None,
                RunMode::Parallel { limit } => Some(limit),
            },
        })?;
        match self
            .run_modes
            .binary_search_by(|m| m.folder.as_str().cmp(&entry.folder))
        {
            Ok(i) => self.run_modes[i] = entry,
            Err(i) => self.run_modes.insert(i, entry),
        }
        Ok(())
    }

    /// Takes the test file `test` out of quarantine; `false` when it wasn't in.
    pub fn release(&mut self, test: &str) -> bool {
        let before = self.quarantine.len();
        self.quarantine.retain(|q| q.test != test);
        self.quarantine.len() != before
    }
}

/// `YYYY-MM-DD` as a day.
fn date(text: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(text, "%Y-%m-%d")
        .ok()
        .filter(|_| text.len() == 10)
}

/// The last day of a quarantine that starts `today`, as `YYYY-MM-DD`, lasting
/// [`QUARANTINE_DAYS`]; `None` when `today` isn't a date.
pub fn default_quarantine_until(today: &str) -> Option<String> {
    date(today)?
        .checked_add_days(Days::new(QUARANTINE_DAYS))
        .map(|day| day.format("%Y-%m-%d").to_string())
}

fn check_hide(hide: &[Rect]) -> Result<(), String> {
    match hide.iter().position(|r| !rect_fits(r)) {
        Some(i) => Err(format!(
            "area {} in \"hide\" doesn't fit the window; x, y, width and height are \
             fractions from 0 to 1 of the window, and the area must have a size and end \
             inside it",
            i + 1
        )),
        None => Ok(()),
    }
}

fn run_mode(entry: RunModeEntry) -> Result<FolderRunMode, String> {
    let folder = entry.folder.trim_end_matches('/');
    let valid = folder == "."
        || (!folder.is_empty()
            && !folder.contains('\\')
            && Path::new(folder)
                .components()
                .all(|c| matches!(c, Component::Normal(_))));
    if !valid {
        return Err(format!(
            "\"{}\" in \"runModes\" isn't a folder of the project; write its path from the \
             project folder, such as checkout, or . for the project folder",
            entry.folder
        ));
    }
    let mode = match (entry.run, entry.limit) {
        (RunKind::Sequential, None) => RunMode::Sequential,
        (RunKind::Sequential, Some(_)) => {
            return Err(format!(
                "\"limit\" of \"{folder}\" in \"runModes\" works only with \"run\": \"parallel\""
            ));
        }
        (RunKind::Parallel, limit) => {
            let limit = limit.unwrap_or(DEFAULT_PARALLEL);
            if !(2..=MAX_PARALLEL).contains(&limit) {
                return Err(format!(
                    "\"limit\" of \"{folder}\" in \"runModes\" is {limit}; it must be from 2 to {MAX_PARALLEL}"
                ));
            }
            RunMode::Parallel { limit }
        }
    };
    Ok(FolderRunMode {
        folder: folder.to_owned(),
        mode,
    })
}

/// A relative path to a test file inside the project, with `/` between folders.
fn is_test_path(path: &str) -> bool {
    path.len() > TEST_SUFFIX.len()
        && path.ends_with(TEST_SUFFIX)
        && !path.contains('\\')
        && Path::new(path)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_validates_and_writes_back_sorted() {
        let text = r#"{"format":1,"retries":2,"quarantine":[
            {"test":"z.flow.json"},
            {"test":"a/login.flow.json","reason":"Bug #12"}]}"#;
        let settings = ProjectSettings::parse(text).unwrap();
        assert_eq!(settings.retries, Some(2));
        assert_eq!(
            settings
                .quarantined("a/login.flow.json", "2026-10-20")
                .unwrap()
                .reason,
            Some("Bug #12".into())
        );
        assert!(settings.quarantined("b.flow.json", "2026-10-20").is_none());
        let written = settings.to_json();
        assert!(written.starts_with("{\n  \"format\": 1,\n  \"retries\": 2,\n"));
        assert!(written.find("a/login").unwrap() < written.find("z.flow").unwrap());
        assert_eq!(ProjectSettings::parse(&written).unwrap(), settings);
        assert_eq!(
            ProjectSettings::default().to_json(),
            "{\n  \"format\": 1\n}\n"
        );
    }

    #[test]
    fn rejects_bad_files() {
        for (text, says) in [
            (r#"{"format":2}"#, "reads up to 1"),
            (r#"{"format":0}"#, "\"format\" must be 1"),
            (r#"{"format":1,"retries":11}"#, "from 0 to 10"),
            (r#"{"format":1,"other":1}"#, "unknown field"),
            (r#"{"format":1,"localOnly":"yes"}"#, "expected a boolean"),
            (
                r#"{"format":1,"hide":[{"x":0.5,"y":0,"width":0.6,"height":0.1}]}"#,
                "area 1 in \"hide\"",
            ),
            (
                r#"{"format":1,"hide":[{"x":0,"y":0,"width":0,"height":0.1}]}"#,
                "area 1 in \"hide\"",
            ),
            (r#"{"format":1,"hide":[{"x":0,"y":0}]}"#, "missing field"),
            (
                r#"{"format":1,"quarantine":[{"test":"../x.flow.json"}]}"#,
                "isn't a test file",
            ),
            (
                r#"{"format":1,"quarantine":[{"test":"/x.flow.json"}]}"#,
                "isn't a test file",
            ),
            (
                r#"{"format":1,"quarantine":[{"test":"notes.txt"}]}"#,
                "isn't a test file",
            ),
            (
                r#"{"format":1,"quarantine":[{"test":"x.flow.json","reason":" "}]}"#,
                "is empty",
            ),
            (
                r#"{"format":1,"quarantine":[{"test":"x.flow.json"},{"test":"x.flow.json"}]}"#,
                "more than once",
            ),
            (
                r#"{"format":1,"quarantine":[{"test":"x.flow.json","until":"soon"}]}"#,
                "write a date",
            ),
            (
                r#"{"format":1,"quarantine":[{"test":"x.flow.json","until":"2026-02-30"}]}"#,
                "write a date",
            ),
            (
                r#"{"format":1,"quarantine":[{"test":"x.flow.json","until":"2026-2-3"}]}"#,
                "write a date",
            ),
            (r#"{"format":1,"suggestQuarantineAfter":0}"#, "from 1 to 10"),
            (
                r#"{"format":1,"suggestQuarantineAfter":11}"#,
                "from 1 to 10",
            ),
        ] {
            let error = ProjectSettings::parse(text).unwrap_err();
            assert!(error.contains(says), "{text}: {error}");
        }
    }

    #[test]
    fn keeps_privacy_settings() {
        let text = r#"{"format":1,"localOnly":true,
            "hide":[{"x":0,"y":0,"width":0.2,"height":1}]}"#;
        let settings = ProjectSettings::parse(text).unwrap();
        assert!(settings.local_only);
        assert_eq!(
            settings.hide,
            [Rect {
                x: 0.0,
                y: 0.0,
                width: 0.2,
                height: 1.0
            }]
        );
        let written = settings.to_json();
        assert!(written.contains("\"localOnly\": true"), "{written}");
        assert_eq!(ProjectSettings::parse(&written).unwrap(), settings);
    }

    #[test]
    fn quarantines_and_releases() {
        let mut settings = ProjectSettings::default();
        settings
            .quarantine("b.flow.json", Some("  "), None)
            .unwrap();
        settings
            .quarantine("a.flow.json", Some(" Bug "), None)
            .unwrap();
        settings
            .quarantine("b.flow.json", Some("Slow server"), None)
            .unwrap();
        assert!(settings.quarantine("../c.flow.json", None, None).is_err());
        let tests: Vec<_> = settings
            .quarantine
            .iter()
            .map(|q| (q.test.as_str(), q.reason.as_deref()))
            .collect();
        assert_eq!(
            tests,
            [
                ("a.flow.json", Some("Bug")),
                ("b.flow.json", Some("Slow server"))
            ]
        );
        assert!(settings.release("a.flow.json"));
        assert!(!settings.release("a.flow.json"));
        assert_eq!(settings.quarantine.len(), 1);
    }

    #[test]
    fn a_quarantine_ends_after_its_last_day() {
        let text = r#"{"format":1,"quarantine":[
            {"test":"a.flow.json","until":"2026-11-03"},
            {"test":"b.flow.json"}]}"#;
        let settings = ProjectSettings::parse(text).unwrap();
        let on = |test: &str, day: &str| settings.quarantined(test, day).is_some();
        assert!(on("a.flow.json", "2026-10-20"));
        assert!(on("a.flow.json", "2026-11-03"));
        assert!(!on("a.flow.json", "2026-11-04"));
        assert!(on("b.flow.json", "2099-01-01"));
        // The entry stays in the file until someone releases it.
        assert_eq!(settings.quarantine.len(), 2);
        assert_eq!(
            ProjectSettings::parse(&settings.to_json()).unwrap(),
            settings
        );
        assert_eq!(
            default_quarantine_until("2026-12-25").as_deref(),
            Some("2027-01-08")
        );
        assert_eq!(default_quarantine_until("tomorrow"), None);
        let mut settings = settings;
        assert!(
            settings
                .quarantine("a.flow.json", None, Some("11/03"))
                .is_err()
        );
        settings
            .quarantine("a.flow.json", Some("Bug"), Some("2026-12-01"))
            .unwrap();
        assert!(settings.quarantined("a.flow.json", "2026-12-01").is_some());
        assert!(settings.quarantined("a.flow.json", "2026-12-02").is_none());
    }

    #[test]
    fn projects_choose_when_quarantine_is_suggested() {
        assert_eq!(ProjectSettings::default().quarantine_threshold(), 3);
        let settings =
            ProjectSettings::parse(r#"{"format":1,"suggestQuarantineAfter":5}"#).unwrap();
        assert_eq!(settings.quarantine_threshold(), 5);
        assert!(settings.to_json().contains("\"suggestQuarantineAfter\": 5"));
    }

    #[test]
    fn folders_run_as_set_for_them_or_above_them() {
        let text = r#"{"format": 1, "runModes": [
            {"folder": "checkout/slow", "run": "sequential"},
            {"folder": ".", "run": "parallel"},
            {"folder": "checkout", "run": "parallel", "limit": 8}
        ]}"#;
        let mut settings = ProjectSettings::parse(text).unwrap();
        assert_eq!(
            settings.run_mode("."),
            RunMode::Parallel {
                limit: DEFAULT_PARALLEL
            }
        );
        assert_eq!(
            settings.run_mode("about"),
            RunMode::Parallel {
                limit: DEFAULT_PARALLEL
            }
        );
        assert_eq!(
            settings.run_mode("checkout/cart"),
            RunMode::Parallel { limit: 8 }
        );
        assert_eq!(settings.run_mode("checkout/slow/pay"), RunMode::Sequential);
        assert_eq!(
            ProjectSettings::default().run_mode("checkout"),
            RunMode::Sequential
        );
        settings
            .set_run_mode("checkout", RunMode::Sequential)
            .unwrap();
        assert_eq!(settings.run_mode("checkout/cart"), RunMode::Sequential);
        let saved = ProjectSettings::parse(&settings.to_json()).unwrap();
        assert_eq!(saved, settings);
        assert!(settings.to_json().contains(
            r#""folder": ".",
      "run": "parallel",
      "limit": 4"#
        ));
        for bad in [
            r#"{"format": 1, "runModes": [{"folder": "../x", "run": "parallel"}]}"#,
            r#"{"format": 1, "runModes": [{"folder": "x", "run": "parallel", "limit": 1}]}"#,
            r#"{"format": 1, "runModes": [{"folder": "x", "run": "sequential", "limit": 2}]}"#,
            r#"{"format": 1, "runModes": [{"folder": "x", "run": "sequential"}, {"folder": "x/", "run": "sequential"}]}"#,
        ] {
            assert!(ProjectSettings::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_project_without_the_file_has_defaults() {
        let dir = std::env::temp_dir().join(format!("jevision-project-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        assert_eq!(
            ProjectSettings::load(&dir).unwrap(),
            ProjectSettings::default()
        );
        let mut settings = ProjectSettings {
            retries: Some(1),
            ..Default::default()
        };
        settings
            .quarantine("t.flow.json", None, Some("2026-11-03"))
            .unwrap();
        settings.save(&dir).unwrap();
        assert_eq!(ProjectSettings::load(&dir).unwrap(), settings);
        fs::remove_dir_all(&dir).unwrap();
    }
}
