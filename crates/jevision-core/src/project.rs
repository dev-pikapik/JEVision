//! Settings a project shares with everyone who runs its tests, kept in `jevision.project.json`
//! in the project folder and committed with the tests: how many times a failed test is retried
//! when its file doesn't say, and the tests in quarantine. A quarantined test still runs, but
//! its failure doesn't fail Run All, the command line or the JUnit report. The file also says
//! how private the project's screens are: areas covered in every screenshot before it is saved
//! or sent anywhere, and whether screenshots may go to AI models outside this computer.

use std::path::{Component, Path};
use std::{fs, io};

use serde::{Deserialize, Serialize};

use crate::Rect;
use crate::model::TEST_SUFFIX;
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
        Ok(Self {
            retries: file.retries,
            quarantine,
            local_only: file.local_only,
            hide: file.hide,
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
