//! Files named by their label rather than by their path, saving a test in the lowest format that
//! holds it, and the backups made before a file is written in a newer format.

use std::path::Path;

use jevision_core::{
    Backup, BackupFile, Flow, Issue, MigratedFile, MigrationProblem, MigrationReport, NewLabel,
    Relinked, ResourceStatus, SaveOutcome,
};

use crate::FlowError;

/// Where the file a test names is now. Paths are relative to the project, with `/`.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum Resolution {
    /// The file with the label; `name` is the name it had when it got the label.
    Found { path: String, name: String },
    /// The file lost its label, and `path` has the same content: offer to restore the label
    /// with [`restore_resource_label`].
    Detached { path: String, name: String },
    /// No file has the label or its content: offer to find it with [`relink_resource`].
    /// `name` is the name the file had, or the label when even that is unknown.
    Missing { name: String },
    /// The reference is a path, not a label.
    NotAResource,
}

/// A label, backup or migration that could not be made; `issue` says why.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum ResourceError {
    #[error("{}", jevision_core::sentence(.issue))]
    Failed { issue: Issue },
}

impl From<Issue> for ResourceError {
    fn from(issue: Issue) -> Self {
        Self::Failed { issue }
    }
}

#[uniffi::remote(Enum)]
pub enum Relinked {
    Labeled,
    UseOther { reference: String },
}

#[uniffi::remote(Enum)]
pub enum SaveOutcome {
    Saved,
    NeedsConfirmation { format: u32, description: String },
}

#[uniffi::remote(Record)]
pub struct Backup {
    pub id: String,
    pub created: String,
    pub files: Vec<BackupFile>,
}

#[uniffi::remote(Record)]
pub struct BackupFile {
    pub path: String,
    pub from_format: u32,
    pub to_format: u32,
}

#[uniffi::remote(Record)]
pub struct MigrationReport {
    pub files: Vec<MigratedFile>,
    pub problems: Vec<MigrationProblem>,
    pub backup: Option<String>,
}

#[uniffi::remote(Record)]
pub struct MigratedFile {
    pub path: String,
    pub from_format: u32,
    pub to_format: u32,
    pub labels: Vec<NewLabel>,
    pub new_sidecars: Vec<String>,
}

#[uniffi::remote(Record)]
pub struct NewLabel {
    pub path: String,
    pub reference: Option<String>,
}

#[uniffi::remote(Record)]
pub struct MigrationProblem {
    pub path: String,
    pub issues: Vec<Issue>,
}

/// Where the file `reference`, a label or a path in a test, is in the project folder `project`.
#[uniffi::export]
pub fn resolve_resource(project: String, reference: String) -> Resolution {
    match jevision_core::resolve_resource(Path::new(&project), &reference) {
        None => Resolution::NotAResource,
        Some(ResourceStatus::Found { path, name }) => Resolution::Found { path, name },
        Some(ResourceStatus::Detached { path, name }) => Resolution::Detached { path, name },
        Some(ResourceStatus::Missing { name }) => Resolution::Missing { name },
    }
}

/// Gives `file`, a path in `project` or relative to it, a label, and returns it as `res:…` to
/// write in the test instead of the path. A file that has a label keeps it.
#[uniffi::export]
pub fn label_resource(project: String, file: String) -> Result<String, ResourceError> {
    Ok(jevision_core::label_resource(
        Path::new(&project),
        Path::new(&file),
    )?)
}

/// Labels `file` again with `reference`, for a file that was moved without its `.jevmeta`.
#[uniffi::export]
pub fn restore_resource_label(
    project: String,
    reference: String,
    file: String,
) -> Result<(), ResourceError> {
    Ok(jevision_core::restore_resource_label(
        Path::new(&project),
        &reference,
        Path::new(&file),
    )?)
}

/// Makes `reference` name `file` from now on, for a missing file the person found elsewhere.
/// When `file` has a label of its own, nothing changes and the tests should be changed to use
/// that label instead.
#[uniffi::export]
pub fn relink_resource(
    project: String,
    reference: String,
    file: String,
) -> Result<Relinked, ResourceError> {
    Ok(jevision_core::relink_resource(
        Path::new(&project),
        &reference,
        Path::new(&file),
    )?)
}

/// Takes the label `reference` off its file again, for a label made for a test the person then
/// saved with plain paths; a label another test of `project` names stays. `true` when it went.
#[uniffi::export]
pub fn remove_resource_label(project: String, reference: String) -> Result<bool, ResourceError> {
    Ok(jevision_core::remove_resource_label(
        Path::new(&project),
        &reference,
    )?)
}

/// The name of the file the label `reference` names, for a view that knows no project: as the
/// projects whose labels were read already know it. `None` when none of them has the label.
#[uniffi::export]
pub fn resource_name(reference: String) -> Option<String> {
    jevision_core::resource_name(&reference)
}

/// A test with the path of each labeled file in place of its label.
#[derive(Debug, Clone, uniffi::Record)]
pub struct PlainPaths {
    pub flow: Flow,
    /// The names of the files that are missing, whose labels stay.
    pub missing: Vec<String>,
}

/// `flow`, the test `test` of `project` (relative to it), with the path of each labeled file,
/// from the test's folder, in place of its label: for a project that keeps plain paths, so an
/// older JEVision opens it.
#[uniffi::export]
pub fn plain_paths(project: String, test: String, flow: Flow) -> PlainPaths {
    let project = Path::new(&project);
    let test = project.join(test);
    let mut flow = flow;
    let labels =
        jevision_core::resolve_labels(&mut flow, test.parent().unwrap_or(project), Some(project));
    let missing = (labels.issues.iter())
        .filter(|issue| issue.code == "resource.missing")
        .filter_map(|issue| issue.param("name").map(str::to_owned))
        .collect();
    PlainPaths { flow, missing }
}

/// Reads the labels of `project` again, after files were moved outside the app.
#[uniffi::export]
pub fn refresh_resources(project: String) {
    jevision_core::refresh_resources(Path::new(&project));
}

/// Brings the label of `file` up to date after the file was written again, such as when a new
/// reference picture was accepted; a file without a label is left alone. `true` when the
/// label changed.
#[uniffi::export]
pub fn update_resource_label(file: String) -> Result<bool, ResourceError> {
    Ok(jevision_core::update_resource_label(Path::new(&file))?)
}

/// Checks the label before an editor changes the resource it describes.
#[uniffi::export]
pub fn check_resource_write(file: String) -> Result<(), ResourceError> {
    Ok(jevision_core::check_resource_write(Path::new(&file))?)
}

/// Writes `flow` as the test `path`, relative to `project`, all at once and in the lowest
/// format that holds it. When that format is newer than every test of the project has, nothing
/// is written unless `allow_raise`: ask the person first.
#[uniffi::export]
pub fn save_test(
    project: String,
    path: String,
    flow: Flow,
    allow_raise: bool,
) -> Result<SaveOutcome, FlowError> {
    jevision_core::save_test(Path::new(&project), &path, &flow, allow_raise).map_err(|issue| {
        FlowError::Invalid {
            issues: vec![issue],
        }
    })
}

/// The backups of `project`, newest first.
#[uniffi::export]
pub fn list_backups(project: String) -> Vec<Backup> {
    jevision_core::list_backups(Path::new(&project))
}

/// Puts the files of the backup `id` back, after backing up the files as they are now, and
/// returns their paths relative to `project`.
#[uniffi::export]
pub fn restore_backup(project: String, id: String) -> Result<Vec<String>, ResourceError> {
    Ok(jevision_core::restore_backup(Path::new(&project), &id)?)
}

/// Brings every test of `project` to the newest format, or on a dry run says what that would
/// change without writing anything.
#[uniffi::export]
pub fn migrate_project(project: String, dry_run: bool) -> Result<MigrationReport, ResourceError> {
    Ok(jevision_core::migrate_project(
        Path::new(&project),
        dry_run,
    )?)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn plain_paths_name_each_labeled_file_by_its_path_from_the_test() {
        let root = std::env::temp_dir().join(format!("jevision-plain-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::create_dir_all(root.join("tests")).unwrap();
        fs::create_dir_all(root.join("images")).unwrap();
        fs::write(root.join("images/go.png"), b"go").unwrap();
        let project = root.to_string_lossy().into_owned();
        let go = label_resource(project.clone(), "images/go.png".to_owned()).unwrap();
        let gone = "res:0123456789abcdef";
        let flow = crate::load_flow(format!(
            r#"{{"format":2,"name":"T","app":"com.example.App","start":"a","blocks":{{
            "a":{{"type":"click","target":{{"image":"{go}"}},"next":"b"}},
            "b":{{"type":"click","target":{{"image":"{gone}"}}}}}}}}"#
        ))
        .unwrap();
        let plain = plain_paths(project, "tests/a.flow.json".to_owned(), flow);
        let json = crate::save_flow(plain.flow);
        assert!(json.contains(r#""image": "../images/go.png""#), "{json}");
        assert!(json.contains(gone), "a missing label stays");
        assert_eq!(plain.missing, [gone]);
        let _ = fs::remove_dir_all(&root);
    }
}
