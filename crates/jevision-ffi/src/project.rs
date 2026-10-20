//! Settings a project shares through its `jevision.project.json`: default retries, the tests
//! in quarantine, whether screenshots may go to AI models outside this computer, and the areas
//! covered in every screenshot.

use std::path::Path;

use jevision_l10n::tr;

use jevision_core::{
    AiChoice, AiTask, FolderRunMode, MAX_QUARANTINE_AFTER, MAX_RETRIES, PROJECT_FILE,
    ProjectSettings, Quarantined, Rect, RunMode, default_quarantine_until,
};

/// The project settings file can't be read or written, or the change isn't allowed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum ProjectSettingsError {
    #[error("{}", tr!("project.file.unreadable", .file, .reason))]
    Unreadable { file: String, reason: String },
    #[error("{reason}")]
    Rejected { reason: String },
    #[error("{}", tr!("project.file.unwritable", .file, .reason))]
    Unwritable { file: String, reason: String },
}

#[uniffi::remote(Record)]
pub struct ProjectSettings {
    pub retries: Option<u32>,
    pub quarantine: Vec<Quarantined>,
    #[uniffi(default = None)]
    pub suggest_quarantine_after: Option<u32>,
    #[uniffi(default = false)]
    pub local_only: bool,
    #[uniffi(default = [])]
    pub hide: Vec<Rect>,
    #[uniffi(default = None)]
    pub ai_screen: Option<AiChoice>,
    #[uniffi(default = None)]
    pub ai_text: Option<AiChoice>,
    #[uniffi(default = [])]
    pub run_modes: Vec<FolderRunMode>,
}

/// What a project can choose an AI for.
#[uniffi::remote(Enum)]
pub enum AiTask {
    Screen,
    Text,
}

/// An AI provider by id and one of its models; empty stands for what the app's settings choose.
#[uniffi::remote(Record)]
pub struct AiChoice {
    pub provider: String,
    pub model: String,
}

#[uniffi::remote(Record)]
pub struct FolderRunMode {
    pub folder: String,
    pub mode: RunMode,
}

#[uniffi::remote(Record)]
pub struct Quarantined {
    pub test: String,
    pub reason: Option<String>,
    #[uniffi(default = None)]
    pub until: Option<String>,
}

/// The settings of the project folder `project`; the defaults when it has no settings file.
#[uniffi::export]
pub fn project_settings(project: String) -> Result<ProjectSettings, ProjectSettingsError> {
    load(Path::new(&project))
}

/// Puts the test file `test`, relative to the project folder, in quarantine with an optional
/// reason, until the end of the day `until` (`YYYY-MM-DD`) or without an end, or changes its
/// reason and end. Returns the saved settings.
#[uniffi::export]
pub fn quarantine_test(
    project: String,
    test: String,
    reason: Option<String>,
    until: Option<String>,
) -> Result<ProjectSettings, ProjectSettingsError> {
    change(&project, |s| {
        s.quarantine(&test, reason.as_deref(), until.as_deref())
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

/// Sets after how many flaky runs of a test within 7 days quarantine is suggested; `None` for
/// the default. Returns the saved settings.
#[uniffi::export]
pub fn set_quarantine_threshold(
    project: String,
    after: Option<u32>,
) -> Result<ProjectSettings, ProjectSettingsError> {
    change(&project, |s| {
        if after.is_some_and(|n| !(1..=MAX_QUARANTINE_AFTER).contains(&n)) {
            return Err(ProjectSettingsError::Rejected {
                reason: tr!("project.quarantineAfter.limit", MAX_QUARANTINE_AFTER),
            });
        }
        s.suggest_quarantine_after = after;
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
                reason: tr!("project.retries.limit", MAX_RETRIES),
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

/// Replaces the areas of the window, in fractions from its top-left corner, covered in every
/// screenshot of the project. Returns the saved settings.
#[uniffi::export]
pub fn set_project_hide(
    project: String,
    hide: Vec<Rect>,
) -> Result<ProjectSettings, ProjectSettingsError> {
    change(&project, |s| {
        s.set_hide(hide)
            .map_err(|reason| ProjectSettingsError::Rejected { reason })
    })
}

/// Sets the AI the project asks for `task`, or `None` to leave it to the app's settings.
/// Returns the saved settings.
#[uniffi::export]
pub fn set_project_ai(
    project: String,
    task: AiTask,
    choice: Option<AiChoice>,
) -> Result<ProjectSettings, ProjectSettingsError> {
    change(&project, |s| {
        s.set_ai(task, choice)
            .map_err(|reason| ProjectSettingsError::Rejected { reason })
    })
}

/// The AI `task` asks when a block says `block`: the block's own choice, then the project's,
/// otherwise empty, which stands for what the app's settings choose.
#[uniffi::export]
pub fn resolve_ai_choice(settings: &ProjectSettings, task: AiTask, block: AiChoice) -> AiChoice {
    AiChoice::resolve(&block, settings.ai(task))
}

/// Sets how the tests of `folder`, relative to the project folder (`.` for the project
/// folder), run in a batch: one after another, or side by side up to a limit. Returns the
/// saved settings.
#[uniffi::export]
pub fn set_folder_run_mode(
    project: String,
    folder: String,
    mode: RunMode,
) -> Result<ProjectSettings, ProjectSettingsError> {
    change(&project, |s| {
        s.set_run_mode(&folder, mode)
            .map_err(|reason| ProjectSettingsError::Rejected { reason })
    })
}

/// How the tests of `folder` run: as set for it or the nearest folder above it, otherwise one
/// after another.
#[uniffi::export]
pub fn folder_run_mode(settings: &ProjectSettings, folder: String) -> RunMode {
    settings.run_mode(&folder)
}

/// Whether the quarantine `entry` is in force on `today` (`YYYY-MM-DD`): it has no end or its
/// last day isn't past.
#[uniffi::export]
pub fn quarantine_active(entry: &Quarantined, today: String) -> bool {
    entry.is_active(&today)
}

/// How many flaky runs of a test within 7 days make its quarantine worth suggesting.
#[uniffi::export]
pub fn quarantine_threshold(settings: &ProjectSettings) -> u32 {
    settings.quarantine_threshold()
}

/// Today's date on this computer as `YYYY-MM-DD`.
#[uniffi::export]
pub fn local_today() -> String {
    jevision_runner::today()
}

/// The last day of a quarantine that starts `today` (`YYYY-MM-DD`) and lasts the usual 14 days.
#[uniffi::export]
pub fn default_quarantine_end(today: String) -> Option<String> {
    default_quarantine_until(&today)
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
            Some("2026-11-03".into()),
        )
        .unwrap();
        assert_eq!(settings.quarantine.len(), 1);
        assert_eq!(settings.quarantine[0].until.as_deref(), Some("2026-11-03"));
        assert!(quarantine_active(
            &settings.quarantine[0],
            "2026-11-03".into()
        ));
        assert!(!quarantine_active(
            &settings.quarantine[0],
            "2026-11-04".into()
        ));
        assert!(matches!(
            quarantine_test(
                project.clone(),
                "login.flow.json".into(),
                None,
                Some("soon".into())
            ),
            Err(ProjectSettingsError::Rejected { .. })
        ));
        assert_eq!(
            default_quarantine_end("2026-10-20".into()).as_deref(),
            Some("2026-11-03")
        );
        assert_eq!(quarantine_threshold(&settings), 3);
        assert_eq!(
            set_quarantine_threshold(project.clone(), Some(5))
                .unwrap()
                .suggest_quarantine_after,
            Some(5)
        );
        for bad in [0, 11] {
            assert!(matches!(
                set_quarantine_threshold(project.clone(), Some(bad)),
                Err(ProjectSettingsError::Rejected { .. })
            ));
        }
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
            quarantine_test(project.clone(), "../x.flow.json".into(), None, None),
            Err(ProjectSettingsError::Rejected { .. })
        ));
        assert!(
            set_project_local_only(project.clone(), true)
                .unwrap()
                .local_only
        );
        let corner = Rect {
            x: 0.7,
            y: 0.0,
            width: 0.3,
            height: 0.08,
        };
        assert_eq!(
            set_project_hide(project.clone(), vec![corner])
                .unwrap()
                .hide,
            vec![corner]
        );
        let outside = Rect { x: 0.8, ..corner };
        assert!(matches!(
            set_project_hide(project.clone(), vec![corner, outside]),
            Err(ProjectSettingsError::Rejected { reason }) if reason.starts_with("area 2 ")
        ));
        let read = project_settings(project.clone()).unwrap();
        assert_eq!((read.retries, read.quarantine.len()), (Some(2), 1));
        assert!(read.local_only);
        assert_eq!(read.hide, vec![corner]);
        assert!(
            release_from_quarantine(project.clone(), "login.flow.json".into())
                .unwrap()
                .quarantine
                .is_empty()
        );

        let choice = |provider: &str, model: &str| AiChoice {
            provider: provider.into(),
            model: model.into(),
        };
        let saved = set_project_ai(
            project.clone(),
            AiTask::Screen,
            Some(choice("ollama", "llava")),
        )
        .unwrap();
        assert_eq!(saved.ai_screen, Some(choice("ollama", "llava")));
        assert_eq!(saved.ai_text, None);
        assert!(matches!(
            set_project_ai(project.clone(), AiTask::Text, Some(choice("jev", ""))),
            Err(ProjectSettingsError::Rejected { .. })
        ));
        let file = std::fs::read_to_string(dir.join(PROJECT_FILE)).unwrap();
        assert!(file.contains(r#""provider": "ollama""#), "{file}");
        assert!(!file.contains("key"), "{file}");
        let read = project_settings(project.clone()).unwrap();
        assert_eq!(
            resolve_ai_choice(&read, AiTask::Screen, choice("", "")),
            choice("ollama", "llava")
        );
        assert_eq!(
            resolve_ai_choice(&read, AiTask::Screen, choice("openai", "")),
            choice("openai", "")
        );
        assert_eq!(
            resolve_ai_choice(&read, AiTask::Text, choice("", "gpt-5")),
            choice("", "gpt-5")
        );
        assert_eq!(
            set_project_ai(project.clone(), AiTask::Screen, None)
                .unwrap()
                .ai_screen,
            None
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
