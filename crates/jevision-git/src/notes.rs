//! What notes need from git: the people to mention and Git LFS for large attached files.

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
                message: format!("`{path}` can't be told apart from a pattern for Git LFS"),
            });
        }
        if !self.lfs_available() {
            return Err(GitError::Failed {
                message: "Git LFS is not installed".to_owned(),
            });
        }
        self.run(&["lfs", "install", "--local"])?;
        self.run(&["lfs", "track", "--", spec])?;
        Ok(())
    }
}
