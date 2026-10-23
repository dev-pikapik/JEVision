//! What notes need from git: the people to mention and Git LFS for large attached files.

use jevision_l10n::tr;

use crate::{GitError, Repo, pathspec};

/// How many of the latest commits are read to find the people who work on the project.
const AUTHOR_COMMITS: &str = "2000";

impl Repo {
    /// The people who committed to the project, each once: whoever git signs with here first, then
    /// the others, the most recent first.
    pub fn authors(&self) -> Result<Vec<String>, GitError> {
        let mut names: Vec<String> = Vec::new();
        let mut add = |name: &str| {
            let name = name.trim();
            if !name.is_empty() && !names.iter().any(|n| n == name) {
                names.push(name.to_owned());
            }
        };
        add(&self.config("user.name")?.unwrap_or_default());
        if self.has_commits()? {
            let out = self.run(&["log", "--format=%aN", "-n", AUTHOR_COMMITS])?;
            out.lines().for_each(&mut add);
        }
        Ok(names)
    }

    /// Whether Git LFS is installed.
    pub fn lfs_available(&self) -> bool {
        self.output(&["lfs", "version"])
            .is_ok_and(|out| out.status.success())
    }

    /// Whether Git LFS already takes the file at `path`, such as through a line in
    /// `.gitattributes`.
    pub fn lfs_tracks(&self, path: &str) -> Result<bool, GitError> {
        let spec = pathspec(path)?;
        let out = self.run(&["check-attr", "filter", "--", spec])?;
        Ok(out.trim_end().ends_with(": lfs"))
    }

    /// Makes Git LFS take the file at `path`: sets LFS up in this repository and writes a line for
    /// the file to `.gitattributes`, which then has to be committed with the file. The file is
    /// meant to be added to git after this, not before: what is already committed stays as it is.
    pub fn lfs_track(&self, path: &str) -> Result<(), GitError> {
        let spec = pathspec(path)?;
        if spec.contains(['*', '?', '[', ']', '\\', '\n', '\r']) {
            return Err(GitError::Failed {
                message: tr!("git.error.lfsPattern", path),
            });
        }
        if !self.lfs_available() {
            return Err(GitError::Failed {
                message: tr!("git.error.noLfs"),
            });
        }
        self.run(&["lfs", "install", "--local"])?;
        self.run(&["lfs", "track", "--", spec])?;
        Ok(())
    }

    /// Drops the lines of `.gitattributes` that made Git LFS take only the files at `paths`, or
    /// files inside the folders at `paths`, once they are gone: the lines [`Self::lfs_track`]
    /// wrote for them. Lines that take other files too, like `*.mp4`, stay. Works without LFS.
    pub fn lfs_untrack(&self, paths: &[String]) -> Result<(), GitError> {
        let gone = paths
            .iter()
            .map(|p| pathspec(p))
            .collect::<Result<Vec<_>, _>>()?;
        let failed = |e: std::io::Error| GitError::Failed {
            message: e.to_string(),
        };
        let file = self.dir.join(".gitattributes");
        let text = match std::fs::read_to_string(&file) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(failed(e)),
        };
        let kept: String = text
            .split_inclusive('\n')
            .filter(|line| {
                !lfs_file(line).is_some_and(|path| {
                    gone.iter().any(|g| {
                        path.strip_prefix(g)
                            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
                    })
                })
            })
            .collect();
        if kept.len() == text.len() {
            return Ok(());
        }
        crate::atomic::write(&file, kept).map_err(failed)
    }
}

/// The one file a `.gitattributes` line hands to Git LFS, written the way `git lfs track` writes
/// it; `None` for any other line, like a comment or a pattern that matches more than one file.
fn lfs_file(line: &str) -> Option<String> {
    let mut fields = line.split_whitespace();
    let pattern = fields.next()?;
    if pattern.starts_with('#') || !fields.any(|f| f == "filter=lfs") {
        return None;
    }
    let path = pattern
        .strip_prefix('/')
        .unwrap_or(pattern)
        .replace("[[:space:]]", " ")
        .replace("\\#", "#");
    // Without a slash a pattern matches the name in every folder.
    let one = (pattern.starts_with('/') || path.contains('/'))
        && !path.contains(['*', '?', '[', '\\', '"']);
    one.then_some(path)
}
