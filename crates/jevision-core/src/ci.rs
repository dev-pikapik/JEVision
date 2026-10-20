//! The GitHub Actions workflow that runs a project's tests, and where it goes in the repository.

use std::fs;
use std::io::{ErrorKind, Write as _};
use std::path::{Path, PathBuf};

/// The workflow file, relative to the top of the repository.
pub const CI_WORKFLOW_FILE: &str = ".github/workflows/jevision.yml";

/// The name of the artifact the workflow uploads: the app looks for it to bring CI runs in.
pub const CI_ARTIFACT: &str = "jevision-reports";

/// The workflow can't be put into the repository.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CiWorkflowError {
    /// GitHub reads workflows only from the repository's top folder.
    #[error("the project folder isn't inside a git repository")]
    NotInRepository,
    #[error("{CI_WORKFLOW_FILE} already exists")]
    Exists,
    #[error("{0}")]
    Unwritable(String),
}

/// The workflow for one project, and the file it belongs in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CiWorkflow {
    /// Where the file goes: [`CI_WORKFLOW_FILE`] in the top folder of the repository; `None`
    /// when the project isn't inside one.
    pub path: Option<PathBuf>,
    pub text: String,
    /// Whether the file is already there.
    pub exists: bool,
}

/// The top folder of the git repository that holds `project`; `None` outside one.
pub fn repository_root(project: &Path) -> Option<PathBuf> {
    let project = fs::canonicalize(project).ok()?;
    project
        .ancestors()
        .find(|folder| folder.join(".git").exists())
        .map(Path::to_path_buf)
}

/// The workflow that runs the tests of `project`, which read the secrets named `secrets`.
pub fn ci_workflow(project: &Path, secrets: &[String]) -> CiWorkflow {
    let root = repository_root(project);
    let relative = root
        .as_ref()
        .zip(fs::canonicalize(project).ok())
        .and_then(|(root, project)| Some(project.strip_prefix(root).ok()?.to_owned()))
        .map(|relative| {
            relative
                .components()
                .map(|part| part.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/")
        })
        .filter(|relative| !relative.is_empty());
    let path = root.map(|root| root.join(CI_WORKFLOW_FILE));
    CiWorkflow {
        exists: path.as_ref().is_some_and(|path| path.exists()),
        text: ci_workflow_text(relative.as_deref().unwrap_or("."), secrets),
        path,
    }
}

/// Writes the workflow into the repository and returns the file. An existing file is replaced
/// only with `replace`; otherwise it stays as it is and [`CiWorkflowError::Exists`] says so.
pub fn write_ci_workflow(
    project: &Path,
    secrets: &[String],
    replace: bool,
) -> Result<PathBuf, CiWorkflowError> {
    let workflow = ci_workflow(project, secrets);
    let path = workflow.path.ok_or(CiWorkflowError::NotInRepository)?;
    let unwritable = |e: std::io::Error| CiWorkflowError::Unwritable(e.to_string());
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder).map_err(unwritable)?;
    }
    if replace {
        fs::write(&path, &workflow.text).map_err(unwritable)?;
    } else {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| match e.kind() {
                ErrorKind::AlreadyExists => CiWorkflowError::Exists,
                _ => unwritable(e),
            })?;
        file.write_all(workflow.text.as_bytes())
            .map_err(unwritable)?;
    }
    Ok(path)
}

/// The workflow text. `tests_path` is the project folder relative to the top of the
/// repository, where CI finds it after checkout. GitHub's own Mac runners have no logged-in
/// user, so the tests run on a self-hosted one.
pub fn ci_workflow_text(tests_path: &str, secrets: &[String]) -> String {
    let mut lines = vec![
        "name: UI tests".to_owned(),
        "on:".to_owned(),
        "  pull_request:".to_owned(),
        "  workflow_dispatch:".to_owned(),
        "jobs:".to_owned(),
        "  ui-tests:".to_owned(),
        "    runs-on: [self-hosted, macOS]".to_owned(),
        "    steps:".to_owned(),
        "      - uses: actions/checkout@v4".to_owned(),
        "      - name: Run UI tests".to_owned(),
    ];
    if !secrets.is_empty() {
        lines.push("        env:".to_owned());
        lines.extend(
            secrets
                .iter()
                .map(|name| format!("          {name}: ${{{{ secrets.{name} }}}}")),
        );
    }
    // A block scalar keeps any folder name out of YAML's way; the shell gets it quoted.
    lines.push("        run: |".to_owned());
    lines.push(format!(
        "          jevision run {} --junit junit.xml --html reports",
        shell_word(tests_path)
    ));
    // The run reports live in a hidden folder, which the action skips unless told to include
    // it. A re-run of the same workflow run makes a new artifact under the same name.
    let runs = match tests_path {
        "." => ".jevision/runs/".to_owned(),
        _ => format!("{tests_path}/.jevision/runs/"),
    };
    lines.extend([
        "      - uses: actions/upload-artifact@v4".to_owned(),
        "        if: always()".to_owned(),
        "        with:".to_owned(),
        format!("          name: {CI_ARTIFACT}"),
        "          include-hidden-files: true".to_owned(),
        "          overwrite: true".to_owned(),
        "          path: |".to_owned(),
        "            junit.xml".to_owned(),
        "            reports/".to_owned(),
        format!("            {runs}"),
    ]);
    lines.join("\n") + "\n"
}

fn shell_word(text: &str) -> String {
    if !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_alphanumeric() || "._-/".contains(c))
    {
        text.to_owned()
    } else {
        format!("'{}'", text.replace('\'', r"'\''"))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    fn folder() -> PathBuf {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "jevision-ci-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        fs::canonicalize(dir).unwrap()
    }

    #[test]
    fn the_workflow_passes_secrets_and_quotes_the_path() {
        let plain = ci_workflow_text("tests/smoke", &[]);
        assert!(
            plain.contains("          jevision run tests/smoke --junit junit.xml --html reports\n")
        );
        assert!(!plain.contains("env:"));
        assert!(plain.contains("runs-on: [self-hosted, macOS]"));

        let names = ["API_KEY".to_owned(), "PASSWORD".to_owned()];
        let spaced = ci_workflow_text("ui tests/it's", &names);
        assert!(spaced.contains(r#"jevision run 'ui tests/it'\''s' --junit"#));
        assert!(spaced.contains(
            "        env:\n          API_KEY: ${{ secrets.API_KEY }}\n          PASSWORD: ${{ secrets.PASSWORD }}\n"
        ));
        assert!(spaced.ends_with("            ui tests/it's/.jevision/runs/\n"));
    }

    #[test]
    fn the_workflow_uploads_the_run_reports_even_when_tests_fail() {
        let step = "      - uses: actions/upload-artifact@v4\n        if: always()\n        with:\n          name: jevision-reports\n          include-hidden-files: true\n          overwrite: true\n          path: |\n            junit.xml\n            reports/\n";
        let top = ci_workflow_text(".", &[]);
        assert!(top.ends_with(&format!("{step}            .jevision/runs/\n")));
        let nested = ci_workflow_text("tests", &[]);
        assert!(nested.ends_with(&format!("{step}            tests/.jevision/runs/\n")));
        assert_eq!(CI_ARTIFACT, "jevision-reports");
    }

    #[test]
    fn it_finds_the_project_inside_its_repository() {
        let repo = folder();
        let project = repo.join("ui tests/mac");
        fs::create_dir_all(&project).unwrap();
        assert_eq!(repository_root(&project), None);
        let outside = ci_workflow(&project, &[]);
        assert_eq!(outside.path, None);
        assert!(!outside.exists);
        assert!(outside.text.contains("jevision run . --junit"));

        fs::create_dir(repo.join(".git")).unwrap();
        let workflow = ci_workflow(&project, &[]);
        assert!(
            workflow
                .text
                .contains("jevision run 'ui tests/mac' --junit")
        );
        assert_eq!(workflow.path, Some(repo.join(CI_WORKFLOW_FILE)));
        assert!(!workflow.exists);
        let top = ci_workflow(&repo, &[]);
        assert!(top.text.contains("jevision run . --junit"));
        fs::remove_dir_all(repo).unwrap();
    }

    #[test]
    fn a_worktree_or_submodule_marks_its_top_with_a_git_file() {
        let repo = folder();
        fs::write(repo.join(".git"), "gitdir: ../elsewhere\n").unwrap();
        assert_eq!(repository_root(&repo), Some(repo.clone()));
        fs::remove_dir_all(repo).unwrap();
    }

    #[test]
    fn it_never_replaces_a_workflow_without_being_told_to() {
        let repo = folder();
        fs::create_dir(repo.join(".git")).unwrap();
        let path = write_ci_workflow(&repo, &[], false).unwrap();
        assert_eq!(path, repo.join(CI_WORKFLOW_FILE));
        assert!(
            fs::read_to_string(&path)
                .unwrap()
                .contains("jevision run .")
        );
        assert!(ci_workflow(&repo, &[]).exists);

        fs::write(&path, "mine").unwrap();
        assert_eq!(
            write_ci_workflow(&repo, &[], false),
            Err(CiWorkflowError::Exists)
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "mine");
        write_ci_workflow(&repo, &["TOKEN".to_owned()], true).unwrap();
        assert!(fs::read_to_string(&path).unwrap().contains("secrets.TOKEN"));
        fs::remove_dir_all(repo).unwrap();
    }

    #[test]
    fn outside_a_repository_nothing_is_written() {
        let dir = folder();
        assert_eq!(
            write_ci_workflow(&dir, &[], false),
            Err(CiWorkflowError::NotInRepository)
        );
        assert!(!dir.join(".github").exists());
        fs::remove_dir_all(dir).unwrap();
    }
}
