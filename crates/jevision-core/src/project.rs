//! Settings a project shares with everyone who runs its tests, kept in `jevision.project.json`
//! in the project folder and committed with the tests: how many times a failed test is retried
//! when its file doesn't say, and the tests in quarantine. A quarantined test still runs, but
//! its failure doesn't fail Run All, the command line or the JUnit report. The file also says
//! how private the project's screens are: areas covered in every screenshot before it is saved
//! or sent anywhere, and whether screenshots may go to AI models outside this computer, which
//! AI the project asks for looking at screens and for writing and explaining tests (provider
//! and model by name; API keys never go in the file), and which folders run their tests side
//! by side rather than one after another.

use std::path::{Component, Path};
use std::{fs, io};

use serde::{Deserialize, Serialize};

use crate::Rect;
use crate::batch::{DEFAULT_PARALLEL, MAX_PARALLEL, RunMode};
use crate::model::{AI_PROVIDERS, TEST_SUFFIX};
use crate::validate::{MAX_RETRIES, rect_fits};

/// Name of the project settings file in the project folder.
pub const PROJECT_FILE: &str = "jevision.project.json";
/// Format of the project settings file written by this build.
pub const PROJECT_FORMAT: u32 = 1;

/// The settings of a project.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProjectSettings {
    /// How many times a failed test is retried when its file doesn't say; `None` retries none.
    pub retries: Option<u32>,
    /// Tests in quarantine, sorted by file.
    pub quarantine: Vec<Quarantined>,
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
            return Err(format!(
                "\"{}\" in \"ai\" → \"{key}\" isn't an AI provider; use one of {}",
                choice.provider,
                AI_PROVIDERS.join(", ")
            ));
        }
        if self == Self::Text && choice.provider == "jev" {
            return Err(format!(
                "\"jev\" in \"ai\" → \"{key}\" can't write tests or explain failures, it only \
                 checks screenshots; choose another provider"
            ));
        }
        Ok(())
    }
}

/// Which AI to ask: a provider by its id, and one of its models. The key and the address of the
/// provider are the person's own and stay in the app's settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AiChoice {
    /// One of [`AI_PROVIDERS`]; empty for the one the app's settings choose.
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
        rename = "localOnly",
        skip_serializing_if = "std::ops::Not::not"
    )]
    local_only: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    hide: Vec<Rect>,
    #[serde(default, skip_serializing_if = "FileAi::is_empty")]
    ai: FileAi,
    #[serde(default, rename = "runModes", skip_serializing_if = "Vec::is_empty")]
    run_modes: Vec<RunModeEntry>,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileAi {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    screen: Option<AiChoice>,
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
        }
        quarantine.sort_by(|a, b| a.test.cmp(&b.test));
        if let Some(twice) = quarantine.windows(2).find(|w| w[0].test == w[1].test) {
            return Err(format!(
                "\"{}\" is in \"quarantine\" more than once",
                twice[0].test
            ));
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
            return Err(format!(
                "the folder \"{}\" is in \"runModes\" more than once",
                twice[0].folder
            ));
        }
        Ok(Self {
            retries: file.retries,
            quarantine,
            local_only: file.local_only,
            hide: file.hide,
            ai_screen: file.ai.screen,
            ai_text: file.ai.text,
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
        };
        let mut text = serde_json::to_string_pretty(&file).expect("settings always serialize");
        text.push('\n');
        text
    }

    /// Writes the settings to the project folder `project`.
    pub fn save(&self, project: &Path) -> io::Result<()> {
        fs::write(project.join(PROJECT_FILE), self.to_json())
    }

    /// The quarantine entry of the test file `test`, relative to the project folder.
    pub fn quarantined(&self, test: &str) -> Option<&Quarantined> {
        self.quarantine
            .binary_search_by(|q| q.test.as_str().cmp(test))
            .ok()
            .map(|i| &self.quarantine[i])
    }

    /// Puts the test file `test` in quarantine, or changes the reason of one already there. A
    /// blank reason counts as none; `Err` when `test` isn't a test file path.
    pub fn quarantine(&mut self, test: &str, reason: Option<&str>) -> Result<(), String> {
        if !is_test_path(test) {
            return Err(format!("\"{test}\" isn't a test file of the project"));
        }
        let entry = Quarantined {
            test: test.to_owned(),
            reason: reason
                .map(str::trim)
                .filter(|r| !r.is_empty())
                .map(str::to_owned),
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
            settings.quarantined("a/login.flow.json").unwrap().reason,
            Some("Bug #12".into())
        );
        assert!(settings.quarantined("b.flow.json").is_none());
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
        settings.quarantine("b.flow.json", Some("  ")).unwrap();
        settings.quarantine("a.flow.json", Some(" Bug ")).unwrap();
        settings
            .quarantine("b.flow.json", Some("Slow server"))
            .unwrap();
        assert!(settings.quarantine("../c.flow.json", None).is_err());
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
                "isn't an AI provider",
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
        settings.quarantine("t.flow.json", None).unwrap();
        settings.save(&dir).unwrap();
        assert_eq!(ProjectSettings::load(&dir).unwrap(), settings);
        fs::remove_dir_all(&dir).unwrap();
    }
}
