//! Settings a project shares through its `jevision.project.json`: default retries, the tests
//! in quarantine, and whether screenshots may go to AI models outside this computer.

use std::path::Path;

use jevision_core::{MAX_RETRIES, PROJECT_FILE, ProjectSettings, Quarantined, Rect};

/// The project settings file can't be read or written, or the change isn't allowed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum ProjectSettingsError {
    #[error("{file} can't be read: {reason}")]
    Unreadable { file: String, reason: String },
    #[error("{reason}")]
    Rejected { reason: String },
    #[error("{file} can't be saved: {reason}")]
    Unwritable { file: String, reason: String },
}

#[uniffi::remote(Record)]
pub struct ProjectSettings {
    pub retries: Option<u32>,
    pub quarantine: Vec<Quarantined>,
    #[uniffi(default = false)]
    pub local_only: bool,
    #[uniffi(default = [])]
    pub hide: Vec<Rect>,
}

#[uniffi::remote(Record)]
pub struct Quarantined {
    pub test: String,
    pub reason: Option<String>,
}

/// The settings of the project folder `project`; the defaults when it has no settings file.
#[uniffi::export]
pub fn project_settings(project: String) -> Result<ProjectSettings, ProjectSettingsError> {
    load(Path::new(&project))
}

/// Puts the test file `test`, relative to the project folder, in quarantine with an optional
/// reason, or changes its reason. Returns the saved settings.
#[uniffi::export]
pub fn quarantine_test(
    project: String,
    test: String,
    reason: Option<String>,
) -> Result<ProjectSettings, ProjectSettingsError> {
    change(&project, |s| {
        s.quarantine(&test, reason.as_deref())
            .map_err(|reason| ProjectSettingsError::Rejected { reason })
    })
}

/// Takes the test file `test`, relative to the project folder, out of quarantine. Returns the
/// saved settings.
#[uniffi::export]
pub fn release_from_quarantine(
    project: String,
    test: String,
) -> Result<ProjectSettings, ProjectSettingsError> {
    change(&project, |s| {
        s.release(&test);
        Ok(())
    })
}

/// Sets how many times a failed test of the project is retried when its file doesn't say;
/// `None` retries none. Returns the saved settings.
#[uniffi::export]
pub fn set_default_retries(
    project: String,
    retries: Option<u32>,
) -> Result<ProjectSettings, ProjectSettingsError> {
    change(&project, |s| {
        if retries.is_some_and(|r| r > MAX_RETRIES) {
            return Err(ProjectSettingsError::Rejected {
                reason: format!("Retries must be from 0 to {MAX_RETRIES}."),
            });
        }
        s.retries = retries;
        Ok(())
    })
}

/// Lets the project's screenshots go only to AI models on this computer, or to any. Returns
/// the saved settings.
#[uniffi::export]
pub fn set_project_local_only(
    project: String,
    local_only: bool,
) -> Result<ProjectSettings, ProjectSettingsError> {
    change(&project, |s| {
        s.local_only = local_only;
        Ok(())
    })
}

/// Name of the project settings file in a project folder.
#[uniffi::export]
pub fn project_settings_file_name() -> String {
    PROJECT_FILE.to_owned()
}

fn load(project: &Path) -> Result<ProjectSettings, ProjectSettingsError> {
    ProjectSettings::load(project).map_err(|reason| ProjectSettingsError::Unreadable {
        file: PROJECT_FILE.to_owned(),
        reason,
    })
}

/// Reads the settings, applies `edit` and saves them, leaving the file alone when nothing
/// changed. A file that can't be read isn't overwritten.
fn change(
    project: &str,
    edit: impl FnOnce(&mut ProjectSettings) -> Result<(), ProjectSettingsError>,
) -> Result<ProjectSettings, ProjectSettingsError> {
    let project = Path::new(project);
    let mut settings = load(project)?;
    let before = settings.clone();
    edit(&mut settings)?;
    if settings != before {
        settings
            .save(project)
            .map_err(|e| ProjectSettingsError::Unwritable {
                file: PROJECT_FILE.to_owned(),
                reason: e.to_string(),
            })?;
    }
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changes_are_saved_and_a_broken_file_is_kept() {
        let dir = std::env::temp_dir().join(format!("jevision-ffi-project-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let project = dir.to_string_lossy().into_owned();
        let settings = quarantine_test(
            project.clone(),
            "login.flow.json".into(),
            Some("Bug 7".into()),
        )
        .unwrap();
        assert_eq!(settings.quarantine.len(), 1);
        assert_eq!(
            set_default_retries(project.clone(), Some(2))
                .unwrap()
                .retries,
            Some(2)
        );
        assert!(matches!(
            set_default_retries(project.clone(), Some(11)),
            Err(ProjectSettingsError::Rejected { .. })
        ));
        assert!(matches!(
            quarantine_test(project.clone(), "../x.flow.json".into(), None),
            Err(ProjectSettingsError::Rejected { .. })
        ));
        assert!(
            set_project_local_only(project.clone(), true)
                .unwrap()
                .local_only
        );
        let read = project_settings(project.clone()).unwrap();
        assert_eq!((read.retries, read.quarantine.len()), (Some(2), 1));
        assert!(read.local_only);
        assert!(
            release_from_quarantine(project.clone(), "login.flow.json".into())
                .unwrap()
                .quarantine
                .is_empty()
        );

        std::fs::write(dir.join(PROJECT_FILE), "{").unwrap();
        assert!(matches!(
            release_from_quarantine(project.clone(), "login.flow.json".into()),
            Err(ProjectSettingsError::Unreadable { .. })
        ));
        assert_eq!(
            std::fs::read_to_string(dir.join(PROJECT_FILE)).unwrap(),
            "{"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
