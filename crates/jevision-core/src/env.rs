//! Environments of a project, such as `staging` and `production`: named sets of values that
//! tests read through `${env.NAME}`, like the address of a server. They live in
//! `jevision.environments.json` in the project folder and are committed with the tests.
//! Secrets never go there; they stay behind `${secret.NAME}`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::{fs, io};

use serde::Deserialize;

use crate::issue::Issue;
use crate::model::Flow;
use crate::validate::{is_identifier, placeholder_fields, placeholders};

/// Name of the environments file in the project folder.
pub const ENVIRONMENTS_FILE: &str = "jevision.environments.json";
/// Format of the environments file written by this build.
pub const ENVIRONMENTS_FORMAT: u32 = 1;
const MAX_NAME_LEN: usize = 64;

/// Values of one environment, by name.
pub type EnvironmentValues = BTreeMap<String, String>;

/// The environments of a project.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Environments {
    /// Every environment, by name.
    pub all: BTreeMap<String, EnvironmentValues>,
    /// The environment a run uses when none is chosen; always one of `all`.
    pub default: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    format: u32,
    #[serde(default)]
    default: Option<String>,
    environments: BTreeMap<String, EnvironmentValues>,
}

impl Environments {
    /// Reads the text of an environments file; `Err` says what is wrong with it.
    pub fn parse(text: &str) -> Result<Self, String> {
        let file: File = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if file.format > ENVIRONMENTS_FORMAT {
            return Err(format!(
                "the file has format {}, this version reads up to {ENVIRONMENTS_FORMAT}",
                file.format
            ));
        }
        if file.format < 1 {
            return Err(format!("\"format\" must be {ENVIRONMENTS_FORMAT}"));
        }
        for (name, values) in &file.environments {
            if !is_environment_name(name) {
                return Err(format!(
                    "\"{name}\" isn't a valid environment name; use letters, digits, - and _"
                ));
            }
            if let Some(bad) = values.keys().find(|key| !is_identifier(key)) {
                return Err(format!(
                    "\"{bad}\" in \"{name}\" isn't a valid value name; use letters, digits and _, \
                     not starting with a digit"
                ));
            }
        }
        if let Some(name) = file
            .default
            .as_deref()
            .filter(|name| !file.environments.contains_key(*name))
        {
            return Err(format!(
                "the default environment \"{name}\" isn't one of \"environments\""
            ));
        }
        Ok(Self {
            all: file.environments,
            default: file.default,
        })
    }

    /// Reads the environments file of the project folder `project`; a project without one has
    /// no environments.
    pub fn load(project: &Path) -> Result<Self, String> {
        match fs::read_to_string(project.join(ENVIRONMENTS_FILE)) {
            Ok(text) => Self::parse(&text),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.to_string()),
        }
    }

    /// The environments of the project that `dir` is in: the nearest folder at or above `dir`
    /// with an environments file or a `.jevision` folder.
    pub fn find(dir: &Path) -> Result<Self, String> {
        dir.ancestors()
            .find(|folder| {
                folder.join(ENVIRONMENTS_FILE).is_file() || folder.join(".jevision").is_dir()
            })
            .map_or_else(|| Ok(Self::default()), Self::load)
    }

    pub fn get(&self, name: &str) -> Option<&EnvironmentValues> {
        self.all.get(name)
    }
}

/// Names of the environment values the test reads through `${env.NAME}`, sorted, without
/// repeats.
pub fn required_env_values(flow: &Flow) -> Vec<String> {
    let names: BTreeSet<&str> = used(flow).into_iter().map(|(_, _, name)| name).collect();
    names.into_iter().map(str::to_owned).collect()
}

/// Problems with the `${env.NAME}` values a test in the folder `dir` reads: values no
/// environment of the project has are errors, values some environments lack are warnings.
pub fn validate_environment_values(flow: &Flow, dir: &Path) -> Vec<Issue> {
    let used = used(flow);
    if used.is_empty() {
        return Vec::new();
    }
    let environments = match Environments::find(dir) {
        Ok(environments) => environments,
        Err(reason) => {
            return vec![
                Issue::error("flow.environments.invalid")
                    .with("file", ENVIRONMENTS_FILE)
                    .with("reason", reason),
            ];
        }
    };
    let mut out = Vec::new();
    for (block, field, name) in used {
        let lacking: Vec<&str> = environments
            .all
            .iter()
            .filter(|(_, values)| !values.contains_key(name))
            .map(|(env, _)| env.as_str())
            .collect();
        if lacking.len() == environments.all.len() {
            out.push(
                Issue::error("flow.env.unknown")
                    .with("block", block)
                    .with("field", &field)
                    .with("name", name),
            );
        } else if !lacking.is_empty() {
            out.push(
                Issue::warning("flow.env.partial")
                    .with("block", block)
                    .with("field", &field)
                    .with("name", name)
                    .with("environments", lacking.join(", ")),
            );
        }
    }
    out
}

/// `(block, field, name)` of every `${env.NAME}`, by block id, each triple once.
fn used(flow: &Flow) -> BTreeSet<(&str, String, &str)> {
    let mut found = BTreeSet::new();
    for b in &flow.blocks {
        for (field, value) in placeholder_fields(&b.kind) {
            for (ns, name) in placeholders(value).into_iter().flatten() {
                if ns == "env" {
                    found.insert((b.id.as_str(), field.clone(), name));
                }
            }
        }
    }
    found
}

fn is_environment_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_NAME_LEN
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flow(text: &str) -> Flow {
        crate::parse(&format!(
            r#"{{"format":1,"name":"T","app":"com.example.App","start":"a","blocks":{{
                "a":{{"type":"type","text":"{text}","next":"b"}},
                "b":{{"type":"type","text":"${{env.HOST}}"}}}}}}"#
        ))
        .unwrap()
    }

    fn project(environments: Option<&str>) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "jevision-env-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(dir.join("tests/login")).unwrap();
        fs::create_dir_all(dir.join(".jevision")).unwrap();
        if let Some(text) = environments {
            fs::write(dir.join(ENVIRONMENTS_FILE), text).unwrap();
        }
        dir
    }

    #[test]
    fn parses_and_rejects_bad_files() {
        let envs = Environments::parse(
            r#"{"format":1,"environments":{"staging":{"HOST":"s.example.com"},"prod-eu":{}}}"#,
        )
        .unwrap();
        assert_eq!(envs.all.keys().collect::<Vec<_>>(), ["prod-eu", "staging"]);
        assert_eq!(envs.get("staging").unwrap()["HOST"], "s.example.com");
        assert_eq!(envs.default, None);
        let envs = Environments::parse(r#"{"format":1,"default":"dev","environments":{"dev":{}}}"#)
            .unwrap();
        assert_eq!(envs.default.as_deref(), Some("dev"));
        for (bad, says) in [
            (r#"{"format":2,"environments":{}}"#, "format 2"),
            (r#"{"format":1,"environments":{"st age":{}}}"#, "st age"),
            (r#"{"format":1,"environments":{"s":{"1X":"v"}}}"#, "1X"),
            (
                r#"{"format":1,"environments":{"s":{"X":1}}}"#,
                "invalid type",
            ),
            (r#"{"format":1,"envs":{}}"#, "unknown field"),
            (
                r#"{"format":1,"default":"prod","environments":{"dev":{}}}"#,
                "default environment \"prod\"",
            ),
        ] {
            let error = Environments::parse(bad).unwrap_err();
            assert!(error.contains(says), "{bad}: {error}");
        }
    }

    #[test]
    fn required_values_are_sorted_and_unique() {
        assert_eq!(
            required_env_values(&flow("${env.USER} ${env.HOST} ${secret.PASS}")),
            ["HOST", "USER"]
        );
    }

    #[test]
    fn unknown_and_partial_values_are_reported() {
        let dir = project(Some(
            r#"{"format":1,"environments":{"dev":{"HOST":"d","USER":"u"},"prod":{"HOST":"p"}}}"#,
        ));
        let issues =
            validate_environment_values(&flow("${env.USER}${env.NOPE}"), &dir.join("tests/login"));
        let summary: Vec<(&str, &str, Option<&str>)> = issues
            .iter()
            .map(|i| {
                (
                    i.code.as_str(),
                    i.param("name").unwrap(),
                    i.param("environments"),
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                ("flow.env.unknown", "NOPE", None),
                ("flow.env.partial", "USER", Some("prod")),
            ]
        );
        assert!(
            crate::validate_in(&flow("${env.NOPE}"), &dir)
                .iter()
                .any(|i| i.code == "flow.env.unknown")
        );

        // Without an environments file every value is unknown; a broken file is one problem.
        let bare = project(None);
        assert_eq!(
            validate_environment_values(&flow("x"), &bare)[0].code,
            "flow.env.unknown"
        );
        let broken = project(Some("{"));
        let issues = validate_environment_values(&flow("x"), &broken);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code, "flow.environments.invalid");
        for dir in [dir, bare, broken] {
            fs::remove_dir_all(dir).unwrap();
        }
    }
}
