//! Environments of a project, such as `staging` and `production`: named sets of values that
//! tests read through `${env.NAME}`, like the address of a server. They live in
//! `jevision.environments.json` in the project folder and are committed with the tests.
//! Secrets never go there; they stay behind `${secret.NAME}`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::{fs, io};

use jevision_l10n::tr;
use serde::{Deserialize, Serialize};

use crate::compat::{SettingsFileError, SettingsMetadata, check_settings_header};
use crate::issue::{Issue, json_problem};
use crate::model::Flow;
use crate::validate::{DiskCache, is_identifier, placeholder_fields, placeholders};

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
    /// Optional requirements and fields preserved for readers from other versions.
    pub metadata: SettingsMetadata,
}

#[derive(Deserialize, Serialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
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
    #[serde(flatten)]
    metadata: SettingsMetadata,
}

impl Environments {
    /// Reads the text of an environments file; `Err` says what is wrong with it.
    pub fn parse(text: &str) -> Result<Self, String> {
        Self::parse_checked(text).map_err(|error| error.to_string())
    }

    fn parse_checked(text: &str) -> Result<Self, SettingsFileError> {
        check_settings_header(text, ENVIRONMENTS_FORMAT)?;
        let invalid = SettingsFileError::Invalid;
        let file: File =
            serde_json::from_str(text).map_err(|error| invalid(json_problem(&error)))?;
        for (name, values) in &file.environments {
            if !is_environment_name(name) {
                return Err(invalid(tr!("env.name.invalid", name)));
            }
            if let Some(bad) = values.keys().find(|key| !is_identifier(key)) {
                return Err(invalid(tr!("env.valueName.invalid", bad, name)));
            }
        }
        if let Some(name) = file
            .default
            .as_deref()
            .filter(|name| !file.environments.contains_key(*name))
        {
            return Err(invalid(tr!("env.default.unknown", name)));
        }
        Ok(Self {
            all: file.environments,
            default: file.default,
            metadata: file.metadata,
        })
    }

    /// Reads the environments file of the project folder `project`; a project without one has
    /// no environments.
    pub fn load(project: &Path) -> Result<Self, String> {
        Self::load_checked(project).map_err(|error| error.to_string())
    }

    /// Reads with structured errors so tools can distinguish an update requirement from damage.
    pub fn load_checked(project: &Path) -> Result<Self, SettingsFileError> {
        match fs::read_to_string(project.join(ENVIRONMENTS_FILE)) {
            Ok(text) => Self::parse_checked(&text),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(SettingsFileError::Invalid(error.to_string())),
        }
    }

    /// The environments of the nearest project at or above `dir`.
    pub fn find(dir: &Path) -> Result<Self, String> {
        Self::find_checked(dir).map_err(|error| error.to_string())
    }

    pub(crate) fn find_checked(dir: &Path) -> Result<Self, SettingsFileError> {
        dir.ancestors()
            .find(|folder| is_project_folder(folder))
            .map_or_else(|| Ok(Self::default()), Self::load_checked)
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

    /// The file in its original format, retaining optional headers and unfamiliar fields.
    pub fn to_json(&self) -> String {
        let file = File {
            format: ENVIRONMENTS_FORMAT,
            default: self.default.clone(),
            environments: self.all.clone(),
            metadata: self.metadata.clone(),
        };
        let mut text = serde_json::to_string_pretty(&file).expect("environments always serialize");
        text.push('\n');
        text
    }

    fn save(mut self, project: &Path) -> Result<(), String> {
        Self::parse(&self.to_json())?;
        let path = project.join(ENVIRONMENTS_FILE);
        match fs::read_to_string(&path) {
            // Optional fields are opaque to this editor. Keep their latest on-disk values,
            // including deletions, rather than restoring an older snapshot of them.
            Ok(text) => self.metadata = Self::parse(&text)?.metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
        crate::atomic::write(&path, self.to_json()).map_err(|error| error.to_string())
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
    environment_issues(flow, dir, &mut DiskCache::default())
}

/// [`validate_environment_values`], reading the environments through `disk`.
pub(crate) fn environment_issues(flow: &Flow, dir: &Path, disk: &mut DiskCache) -> Vec<Issue> {
    let used = used(flow);
    if used.is_empty() {
        return Vec::new();
    }
    let environments = match disk.environments(dir) {
        Ok(environments) => environments,
        Err(error) => return vec![error.issue(ENVIRONMENTS_FILE, "flow.environments.invalid")],
    };
    let mut out = environments.metadata.warnings(ENVIRONMENTS_FILE);
    for (block, field, name) in used
        .into_iter()
        .filter(|(_, _, name)| *name != ENVIRONMENT_NAME)
    {
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
            (r#"{"format":1,"envs":{}}"#, "missing field"),
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
    fn saving_stale_environments_preserves_an_unreadable_or_newer_file() {
        let dir = project(Some(
            r#"{"format":1,"environments":{"dev":{"HOST":"old"}}}"#,
        ));
        let mut edited = Environments::load(&dir).unwrap();
        edited
            .all
            .get_mut("dev")
            .unwrap()
            .insert("HOST".into(), "new".into());
        let file = dir.join(ENVIRONMENTS_FILE);
        for changed in [
            r#"{"format":99,"future":true}"#,
            r#"{"format":1,"minVersion":"9999.0.0","environments":{"dev":{}}}"#,
            "{",
        ] {
            fs::write(&file, changed).unwrap();
            assert!(edited.clone().save(&dir).is_err());
            assert_eq!(fs::read_to_string(&file).unwrap(), changed);
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn optional_headers_and_fields_survive_edits_and_stale_saves() {
        let original = r#"{"format":1,"$schema":"https://example.com/env.schema.json",
            "minVersion":"1.0.0+build.7","future":{"nested":[1,null,{"enabled":true}]},
            "removedLater":true,"environments":{"dev":{"HOST":"old"}}}"#;
        let dir = project(Some(original));
        let parsed = Environments::load(&dir).unwrap();
        assert_eq!(Environments::parse(&parsed.to_json()).unwrap(), parsed);
        Environments::set_value(&dir, "dev", "HOST", "edited").unwrap();
        Environments::add(&dir, "staging", EnvironmentValues::new()).unwrap();
        let current = Environments::load(&dir).unwrap();
        assert_eq!(current.metadata, parsed.metadata);
        assert_eq!(current.get("dev").unwrap()["HOST"], "edited");
        assert!(current.get("staging").is_some());

        let mut stale = current.clone();
        stale
            .all
            .get_mut("dev")
            .unwrap()
            .insert("HOST".into(), "last edit".into());
        let mut external: serde_json::Value = serde_json::from_str(&current.to_json()).unwrap();
        external.as_object_mut().unwrap().remove("removedLater");
        external["future"] = serde_json::json!({"replaced": [false, 42]});
        external["addedLater"] = serde_json::json!(["keep me"]);
        external["$schema"] = "https://example.com/new.schema.json".into();
        fs::write(dir.join(ENVIRONMENTS_FILE), external.to_string()).unwrap();
        stale.save(&dir).unwrap();
        let saved = Environments::load(&dir).unwrap();
        assert_eq!(
            saved.metadata,
            Environments::parse(&external.to_string()).unwrap().metadata
        );
        assert_eq!(saved.get("dev").unwrap()["HOST"], "last edit");
        assert_eq!(saved.metadata.min_version.as_deref(), Some("1.0.0+build.7"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn requirements_are_checked_before_payloads_and_refuse_edits() {
        for (text, code) in [
            (
                r#"{"format":99,"environments":["future payload"]}"#,
                "settings.format.tooNew",
            ),
            (
                r#"{"format":1,"minVersion":"9999.0.0","environments":false}"#,
                "settings.needsNewer",
            ),
            (
                r#"{"format":1,"minVersion":"1.0.0+","environments":{}}"#,
                "settings.minVersion.invalid",
            ),
            (
                r#"{"format":1,"minVersion":null,"environments":{}}"#,
                "settings.minVersion.invalid",
            ),
            (
                r#"{"format":1,"minVersion":1,"environments":{}}"#,
                "settings.minVersion.invalid",
            ),
        ] {
            let dir = project(Some(text));
            let error = Environments::load_checked(&dir).unwrap_err();
            assert_eq!(error.issue(ENVIRONMENTS_FILE, "invalid").code, code);
            assert_eq!(
                error.requires_newer(),
                code != "settings.minVersion.invalid"
            );
            let issues = validate_environment_values(&flow("${env.HOST}"), &dir);
            assert_eq!(issues.len(), 1, "{text}");
            let built_in_only = validate_environment_values(&flow("${env.ENVIRONMENT}"), &dir);
            assert_eq!(built_in_only.len(), 1, "{text}");
            assert_eq!(built_in_only[0].code, code);
            assert_eq!(issues[0].code, code);
            assert_eq!(issues[0].param("file"), Some(ENVIRONMENTS_FILE));
            assert!(Environments::add(&dir, "new", EnvironmentValues::new()).is_err());
            assert!(Environments::set_value(&dir, "dev", "HOST", "changed").is_err());
            assert_eq!(
                fs::read_to_string(dir.join(ENVIRONMENTS_FILE)).unwrap(),
                text
            );
            fs::remove_dir_all(dir).unwrap();
        }
        for text in [
            r#"{"format":1,"$schema":null,"environments":{}}"#,
            r#"{"format":1,"$schema":{},"environments":{}}"#,
            r#"{"format":1,"environments":{},"environments":{}}"#,
        ] {
            assert!(Environments::parse(text).is_err(), "{text}");
        }
    }

    #[test]
    fn optional_fields_are_reported_without_changing_execution_values() {
        let dir = project(Some(
            r#"{"format":1,"minVersion":"1.0.0","future":{"HOST":"ignored"},
            "environments":{"dev":{"HOST":"actual"}}}"#,
        ));
        let issues = validate_environment_values(&flow("${env.HOST}"), &dir);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code, "settings.field.unknown");
        assert_eq!(issues[0].severity, crate::Severity::Warning);
        assert_eq!(issues[0].param("name"), Some("future"));
        assert_eq!(issues[0].param("file"), Some(ENVIRONMENTS_FILE));
        assert!(crate::issue::describe_in("en", &issues[0]).contains(ENVIRONMENTS_FILE));
        assert_eq!(
            Environments::load(&dir).unwrap().get("dev").unwrap()["HOST"],
            "actual"
        );
        fs::remove_dir_all(dir).unwrap();
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
