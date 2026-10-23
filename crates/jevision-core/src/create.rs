//! Starting a project: where new projects go, whether a name will do, the new folder itself,
//! empty or a copy of another one, and the `.gitignore` that keeps run results out of git.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::{fs, io};

use jevision_l10n::tr;

use crate::atomic::write as write_atomically;
use crate::folder::project_files;
use crate::model::TEST_SUFFIX;
use crate::project::{PROJECT_FILE, ProjectSettings};

/// The `.gitignore` line that keeps the run folder, `.jevision` with its reports, screenshots
/// and videos, out of the repository.
pub const GITIGNORE_RUN_FOLDER: &str = ".jevision/";

/// The lines of a new project's `.gitignore`: run reports and videos, the Finder's folder
/// settings and a local `.env` file of secrets.
pub const NEW_PROJECT_GITIGNORE: [&str; 3] = [GITIGNORE_RUN_FOLDER, ".DS_Store", ".env"];

/// What a copy of a project leaves behind: its history, its runs and the Finder's settings.
const NOT_COPIED: [&str; 3] = [".git", ".jevision", ".DS_Store"];

/// The longest file name macOS allows, in bytes of UTF-8.
const MAX_NAME_BYTES: usize = 255;

/// What a new project starts from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectStarter {
    /// No tests yet.
    Empty,
    /// A copy of the files in the folder `folder`, such as the bundled example or a team's
    /// template.
    CopyOf { folder: String },
}

/// A project [`create_project`] made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedProject {
    /// The new project folder.
    pub path: String,
    /// Whether a new git repository was started in it.
    pub git: bool,
    /// The test to open first, relative to the folder with `/`: one at the top of the project
    /// before one in a folder, then by name. `None` when there are no tests.
    pub first_test: Option<String>,
}

/// Folders to offer for a new project, best first: `Documents/JEVision` in the home folder
/// `home`, then `Documents/GitHub` when there is one (GitHub Desktop keeps repositories there).
/// The first may not exist yet: [`create_project`] makes it.
pub fn project_locations(home: &Path) -> Vec<String> {
    let documents = home.join("Documents");
    let github = documents.join("GitHub");
    let mut places = vec![documents.join("JEVision")];
    if github.is_dir() {
        places.push(github);
    }
    places
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect()
}

/// Why `name` can't be the folder of a new project in `parent`, in the user's language; `None`
/// when it can. Spaces around the name don't count: [`create_project`] drops them. A folder of
/// that name that is empty, but for the Finder's `.DS_Store`, can be used.
pub fn project_name_problem(parent: &Path, name: &str) -> Option<String> {
    let name = name.trim();
    if name.is_empty() {
        Some(tr!("project.name.empty"))
    } else if name.chars().any(|c| c == '/' || c == ':' || c.is_control()) {
        Some(tr!("project.name.characters"))
    } else if name.starts_with('.') {
        Some(tr!("project.name.dot"))
    } else if name.len() > MAX_NAME_BYTES {
        Some(tr!("project.name.long"))
    } else if !is_free(&parent.join(name)) {
        Some(tr!("project.name.taken", name))
    } else {
        None
    }
}

/// Makes the project folder `name` in `parent`, and `parent` too when it isn't there yet.
///
/// The project gets the files of `starter`, a `jevision.project.json` when it has none, and
/// the lines of [`NEW_PROJECT_GITIGNORE`] its `.gitignore` doesn't have yet. A copy leaves out
/// `.git`, `.jevision` and `.DS_Store` at any depth, and symbolic links that lead out of the
/// folder copied. Then `start_git` may start a repository in the folder, before the first
/// commit; it tells whether it did.
///
/// The folder is put together under a hidden name next to where it goes and only then renamed,
/// so it never shows half made: on any error nothing is left behind. The error is a sentence
/// in the user's language.
pub fn create_project(
    parent: &Path,
    name: &str,
    starter: &ProjectStarter,
    start_git: impl FnOnce(&Path) -> bool,
) -> Result<CreatedProject, String> {
    if let Some(problem) = project_name_problem(parent, name) {
        return Err(problem);
    }
    let failed = |e: io::Error| tr!("project.create.failed", e.to_string());
    let source = match starter {
        ProjectStarter::Empty => None,
        ProjectStarter::CopyOf { folder } => Some(
            fs::canonicalize(folder)
                .ok()
                .filter(|f| f.is_dir())
                .ok_or_else(|| tr!("project.template.missing", folder))?,
        ),
    };
    fs::create_dir_all(parent).map_err(failed)?;
    let staging = staging_folder(&fs::canonicalize(parent).map_err(failed)?).map_err(failed)?;
    let target = parent.join(name.trim());
    let mut git = false;
    let made = source
        .as_deref()
        .map_or(Ok(()), |source| {
            copy_folder(source, &staging, source, &staging)
        })
        .and_then(|()| {
            let settings = staging.join(PROJECT_FILE);
            if settings.exists() {
                Ok(())
            } else {
                fs::write(settings, ProjectSettings::default().to_json())
            }
        })
        .and_then(|()| gitignore_add(&staging, &NEW_PROJECT_GITIGNORE))
        .and_then(|()| {
            git = start_git(&staging);
            move_into_place(&staging, &target)
        });
    if let Err(e) = made {
        let _ = fs::remove_dir_all(&staging);
        return Err(failed(e));
    }
    let first_test = project_files(&target, &[TEST_SUFFIX])
        .into_iter()
        .min_by_key(|file| (file.matches('/').count(), file.clone()));
    Ok(CreatedProject {
        path: target.to_string_lossy().into_owned(),
        git,
        first_test,
    })
}

/// Whether `project` is inside a git repository whose ignore files don't keep its run folder
/// out yet: the `.gitignore` files of the project and the folders above it up to the top of
/// the repository, and the repository's `info/exclude`. Reads the files instead of asking git,
/// which may not be installed.
pub fn gitignore_needs_run_folder(project: &Path) -> bool {
    let project: PathBuf = project.components().collect();
    let mut folder = project.as_path();
    loop {
        let anchored = folder == project;
        if ignores_run_folder(&folder.join(".gitignore"), anchored) {
            return false;
        }
        let git = folder.join(".git");
        if git.exists() {
            return !ignores_run_folder(&git.join("info/exclude"), anchored);
        }
        match folder.parent() {
            Some(parent) => folder = parent,
            None => return false,
        }
    }
}

/// Adds the lines of `lines` that the project's own `.gitignore` doesn't have yet at its end,
/// making the file when there is none. A line it has in another spelling, such as
/// `/.jevision` for `.jevision/`, counts as there.
pub fn gitignore_add(project: &Path, lines: &[&str]) -> io::Result<()> {
    let file = project.join(".gitignore");
    let mut text = match fs::read_to_string(&file) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e),
    };
    let mut there: HashSet<String> = text
        .lines()
        .map(|line| ignore_pattern(line).0.to_owned())
        .collect();
    let missing: Vec<&str> = lines
        .iter()
        .copied()
        .filter(|line| there.insert(ignore_pattern(line).0.to_owned()))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    for line in missing {
        text.push_str(line);
        text.push('\n');
    }
    write_atomically(&file, &text)
}

/// Whether the ignore file `file` has a line for the run folder in its own folder. `anchored`
/// allows lines that only apply to that folder itself, like `/.jevision/`.
fn ignores_run_folder(file: &Path, anchored: bool) -> bool {
    let name = GITIGNORE_RUN_FOLDER.trim_end_matches('/');
    fs::read_to_string(file).is_ok_and(|text| {
        text.lines().any(|line| {
            let (ignored, only_here) = ignore_pattern(line);
            ignored == name && (anchored || !only_here)
        })
    })
}

/// What a line of an ignore file leaves out, written the shortest way, and whether only in the
/// folder of that file: `/.jevision/*` is `(".jevision", true)`, `**/.jevision/` is
/// `(".jevision", false)`.
fn ignore_pattern(line: &str) -> (&str, bool) {
    let line = line.trim();
    let (line, anywhere) = match line.strip_prefix("**/") {
        Some(rest) => (rest, true),
        None => (line, false),
    };
    let name = line.trim_start_matches('/');
    let name = name
        .strip_suffix("/**")
        .or_else(|| name.strip_suffix("/*"))
        .unwrap_or(name)
        .trim_end_matches('/');
    (name, !anywhere && line.trim_end_matches('/').contains('/'))
}

/// Whether nothing is at `path`, or an empty folder (but for the Finder's `.DS_Store`).
fn is_free(path: &Path) -> bool {
    match fs::symlink_metadata(path) {
        Err(e) => e.kind() == io::ErrorKind::NotFound,
        Ok(meta) if meta.is_dir() => fs::read_dir(path).is_ok_and(|entries| {
            entries
                .flatten()
                .all(|entry| entry.file_name() == ".DS_Store")
        }),
        Ok(_) => false,
    }
}

/// A new hidden folder in `parent` to put a project together in.
fn staging_folder(parent: &Path) -> io::Result<PathBuf> {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    loop {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let folder = parent.join(format!(".jevision-new-{}-{n}", std::process::id()));
        match fs::create_dir(&folder) {
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            made => return made.map(|()| folder),
        }
    }
}

/// Copies what is in the folder `from` into the folder `to`, but for [`NOT_COPIED`], the folder
/// `skip` and symbolic links that lead out of the folder `root`. Copies can be written to, even
/// of files that couldn't.
fn copy_folder(from: &Path, to: &Path, root: &Path, skip: &Path) -> io::Result<()> {
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name();
        let path = entry.path();
        if NOT_COPIED.iter().any(|n| name == *n) || path == skip {
            continue;
        }
        let kind = entry.file_type()?;
        let copy = to.join(&name);
        if kind.is_dir() {
            fs::create_dir(&copy)?;
            copy_folder(&path, &copy, root, skip)?;
        } else if kind.is_file() {
            fs::copy(&path, &copy)?;
            make_writable(&copy)?;
        } else if kind.is_symlink() {
            copy_link(&path, &copy, root)?;
        }
    }
    Ok(())
}

/// Copies the symbolic link `link` when it leads, by a relative path, to something inside the
/// folder `root`; otherwise leaves it out.
#[cfg(unix)]
fn copy_link(link: &Path, copy: &Path, root: &Path) -> io::Result<()> {
    let target = fs::read_link(link)?;
    let inside = target.is_relative()
        && fs::canonicalize(link).is_ok_and(|resolved| resolved.starts_with(root));
    if inside {
        std::os::unix::fs::symlink(target, copy)?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn copy_link(_: &Path, _: &Path, _: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn make_writable(file: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(file)?.permissions();
    if permissions.mode() & 0o200 == 0 {
        permissions.set_mode(permissions.mode() | 0o200);
        fs::set_permissions(file, permissions)?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn make_writable(file: &Path) -> io::Result<()> {
    let mut permissions = fs::metadata(file)?.permissions();
    if permissions.readonly() {
        permissions.set_readonly(false);
        fs::set_permissions(file, permissions)?;
    }
    Ok(())
}

/// Renames the folder `staging` to `target`, which may be an empty folder already.
fn move_into_place(staging: &Path, target: &Path) -> io::Result<()> {
    if fs::symlink_metadata(target).is_ok_and(|meta| meta.is_dir()) {
        let _ = fs::remove_file(target.join(".DS_Store"));
        fs::remove_dir(target)?;
    }
    fs::rename(staging, target)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch folder, removed with everything in it at the end of the test.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("jevision-create-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(fs::canonicalize(dir).unwrap())
        }

        fn write(&self, path: &str, text: &str) -> PathBuf {
            let file = self.0.join(path);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(&file, text).unwrap();
            file
        }

        fn read(&self, path: &str) -> String {
            fs::read_to_string(self.0.join(path)).unwrap()
        }

        /// What is in the folder `path`, at any depth, sorted; folders end in `/`.
        fn tree(&self, path: &str) -> Vec<String> {
            fn walk(folder: &Path, prefix: &str, out: &mut Vec<String>) {
                for entry in fs::read_dir(folder).unwrap().flatten() {
                    let name = format!("{prefix}{}", entry.file_name().to_string_lossy());
                    if entry.file_type().unwrap().is_dir() {
                        out.push(format!("{name}/"));
                        walk(&entry.path(), &format!("{name}/"), out);
                    } else {
                        out.push(name);
                    }
                }
            }
            let mut out = Vec::new();
            walk(&self.0.join(path), "", &mut out);
            out.sort();
            out
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn new_projects_go_to_documents() {
        let s = Scratch::new("places");
        let home = s.0.join("home");
        fs::create_dir_all(home.join("Documents")).unwrap();
        let place = |p: &str| home.join(p).to_string_lossy().into_owned();
        assert_eq!(project_locations(&home), [place("Documents/JEVision")]);
        fs::create_dir_all(home.join("Documents/GitHub")).unwrap();
        assert_eq!(
            project_locations(&home),
            [place("Documents/JEVision"), place("Documents/GitHub")]
        );
    }

    #[test]
    fn a_name_that_cannot_be_a_folder_says_why() {
        let s = Scratch::new("names");
        s.write("Shop/old.flow.json", "{}");
        s.write("Bank/.DS_Store", "");
        s.write("notes.txt", "");
        let problem = |name: &str| project_name_problem(&s.0, name);
        for (name, said) in [
            ("  ", tr!("project.name.empty")),
            ("a/b", tr!("project.name.characters")),
            ("Shop: QA", tr!("project.name.characters")),
            ("line\nbreak", tr!("project.name.characters")),
            (".hidden", tr!("project.name.dot")),
            (&"я".repeat(128), tr!("project.name.long")),
            (" Shop ", tr!("project.name.taken", "Shop")),
            ("notes.txt", tr!("project.name.taken", "notes.txt")),
        ] {
            assert_eq!(problem(name), Some(said), "{name:?}");
        }
        assert_eq!(problem(&"я".repeat(127)), None);
        assert_eq!(problem("Bank"), None);
        assert_eq!(problem("Café orders"), None);
    }

    #[test]
    fn an_empty_project_has_settings_and_a_gitignore() {
        let s = Scratch::new("empty");
        let parent = s.0.join("Documents/JEVision");
        let mut started = None;
        let made = create_project(&parent, " Shop ", &ProjectStarter::Empty, |folder| {
            started = Some(folder.to_path_buf());
            true
        })
        .unwrap();
        let path = parent.join("Shop");
        assert_eq!(
            made,
            CreatedProject {
                path: path.to_string_lossy().into_owned(),
                git: true,
                first_test: None,
            }
        );
        assert_eq!(
            s.tree("Documents/JEVision"),
            ["Shop/", "Shop/.gitignore", "Shop/jevision.project.json"]
        );
        assert_eq!(
            s.read("Documents/JEVision/Shop/.gitignore"),
            ".jevision/\n.DS_Store\n.env\n"
        );
        assert_eq!(
            s.read("Documents/JEVision/Shop/jevision.project.json"),
            ProjectSettings::default().to_json()
        );
        // Git starts in the folder before it's renamed, so it never sees a half-made project.
        let started = started.unwrap();
        assert_eq!(
            started.parent(),
            Some(fs::canonicalize(&parent).unwrap().as_path())
        );
        assert!(
            started
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".jevision-new-")
        );
    }

    #[test]
    fn a_copy_leaves_out_history_runs_and_links_that_lead_away() {
        let s = Scratch::new("copy");
        s.write("template/b.flow.json", "{}");
        s.write("template/login/a.flow.json", "{}");
        s.write("template/data/users.csv", "name\n");
        s.write("template/.gitignore", "build\n/.jevision\n");
        s.write("template/jevision.project.json", "{\"format\":1}\n");
        s.write("template/.git/HEAD", "ref: refs/heads/main\n");
        s.write("template/.jevision/runs/r/report.json", "{}");
        s.write("template/login/.DS_Store", "");
        s.write("template/.env.example", "TOKEN=\n");
        s.write("secret.txt", "outside");
        std::os::unix::fs::symlink("data/users.csv", s.0.join("template/users.csv")).unwrap();
        std::os::unix::fs::symlink("../secret.txt", s.0.join("template/secret.txt")).unwrap();
        std::os::unix::fs::symlink(
            s.0.join("template/b.flow.json"),
            s.0.join("template/abs.flow.json"),
        )
        .unwrap();
        let locked = s.0.join("template/b.flow.json");
        let mut mode = fs::metadata(&locked).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut mode, 0o444);
        fs::set_permissions(&locked, mode).unwrap();

        let template = s.0.join("template").to_string_lossy().into_owned();
        let starter = ProjectStarter::CopyOf { folder: template };
        let made = create_project(&s.0.join("projects"), "Shop", &starter, |_| false).unwrap();
        assert!(!made.git);
        assert_eq!(made.first_test.as_deref(), Some("b.flow.json"));
        assert_eq!(
            s.tree("projects/Shop"),
            [
                ".env.example",
                ".gitignore",
                "b.flow.json",
                "data/",
                "data/users.csv",
                "jevision.project.json",
                "login/",
                "login/a.flow.json",
                "users.csv",
            ]
        );
        assert_eq!(s.read("projects/Shop/users.csv"), "name\n");
        assert_eq!(
            s.read("projects/Shop/.gitignore"),
            "build\n/.jevision\n.DS_Store\n.env\n"
        );
        assert_eq!(
            s.read("projects/Shop/jevision.project.json"),
            "{\"format\":1}\n"
        );
        assert!(
            !fs::metadata(s.0.join("projects/Shop/b.flow.json"))
                .unwrap()
                .permissions()
                .readonly()
        );
    }

    #[test]
    fn a_project_made_inside_its_template_does_not_copy_itself() {
        let s = Scratch::new("inside");
        s.write("t/a.flow.json", "{}");
        let starter = ProjectStarter::CopyOf {
            folder: s.0.join("t").to_string_lossy().into_owned(),
        };
        create_project(&s.0.join("t"), "Copy", &starter, |_| false).unwrap();
        assert_eq!(
            s.tree("t"),
            [
                "Copy/",
                "Copy/.gitignore",
                "Copy/a.flow.json",
                "Copy/jevision.project.json",
                "a.flow.json"
            ]
        );
    }

    #[test]
    fn a_failed_project_leaves_nothing_behind() {
        let s = Scratch::new("failed");
        s.write("projects/Taken/a.flow.json", "{}");
        s.write("projects/Empty/.DS_Store", "");
        let projects = s.0.join("projects");
        let missing = s.0.join("missing").to_string_lossy().into_owned();
        let starter = ProjectStarter::CopyOf {
            folder: missing.clone(),
        };
        assert_eq!(
            create_project(&projects, "New", &starter, |_| true),
            Err(tr!("project.template.missing", missing))
        );
        assert_eq!(
            create_project(&projects, "Taken", &ProjectStarter::Empty, |_| true),
            Err(tr!("project.name.taken", "Taken"))
        );
        // Someone else fills the folder while the project is put together: it stays theirs.
        let made = create_project(&projects, "New", &ProjectStarter::Empty, |_| {
            fs::create_dir(projects.join("New")).unwrap();
            fs::write(projects.join("New/theirs.txt"), "").unwrap();
            true
        });
        assert!(made.is_err());
        assert_eq!(
            s.tree("projects"),
            [
                "Empty/",
                "Empty/.DS_Store",
                "New/",
                "New/theirs.txt",
                "Taken/",
                "Taken/a.flow.json"
            ]
        );
        // A folder that only holds the Finder's settings is free to use.
        create_project(&projects, "Empty", &ProjectStarter::Empty, |_| false).unwrap();
        assert_eq!(
            s.tree("projects/Empty"),
            [".gitignore", "jevision.project.json"]
        );
    }

    #[test]
    fn the_run_folder_is_offered_only_inside_a_repository_that_keeps_it() {
        let s = Scratch::new("ignore");
        s.write("plain/a.flow.json", "{}");
        assert!(!gitignore_needs_run_folder(&s.0.join("plain")));

        s.write("repo/.git/HEAD", "ref: refs/heads/main\n");
        s.write("repo/qa/a.flow.json", "{}");
        let qa = s.0.join("repo/qa");
        assert!(gitignore_needs_run_folder(&qa));
        // A line anchored to the top of the repository doesn't reach the project's folder.
        s.write("repo/.gitignore", "/.jevision/\n");
        assert!(gitignore_needs_run_folder(&qa));
        s.write("repo/.gitignore", "**/.jevision\n");
        assert!(!gitignore_needs_run_folder(&qa));
        s.write("repo/.gitignore", "");
        s.write("repo/.git/info/exclude", ".jevision\n");
        assert!(!gitignore_needs_run_folder(&qa));
        s.write("repo/.git/info/exclude", "");
        for line in [".jevision/*", "/.jevision", " .jevision/** "] {
            s.write("repo/qa/.gitignore", line);
            assert!(!gitignore_needs_run_folder(&qa), "{line}");
        }
        s.write("repo/qa/.gitignore", "build\n");
        assert!(gitignore_needs_run_folder(&qa));
        gitignore_add(&qa, &[GITIGNORE_RUN_FOLDER]).unwrap();
        assert_eq!(s.read("repo/qa/.gitignore"), "build\n.jevision/\n");
        assert!(!gitignore_needs_run_folder(&qa));
    }

    #[test]
    fn lines_already_in_the_gitignore_are_not_added_again() {
        let s = Scratch::new("merge");
        s.write("p/.gitignore", "# local\n/.jevision\n.DS_Store/\nbuild");
        gitignore_add(&s.0.join("p"), &NEW_PROJECT_GITIGNORE).unwrap();
        assert_eq!(
            s.read("p/.gitignore"),
            "# local\n/.jevision\n.DS_Store/\nbuild\n.env\n"
        );
        gitignore_add(&s.0.join("p"), &NEW_PROJECT_GITIGNORE).unwrap();
        assert_eq!(
            s.read("p/.gitignore"),
            "# local\n/.jevision\n.DS_Store/\nbuild\n.env\n"
        );
        assert_eq!(s.tree("p"), [".gitignore"]);
    }
}
