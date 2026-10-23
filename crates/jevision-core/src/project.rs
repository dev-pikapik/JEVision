//! Settings a project shares with everyone who runs its tests, kept in `jevision.project.json`
//! in the project folder and committed with the tests: how many times a failed test is retried
//! when its file doesn't say, and the tests in quarantine, each until a date if it has one. A
//! quarantined test still runs, but its failure doesn't fail Run All, the command line or the
//! JUnit report; once the date is past the test counts as any other. The file also says
//! how private the project's screens are: areas covered in every screenshot before it is saved
//! or sent anywhere, and whether screenshots may go to AI models outside this computer, which
//! AI the project asks for looking at screens and for writing and explaining tests (provider
//! and model by name; API keys never go in the file), and which folders run their tests side
//! by side rather than one after another, and whether apps a test resets start with an empty
//! home folder of their own.

use std::path::{Component, Path};
use std::{fs, io};

use chrono::{Days, NaiveDate};
use jevision_l10n::tr;
use serde::{Deserialize, Serialize};

use crate::Rect;
use crate::batch::{DEFAULT_PARALLEL, MAX_PARALLEL, RunMode};
use crate::issue::json_problem;
use crate::model::{AI_PROVIDERS, TEST_SUFFIX};
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

/// Most days a project can keep its saved runs, or their videos and screenshots, for.
pub const MAX_RETENTION_DAYS: u32 = 3650;
/// Most saved runs of each test a project can keep.
pub const MAX_RETENTION_RUNS: u32 = 100_000;
/// Most megabytes a project can let its saved runs take.
pub const MAX_RETENTION_MB: u32 = 10_000_000;

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
    /// The AI that checks screens by meaning, when the project doesn't leave it to the app's
    /// settings. A check that names its own provider still asks that one.
    pub ai_screen: Option<AiChoice>,
    /// The AI that drafts tests and explains failed runs, when the project doesn't leave it
    /// to the app's settings.
    pub ai_text: Option<AiChoice>,
    /// How the tests of folders run, set explicitly, sorted by folder. A folder without an
    /// entry runs like the nearest folder above it that has one; the project folder, without
    /// one, runs one test after another.
    pub run_modes: Vec<FolderRunMode>,
    /// An `openApp` block that resets the app's state starts it with an empty home folder of
    /// its own, as on its first launch, instead of only without its last windows. The person's
    /// own settings and files of the app stay as they are. Off unless the project turns it on.
    pub clean_home: bool,
    /// How long saved runs are kept, when the project chose; `None` keeps them as
    /// [`RunRetention::default`] says.
    pub retention: Option<RunRetention>,
}

/// How much of its history a project keeps in `.jevision/runs`. Each limit is `None` for none.
/// Whatever the limits, the latest run of every test and runs still going are kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RunRetention {
    /// Runs that started more than this many days ago are deleted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_age_days: Option<u32>,
    /// Of the runs of one test, only this many of the latest are kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_runs_per_test: Option<u32>,
    /// Videos and screenshots of runs older than this many days are deleted; their reports
    /// stay.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_max_age_days: Option<u32>,
    /// While the saved runs take more megabytes than this, the oldest are deleted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_total_mb: Option<u32>,
}

impl Default for RunRetention {
    /// Reports for 180 days and at most 300 per test, videos and screenshots for 30 days, and
    /// no limit on the space taken.
    fn default() -> Self {
        Self {
            max_age_days: Some(180),
            max_runs_per_test: Some(300),
            media_max_age_days: Some(30),
            max_total_mb: None,
        }
    }
}

impl RunRetention {
    /// `Err` names the first limit that is out of range.
    pub fn check(&self) -> Result<(), String> {
        for (field, value, max) in [
            ("maxAgeDays", self.max_age_days, MAX_RETENTION_DAYS),
            ("maxRunsPerTest", self.max_runs_per_test, MAX_RETENTION_RUNS),
            (
                "mediaMaxAgeDays",
                self.media_max_age_days,
                MAX_RETENTION_DAYS,
            ),
            ("maxTotalMb", self.max_total_mb, MAX_RETENTION_MB),
        ] {
            if let Some(value) = value.filter(|v| !(1..=max).contains(v)) {
                return Err(tr!("project.retention.range", field, value, max));
            }
        }
        Ok(())
    }
}

/// What a project can choose an AI for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiTask {
    /// Checks by meaning, which look at screenshots.
    Screen,
    /// Drafting tests and explaining failed runs, which converse.
    Text,
}

impl AiTask {
    /// The key of the task in the project file, under `ai`.
    fn key(self) -> &'static str {
        match self {
            Self::Screen => "screen",
            Self::Text => "text",
        }
    }

    /// `Err` says why `choice` can't do the task.
    fn check(self, choice: &AiChoice) -> Result<(), String> {
        let key = self.key();
        if !AI_PROVIDERS.contains(&choice.provider.as_str()) {
            return Err(tr!(
                "project.ai.unknownProvider",
                &choice.provider,
                key,
                AI_PROVIDERS.join(", ")
            ));
        }
        if self == Self::Text && choice.provider == "jev" {
            return Err(tr!("project.ai.jevText", key));
        }
        Ok(())
    }
}

/// Which AI to ask: a provider by its id, and one of its models. The key and the address of the
/// provider are the person's own and stay in the app's settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct AiChoice {
    /// One of [`AI_PROVIDERS`]; empty for the one the app's settings choose.
    #[cfg_attr(test, schemars(extend("enum" = AI_PROVIDERS)))]
    pub provider: String,
    /// Empty for the model the settings choose for the provider.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub model: String,
}

impl AiChoice {
    /// The AI a task asks, most specific first: what the block itself says, then the project,
    /// then the app's settings, which an empty provider and an empty model stand for. A model
    /// a block names without a provider is for the provider that is in force.
    pub fn resolve(block: &Self, project: Option<&Self>) -> Self {
        match project {
            Some(p) if block.provider.is_empty() || block.provider == p.provider => Self {
                provider: p.provider.clone(),
                model: if block.model.is_empty() {
                    p.model.clone()
                } else {
                    block.model.clone()
                },
            },
            _ => block.clone(),
        }
    }
}

/// How the tests of a folder run when the folder, or a folder above it, is run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderRunMode {
    /// Relative to the project folder, with `/` between folders; `.` is the project folder.
    pub folder: String,
    pub mode: RunMode,
}

/// How the tests of one folder run.
#[derive(Serialize, Deserialize)]
#[cfg_attr(
    test,
    derive(schemars::JsonSchema),
    schemars(extend(
        "if" = {"properties": {"run": {"const": "sequential"}}},
        "then" = {"properties": {"limit": {"type": "null"}}}
    ))
)]
#[serde(deny_unknown_fields)]
struct RunModeEntry {
    /// Relative to the project folder, with `/` between folders; `.` is the project folder.
    #[cfg_attr(test, schemars(regex(pattern = r"^[^/\\][^\\]*$")))]
    folder: String,
    run: RunKind,
    /// Most tests that run at once, for `parallel` only.
    #[cfg_attr(
        test,
        schemars(range(min = 2, max = MAX_PARALLEL), extend("default" = DEFAULT_PARALLEL))
    )]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    limit: Option<u32>,
}

/// Whether the tests of the folder run one after another or side by side.
#[derive(Serialize, Deserialize, PartialEq)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
enum RunKind {
    /// One test after another.
    Sequential,
    /// Several tests at once, each in its own copy of the application.
    Parallel,
}

/// A test in quarantine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Quarantined {
    /// Test file, relative to the project folder, with `/` between folders.
    #[cfg_attr(test, schemars(regex(pattern = r"^[^/\\][^\\]*\.flow\.json$")))]
    pub test: String,
    /// Why the test is in quarantine, such as a link to the bug that makes it fail.
    #[cfg_attr(test, schemars(regex(pattern = r"\S")))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Last day the test is in quarantine, as `YYYY-MM-DD`; absent for a quarantine without an
    /// end.
    #[cfg_attr(test, schemars(regex(pattern = r"^[0-9]{4}-[0-9]{2}-[0-9]{2}$")))]
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
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub(crate) struct File {
    format: u32,
    /// How many times a failed test is retried when its file doesn't say; without it, none.
    #[cfg_attr(test, schemars(range(max = MAX_RETRIES)))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    retries: Option<u32>,
    /// Tests in quarantine: they still run, but their failure doesn't fail Run All, the command
    /// line or the JUnit report until their last day is past.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    quarantine: Vec<Quarantined>,
    /// Flaky runs of a test within 7 days after which quarantine is suggested.
    #[cfg_attr(
        test,
        schemars(
            range(min = 1, max = MAX_QUARANTINE_AFTER),
            extend("default" = DEFAULT_QUARANTINE_AFTER)
        )
    )]
    #[serde(
        default,
        rename = "suggestQuarantineAfter",
        skip_serializing_if = "Option::is_none"
    )]
    suggest_quarantine_after: Option<u32>,
    /// Screenshots go only to AI models running on this computer; checks that would need a
    /// cloud service fail instead.
    #[serde(
        default,
        rename = "localOnly",
        skip_serializing_if = "std::ops::Not::not"
    )]
    local_only: bool,
    /// Areas of the window, in fractions from its top-left corner, covered in every screenshot
    /// before it is saved or sent to an AI model.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    hide: Vec<Rect>,
    /// The AI the project asks, when it doesn't leave the choice to the app's settings.
    #[serde(default, skip_serializing_if = "FileAi::is_empty")]
    ai: FileAi,
    /// How the tests of folders run. A folder without an entry runs like the nearest folder
    /// above it that has one; the project folder, without one, runs one test after another.
    #[serde(default, rename = "runModes", skip_serializing_if = "Vec::is_empty")]
    run_modes: Vec<RunModeEntry>,
    #[serde(
        default,
        rename = "cleanHome",
        skip_serializing_if = "std::ops::Not::not"
    )]
    clean_home: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    retention: Option<RunRetention>,
}

#[derive(Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
struct FileAi {
    /// The AI that checks screens by meaning. A check that names its own provider still asks
    /// that one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    screen: Option<AiChoice>,
    /// The AI that drafts tests and explains failed runs; `jev` only looks at screens, so it
    /// can't be this one.
    #[cfg_attr(
        test,
        schemars(extend("not" = {
            "type": "object",
            "properties": {"provider": {"const": "jev"}},
            "required": ["provider"]
        }))
    )]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    text: Option<AiChoice>,
}

impl FileAi {
    fn is_empty(&self) -> bool {
        self.screen.is_none() && self.text.is_none()
    }
}

impl ProjectSettings {
    /// Reads the text of a project settings file; `Err` says what is wrong with it.
    pub fn parse(text: &str) -> Result<Self, String> {
        let file: File = serde_json::from_str(text).map_err(|e| json_problem(&e))?;
        if file.format > PROJECT_FORMAT {
            return Err(tr!("project.format.tooNew", file.format, PROJECT_FORMAT));
        }
        if file.format < 1 {
            return Err(tr!("project.format.invalid", PROJECT_FORMAT));
        }
        if let Some(retries) = file.retries.filter(|r| *r > MAX_RETRIES) {
            return Err(tr!("project.retries.range", retries, MAX_RETRIES));
        }
        if let Some(after) = file
            .suggest_quarantine_after
            .filter(|n| !(1..=MAX_QUARANTINE_AFTER).contains(n))
        {
            return Err(tr!(
                "project.quarantineAfter.range",
                after,
                MAX_QUARANTINE_AFTER
            ));
        }
        let mut quarantine = file.quarantine;
        for entry in &quarantine {
            if !is_test_path(&entry.test) {
                return Err(tr!("project.quarantine.notTest", &entry.test, TEST_SUFFIX));
            }
            if entry.reason.as_deref().is_some_and(|r| r.trim().is_empty()) {
                return Err(tr!("project.quarantine.emptyReason", &entry.test));
            }
            if let Some(until) = entry.until.as_deref().filter(|u| date(u).is_none()) {
                return Err(tr!("project.quarantine.until", &entry.test, until));
            }
        }
        quarantine.sort_by(|a, b| a.test.cmp(&b.test));
        if let Some(twice) = quarantine.windows(2).find(|w| w[0].test == w[1].test) {
            return Err(tr!("project.quarantine.twice", &twice[0].test));
        }
        check_hide(&file.hide)?;
        for (task, choice) in [
            (AiTask::Screen, &file.ai.screen),
            (AiTask::Text, &file.ai.text),
        ] {
            if let Some(choice) = choice {
                task.check(choice)?;
            }
        }
        let mut run_modes = Vec::with_capacity(file.run_modes.len());
        for entry in file.run_modes {
            run_modes.push(run_mode(entry)?);
        }
        run_modes.sort_by(|a, b| a.folder.cmp(&b.folder));
        if let Some(twice) = run_modes.windows(2).find(|w| w[0].folder == w[1].folder) {
            return Err(tr!("project.runModes.twice", &twice[0].folder));
        }
        if let Some(retention) = &file.retention {
            retention.check()?;
        }
        Ok(Self {
            retries: file.retries,
            quarantine,
            suggest_quarantine_after: file.suggest_quarantine_after,
            local_only: file.local_only,
            hide: file.hide,
            ai_screen: file.ai.screen,
            ai_text: file.ai.text,
            run_modes,
            clean_home: file.clean_home,
            retention: file.retention,
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
            ai: FileAi {
                screen: self.ai_screen.clone(),
                text: self.ai_text.clone(),
            },
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
            clean_home: self.clean_home,
            retention: self.retention,
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
            return Err(tr!("project.test.invalid", test));
        }
        if let Some(until) = until.filter(|u| date(u).is_none()) {
            return Err(tr!("project.date.invalid", until));
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

    /// How long the project keeps its saved runs: as it chose, otherwise the defaults.
    pub fn run_retention(&self) -> RunRetention {
        self.retention.unwrap_or_default()
    }

    /// Chooses how long the project keeps its saved runs; `None` goes back to the defaults.
    /// `Err` names the first limit that is out of range.
    pub fn set_retention(&mut self, retention: Option<RunRetention>) -> Result<(), String> {
        if let Some(retention) = &retention {
            retention.check()?;
        }
        self.retention = retention;
        Ok(())
    }

    /// Replaces the areas hidden in every screenshot; `Err` names the first one that doesn't
    /// fit the window.
    pub fn set_hide(&mut self, hide: Vec<Rect>) -> Result<(), String> {
        check_hide(&hide)?;
        self.hide = hide;
        Ok(())
    }

    /// The AI the project chose for `task`.
    pub fn ai(&self, task: AiTask) -> Option<&AiChoice> {
        match task {
            AiTask::Screen => self.ai_screen.as_ref(),
            AiTask::Text => self.ai_text.as_ref(),
        }
    }

    /// Chooses the AI the project asks for `task`; `None` leaves it to the app's settings.
    /// `Err` when the provider isn't known or can't do the task.
    pub fn set_ai(&mut self, task: AiTask, choice: Option<AiChoice>) -> Result<(), String> {
        let choice = choice.map(|c| AiChoice {
            provider: c.provider.trim().to_owned(),
            model: c.model.trim().to_owned(),
        });
        if let Some(c) = &choice {
            task.check(c)?;
        }
        match task {
            AiTask::Screen => self.ai_screen = choice,
            AiTask::Text => self.ai_text = choice,
        }
        Ok(())
    }

    /// How the tests of `folder`, relative to the project folder (`.` for the project folder),
    /// run: as set for it or for the nearest folder above it, otherwise one after another.
    pub fn run_mode(&self, folder: &str) -> RunMode {
        self.run_mode_setting(folder)
            .map_or(RunMode::Sequential, |m| m.mode)
    }

    /// The setting [`Self::run_mode`] follows for `folder`: the one for it or for the nearest
    /// folder above it; `None` when there is none.
    pub fn run_mode_setting(&self, folder: &str) -> Option<&FolderRunMode> {
        let mut folder = folder.trim_matches('/');
        loop {
            let key = if folder.is_empty() { "." } else { folder };
            if let Some(m) = self.run_modes.iter().find(|m| m.folder == key) {
                return Some(m);
            }
            if folder.is_empty() || folder == "." {
                return None;
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
        Some(i) => Err(tr!("project.hide.area", i + 1)),
        None => Ok(()),
    }
}

/// The deepest folder that holds all of `folders`, each relative to the project folder with `/`
/// between folders; `.` for the project folder.
pub fn shared_folder(folders: &[String]) -> String {
    let parts = |folder: &str| -> Vec<String> {
        (folder.split('/'))
            .filter(|part| !part.is_empty() && *part != ".")
            .map(str::to_owned)
            .collect()
    };
    let mut iter = folders.iter();
    let mut shared = iter.next().map(|f| parts(f)).unwrap_or_default();
    for folder in iter {
        let same = shared
            .iter()
            .zip(parts(folder))
            .take_while(|(a, b)| **a == *b)
            .count();
        shared.truncate(same);
    }
    if shared.is_empty() {
        ".".to_owned()
    } else {
        shared.join("/")
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
        return Err(tr!("project.runModes.folder", &entry.folder));
    }
    let mode = match (entry.run, entry.limit) {
        (RunKind::Sequential, None) => RunMode::Sequential,
        (RunKind::Sequential, Some(_)) => {
            return Err(tr!("project.runModes.limitSequential", folder));
        }
        (RunKind::Parallel, limit) => {
            let limit = limit.unwrap_or(DEFAULT_PARALLEL);
            if !(2..=MAX_PARALLEL).contains(&limit) {
                return Err(tr!(
                    "project.runModes.limitRange",
                    folder,
                    limit,
                    MAX_PARALLEL
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
            (r#"{"format":0}"#, "“format” must be 1"),
            (r#"{"format":1,"retries":11}"#, "from 0 to 10"),
            (r#"{"format":1,"other":1}"#, "unknown field"),
            (r#"{"format":1,"localOnly":"yes"}"#, "expected a boolean"),
            (
                r#"{"format":1,"hide":[{"x":0.5,"y":0,"width":0.6,"height":0.1}]}"#,
                "area 1 in “hide”",
            ),
            (
                r#"{"format":1,"hide":[{"x":0,"y":0,"width":0,"height":0.1}]}"#,
                "area 1 in “hide”",
            ),
            (r#"{"format":1,"hide":[{"x":0,"y":0}]}"#, "missing field"),
            (
                r#"{"format":1,"quarantine":[{"test":"../x.flow.json"}]}"#,
                "isn’t a test file",
            ),
            (
                r#"{"format":1,"quarantine":[{"test":"/x.flow.json"}]}"#,
                "isn’t a test file",
            ),
            (
                r#"{"format":1,"quarantine":[{"test":"notes.txt"}]}"#,
                "isn’t a test file",
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
            (
                r#"{"format":1,"retention":{"maxAgeDays":0}}"#,
                "“maxAgeDays” in “retention” is 0; it must be from 1 to 3650",
            ),
            (
                r#"{"format":1,"retention":{"maxTotalMb":10000001}}"#,
                "from 1 to 10000000",
            ),
            (r#"{"format":1,"retention":{"days":30}}"#, "unknown field"),
        ] {
            let error = ProjectSettings::parse(text).unwrap_err();
            assert!(error.contains(says), "{text}: {error}");
        }
    }

    #[test]
    fn keeps_how_long_runs_are_kept() {
        let old = ProjectSettings::parse(r#"{"format":1,"retries":1}"#).unwrap();
        assert_eq!(old.retention, None);
        assert_eq!(old.run_retention(), RunRetention::default());
        assert!(!old.to_json().contains("retention"));

        let text = r#"{"format":1,"retention":{"mediaMaxAgeDays":7,"maxAgeDays":90}}"#;
        let mut settings = ProjectSettings::parse(text).unwrap();
        let chosen = RunRetention {
            max_age_days: Some(90),
            max_runs_per_test: None,
            media_max_age_days: Some(7),
            max_total_mb: None,
        };
        assert_eq!(settings.run_retention(), chosen);
        let written = settings.to_json();
        assert!(
            written.contains(
                "\"retention\": {\n    \"maxAgeDays\": 90,\n    \"mediaMaxAgeDays\": 7\n  }"
            ),
            "{written}"
        );
        assert_eq!(ProjectSettings::parse(&written).unwrap(), settings);

        let too_long = RunRetention {
            media_max_age_days: Some(MAX_RETENTION_DAYS + 1),
            ..chosen
        };
        assert!(settings.set_retention(Some(too_long)).is_err());
        assert_eq!(settings.retention, Some(chosen));
        settings.set_retention(None).unwrap();
        assert_eq!(settings.run_retention(), RunRetention::default());
        assert_eq!(settings.to_json(), "{\n  \"format\": 1\n}\n");
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
    fn a_clean_home_is_off_unless_the_project_turns_it_on() {
        let old = ProjectSettings::parse(r#"{"format":1,"retries":1}"#).unwrap();
        assert!(!old.clean_home);
        assert!(!ProjectSettings::default().clean_home);
        assert!(!old.to_json().contains("cleanHome"));
        let on = ProjectSettings::parse(r#"{"format":1,"cleanHome":true}"#).unwrap();
        assert!(on.clean_home);
        let written = on.to_json();
        assert_eq!(written, "{\n  \"format\": 1,\n  \"cleanHome\": true\n}\n");
        assert_eq!(ProjectSettings::parse(&written).unwrap(), on);
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
    fn the_shared_folder_is_the_deepest_that_holds_them_all() {
        let shared = |folders: &[&str]| {
            shared_folder(&folders.iter().map(|f| f.to_string()).collect::<Vec<_>>())
        };
        assert_eq!(shared(&["checkout/cart"]), "checkout/cart");
        assert_eq!(
            shared(&["checkout/cart", "checkout/pay", "checkout"]),
            "checkout"
        );
        assert_eq!(shared(&["checkout", "checkout-old"]), ".");
        assert_eq!(shared(&["checkout", "."]), ".");
        assert_eq!(shared(&[]), ".");
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
            settings
                .run_mode_setting("checkout/cart")
                .map(|m| m.folder.as_str()),
            Some("checkout")
        );
        assert_eq!(
            ProjectSettings::default().run_mode_setting("checkout"),
            None
        );
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

    fn choice(provider: &str, model: &str) -> AiChoice {
        AiChoice {
            provider: provider.into(),
            model: model.into(),
        }
    }

    #[test]
    fn the_ai_of_a_project_is_read_validated_and_written_without_keys() {
        let text = r#"{"format":1,"ai":{"screen":{"provider":"ollama","model":"llava"},
            "text":{"provider":"anthropic"}}}"#;
        let settings = ProjectSettings::parse(text).unwrap();
        assert_eq!(
            settings.ai(AiTask::Screen),
            Some(&choice("ollama", "llava"))
        );
        assert_eq!(settings.ai(AiTask::Text), Some(&choice("anthropic", "")));
        let written = settings.to_json();
        assert!(written.contains("\"ai\": {\n    \"screen\": {\n      \"provider\": \"ollama\",\n      \"model\": \"llava\"\n    },\n    \"text\": {\n      \"provider\": \"anthropic\"\n    }\n  }"), "{written}");
        assert_eq!(ProjectSettings::parse(&written).unwrap(), settings);
        assert_eq!(
            ProjectSettings::default().to_json(),
            "{\n  \"format\": 1\n}\n",
            "no choice, nothing written"
        );
        for (bad, says) in [
            (
                r#"{"format":1,"ai":{"screen":{"provider":"nope"}}}"#,
                "isn’t an AI provider",
            ),
            (
                r#"{"format":1,"ai":{"text":{"provider":"jev"}}}"#,
                "only checks screenshots",
            ),
            (
                r#"{"format":1,"ai":{"screen":{"provider":"openai","key":"sk-1"}}}"#,
                "unknown field `key`",
            ),
            (
                r#"{"format":1,"ai":{"screen":{"provider":"openai"},"speech":{}}}"#,
                "unknown field `speech`",
            ),
        ] {
            let error = ProjectSettings::parse(bad).unwrap_err();
            assert!(error.contains(says), "{bad}: {error}");
        }
        assert!(
            ProjectSettings::parse(r#"{"format":1,"ai":{"screen":{"provider":"jev"}}}"#).is_ok()
        );

        let mut settings = ProjectSettings::default();
        settings
            .set_ai(AiTask::Text, Some(choice(" openai ", " gpt-5-mini ")))
            .unwrap();
        assert_eq!(
            settings.ai(AiTask::Text),
            Some(&choice("openai", "gpt-5-mini"))
        );
        assert!(
            settings
                .set_ai(AiTask::Text, Some(choice("jev", "")))
                .is_err()
        );
        assert_eq!(
            settings.ai(AiTask::Text),
            Some(&choice("openai", "gpt-5-mini"))
        );
        settings.set_ai(AiTask::Text, None).unwrap();
        assert_eq!(settings.ai(AiTask::Text), None);
    }

    #[test]
    fn a_block_beats_the_project_which_beats_the_app() {
        let project = choice("anthropic", "claude-haiku");
        let resolve =
            |block: AiChoice, project: Option<&AiChoice>| AiChoice::resolve(&block, project);
        // Nothing chosen: the app's settings, which empty stands for.
        assert_eq!(resolve(choice("", ""), None), choice("", ""));
        assert_eq!(resolve(choice("", "m"), None), choice("", "m"));
        // The project's choice, whole.
        assert_eq!(resolve(choice("", ""), Some(&project)), project);
        // A block's own provider wins, with its own model only.
        assert_eq!(
            resolve(choice("openai", ""), Some(&project)),
            choice("openai", "")
        );
        assert_eq!(
            resolve(choice("openai", "gpt-5"), Some(&project)),
            choice("openai", "gpt-5")
        );
        // A block's own model belongs to the provider in force.
        assert_eq!(
            resolve(choice("", "claude-opus"), Some(&project)),
            choice("anthropic", "claude-opus")
        );
        // The same provider as the project's keeps the project's model unless the block has one.
        assert_eq!(resolve(choice("anthropic", ""), Some(&project)), project);
        assert_eq!(
            resolve(choice("anthropic", "claude-opus"), Some(&project)),
            choice("anthropic", "claude-opus")
        );
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

    #[test]
    fn problems_read_in_the_language_asked_for() {
        let text = r#"{"format":1,"quarantine":[{"test":"x.flow.json","until":"soon"}]}"#;
        assert_eq!(
            ProjectSettings::parse(text).unwrap_err(),
            "“until” of “x.flow.json” in “quarantine” is “soon”; write a date such as 2026-11-03"
        );
        assert_eq!(
            jevision_l10n::tr_in!("ru", "project.quarantine.until", "x.flow.json", "soon"),
            "«until» у «x.flow.json» в «quarantine» — «soon»; напишите дату, например 2026-11-03"
        );
    }
}
