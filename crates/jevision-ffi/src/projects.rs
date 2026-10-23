//! The projects screen: the list of recent projects, a summary of each for its card, a new
//! project or a copy of a repository, and the project a folder or file opened from the Finder
//! stands for.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use jevision_core::{
    CreatedProject, ProjectLocation, ProjectStarter, RecentProject, TEST_SUFFIX, project_files,
};
use jevision_git::{CloneFailure, GitError};
use jevision_l10n::tr;
use jevision_runner::{Run, Verdict};

use crate::git::git;

/// Most groups of tests a [`ProjectSummary`] shows.
const SUMMARY_FOLDERS: usize = 6;
/// Most tests a group of a [`ProjectSummary`] shows.
const SUMMARY_TESTS: usize = 12;

#[uniffi::remote(Record)]
pub struct RecentProject {
    pub path: String,
    pub name: String,
    pub bookmark: Vec<u8>,
    pub pinned: bool,
    pub opened_at: i64,
}

/// The list of recent projects from the text [`recent_projects_encode`] wrote, which the app
/// keeps in its settings. Text that isn't a list reads as an empty one; a folder listed twice
/// is kept once.
///
/// Every `recent_projects_…` function returns the list in the order it is shown: pinned
/// projects first, then the most recently opened. A folder is in it once, by its absolute path
/// without a `/` at the end, doubled `/` or `.` parts; paths are otherwise compared exactly.
/// Besides the pinned projects the list keeps the 30 opened last.
#[uniffi::export]
pub fn recent_projects_decode(json: String) -> Vec<RecentProject> {
    jevision_core::recent_projects_decode(&json)
}

/// The text to keep the list in, for [`recent_projects_decode`].
#[uniffi::export]
pub fn recent_projects_encode(list: Vec<RecentProject>) -> String {
    jevision_core::recent_projects_encode(&list)
}

/// The list after the project in `path` was opened at `now` (seconds since 1970): added, or
/// moved up with its new name and bookmark. An empty `name` stands for the folder's name and
/// an empty `bookmark` keeps the one the list has. A pinned project stays pinned. A path that
/// isn't absolute changes nothing.
#[uniffi::export]
pub fn recent_projects_opened(
    list: Vec<RecentProject>,
    path: String,
    name: String,
    bookmark: Vec<u8>,
    now: i64,
) -> Vec<RecentProject> {
    jevision_core::recent_projects_opened(list, &path, &name, bookmark, now)
}

/// The list with the project in `path` pinned or not. Unpinned, it takes its place by when it
/// was opened, and leaves the list if 30 others were opened after it.
#[uniffi::export]
pub fn recent_projects_set_pinned(
    list: Vec<RecentProject>,
    path: String,
    pinned: bool,
) -> Vec<RecentProject> {
    jevision_core::recent_projects_set_pinned(list, &path, pinned)
}

/// The list without the project in `path`; its folder stays where it is.
#[uniffi::export]
pub fn recent_projects_removed(list: Vec<RecentProject>, path: String) -> Vec<RecentProject> {
    jevision_core::recent_projects_removed(list, &path)
}

/// The list after a project's folder moved from `old_path` to `new_path`, where it's called
/// `name` (empty for the folder's name) and opened with `bookmark` (empty to keep the one the
/// list has). It stays pinned and keeps when it was opened; if the list already had
/// `new_path`, the two become one.
#[uniffi::export]
pub fn recent_projects_relocated(
    list: Vec<RecentProject>,
    old_path: String,
    new_path: String,
    name: String,
    bookmark: Vec<u8>,
) -> Vec<RecentProject> {
    jevision_core::recent_projects_relocated(list, &old_path, &new_path, &name, bookmark)
}

/// The list with only its pinned projects, as File → Open Recent → Clear Menu leaves it.
#[uniffi::export]
pub fn recent_projects_cleared(list: Vec<RecentProject>) -> Vec<RecentProject> {
    jevision_core::recent_projects_cleared(list)
}

/// The projects a search for `query` finds: those whose name or folder has every word of it,
/// whatever the case and accents (`cafe` finds `Café`, `елка` finds `Ёлка`). A blank query
/// finds them all.
#[uniffi::export]
pub fn recent_projects_matching(list: Vec<RecentProject>, query: String) -> Vec<RecentProject> {
    jevision_core::recent_projects_matching(list, &query)
}

/// A project at a glance, for its card on the projects screen.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ProjectSummary {
    /// Whether the folder is there. When it isn't (moved, deleted, a disk not connected) the
    /// rest is empty but `name`.
    pub exists: bool,
    /// The folder's name.
    pub name: String,
    /// Tests in the project, at any depth.
    pub test_count: u32,
    /// The git branch checked out; `None` outside git, and when no branch is, as in the middle
    /// of a rebase.
    pub branch: Option<String>,
    /// The project's latest run, of any test.
    pub last_run: Option<ProjectLastRun>,
    /// A preview of the tests: the ones at the top of the project first, then the first
    /// folders by name, 6 groups at most, each with its first 12 tests.
    pub folders: Vec<ProjectFolder>,
}

/// The latest run of a project.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ProjectLastRun {
    /// The test's name, as the run saved it.
    pub test: String,
    /// The test file, relative to the project with `/`.
    pub file: String,
    pub verdict: Verdict,
    /// When the run ended, in seconds since 1970; `None` when its report has no readable time.
    pub finished_at: Option<i64>,
}

/// A group of tests in a [`ProjectSummary`]: a folder at the top of the project with
/// everything inside it, or the tests at the top of the project.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ProjectFolder {
    /// The folder, relative to the project; empty for the tests at the top of the project.
    pub path: String,
    /// All the tests in the group, more than `tests` has when there are over 12.
    pub test_count: u32,
    /// The first tests of the group by path.
    pub tests: Vec<ProjectTest>,
}

/// A test in a [`ProjectFolder`].
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ProjectTest {
    /// The test file, relative to the project with `/`.
    pub file: String,
    /// How its latest run ended; `None` when it hasn't run.
    pub last: Option<Verdict>,
}

/// A summary of the project in the folder `root` for its card. Reads only the folder, the run
/// reports it needs and git's `HEAD` file, without running anything, so it's quick for a
/// project with thousands of tests and runs; still, call it off the main thread.
#[uniffi::export]
pub fn project_summary(root: String) -> ProjectSummary {
    let root = Path::new(&root);
    let name = root
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    if !root.is_dir() {
        return ProjectSummary {
            exists: false,
            name,
            test_count: 0,
            branch: None,
            last_run: None,
            folders: Vec::new(),
        };
    }
    let files = project_files(root, &[TEST_SUFFIX]);
    // Tests at the top go first: a path without `/` sorts among the folders by its name.
    let (top, inside): (Vec<&String>, Vec<&String>) =
        files.iter().partition(|file| !file.contains('/'));
    let mut groups: Vec<(&str, Vec<&String>)> = Vec::new();
    if !top.is_empty() {
        groups.push(("", top));
    }
    for file in inside {
        let folder = file.split('/').next().unwrap_or_default();
        let full = groups.len() == SUMMARY_FOLDERS;
        match groups.last_mut() {
            Some((last, tests)) if *last == folder => tests.push(file),
            _ if full => break,
            _ => groups.push((folder, vec![file])),
        }
    }
    let shown: Vec<String> = groups
        .iter()
        .flat_map(|(_, tests)| tests.iter().take(SUMMARY_TESTS).map(|f| (*f).clone()))
        .collect();
    let latest = Run::latest(root, &shown);
    let folders = groups
        .iter()
        .map(|(path, tests)| ProjectFolder {
            path: (*path).to_owned(),
            test_count: count(tests.len()),
            tests: tests
                .iter()
                .take(SUMMARY_TESTS)
                .map(|file| ProjectTest {
                    file: (*file).clone(),
                    last: latest.get(*file).map(|run| run.report.verdict),
                })
                .collect(),
        })
        .collect();
    ProjectSummary {
        exists: true,
        name,
        test_count: count(files.len()),
        branch: jevision_git::head_branch(root),
        last_run: Run::newest(root).map(|run| ProjectLastRun {
            finished_at: run.report.finished_at(),
            test: run.report.test,
            file: run.report.file,
            verdict: run.report.verdict,
        }),
        folders,
    }
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

#[uniffi::remote(Enum)]
pub enum ProjectStarter {
    Empty,
    CopyOf { folder: String },
}

#[uniffi::remote(Record)]
pub struct CreatedProject {
    pub path: String,
    pub git: bool,
    pub first_test: Option<String>,
}

/// A new project or its `.gitignore` couldn't be made; `reason` says why in the user's
/// language.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum ProjectFolderError {
    #[error("{reason}")]
    Failed { reason: String },
}

/// Folders to offer for a new project, best first: `Documents/JEVision` in the home folder
/// `home`, then `Documents/GitHub` when there is one (GitHub Desktop keeps repositories
/// there). The first may not be there yet: [`create_project`] makes it.
#[uniffi::export]
pub fn project_locations(home: String) -> Vec<String> {
    jevision_core::project_locations(Path::new(&home))
}

/// Why `name` can't be the folder of a new project in `parent`, in the user's language, to
/// show under the name field; `None` when it can. Spaces around the name don't count. Says
/// so for an empty name, one with `/`, `:` or a line break, one starting with a dot, one too
/// long for macOS, and a folder that is already there with something in it.
#[uniffi::export]
pub fn project_name_problem(parent: String, name: String) -> Option<String> {
    jevision_core::project_name_problem(Path::new(&parent), &name)
}

/// Makes the project folder `name` in `parent` (made too when it isn't there) from `starter`:
/// empty, or a copy of a folder such as the bundled example or a team's template, without its
/// `.git`, `.jevision`, `.DS_Store` and symbolic links that lead out of it. The project gets
/// a `jevision.project.json` when it has none and a `.gitignore` that keeps run reports,
/// videos, the Finder's `.DS_Store` and a `.env` file out of git. It becomes a new git
/// repository, with nothing committed yet, when git is installed and `parent` isn't in a
/// repository already. The folder shows up whole or not at all.
#[uniffi::export]
pub fn create_project(
    parent: String,
    name: String,
    starter: ProjectStarter,
) -> Result<CreatedProject, ProjectFolderError> {
    let parent = Path::new(&parent);
    jevision_core::create_project(parent, &name, &starter, |folder| {
        history_in(parent) == NewProjectHistory::Own
            && git().is_ok_and(|git| git.init(folder).is_ok())
    })
    .map_err(|reason| ProjectFolderError::Failed { reason })
}

/// Where the history of changes of a project [`create_project`] would make in a folder is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum NewProjectHistory {
    /// In a new git repository of the project's own.
    Own,
    /// In the git repository the folder is in already.
    Enclosing,
    /// Nowhere: git isn't installed on this Mac.
    Unavailable,
}

/// Where the history of a project made in `parent` by [`create_project`] would be kept, to
/// say so before it is made. `parent` needn't be there yet.
#[uniffi::export]
pub fn new_project_history(parent: String) -> NewProjectHistory {
    history_in(Path::new(&parent))
}

fn history_in(parent: &Path) -> NewProjectHistory {
    let Ok(git) = git() else {
        return NewProjectHistory::Unavailable;
    };
    // Folders still to be made can't be in a repository the nearest one that's there isn't in.
    let existing = parent.ancestors().find(|p| p.is_dir()).unwrap_or(parent);
    if matches!(git.open(existing), Ok(None)) {
        NewProjectHistory::Own
    } else {
        NewProjectHistory::Enclosing
    }
}

#[uniffi::remote(Record)]
pub struct ProjectLocation {
    pub root: String,
    pub test: Option<String>,
}

/// The project to open for `path`, a folder or file opened from the Finder or dropped on the
/// app's icon:
/// - a folder opens the nearest project around it: the closest folder, itself or one above,
///   with a `jevision.project.json`, a `.jevision` folder or a `.git`, looking no higher than
///   inside the home folder and the disk it's on; with none, the folder itself;
/// - a test file (`*.flow.json`) opens the project around its folder, at that test;
/// - a `jevision.project.json` opens its folder.
///
/// `None` for anything else and for a path that isn't there.
#[uniffi::export]
pub fn project_location_for(path: String) -> Option<ProjectLocation> {
    let home = std::env::var_os("HOME").unwrap_or_else(|| "/".into());
    jevision_core::project_location_for(Path::new(&path), Path::new(&home))
}

/// Whether `project` is in a git repository that doesn't keep its run folder (`.jevision`, with
/// reports, screenshots and videos) out of commits yet. Reads the ignore files instead of
/// running git.
#[uniffi::export]
pub fn gitignore_needs_run_folder(project: String) -> bool {
    jevision_core::gitignore_needs_run_folder(Path::new(&project))
}

/// Adds the run folder to the project's own `.gitignore`, making the file when there is none;
/// nothing changes when it's there already.
#[uniffi::export]
pub fn gitignore_add_run_folder(project: String) -> Result<(), ProjectFolderError> {
    jevision_core::gitignore_add(Path::new(&project), &[jevision_core::GITIGNORE_RUN_FOLDER])
        .map_err(|e| ProjectFolderError::Failed {
            reason: tr!("project.file.unwritable", ".gitignore", e.to_string()),
        })
}

/// The folder name a copy of the repository at `url` gets, to offer as the project's name:
/// `https://github.com/acme/shop-tests.git` → `shop-tests`. Empty when the address has none.
#[uniffi::export]
pub fn clone_folder_name(url: String) -> String {
    jevision_git::clone_folder_name(&url)
}

/// The repository address `text` is, trimmed, to fill in for a copy when the user has one on
/// the clipboard; `None` for other text, such as a sentence or a web site.
#[uniffi::export]
pub fn repository_address(text: String) -> Option<String> {
    jevision_git::repository_address(&text)
}

/// What kept a repository from being copied, for the app to choose what to offer next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum CloneProblem {
    /// Git isn't installed on this Mac.
    NotInstalled,
    /// Nothing is at the address, or the account can't see it.
    NotFound,
    /// The server didn't let the user in: signing in to GitHub may help.
    NoAccess,
    /// The server can't be reached.
    NoNetwork,
    /// The folder to copy into has something in it.
    FolderExists,
    /// The folder name can't be used, or the address is empty.
    InvalidName,
    Other,
}

/// Why [`RepositoryClone::run`] didn't copy the repository. Nothing it made is left on disk.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum CloneError {
    /// [`RepositoryClone::cancel`] stopped it.
    #[error("{}", tr!("clone.cancelled"))]
    Cancelled,
    /// `message` says what went wrong in the user's language; `detail` is git's own line
    /// about it, for those who want to know more, empty when there is none.
    #[error("{message}")]
    Failed {
        problem: CloneProblem,
        message: String,
        detail: String,
    },
}

impl CloneError {
    fn new(problem: CloneProblem, message: String) -> Self {
        Self::Failed {
            problem,
            message,
            detail: String::new(),
        }
    }
}

/// Copies a repository from a server into a new project folder. Call [`Self::run`] off the
/// main thread; [`Self::cancel`] may be called meanwhile from any thread. One object copies
/// one repository.
#[derive(uniffi::Object)]
pub struct RepositoryClone {
    cancelled: AtomicBool,
}

#[uniffi::export]
impl RepositoryClone {
    #[uniffi::constructor]
    pub fn new() -> Self {
        Self {
            cancelled: AtomicBool::new(false),
        }
    }

    /// Copies the repository at `url` into the new folder `name` in `parent`, made when it
    /// isn't there, and returns the folder. Waits until the copy is done. Nothing is asked
    /// on the way: the user's GitHub sign-in is on offer, a server reached over ssh for the
    /// first time is trusted, and where a password would be wanted the copy fails with
    /// [`CloneProblem::NoAccess`]. The copy isn't changed: no `.gitignore` or settings file
    /// is added.
    pub fn run(&self, url: String, parent: String, name: String) -> Result<String, CloneError> {
        let url = url.trim();
        if url.is_empty() {
            return Err(CloneError::new(
                CloneProblem::InvalidName,
                tr!("clone.url.empty"),
            ));
        }
        let into = Path::new(&parent).join(name.trim());
        if let Some(problem) = jevision_core::project_name_problem(Path::new(&parent), &name) {
            let kind = if into.exists() && !name.trim().is_empty() {
                CloneProblem::FolderExists
            } else {
                CloneProblem::InvalidName
            };
            return Err(CloneError::new(kind, problem));
        }
        let git = git().map_err(|_| {
            CloneError::new(CloneProblem::NotInstalled, tr!("clone.error.notInstalled"))
        })?;
        match git.clone_repository(url, &into, &self.cancelled) {
            Ok(_) => Ok(into.to_string_lossy().into_owned()),
            Err(CloneFailure::Cancelled) => Err(CloneError::Cancelled),
            Err(CloneFailure::FolderExists) => Err(CloneError::new(
                CloneProblem::FolderExists,
                tr!("project.name.taken", name.trim()),
            )),
            Err(CloneFailure::Git { error, detail }) => {
                let (problem, message) = match error {
                    GitError::NotInstalled => {
                        (CloneProblem::NotInstalled, tr!("clone.error.notInstalled"))
                    }
                    GitError::RemoteNotFound => {
                        (CloneProblem::NotFound, tr!("clone.error.notFound"))
                    }
                    GitError::SignIn => (CloneProblem::NoAccess, tr!("clone.error.noAccess")),
                    GitError::Unreachable => {
                        (CloneProblem::NoNetwork, tr!("clone.error.noNetwork"))
                    }
                    other => (
                        CloneProblem::Other,
                        tr!("clone.error.failed", other.to_string()),
                    ),
                };
                Err(CloneError::Failed {
                    problem,
                    message,
                    detail,
                })
            }
        }
    }

    /// Stops the copy: git is stopped, the half-made folder removed, and [`Self::run`]
    /// returns [`CloneError::Cancelled`].
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

impl Default for RepositoryClone {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use super::*;

    /// A scratch folder, removed with everything in it at the end of the test.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "jevision-ffi-projects-{}-{name}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(fs::canonicalize(dir).unwrap())
        }

        fn write(&self, path: &str, text: &str) {
            let file = self.0.join(path);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, text).unwrap();
        }

        fn path(&self, path: &str) -> String {
            self.0.join(path).to_string_lossy().into_owned()
        }

        /// A saved run of `file` that started at `second` (a number of seconds into a day).
        fn run(&self, project: &str, file: &str, second: u32, verdict: &str) {
            let (h, m, s) = (second / 3600, second / 60 % 60, second % 60);
            let slug = file
                .rsplit('/')
                .next()
                .unwrap()
                .trim_end_matches(".flow.json");
            self.write(
                &format!("{project}/.jevision/runs/20261018-{h:02}{m:02}{s:02}-{slug}/report.json"),
                &format!(
                    r#"{{"format":1,"test":"{slug}","file":"{file}","app":"a",
                    "startedAt":"2026-10-18T{h:02}:{m:02}:{s:02}.000Z","durationMs":2000,
                    "verdict":"{verdict}","flaky":false,"virtualMachine":false,"attempts":[]}}"#
                ),
            );
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_card_shows_the_tests_their_runs_and_the_branch() {
        let s = Scratch::new("summary");
        for file in [
            "top.flow.json",
            "a/one.flow.json",
            "a/deep/two.flow.json",
            "b/three.flow.json",
        ] {
            s.write(&format!("p/{file}"), "{}");
        }
        s.write("p/a/notes.txt", "");
        s.write("p/.git/HEAD", "ref: refs/heads/feature/cards\n");
        s.run("p", "a/one.flow.json", 60, "failed");
        s.run("p", "a/one.flow.json", 120, "passed");
        s.run("p", "top.flow.json", 90, "blocked");

        let summary = project_summary(s.path("p"));
        let test = |file: &str, last: Option<Verdict>| ProjectTest {
            file: file.into(),
            last,
        };
        assert_eq!(
            summary,
            ProjectSummary {
                exists: true,
                name: "p".into(),
                test_count: 4,
                branch: Some("feature/cards".into()),
                last_run: Some(ProjectLastRun {
                    test: "one".into(),
                    file: "a/one.flow.json".into(),
                    verdict: Verdict::Passed,
                    finished_at: Some(1_792_281_722),
                }),
                folders: vec![
                    ProjectFolder {
                        path: String::new(),
                        test_count: 1,
                        tests: vec![test("top.flow.json", Some(Verdict::Blocked))],
                    },
                    ProjectFolder {
                        path: "a".into(),
                        test_count: 2,
                        tests: vec![
                            test("a/deep/two.flow.json", None),
                            test("a/one.flow.json", Some(Verdict::Passed)),
                        ],
                    },
                    ProjectFolder {
                        path: "b".into(),
                        test_count: 1,
                        tests: vec![test("b/three.flow.json", None)],
                    },
                ],
            }
        );
        let gone = project_summary(s.path("gone"));
        assert!(!gone.exists && gone.name == "gone" && gone.folders.is_empty());
    }

    #[test]
    fn a_card_of_a_big_project_shows_a_few_tests_and_reads_little() {
        let s = Scratch::new("big");
        // 1000 tests in 10 folders, each run once, and the first folder's tests 1000 times more.
        for folder in 0..10 {
            for n in 0..100 {
                let file = format!("f{folder}/t{n:03}.flow.json");
                s.write(&format!("p/{file}"), "{}");
                s.run(
                    "p",
                    &file,
                    folder * 100 + n,
                    if n % 2 == 0 { "passed" } else { "failed" },
                );
            }
        }
        for n in 0..1000 {
            s.run(
                "p",
                &format!("f0/t{:03}.flow.json", n % 100),
                2000 + n,
                "cancelled",
            );
        }
        let started = Instant::now();
        let summary = project_summary(s.path("p"));
        // Generous: reading every report or walking the runs for each test would still pass
        // a tight limit on a fast disk, but not this many folders on a slow one.
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
        assert_eq!(summary.test_count, 1000);
        assert_eq!(summary.folders.len(), SUMMARY_FOLDERS);
        assert!(summary.folders.iter().all(|f| f.test_count == 100));
        assert!(
            summary
                .folders
                .iter()
                .all(|f| f.tests.len() == SUMMARY_TESTS)
        );
        assert_eq!(summary.folders[5].path, "f5");
        assert_eq!(
            summary.folders[0].tests[3],
            ProjectTest {
                file: "f0/t003.flow.json".into(),
                last: Some(Verdict::Cancelled),
            }
        );
        assert_eq!(summary.folders[1].tests[3].last, Some(Verdict::Failed));
        assert_eq!(summary.folders[1].tests[4].last, Some(Verdict::Passed));
        let last = summary.last_run.unwrap();
        assert_eq!(
            (last.file.as_str(), last.verdict),
            ("f0/t099.flow.json", Verdict::Cancelled)
        );
    }

    #[test]
    fn a_new_project_and_its_run_folder_line() {
        let s = Scratch::new("create");
        let made = create_project(
            s.path("Documents/JEVision"),
            "Shop".into(),
            ProjectStarter::Empty,
        )
        .unwrap();
        assert_eq!(made.path, s.path("Documents/JEVision/Shop"));
        assert_eq!(made.git, git().is_ok());
        assert_eq!(
            new_project_history(s.path("Documents/JEVision")),
            if made.git {
                NewProjectHistory::Own
            } else {
                NewProjectHistory::Unavailable
            }
        );
        assert!(!gitignore_needs_run_folder(made.path.clone()));
        assert_eq!(
            create_project(
                s.path("Documents/JEVision"),
                "Shop".into(),
                ProjectStarter::Empty
            ),
            Err(ProjectFolderError::Failed {
                reason: tr!("project.name.taken", "Shop"),
            })
        );
        // Inside a repository already, a project doesn't get one of its own.
        if made.git {
            for parent in [made.path.clone(), format!("{}/Not/Made/Yet", made.path)] {
                assert_eq!(new_project_history(parent), NewProjectHistory::Enclosing);
            }
            let inner =
                create_project(made.path.clone(), "Inner".into(), ProjectStarter::Empty).unwrap();
            assert!(!inner.git);
            assert!(!Path::new(&inner.path).join(".git").exists());
        }
    }

    #[test]
    fn a_copy_lands_in_its_folder_or_says_what_kept_it() {
        let Ok(git) = git() else { return };
        let s = Scratch::new("clone-file");
        s.write("origin/README.md", "Shop\n");
        git.init(&s.0.join("origin")).unwrap();
        let url = format!("file://{}", s.path("origin"));
        assert_eq!(clone_folder_name(url.clone()), "origin");
        assert_eq!(
            RepositoryClone::new().run(url, s.path("Projects"), " shop ".into()),
            Ok(s.path("Projects/shop"))
        );
        assert!(s.0.join("Projects/shop/.git").is_dir());

        let missing = format!("file://{}", s.path("missing"));
        match RepositoryClone::new().run(missing, s.path("Projects"), "gone".into()) {
            Err(CloneError::Failed {
                problem,
                message,
                detail,
            }) => {
                assert_eq!(problem, CloneProblem::NotFound);
                assert_eq!(message, tr!("clone.error.notFound"));
                assert!(detail.contains("missing"), "{detail}");
            }
            other => panic!("{other:?}"),
        }
        assert!(!s.0.join("Projects/gone").exists());
    }

    #[test]
    fn a_copy_is_refused_before_git_runs_when_the_name_or_address_wont_do() {
        let s = Scratch::new("clone");
        s.write("taken/file.txt", "");
        let copy = RepositoryClone::new();
        let problem = |url: &str, name: &str| match copy.run(url.into(), s.path(""), name.into()) {
            Err(CloneError::Failed { problem, .. }) => problem,
            other => panic!("{other:?}"),
        };
        assert_eq!(problem("  ", "shop"), CloneProblem::InvalidName);
        assert_eq!(
            problem("https://example.com/shop.git", ".shop"),
            CloneProblem::InvalidName
        );
        assert_eq!(
            problem("https://example.com/shop.git", "taken"),
            CloneProblem::FolderExists
        );
        copy.cancel();
        assert_eq!(
            copy.run(
                "https://example.com/shop.git".into(),
                s.path(""),
                "shop".into()
            ),
            if git().is_ok() {
                Err(CloneError::Cancelled)
            } else {
                Err(CloneError::new(
                    CloneProblem::NotInstalled,
                    tr!("clone.error.notInstalled"),
                ))
            }
        );
        assert!(!s.0.join("shop").exists());
    }
}
