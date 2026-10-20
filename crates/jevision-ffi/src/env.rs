//! Environments of a project and the data rows of a test, for choosing how a run starts.

use std::collections::HashMap;
use std::path::Path;

use jevision_core::{AddEnvironmentError, DataError, DataTable, Environments, SetValueError};

use crate::{DataRowRange, Flow, Issue};

/// The project's environments file can't be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum EnvironmentsError {
    #[error("{file} can't be read: {reason}")]
    Unreadable { file: String, reason: String },
}

/// One environment of a project, such as `staging`, with the values tests read through
/// `${env.NAME}`. None of them is secret.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ProjectEnvironment {
    pub name: String,
    pub values: HashMap<String, String>,
}

/// The environments in `jevision.environments.json` of the project folder `project`, sorted by
/// name; none when the file isn't there.
#[uniffi::export]
pub fn project_environments(project: String) -> Result<Vec<ProjectEnvironment>, EnvironmentsError> {
    Ok(load(&project)?
        .all
        .into_iter()
        .map(|(name, values)| ProjectEnvironment {
            name,
            values: values.into_iter().collect(),
        })
        .collect())
}

/// The environment a run of the project folder `project` uses when none is chosen: the
/// `default` of its environments file, if it names one.
#[uniffi::export]
pub fn default_environment(project: String) -> Result<Option<String>, EnvironmentsError> {
    Ok(load(&project)?.default)
}

fn load(project: &str) -> Result<Environments, EnvironmentsError> {
    Environments::load(Path::new(project)).map_err(|reason| EnvironmentsError::Unreadable {
        file: jevision_core::ENVIRONMENTS_FILE.to_owned(),
        reason,
    })
}

/// Why an environment can't be added to a project.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum NewEnvironmentError {
    #[error("\"{name}\" isn't a valid environment name; use letters, digits, - and _")]
    InvalidName { name: String },
    #[error("there's an environment \"{name}\" already")]
    Taken { name: String },
    #[error(
        "\"{name}\" isn't a valid value name; use letters, digits and _, not starting with a digit"
    )]
    InvalidValueName { name: String },
    #[error("{file} can't be read: {reason}")]
    Unreadable { file: String, reason: String },
    #[error("{file} can't be saved: {reason}")]
    Unwritable { file: String, reason: String },
}

fn new_environment_error(name: &str, error: AddEnvironmentError) -> NewEnvironmentError {
    let file = jevision_core::ENVIRONMENTS_FILE.to_owned();
    match error {
        AddEnvironmentError::InvalidName => NewEnvironmentError::InvalidName {
            name: name.to_owned(),
        },
        AddEnvironmentError::Taken => NewEnvironmentError::Taken {
            name: name.to_owned(),
        },
        AddEnvironmentError::InvalidValueName(name) => {
            NewEnvironmentError::InvalidValueName { name }
        }
        AddEnvironmentError::Unreadable(reason) => NewEnvironmentError::Unreadable { file, reason },
        AddEnvironmentError::Unwritable(reason) => NewEnvironmentError::Unwritable { file, reason },
    }
}

/// What keeps an environment `name` with values named `value_names` from being added to the
/// project folder `project`; `None` when nothing does.
#[uniffi::export]
pub fn new_environment_problem(
    project: String,
    name: String,
    value_names: Vec<String>,
) -> Option<NewEnvironmentError> {
    let current = match Environments::load(Path::new(&project)) {
        Ok(current) => current,
        Err(reason) => {
            return Some(new_environment_error(
                &name,
                AddEnvironmentError::Unreadable(reason),
            ));
        }
    };
    current
        .check_new(&name, value_names.iter().map(String::as_str))
        .err()
        .map(|error| new_environment_error(&name, error))
}

/// Adds the environment `name` with `values` to the environments file of the project folder
/// `project`, writing the file when there is none.
#[uniffi::export]
pub fn add_environment(
    project: String,
    name: String,
    values: HashMap<String, String>,
) -> Result<(), NewEnvironmentError> {
    Environments::add(Path::new(&project), &name, values.into_iter().collect())
        .map_err(|error| new_environment_error(&name, error))
}

/// Why a value of an environment can't be set.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum EnvironmentValueError {
    #[error("there's no environment \"{name}\"")]
    UnknownEnvironment { name: String },
    #[error(
        "\"{name}\" isn't a valid value name; use letters, digits and _, not starting with a digit"
    )]
    InvalidValueName { name: String },
    #[error("{file} can't be read: {reason}")]
    Unreadable { file: String, reason: String },
    #[error("{file} can't be saved: {reason}")]
    Unwritable { file: String, reason: String },
}

/// Sets the value `name` of `environment` in the environments file of the project folder
/// `project`, adding it when the environment lacks it.
#[uniffi::export]
pub fn set_environment_value(
    project: String,
    environment: String,
    name: String,
    value: String,
) -> Result<(), EnvironmentValueError> {
    let file = jevision_core::ENVIRONMENTS_FILE.to_owned();
    Environments::set_value(Path::new(&project), &environment, &name, &value).map_err(|error| {
        match error {
            SetValueError::UnknownEnvironment => {
                EnvironmentValueError::UnknownEnvironment { name: environment }
            }
            SetValueError::InvalidValueName => EnvironmentValueError::InvalidValueName { name },
            SetValueError::Unreadable(reason) => EnvironmentValueError::Unreadable { file, reason },
            SetValueError::Unwritable(reason) => EnvironmentValueError::Unwritable { file, reason },
        }
    })
}

/// Name of the environments file in a project folder.
#[uniffi::export]
pub fn environments_file_name() -> String {
    jevision_core::ENVIRONMENTS_FILE.to_owned()
}

/// Names of the environment values the test reads through `${env.NAME}`, sorted.
#[uniffi::export]
pub fn required_env_values(flow: Flow) -> Vec<String> {
    jevision_core::required_env_values(&flow)
}

/// The environment values each block of the test reads through `${env.NAME}`, by block id,
/// names sorted; `ENVIRONMENT` is the name of the environment itself. Blocks that read none
/// aren't listed.
#[uniffi::export]
pub fn block_env_reads(flow: Flow) -> HashMap<String, Vec<String>> {
    jevision_core::env_reads(&flow).into_iter().collect()
}

/// Name every run has as `${env.ENVIRONMENT}`: the name of the environment chosen for it.
#[uniffi::export]
pub fn environment_name_value() -> String {
    jevision_core::ENVIRONMENT_NAME.to_owned()
}

/// Problems with the environment values a test saved in `folder` reads, given its project's
/// environments.
#[uniffi::export]
pub fn environment_issues(flow: Flow, folder: String) -> Vec<Issue> {
    jevision_core::validate_environment_values(&flow, Path::new(&folder))
}

/// A data table of a `useData` block: a CSV file, with how to write it back the same way.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DataRows {
    pub columns: Vec<String>,
    /// Every row has one value per column.
    pub rows: Vec<Vec<String>>,
    /// `,`, `;` or a tab.
    #[uniffi(default = ",")]
    pub delimiter: String,
    /// Lines end with CR LF rather than LF.
    #[uniffi(default = false)]
    pub crlf: bool,
    /// The file starts with a UTF-8 byte order mark.
    #[uniffi(default = false)]
    pub bom: bool,
}

impl From<DataTable> for DataRows {
    fn from(t: DataTable) -> Self {
        Self {
            columns: t.columns,
            rows: t.rows,
            delimiter: t.delimiter.to_string(),
            crlf: t.crlf,
            bom: t.bom,
        }
    }
}

impl From<DataRows> for DataTable {
    fn from(t: DataRows) -> Self {
        Self {
            columns: t.columns,
            rows: t.rows,
            delimiter: t.delimiter.chars().next().unwrap_or(','),
            crlf: t.crlf,
            bom: t.bom,
        }
    }
}

/// Why a CSV file can't be used as a data table: `code` (such as `data.rowLength`) and the
/// values its localized text refers to, such as `line`; `message` says it in English.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum DataFileError {
    #[error("{message}")]
    Invalid {
        code: String,
        params: HashMap<String, String>,
        message: String,
    },
}

impl From<DataError> for DataFileError {
    fn from(e: DataError) -> Self {
        Self::Invalid {
            code: e.code().to_owned(),
            params: e
                .params()
                .into_iter()
                .map(|(name, value)| (name.to_owned(), value))
                .collect(),
            message: e.to_string(),
        }
    }
}

/// Reads the CSV file at `path`.
#[uniffi::export]
pub fn read_data_rows(path: String) -> Result<DataRows, DataFileError> {
    Ok(DataTable::load(Path::new(&path))?.into())
}

/// Writes `table` to `path` with its delimiter, line endings and byte order mark. A table with
/// a blank or repeated column name or a row of the wrong length is not written.
#[uniffi::export]
pub fn write_data_rows(path: String, table: DataRows) -> Result<(), DataFileError> {
    Ok(DataTable::from(table).save(Path::new(&path))?)
}

/// What each row is called in reports: its value in a `name` column, or else in its first;
/// empty when that is blank.
#[uniffi::export]
pub fn data_row_labels(table: DataRows) -> Vec<String> {
    let table = DataTable::from(table);
    (0..table.rows.len()).map(|row| table.label(row)).collect()
}

/// The data file a run of `flow` goes through row by row, relative to the test's folder: that
/// of the first `useData` block the test comes to, when it has no placeholders.
#[uniffi::export]
pub fn test_data_file(flow: Flow) -> Option<String> {
    jevision_core::primary_data_file(&flow).map(str::to_owned)
}

/// Problems with the data tables a test saved in `folder` reads: files that aren't valid tables
/// and `${data.COLUMN}` naming a column its table lacks.
#[uniffi::export]
pub fn data_issues(flow: Flow, folder: String) -> Vec<Issue> {
    jevision_core::validate_data(&flow, Path::new(&folder))
}

/// Reads a choice of data rows as people write it, rows counted from 1: `all` (`None`), a row
/// such as `3` or a range such as `2-4`. The range returned counts rows from 0.
#[uniffi::export]
pub fn parse_data_rows(text: String) -> Result<Option<DataRowRange>, DataRowsChoiceError> {
    let range = jevision_core::parse_row_choice(&text)
        .map_err(|message| DataRowsChoiceError::Invalid { message })?;
    Ok(range.map(|r| DataRowRange {
        first: u32::try_from(*r.start()).unwrap_or(u32::MAX),
        last: u32::try_from(*r.end()).unwrap_or(u32::MAX),
    }))
}

/// A choice of data rows that can't be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum DataRowsChoiceError {
    #[error("{message}")]
    Invalid { message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_environments_and_reads_rows() {
        let dir = std::env::temp_dir().join(format!("jevision-ffi-env-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let project = dir.to_string_lossy().into_owned();
        assert_eq!(project_environments(project.clone()), Ok(Vec::new()));

        std::fs::write(
            dir.join("jevision.environments.json"),
            r#"{"format":1,"environments":{"stage":{"HOST":"s"},"dev":{}}}"#,
        )
        .unwrap();
        let found = project_environments(project.clone()).unwrap();
        assert_eq!(
            found.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
            ["dev", "stage"]
        );
        assert_eq!(found[1].values["HOST"], "s");

        std::fs::write(dir.join("jevision.environments.json"), "{").unwrap();
        assert!(matches!(
            project_environments(project),
            Err(EnvironmentsError::Unreadable { .. })
        ));

        let file = |name: &str| dir.join(name).to_string_lossy().into_owned();
        std::fs::write(dir.join("rows.csv"), "name;city\r\nAnn;Rome\r\nBo;\r\n").unwrap();
        let mut rows = read_data_rows(file("rows.csv")).unwrap();
        assert_eq!(rows.rows, [["Ann", "Rome"], ["Bo", ""]]);
        assert_eq!((rows.delimiter.as_str(), rows.crlf), (";", true));
        assert_eq!(data_row_labels(rows.clone()), ["Ann", "Bo"]);
        rows.rows[1][1] = "Oslo; Norway".into();
        write_data_rows(file("rows.csv"), rows).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("rows.csv")).unwrap(),
            "name;city\r\nAnn;Rome\r\nBo;\"Oslo; Norway\"\r\n"
        );
        assert!(matches!(
            read_data_rows(file("none.csv")),
            Err(DataFileError::Invalid { code, .. }) if code == "data.unreadable"
        ));
        std::fs::write(dir.join("bad.csv"), "a,b\n1\n").unwrap();
        let Err(DataFileError::Invalid { code, params, .. }) = read_data_rows(file("bad.csv"))
        else {
            panic!("a short row is an error")
        };
        assert_eq!(
            (code.as_str(), params["line"].as_str()),
            ("data.rowLength", "2")
        );
        assert_eq!(
            parse_data_rows("2-3".into()),
            Ok(Some(DataRowRange { first: 1, last: 2 }))
        );
        assert_eq!(parse_data_rows("all".into()), Ok(None));
        assert!(parse_data_rows("0".into()).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
