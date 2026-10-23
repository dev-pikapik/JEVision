//! Environments of a project, such as `staging` and `production`: named sets of values that
//! tests read through `${env.NAME}`, like the address of a server. They live in
//! `jevision.environments.json` in the project folder and are committed with the tests.
//! Secrets never go there; they stay behind `${secret.NAME}`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::{fs, io};

use jevision_l10n::tr;
use serde::{Deserialize, Serialize};

use crate::issue::{Issue, json_problem};
use crate::model::Flow;
use crate::validate::{is_identifier, placeholder_fields, placeholders};

/// Name of the environments file in the project folder.
pub const ENVIRONMENTS_FILE: &str = "jevision.environments.json";
/// Format of the environments file written by this build.
pub const ENVIRONMENTS_FORMAT: u32 = 1;
pub(crate) const MAX_NAME_LEN: usize = 64;
/// The value every run has: `${env.ENVIRONMENT}` is the name of the environment chosen for it,
/// empty when none is. An environment that has a value of this name itself keeps it.
pub const ENVIRONMENT_NAME: &str = "ENVIRONMENT";

/// Values of one environment, by name.
pub type EnvironmentValues = BTreeMap<String, String>;

/// A project folder has the environments file or the `.jevision` folder the app makes in the
/// project it opens.
pub(crate) fn is_project_folder(folder: &Path) -> bool {
    folder.join(ENVIRONMENTS_FILE).is_file() || folder.join(".jevision").is_dir()
}

/// The environments of a project.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Environments {
    /// Every environment, by name.
    pub all: BTreeMap<String, EnvironmentValues>,
    /// The environment a run uses when none is chosen; always one of `all`.
    pub default: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub(crate) struct File {
    format: u32,
    /// The environment a run uses when none is chosen; one of `environments`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    default: Option<String>,
    /// Every environment by its name, of letters, digits, `-` and `_`. Each holds its values by
    /// name, which tests read as `${env.NAME}`.
    #[cfg_attr(
        test,
        schemars(extend(
            "propertyNames" = {"pattern": crate::schema::environment_name()},
            "additionalProperties" = {
                "type": "object",
                "propertyNames": {"pattern": "^[A-Za-z_][A-Za-z0-9_]*$"},
                "additionalProperties": {"type": "string"}
            }
        ))
    )]
    environments: BTreeMap<String, EnvironmentValues>,
}

impl Environments {
    /// Reads the text of an environments file; `Err` says what is wrong with it.
    pub fn parse(text: &str) -> Result<Self, String> {
        let file: File = serde_json::from_str(text).map_err(|e| json_problem(&e))?;
        if file.format > ENVIRONMENTS_FORMAT {
            return Err(tr!(
                "project.format.tooNew",
                file.format,
                ENVIRONMENTS_FORMAT
            ));
        }
        if file.format < 1 {
            return Err(tr!("project.format.invalid", ENVIRONMENTS_FORMAT));
        }
        for (name, values) in &file.environments {
            if !is_environment_name(name) {
                return Err(tr!("env.name.invalid", name));
            }
            if let Some(bad) = values.keys().find(|key| !is_identifier(key)) {
                return Err(tr!("env.valueName.invalid", bad, name));
            }
        }
        if let Some(name) = file
            .default
            .as_deref()
            .filter(|name| !file.environments.contains_key(*name))
        {
            return Err(tr!("env.default.unknown", name));
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
            .find(|folder| is_project_folder(folder))
            .map_or_else(|| Ok(Self::default()), Self::load)
    }

    pub fn get(&self, name: &str) -> Option<&EnvironmentValues> {
        self.all.get(name)
    }

    /// Why an environment `name` with values named `values` can't join these; `Ok` when it can.
    pub fn check_new<'a>(
        &self,
        name: &str,
        values: impl IntoIterator<Item = &'a str>,
    ) -> Result<(), AddEnvironmentError> {
        if !is_environment_name(name) {
            return Err(AddEnvironmentError::InvalidName);
        }
        if self
            .all
            .keys()
            .any(|taken| taken.eq_ignore_ascii_case(name))
        {
            return Err(AddEnvironmentError::Taken);
        }
        match values.into_iter().find(|value| !is_identifier(value)) {
            Some(bad) => Err(AddEnvironmentError::InvalidValueName(bad.to_owned())),
            None => Ok(()),
        }
    }

    /// Adds the environment `name` with `values` to the environments file of the project
    /// folder `project`, writing the file when there is none. The file is read again first, so
    /// changes made to it meanwhile stay.
    pub fn add(
        project: &Path,
        name: &str,
        values: EnvironmentValues,
    ) -> Result<(), AddEnvironmentError> {
        let mut current = Self::load(project).map_err(AddEnvironmentError::Unreadable)?;
        current.check_new(name, values.keys().map(String::as_str))?;
        current.all.insert(name.to_owned(), values);
        current
            .save(project)
            .map_err(AddEnvironmentError::Unwritable)
    }

    /// Sets the value `value_name` of the environment `name` in the environments file of the
    /// project folder `project`, adding the value when the environment lacks it. The file is
    /// read again first, so changes made to it meanwhile stay.
    pub fn set_value(
        project: &Path,
        name: &str,
        value_name: &str,
        value: &str,
    ) -> Result<(), SetValueError> {
        if !is_identifier(value_name) {
            return Err(SetValueError::InvalidValueName);
        }
        let mut current = Self::load(project).map_err(SetValueError::Unreadable)?;
        current
            .all
            .get_mut(name)
            .ok_or(SetValueError::UnknownEnvironment)?
            .insert(value_name.to_owned(), value.to_owned());
        current.save(project).map_err(SetValueError::Unwritable)
    }

    fn save(self, project: &Path) -> Result<(), String> {
        let file = File {
            format: ENVIRONMENTS_FORMAT,
            default: self.default,
            environments: self.all,
        };
        let mut text = serde_json::to_string_pretty(&file).map_err(|e| e.to_string())?;
        text.push('\n');
        crate::atomic::write(&project.join(ENVIRONMENTS_FILE), text).map_err(|e| e.to_string())
    }
}

/// Why an environment can't be added.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddEnvironmentError {
    /// Empty, too long, or with characters other than letters, digits, `-` and `_`.
    InvalidName,
    /// The project has an environment of that name, in any case, already.
    Taken,
    /// A value name that isn't letters, digits and `_`, not starting with a digit.
    InvalidValueName(String),
    /// The environments file is there but can't be read.
    Unreadable(String),
    /// The environments file can't be saved.
    Unwritable(String),
}

/// Why a value of an environment can't be set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetValueError {
    /// The project has no environment of that name.
    UnknownEnvironment,
    /// A value name that isn't letters, digits and `_`, not starting with a digit.
    InvalidValueName,
    /// The environments file is there but can't be read.
    Unreadable(String),
    /// The environments file can't be saved.
    Unwritable(String),
}

/// Names of the environment values the test reads through `${env.NAME}` and an environment has
/// to provide, sorted, without repeats. `ENVIRONMENT` is left out: every run has it.
pub fn required_env_values(flow: &Flow) -> Vec<String> {
    let names: BTreeSet<&str> = used(flow)
        .into_iter()
        .map(|(_, _, name)| name)
        .filter(|name| *name != ENVIRONMENT_NAME)
        .collect();
    names.into_iter().map(str::to_owned).collect()
}

/// The `${env.NAME}` values each block reads, by block id: names sorted, without repeats,
/// `ENVIRONMENT` included. Blocks that read none aren't listed.
pub fn env_reads(flow: &Flow) -> BTreeMap<String, Vec<String>> {
    let mut reads: BTreeMap<String, BTreeSet<&str>> = BTreeMap::new();
    for (block, _, name) in used(flow) {
        reads.entry(block.to_owned()).or_default().insert(name);
    }
    reads
        .into_iter()
        .map(|(block, names)| (block, names.into_iter().map(str::to_owned).collect()))
        .collect()
}

/// Problems with the `${env.NAME}` values a test in the folder `dir` reads: values no
/// environment of the project has are errors, values some environments lack are warnings.
pub fn validate_environment_values(flow: &Flow, dir: &Path) -> Vec<Issue> {
    let used: Vec<_> = used(flow)
        .into_iter()
        .filter(|(_, _, name)| *name != ENVIRONMENT_NAME)
        .collect();
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

pub(crate) fn is_environment_name(s: &str) -> bool {
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
                "default environment “prod”",
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
    fn every_run_knows_its_environment_without_a_value_for_it() {
        let reads = flow("${env.ENVIRONMENT} ${env.HOST}");
        assert_eq!(required_env_values(&reads), ["HOST"]);
        let dir = project(Some(r#"{"format":1,"environments":{"dev":{"HOST":"d"}}}"#));
        assert!(validate_environment_values(&reads, &dir.join("tests/login")).is_empty());
        let reads_of = env_reads(&reads);
        assert_eq!(reads_of["a"], ["ENVIRONMENT", "HOST"]);
        assert_eq!(reads_of["b"], ["HOST"]);
        assert!(env_reads(&crate::parse(
            r#"{"format":1,"name":"T","app":"com.example.App","start":"a","blocks":{"a":{"type":"pressKeys","keys":"cmd+s"}}}"#
        )
        .unwrap())
        .is_empty());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_value_is_changed_or_added_in_one_environment() {
        let dir = project(Some(
            r#"{"format":1,"default":"dev","environments":{"dev":{"HOST":"d"},"prod":{"HOST":"p"}}}"#,
        ));
        Environments::set_value(&dir, "dev", "HOST", "d2.example.com").unwrap();
        Environments::set_value(&dir, "dev", "USER", "me").unwrap();
        let envs = Environments::load(&dir).unwrap();
        assert_eq!(envs.get("dev").unwrap()["HOST"], "d2.example.com");
        assert_eq!(envs.get("dev").unwrap()["USER"], "me");
        assert_eq!(envs.get("prod").unwrap()["HOST"], "p");
        assert_eq!(envs.default.as_deref(), Some("dev"));

        assert_eq!(
            Environments::set_value(&dir, "nope", "HOST", "x"),
            Err(SetValueError::UnknownEnvironment)
        );
        assert_eq!(
            Environments::set_value(&dir, "dev", "1X", "x"),
            Err(SetValueError::InvalidValueName)
        );
        let broken = project(Some("{"));
        assert!(matches!(
            Environments::set_value(&broken, "dev", "HOST", "x"),
            Err(SetValueError::Unreadable(_))
        ));
        assert_eq!(
            fs::read_to_string(broken.join(ENVIRONMENTS_FILE)).unwrap(),
            "{"
        );
        for dir in [dir, broken] {
            fs::remove_dir_all(dir).unwrap();
        }
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

    #[test]
    fn environments_are_added_to_the_file_or_refused() {
        let dir = project(None);
        let values = |pairs: &[(&str, &str)]| -> EnvironmentValues {
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect()
        };
        Environments::add(&dir, "staging", values(&[("HOST", "s.example.com")])).unwrap();
        Environments::add(&dir, "dev", EnvironmentValues::new()).unwrap();
        let text = fs::read_to_string(dir.join(ENVIRONMENTS_FILE)).unwrap();
        assert!(text.starts_with("{\n  \"format\": 1,\n"), "{text}");
        assert!(text.ends_with("}\n"));
        let envs = Environments::load(&dir).unwrap();
        assert_eq!(envs.all.keys().collect::<Vec<_>>(), ["dev", "staging"]);
        assert_eq!(envs.get("staging").unwrap()["HOST"], "s.example.com");

        let refused = |name: &str, value: &str| {
            Environments::add(&dir, name, values(&[(value, "v")])).unwrap_err()
        };
        assert_eq!(refused("Staging", "X"), AddEnvironmentError::Taken);
        assert_eq!(refused("st age", "X"), AddEnvironmentError::InvalidName);
        assert_eq!(refused("", "X"), AddEnvironmentError::InvalidName);
        assert_eq!(
            refused("prod", "1X"),
            AddEnvironmentError::InvalidValueName("1X".to_owned())
        );
        assert_eq!(Environments::load(&dir).unwrap().all.len(), 2);

        // A default the file names is kept; a broken file isn't overwritten.
        let named = project(Some(
            r#"{"format":1,"default":"dev","environments":{"dev":{}}}"#,
        ));
        Environments::add(&named, "prod", EnvironmentValues::new()).unwrap();
        assert_eq!(
            Environments::load(&named).unwrap().default.as_deref(),
            Some("dev")
        );
        let broken = project(Some("{"));
        assert!(matches!(
            Environments::add(&broken, "prod", EnvironmentValues::new()),
            Err(AddEnvironmentError::Unreadable(_))
        ));
        assert_eq!(
            fs::read_to_string(broken.join(ENVIRONMENTS_FILE)).unwrap(),
            "{"
        );
        for dir in [dir, named, broken] {
            fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn problems_read_in_the_language_asked_for() {
        assert_eq!(
            Environments::parse(r#"{"format":1,"environments":{"st age":{}}}"#).unwrap_err(),
            "“st age” isn’t a valid environment name; use letters, digits, - and _"
        );
        assert_eq!(
            jevision_l10n::tr_in!("ru", "env.name.invalid", "st age"),
            "«st age» не подходит как имя окружения: используйте буквы, цифры, - и _"
        );
    }
}
