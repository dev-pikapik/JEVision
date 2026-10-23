//! A project's folder on disk: the files of its tests, and the project a folder or a file
//! opened from the Finder belongs to.

use std::fs;
use std::path::{Path, PathBuf};

use crate::model::TEST_SUFFIX;
use crate::project::PROJECT_FILE;

/// Endings of folders macOS shows as one file, such as an app: nothing in them is a test.
const PACKAGES: [&str; 14] = [
    "app",
    "appex",
    "bundle",
    "framework",
    "kext",
    "plugin",
    "xpc",
    "xcodeproj",
    "xcworkspace",
    "xcarchive",
    "playground",
    "photoslibrary",
    "rtfd",
    "pkg",
];

/// Files in the folder `root` and the folders inside it whose names end in one of `endings`,
/// in any case, relative to `root` with `/` and sorted. Left out, as the app's project
/// browser leaves them out: hidden files and folders (such as `.git` and `.jevision`),
/// symbolic links, packages such as apps, and folders that can't be read.
pub fn project_files(root: &Path, endings: &[&str]) -> Vec<String> {
    fn walk(folder: &Path, prefix: &str, endings: &[&str], files: &mut Vec<String>) {
        let Ok(entries) = fs::read_dir(folder) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if name.starts_with('.') {
                continue;
            }
            let path = format!("{prefix}{name}");
            if kind.is_dir() && !is_package(&name) {
                walk(&entry.path(), &format!("{path}/"), endings, files);
            } else if kind.is_file() && endings.iter().any(|e| ends_with(&name, e)) {
                files.push(path);
            }
        }
    }
    let mut files = Vec::new();
    walk(root, "", endings, &mut files);
    files.sort();
    files
}

/// A project, and the test in it, that a folder or file opened from the Finder stands for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectLocation {
    /// The project folder, without a `/` at the end.
    pub root: String,
    /// The test file that was opened, relative to `root` with `/`; `None` for a folder.
    pub test: Option<String>,
}

/// The project to open for `path`, a folder or a file opened from the Finder, with `home` the
/// user's home folder:
/// - a folder opens the nearest project around it: the closest folder, itself or one above
///   it, that has a `jevision.project.json`, a `.jevision` folder or a `.git`. The search
///   stops below `home` and at the top of the disk the folder is on; when nothing is found
///   the folder itself is the project;
/// - a test file (`*.flow.json`) opens the project found the same way from its folder, at
///   that test;
/// - a `jevision.project.json` opens its folder.
///
/// Anything else, or a path that isn't there, is `None`.
pub fn project_location_for(path: &Path, home: &Path) -> Option<ProjectLocation> {
    let path: PathBuf = path.components().collect();
    let metadata = fs::metadata(&path).ok()?;
    if metadata.is_dir() {
        return Some(ProjectLocation {
            root: text(&project_around(&path, home)),
            test: None,
        });
    }
    let name = path.file_name()?.to_string_lossy();
    let folder = path.parent()?;
    if name == PROJECT_FILE {
        return Some(ProjectLocation {
            root: text(folder),
            test: None,
        });
    }
    if !metadata.is_file() || !ends_with(&name, TEST_SUFFIX) {
        return None;
    }
    let root = project_around(folder, home);
    let test = path.strip_prefix(&root).ok().map(|relative| {
        relative
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/")
    });
    Some(ProjectLocation {
        root: text(&root),
        test,
    })
}

/// The project folder `folder` is in, as [`project_location_for`] finds it.
fn project_around(folder: &Path, home: &Path) -> PathBuf {
    let is_project = |dir: &Path| {
        dir.join(PROJECT_FILE).is_file()
            || dir.join(".jevision").is_dir()
            || dir.join(".git").exists()
    };
    let mut dir = folder;
    while !is_project(dir) {
        match dir.parent() {
            Some(parent) if !home.starts_with(parent) && same_disk(dir, parent) => dir = parent,
            _ => return folder.to_path_buf(),
        }
    }
    dir.to_path_buf()
}

#[cfg(unix)]
fn same_disk(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (fs::metadata(a), fs::metadata(b)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev(),
        _ => false,
    }
}

#[cfg(not(unix))]
fn same_disk(_: &Path, _: &Path) -> bool {
    true
}

fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Whether the file name `name` is `ending` with something before it, in any case.
fn ends_with(name: &str, ending: &str) -> bool {
    let name = name.to_lowercase();
    name.len() > ending.len() && name.ends_with(ending)
}

fn is_package(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|e| PACKAGES.iter().any(|p| e.eq_ignore_ascii_case(p)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::BLOCK_SUFFIX;

    /// A scratch folder, removed with everything in it at the end of the test.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("jevision-folder-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(fs::canonicalize(dir).unwrap())
        }

        fn touch(&self, path: &str) -> PathBuf {
            let file = self.0.join(path);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(&file, "{}").unwrap();
            file
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn tests_are_found_as_the_project_browser_finds_them() {
        let s = Scratch::new("files");
        for file in [
            "b.flow.json",
            "a/Login.FLOW.JSON",
            "a/deep/x.flow.json",
            "a/helper.block.json",
            "notes.txt",
            ".flow.json",
            ".hidden/h.flow.json",
            ".jevision/runs/r.flow.json",
            "Demo.app/Contents/t.flow.json",
            "v1.2/t.flow.json",
        ] {
            s.touch(file);
        }
        std::os::unix::fs::symlink(s.0.join("b.flow.json"), s.0.join("link.flow.json")).unwrap();
        std::os::unix::fs::symlink(s.0.join("a"), s.0.join("linked")).unwrap();
        assert_eq!(
            project_files(&s.0, &[TEST_SUFFIX]),
            [
                "a/Login.FLOW.JSON",
                "a/deep/x.flow.json",
                "b.flow.json",
                "v1.2/t.flow.json"
            ]
        );
        assert_eq!(
            project_files(&s.0.join("a"), &[TEST_SUFFIX, BLOCK_SUFFIX]),
            ["Login.FLOW.JSON", "deep/x.flow.json", "helper.block.json"]
        );
        assert!(project_files(&s.0.join("missing"), &[TEST_SUFFIX]).is_empty());
    }

    #[test]
    fn a_folder_opens_the_nearest_project_around_it() {
        let s = Scratch::new("around");
        let home = s.0.join("home");
        s.touch("home/marked/jevision.project.json");
        s.touch("home/marked/login/deep/x.txt");
        s.touch("home/runs/.jevision/runs/x");
        s.touch("home/repo/.git");
        s.touch("home/repo/qa/smoke/s.flow.json");
        s.touch("home/plain/tests/t.flow.json");
        s.touch("home/.git");
        let at = |path: &str| project_location_for(&s.0.join(path), &home);
        let root = |path: &str| Some(text(&s.0.join(path)));

        assert_eq!(
            at("home/marked/login/deep").unwrap().root,
            root("home/marked").unwrap()
        );
        assert_eq!(
            at("home/marked/").unwrap().root,
            root("home/marked").unwrap()
        );
        assert_eq!(at("home/runs").unwrap().root, root("home/runs").unwrap());
        assert_eq!(
            at("home/repo/qa/smoke").unwrap().root,
            root("home/repo").unwrap()
        );
        // The home folder's own repository is not a project of every folder in it.
        let plain = at("home/plain/tests").unwrap();
        assert_eq!(
            (Some(plain.root), plain.test),
            (root("home/plain/tests"), None)
        );
        assert_eq!(at("home/missing"), None);
    }

    #[test]
    fn a_file_opens_its_project_at_the_test() {
        let s = Scratch::new("file");
        let home = s.0.join("home");
        s.touch("home/p/jevision.project.json");
        s.touch("home/p/login/Sign-in.Flow.json");
        s.touch("home/p/notes.txt");
        s.touch("home/loose/a.flow.json");
        let at = |path: &str| project_location_for(&s.0.join(path), &home);

        let test = at("home/p/login/Sign-in.Flow.json").unwrap();
        assert_eq!(test.root, text(&s.0.join("home/p")));
        assert_eq!(test.test.as_deref(), Some("login/Sign-in.Flow.json"));
        let settings = at("home/p/jevision.project.json").unwrap();
        assert_eq!(
            (settings.root, settings.test),
            (text(&s.0.join("home/p")), None)
        );
        let loose = at("home/loose/a.flow.json").unwrap();
        assert_eq!(loose.root, text(&s.0.join("home/loose")));
        assert_eq!(loose.test.as_deref(), Some("a.flow.json"));
        assert_eq!(at("home/p/notes.txt"), None);
        assert_eq!(at("home/p/gone.flow.json"), None);
    }
}
