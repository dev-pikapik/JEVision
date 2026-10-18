//! Environments of a project and the data rows of a test, for choosing how a run starts.

use std::collections::HashMap;
use std::path::Path;

use jevision_core::Environments;

use crate::{Flow, Issue};

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
    let environments = Environments::load(Path::new(&project)).map_err(|reason| {
        EnvironmentsError::Unreadable {
            file: jevision_core::ENVIRONMENTS_FILE.to_owned(),
            reason,
        }
    })?;
    Ok(environments
        .0
        .into_iter()
        .map(|(name, values)| ProjectEnvironment {
            name,
            values: values.into_iter().collect(),
        })
        .collect())
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

/// Problems with the environment values a test saved in `folder` reads, given its project's
/// environments.
#[uniffi::export]
pub fn environment_issues(flow: Flow, folder: String) -> Vec<Issue> {
    jevision_core::validate_environment_values(&flow, Path::new(&folder))
}

/// A data table of a `useData` block.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DataRows {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// Reads the CSV file at `path`; `None` when it can't be read.
#[uniffi::export]
pub fn read_data_rows(path: String) -> Option<DataRows> {
    let table = jevision_runner::DataTable::load(Path::new(&path)).ok()?;
    Some(DataRows {
        columns: table.columns,
        rows: table.rows,
    })
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

        std::fs::write(dir.join("rows.csv"), "name\nAnn\nBo\n").unwrap();
        let rows = read_data_rows(dir.join("rows.csv").to_string_lossy().into_owned()).unwrap();
        assert_eq!(rows.rows, [["Ann"], ["Bo"]]);
        assert_eq!(
            read_data_rows(dir.join("none.csv").to_string_lossy().into_owned()),
            None
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
