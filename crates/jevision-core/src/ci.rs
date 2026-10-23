//! The GitHub Actions workflow that runs a project's tests, and where it goes in the repository.

use std::fs;
use std::io::{ErrorKind, Write as _};
use std::path::{Path, PathBuf};

use jevision_l10n::tr;

/// The workflow file, relative to the top of the repository.
pub const CI_WORKFLOW_FILE: &str = ".github/workflows/jevision.yml";

/// The name of the artifact the workflow uploads: the app looks for it to bring CI runs in.
pub const CI_ARTIFACT: &str = "jevision-reports";

/// The last result of each test, relative to the project folder; not committed. A run is told
/// apart from the one before by the run folders, and where a CI job starts without them, by
/// this file, which the workflow keeps between jobs.
pub const LAST_RESULTS_FILE: &str = ".jevision/last-results.json";

/// The workflow can't be put into the repository.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CiWorkflowError {
    /// GitHub reads workflows only from the repository's top folder.
    #[error("{}", tr!("ci.notInRepository"))]
    NotInRepository,
    #[error("{}", tr!("ci.exists", CI_WORKFLOW_FILE))]
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
    ];
    let inside = |file: &str| match tests_path {
        "." => file.to_owned(),
        _ => format!("{tests_path}/{file}"),
    };
    // GitHub looks for a cache of this branch first, then of the base and default branches; a
    // branch name in the key could hold a comma, which the cache action refuses.
    let cache = [
        "        with:".to_owned(),
        "          path: |".to_owned(),
        format!("            {}", inside(LAST_RESULTS_FILE)),
        "          key: jevision-last-results-${{ github.run_id }}-${{ github.run_attempt }}"
            .to_owned(),
    ];
    lines.extend([
        "      - name: Restore the last result of each test".to_owned(),
        "        uses: actions/cache/restore@v4".to_owned(),
    ]);
    lines.extend(cache.clone());
    // A file only this Mac has, or one named in the wrong case, stops the job here with the
    // name of the file instead of failing tests one by one.
    lines.extend([
        "          restore-keys: jevision-last-results-".to_owned(),
        "      - name: Check the project carries its files".to_owned(),
        "        run: |".to_owned(),
        format!("          jevision check {}", shell_word(tests_path)),
        "      - name: Run UI tests".to_owned(),
    ]);
    if !secrets.is_empty() {
        lines.push("        env:".to_owned());
        lines.extend(
            secrets
                .iter()
                .map(|name| format!("          {name}: ${{{{ secrets.{name} }}}}")),
        );
    }
    // A block scalar keeps any folder name out of YAML's way; the shell gets it quoted. A fresh
    // checkout has no `.jevision` folder to mark the project, so the run is told where it is:
    // otherwise it could keep its results at the top of the repository, away from the cache.
    let project = shell_word(tests_path);
    lines.push("        run: |".to_owned());
    lines.push(format!(
        "          jevision run --project {project} {project} --junit junit.xml --html reports"
    ));
    lines.extend([
        "      - name: Save the last result of each test".to_owned(),
        "        if: always()".to_owned(),
        "        uses: actions/cache/save@v4".to_owned(),
    ]);
    lines.extend(cache);
    // The run reports live in a hidden folder, which the action skips unless told to include
    // it. A re-run of the same workflow run makes a new artifact under the same name.
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
        format!("            {}", inside(".jevision/runs/")),
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
            plain.contains("          jevision run --project tests/smoke tests/smoke --junit junit.xml --html reports\n")
        );
        assert!(!plain.contains("env:"));
        assert!(plain.contains("runs-on: [self-hosted, macOS]"));

        let names = ["API_KEY".to_owned(), "PASSWORD".to_owned()];
        let spaced = ci_workflow_text("ui tests/it's", &names);
        assert!(
            spaced.contains(
                r#"jevision run --project 'ui tests/it'\''s' 'ui tests/it'\''s' --junit"#
            )
        );
        assert!(spaced.contains(
            "        env:\n          API_KEY: ${{ secrets.API_KEY }}\n          PASSWORD: ${{ secrets.PASSWORD }}\n"
        ));
        assert!(spaced.ends_with("            ui tests/it's/.jevision/runs/\n"));
    }

    #[test]
    fn the_workflow_checks_the_project_carries_its_files_before_the_run() {
        let step = |path: &str| {
            format!(
                "      - name: Check the project carries its files\n        run: |\n          jevision check {path}\n      - name: Run UI tests\n"
            )
        };
        let text = ci_workflow_text("tests/smoke", &[]);
        assert!(text.contains(&step("tests/smoke")), "{text}");
        let odd = ci_workflow_text("ui tests: it's #1", &[]);
        assert!(odd.contains(&step(r#"'ui tests: it'\''s #1'"#)), "{odd}");
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
    fn the_workflow_keeps_the_last_result_of_each_test_between_jobs() {
        let with = "        with:\n          path: |\n            tests/.jevision/last-results.json\n          key: jevision-last-results-${{ github.run_id }}-${{ github.run_attempt }}\n";
        let text = ci_workflow_text("tests", &[]);
        let restore = format!(
            "      - uses: actions/checkout@v4\n      - name: Restore the last result of each test\n        uses: actions/cache/restore@v4\n{with}          restore-keys: jevision-last-results-\n"
        );
        let save = format!(
            "--junit junit.xml --html reports\n      - name: Save the last result of each test\n        if: always()\n        uses: actions/cache/save@v4\n{with}      - uses: actions/upload-artifact@v4\n"
        );
        assert!(text.contains(&restore), "{text}");
        assert!(text.contains(&save), "{text}");
        let top = ci_workflow_text(".", &[]);
        assert!(top.contains("          path: |\n            .jevision/last-results.json\n"));
        assert_eq!(LAST_RESULTS_FILE, ".jevision/last-results.json");
    }

    #[test]
    fn the_run_keeps_its_results_where_the_cache_and_the_artifact_look() {
        // A fresh checkout has no `.jevision` folder to mark the project, and without
        // `--project` the run could take the top of the repository for it.
        for (folder, cached) in [
            ("tests", "tests/.jevision/"),
            (".", ".jevision/"),
            ("ui tests", "ui tests/.jevision/"),
        ] {
            let text = ci_workflow_text(folder, &[]);
            let quoted = shell_word(folder);
            assert!(
                text.contains(&format!(
                    "          jevision run --project {quoted} {quoted} --junit"
                )),
                "{text}"
            );
            assert!(
                text.contains(&format!("            {cached}last-results.json\n")),
                "{text}"
            );
            assert!(
                text.ends_with(&format!("            {cached}runs/\n")),
                "{text}"
            );
        }
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
        assert!(outside.text.contains("jevision run --project . . --junit"));

        fs::create_dir(repo.join(".git")).unwrap();
        let workflow = ci_workflow(&project, &[]);
        assert!(
            workflow
                .text
                .contains("jevision run --project 'ui tests/mac' 'ui tests/mac' --junit")
        );
        assert_eq!(workflow.path, Some(repo.join(CI_WORKFLOW_FILE)));
        assert!(!workflow.exists);
        let top = ci_workflow(&repo, &[]);
        assert!(top.text.contains("jevision run --project . . --junit"));
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
                .contains("jevision run --project . .")
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

    #[test]
    fn errors_read_in_the_language_asked_for() {
        assert_eq!(
            CiWorkflowError::Exists.to_string(),
            format!("{CI_WORKFLOW_FILE} already exists")
        );
        assert_eq!(
            jevision_l10n::tr_in!("ru", "ci.exists", CI_WORKFLOW_FILE),
            format!("{CI_WORKFLOW_FILE} уже существует")
        );
        assert_eq!(
            jevision_l10n::tr_in!("ru", "ci.notInRepository"),
            "папка проекта не находится внутри git-репозитория"
        );
    }
}
