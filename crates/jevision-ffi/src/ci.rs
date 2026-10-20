//! The GitHub Actions workflow that runs a project's tests, written into its repository.

use std::collections::BTreeSet;
use std::path::Path;

use jevision_core::{CiWorkflowError, parse, required_secrets};

/// The workflow file of a project, and whether the repository already has one.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CiWorkflowFile {
    /// Where it goes: in the top folder of the project's git repository; `None` when the
    /// project folder isn't inside one.
    pub path: Option<String>,
    pub text: String,
    pub exists: bool,
}

/// The workflow can't be written.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum CiError {
    /// GitHub reads workflows only from the top folder of a repository.
    #[error("the project folder isn't inside a git repository")]
    NotInRepository,
    /// A workflow with this name is there and wasn't to be replaced.
    #[error("the repository already has this workflow")]
    Exists,
    #[error("{reason}")]
    Unwritable { reason: String },
}

impl From<CiWorkflowError> for CiError {
    fn from(error: CiWorkflowError) -> Self {
        match error {
            CiWorkflowError::NotInRepository => Self::NotInRepository,
            CiWorkflowError::Exists => Self::Exists,
            CiWorkflowError::Unwritable(reason) => Self::Unwritable { reason },
        }
    }
}

/// The workflow for the project, with the secrets its `tests` (test file paths) read. Reads
/// only this Mac.
#[uniffi::export]
pub fn ci_workflow(project: String, tests: Vec<String>) -> CiWorkflowFile {
    let workflow = jevision_core::ci_workflow(Path::new(&project), &secrets(&tests));
    CiWorkflowFile {
        path: workflow
            .path
            .map(|path| path.to_string_lossy().into_owned()),
        text: workflow.text,
        exists: workflow.exists,
    }
}

/// Writes the workflow into the repository and returns its path. A workflow that is already
/// there stays unless `replace`.
#[uniffi::export]
pub fn add_ci_workflow(
    project: String,
    tests: Vec<String>,
    replace: bool,
) -> Result<String, CiError> {
    let path = jevision_core::write_ci_workflow(Path::new(&project), &secrets(&tests), replace)?;
    Ok(path.to_string_lossy().into_owned())
}

/// Every secret the test files read, once each, sorted. Files that can't be read are skipped.
fn secrets(tests: &[String]) -> Vec<String> {
    tests
        .iter()
        .filter_map(|file| parse(&std::fs::read_to_string(file).ok()?).ok())
        .flat_map(|flow| required_secrets(&flow))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_collects_the_secrets_of_readable_tests_only() {
        let dir = std::env::temp_dir().join(format!("jevision-ffi-ci-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        let test = dir.join("a.flow.json");
        std::fs::write(
            &test,
            r#"{"format":1,"name":"T","app":"com.example.App","start":"a","blocks":{
                "a":{"type":"type","text":"${secret.TOKEN} ${secret.ALPHA}"}}}"#,
        )
        .unwrap();
        let tests = vec![
            test.to_string_lossy().into_owned(),
            dir.join("missing.flow.json").to_string_lossy().into_owned(),
        ];
        assert_eq!(secrets(&tests), ["ALPHA", "TOKEN"]);
        let workflow = ci_workflow(dir.to_string_lossy().into_owned(), tests);
        assert!(workflow.text.contains("ALPHA: ${{ secrets.ALPHA }}"));
        assert!(!workflow.exists);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
