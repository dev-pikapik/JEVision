//! Git for a JEVision project: what changed, the history of a test, commits, branches and
//! syncing with the server.
//!
//! Everything goes through the `git` command installed on the computer rather than a library,
//! so that whatever the user set up for it works exactly as in Terminal: credential helpers
//! (the macOS Keychain, GitHub CLI), ssh-agent and `~/.ssh/config`, hooks, LFS and signing.
//! Paths are relative to the project folder, which may be anywhere inside the repository.

use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output, Stdio};

use jevision_l10n::{tr, tr_in};

mod github;
mod notes;
mod signin;
#[cfg(test)]
mod tests;

pub use github::{
    API, ATTACHMENTS_BRANCH, Artifact, GitHub, GitHubAccount, GitHubIssue, GitHubRepo, IssueState,
    IssueStatus, PullRequest, PullRequestChanges, PullRequestDraft,
};
pub use signin::{DeviceCode, DeviceFlow, SITE, SignInStep, forget_token, save_token};

/// Why a git operation didn't happen. Nothing on disk changed unless the variant says so.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitError {
    NotInstalled,
    NotRepository,
    NoCommits,
    NoRemote,
    NoUpstream {
        branch: String,
    },
    Detached,
    SignIn,
    RemoteNotFound,
    Unreachable,
    Rejected,
    Conflict {
        paths: Vec<String>,
    },
    LocalChanges {
        paths: Vec<String>,
    },
    NoIdentity,
    EmptyMessage,
    NothingToCommit,
    BadBranchName {
        name: String,
    },
    BranchExists {
        name: String,
    },
    NotInCommit {
        path: String,
    },
    NoGitHubCli,
    GitHubSignedOut,
    SignInExpired,
    SignInDenied,
    SignInUnavailable,
    NotGitHub,
    NoPermission,
    NotPushed {
        branch: String,
    },
    PullRequestExists,
    NothingToPropose {
        base: String,
    },
    /// `message` is already in words: git's own, or one of ours in the process language.
    Failed {
        message: String,
    },
}

impl GitError {
    /// What went wrong, in `lang`.
    pub fn text_in(&self, lang: &str) -> String {
        match self {
            Self::NotInstalled => tr_in!(lang, "git.error.notInstalled"),
            Self::NotRepository => tr_in!(lang, "git.error.notRepository"),
            Self::NoCommits => tr_in!(lang, "git.error.noCommits"),
            Self::NoRemote => tr_in!(lang, "git.error.noRemote"),
            Self::NoUpstream { branch } => tr_in!(lang, "git.error.noUpstream", branch),
            Self::Detached => tr_in!(lang, "git.error.detached"),
            Self::SignIn => tr_in!(lang, "git.error.signIn"),
            Self::RemoteNotFound => tr_in!(lang, "git.error.remoteNotFound"),
            Self::Unreachable => tr_in!(lang, "git.error.unreachable"),
            Self::Rejected => tr_in!(lang, "git.error.rejected"),
            Self::Conflict { paths } => tr_in!(lang, "git.error.conflict", paths.join(", ")),
            Self::LocalChanges { paths } => {
                tr_in!(lang, "git.error.localChanges", paths.join(", "))
            }
            Self::NoIdentity => tr_in!(lang, "git.error.noIdentity"),
            Self::EmptyMessage => tr_in!(lang, "git.error.emptyMessage"),
            Self::NothingToCommit => tr_in!(lang, "git.error.nothingToCommit"),
            Self::BadBranchName { name } => tr_in!(lang, "git.error.badBranchName", name),
            Self::BranchExists { name } => tr_in!(lang, "git.error.branchExists", name),
            Self::NotInCommit { path } => tr_in!(lang, "git.error.notInCommit", path),
            Self::NoGitHubCli => tr_in!(lang, "git.error.noGitHubCli"),
            Self::GitHubSignedOut => tr_in!(lang, "git.error.gitHubSignedOut"),
            Self::SignInExpired => tr_in!(lang, "git.error.signInExpired"),
            Self::SignInDenied => tr_in!(lang, "git.error.signInDenied"),
            Self::SignInUnavailable => tr_in!(lang, "git.error.signInUnavailable"),
            Self::NotGitHub => tr_in!(lang, "git.error.notGitHub"),
            Self::NoPermission => tr_in!(lang, "git.error.noPermission"),
            Self::NotPushed { branch } => tr_in!(lang, "git.error.notPushed", branch),
            Self::PullRequestExists => tr_in!(lang, "git.error.pullRequestExists"),
            Self::NothingToPropose { base } => tr_in!(lang, "git.error.nothingToPropose", base),
            Self::Failed { message } => message.clone(),
        }
    }
}

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text_in(&jevision_l10n::language()))
    }
}

impl std::error::Error for GitError {}

/// What happened to a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    /// Changed both here and on the other side of a merge that isn't finished.
    Conflicted,
}

/// A changed file, relative to the project folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    pub path: String,
    /// The path before a rename, when it was inside the project.
    pub from: Option<String>,
    pub kind: ChangeKind,
}

/// Where the project stands: its branch, how it compares with the server and what changed
/// since the last commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitStatus {
    /// `None` when no branch is checked out.
    pub branch: Option<String>,
    /// The server branch this one syncs with, such as `origin/main`.
    pub upstream: Option<String>,
    /// Commits here that the server doesn't have.
    pub ahead: u32,
    /// Commits on the server that aren't here.
    pub behind: u32,
    pub has_commits: bool,
    /// The repository on the server, such as `github.com/acme/qa`.
    pub remote: Option<String>,
    /// `owner/name` when that server is GitHub.
    pub github: Option<String>,
    pub changes: Vec<FileChange>,
}

/// A commit that changed the files asked about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitCommit {
    pub id: String,
    pub short_id: String,
    /// The first parent: the version the changes were made to. `None` for the first commit.
    pub parent: Option<String>,
    pub author: String,
    /// Seconds since 1970.
    pub time: i64,
    /// The first line of the message.
    pub summary: String,
    /// The files asked about that this commit changed.
    pub files: Vec<FileChange>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitBranch {
    pub name: String,
    pub is_current: bool,
    /// Only on the server so far; switching to it makes a local branch that syncs with it.
    pub is_remote: bool,
}

/// Who commits are signed with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitIdentity {
    pub name: String,
    pub email: String,
}

/// One file in three versions, for settling a conflict; `None` where the file isn't.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictVersions {
    /// Where the two sides parted.
    pub base: Option<Vec<u8>>,
    /// The latest commit here.
    pub mine: Option<Vec<u8>>,
    /// The latest commit on the server.
    pub theirs: Option<Vec<u8>>,
}

/// The `git` command and the environment it runs in.
#[derive(Debug, Clone)]
pub struct Git {
    exe: PathBuf,
    /// GitHub CLI, offered to git as a credential helper for github.com.
    gh: Option<PathBuf>,
    env: Vec<(String, String)>,
}

/// A credential helper for github.com that answers with the token in [`GITHUB_TOKEN_VARIABLE`],
/// so the token is never on a command line.
const GITHUB_CREDENTIAL: &str = "credential.https://github.com.helper=!f() { test \"$1\" = get || return 0; echo username=x-access-token; echo \"password=$JEVISION_GITHUB_TOKEN\"; }; f";
const GITHUB_TOKEN_VARIABLE: &str = "JEVISION_GITHUB_TOKEN";

/// Folders where command line tools live besides `PATH`, which is short for apps started
/// from the Finder.
const TOOL_DIRS: [&str; 3] = ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"];

impl Git {
    /// The git installed on this computer, if any.
    pub fn find() -> Option<Self> {
        Some(Self {
            exe: find_tool("git")?,
            gh: find_tool("gh"),
            env: Vec::new(),
        })
    }

    /// This `git` executable, without GitHub CLI.
    pub fn at(exe: impl Into<PathBuf>) -> Self {
        Self {
            exe: exe.into(),
            gh: None,
            env: Vec::new(),
        }
    }

    /// Runs git with an extra environment variable.
    pub fn env(mut self, key: &str, value: &str) -> Self {
        self.env.push((key.to_owned(), value.to_owned()));
        self
    }

    /// The repository the project folder is in; `None` when it isn't in one.
    pub fn open(&self, project: &Path) -> Result<Option<Repo>, GitError> {
        let out = self.output(
            project,
            &["rev-parse", "--is-inside-work-tree", "--show-prefix"],
        )?;
        let text = String::from_utf8_lossy(&out.stdout);
        let mut lines = text.lines();
        if !out.status.success() || lines.next() != Some("true") {
            return Ok(None);
        }
        Ok(Some(Repo {
            git: self.clone(),
            dir: project.to_path_buf(),
            prefix: lines.next().unwrap_or_default().to_owned(),
        }))
    }

    /// Starts tracking the project folder with a new repository.
    pub fn init(&self, project: &Path) -> Result<Repo, GitError> {
        check(self.output(project, &["init", "--quiet"])?)?;
        self.open(project)?.ok_or(GitError::NotRepository)
    }

    /// Signs the user's commits in every repository with this name and email.
    pub fn set_identity(&self, project: &Path, identity: &GitIdentity) -> Result<(), GitError> {
        let (name, email) = (identity.name.trim(), identity.email.trim());
        if name.is_empty() || email.is_empty() {
            return Err(GitError::NoIdentity);
        }
        check(self.output(project, &["config", "--global", "user.name", name])?)?;
        check(self.output(project, &["config", "--global", "user.email", email])?)?;
        Ok(())
    }

    fn command(&self, dir: &Path) -> Command {
        let mut command = Command::new(&self.exe);
        command
            .current_dir(dir)
            .stdin(Stdio::null())
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            // Messages are matched in English; paths always come NUL-separated, never quoted.
            .env("LC_ALL", "C")
            .env("LANGUAGE", "C")
            // No one can answer a prompt: fail instead of waiting forever.
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_MERGE_AUTOEDIT", "no")
            .env("GIT_LITERAL_PATHSPECS", "1")
            // Looking doesn't write: no lock files, no change events for the folder watcher.
            .env("GIT_OPTIONAL_LOCKS", "0")
            .envs(self.env.iter().map(|(k, v)| (k, v)));
        command
    }

    fn output(&self, dir: &Path, args: &[&str]) -> Result<Output, GitError> {
        self.spawn(self.command(dir).args(args))
    }

    fn spawn(&self, command: &mut Command) -> Result<Output, GitError> {
        command.output().map_err(|e| {
            if self.exe.is_file() {
                GitError::Failed {
                    message: e.to_string(),
                }
            } else {
                GitError::NotInstalled
            }
        })
    }
}

/// A git repository, seen from the project folder inside it.
#[derive(Debug, Clone)]
pub struct Repo {
    git: Git,
    dir: PathBuf,
    /// The project folder relative to the top of the repository: empty or ending in `/`.
    prefix: String,
}

impl Repo {
    pub fn status(&self) -> Result<GitStatus, GitError> {
        let out = self.run(&[
            "status",
            "--porcelain=v2",
            "-z",
            "--branch",
            "--untracked-files=all",
            "--",
            ".",
        ])?;
        let mut status = GitStatus {
            branch: None,
            upstream: None,
            ahead: 0,
            behind: 0,
            has_commits: false,
            remote: None,
            github: None,
            changes: Vec::new(),
        };
        let mut fields = out.split('\0');
        while let Some(entry) = fields.next() {
            if let Some(header) = entry.strip_prefix("# ") {
                let (key, value) = header.split_once(' ').unwrap_or((header, ""));
                match key {
                    "branch.oid" => status.has_commits = value != "(initial)",
                    "branch.head" if value != "(detached)" => status.branch = Some(value.into()),
                    "branch.upstream" => status.upstream = Some(value.into()),
                    "branch.ab" => {
                        for n in value.split(' ') {
                            if let Some(a) = n.strip_prefix('+') {
                                status.ahead = a.parse().unwrap_or(0);
                            } else if let Some(b) = n.strip_prefix('-') {
                                status.behind = b.parse().unwrap_or(0);
                            }
                        }
                    }
                    _ => {}
                }
                continue;
            }
            let (kind, path, from) = match entry.split_at_checked(2) {
                Some(("1 ", rest)) => (tracked_kind(rest), nth_field(rest, 7), None),
                Some(("2 ", rest)) => (
                    ChangeKind::Renamed,
                    nth_field(rest, 8),
                    fields.next().and_then(|p| self.inside(p)),
                ),
                Some(("u ", rest)) => (ChangeKind::Conflicted, nth_field(rest, 9), None),
                Some(("? ", path)) => (ChangeKind::Added, path, None),
                _ => continue,
            };
            if let Some(path) = self.inside(path) {
                status.changes.push(FileChange { path, from, kind });
            }
        }
        if let Some(remote) = self.server(status.upstream.as_deref())? {
            let url = self.config(&format!("remote.{remote}.url"))?;
            status.remote = url.as_deref().map(display_url);
            status.github = url
                .as_deref()
                .and_then(GitHubRepo::from_url)
                .map(|r| r.full_name());
        }
        Ok(status)
    }

    /// The remote the branch syncs with, else the one it would be pushed to.
    fn server(&self, upstream: Option<&str>) -> Result<Option<String>, GitError> {
        let remotes = self.remotes()?;
        Ok(upstream
            .and_then(|u| remotes.iter().find(|r| u.starts_with(&format!("{r}/"))))
            .cloned()
            .or_else(|| default_remote(remotes)))
    }

    /// Commits that changed `path`, a test or a folder relative to the project (empty for the
    /// whole project), newest first. A test is followed across renames.
    pub fn history(&self, path: &str) -> Result<Vec<GitCommit>, GitError> {
        if !self.has_commits()? {
            return Ok(Vec::new());
        }
        let spec = pathspec(path)?;
        let mut args = vec![
            "log",
            "-z",
            "--name-status",
            "--format=%x1e%H%x1f%h%x1f%P%x1f%an%x1f%at%x1f%s",
        ];
        if !self.dir.join(path).is_dir() {
            args.push("--follow");
        }
        args.extend(["--", spec]);
        let out = self.run(&args)?;
        Ok(out
            .split('\x1e')
            .filter_map(|record| {
                let (header, files) = record.split_once('\0').unwrap_or((record, ""));
                let mut h = header.split('\x1f');
                let mut field = || h.next().unwrap_or_default().to_owned();
                let (id, short_id, parents, author, time, summary) =
                    (field(), field(), field(), field(), field(), field());
                (!id.is_empty()).then(|| GitCommit {
                    parent: parents
                        .split(' ')
                        .next()
                        .filter(|p| !p.is_empty())
                        .map(Into::into),
                    time: time.parse().unwrap_or(0),
                    files: self.name_status(files.trim_start_matches('\n')),
                    id,
                    short_id,
                    author,
                    summary,
                })
            })
            .collect())
    }

    /// The contents of `path` in `commit`; `None` when the file isn't there.
    pub fn file(&self, commit: &str, path: &str) -> Result<Option<Vec<u8>>, GitError> {
        let spec = format!("{commit}:./{}", pathspec(path)?);
        let out = self.output(&["rev-parse", "--quiet", "--verify", &spec])?;
        if !out.status.success() {
            return Ok(None);
        }
        let blob = String::from_utf8_lossy(&out.stdout).trim().to_owned();
        let out = self.output(&["cat-file", "blob", &blob])?;
        Ok(Some(check(out)?))
    }

    /// Writes `path` with the contents `source` had in `commit`: the same file under the name
    /// it had then, when it was renamed since.
    pub fn restore(&self, commit: &str, source: &str, path: &str) -> Result<(), GitError> {
        let bytes = self
            .file(commit, source)?
            .ok_or_else(|| GitError::NotInCommit {
                path: source.into(),
            })?;
        let file = self.dir.join(pathspec(path)?);
        let failed = |e: std::io::Error| GitError::Failed {
            message: e.to_string(),
        };
        if let Some(folder) = file.parent() {
            std::fs::create_dir_all(folder).map_err(failed)?;
        }
        std::fs::write(file, bytes).map_err(failed)
    }

    /// Commits the changes to `paths` alone, with exactly the user's `message`. Other changes,
    /// staged or not, stay as they were. Returns the new commit's id.
    pub fn commit(&self, paths: &[String], message: &str) -> Result<String, GitError> {
        if message.trim().is_empty() {
            return Err(GitError::EmptyMessage);
        }
        if paths.is_empty() {
            return Err(GitError::NothingToCommit);
        }
        let specs = paths
            .iter()
            .map(|p| pathspec(p))
            .collect::<Result<Vec<_>, _>>()?;
        // New files have to be known to git before a commit can name them; files that are
        // gone are known already.
        let mut add = vec!["add", "--all", "--"];
        add.extend(
            specs
                .iter()
                .filter(|p| self.dir.join(p).symlink_metadata().is_ok()),
        );
        if add.len() > 3 {
            self.run(&add)?;
        }
        let mut commit = vec!["commit", "--quiet", "--message", message, "--"];
        commit.extend(&specs);
        self.run(&commit)?;
        Ok(self.run(&["rev-parse", "HEAD"])?.trim().to_owned())
    }

    /// Sends the branch's commits to the server. A branch that isn't there yet is created and
    /// synced with from then on.
    pub fn push(&self) -> Result<(), GitError> {
        let branch = self.branch()?;
        if !self.has_commits()? {
            return Err(GitError::NoCommits);
        }
        if self.upstream()?.is_some() {
            self.network(&["push", "--quiet"])
        } else {
            let remote = default_remote(self.remotes()?).ok_or(GitError::NoRemote)?;
            let refspec = format!("HEAD:refs/heads/{branch}");
            self.network(&["push", "--quiet", "--set-upstream", &remote, &refspec])
        }
    }

    /// Brings in the server's commits, merging them with the ones made here. When both
    /// changed the same lines the merge is undone, leaving every file as it was, to be settled
    /// with [`Repo::conflict_versions`] and [`Repo::finish_pull`].
    pub fn pull(&self) -> Result<(), GitError> {
        let branch = self.branch()?;
        if self.upstream()?.is_none() {
            return Err(if default_remote(self.remotes()?).is_none() {
                GitError::NoRemote
            } else {
                GitError::NoUpstream { branch }
            });
        }
        match self.network(&["pull", "--quiet", "--no-rebase", "--ff", "--no-edit"]) {
            Err(error) => {
                let paths = self.unmerged()?;
                if paths.is_empty() {
                    return Err(error);
                }
                self.run(&["merge", "--abort"])?;
                Err(GitError::Conflict { paths })
            }
            ok => ok,
        }
    }

    /// A file both sides changed, for settling a conflict [`Repo::pull`] reported: as it was
    /// where this branch and the server's parted, here, and on the server. `None` where the
    /// file isn't, as when one side removed it or the histories share no commit.
    pub fn conflict_versions(&self, path: &str) -> Result<ConflictVersions, GitError> {
        let upstream = self.upstream_or_error()?;
        let base = self.output(&["merge-base", "HEAD", &upstream])?;
        let base = String::from_utf8_lossy(&base.stdout).trim().to_owned();
        Ok(ConflictVersions {
            base: if base.is_empty() {
                None
            } else {
                self.file(&base, path)?
            },
            mine: self.file("HEAD", path)?,
            theirs: self.file(&upstream, path)?,
        })
    }

    /// Brings in the server's commits after a conflict, with the files that conflicted as the
    /// user settled them (`None` removes one), and commits the merge with git's usual message.
    /// Returns the new commit's id. When a file that isn't among `files` still conflicts, or
    /// anything else fails, the merge is undone and every file is as it was.
    pub fn finish_pull(&self, files: &[(String, Option<Vec<u8>>)]) -> Result<String, GitError> {
        let upstream = self.upstream_or_error()?;
        let specs = files
            .iter()
            .map(|(path, _)| pathspec(path))
            .collect::<Result<Vec<_>, _>>()?;
        let merge = self.output(&["merge", "--quiet", "--no-ff", "--no-commit", &upstream])?;
        let merging = self
            .output(&["rev-parse", "--quiet", "--verify", "MERGE_HEAD"])?
            .status
            .success();
        if !merging {
            self.check(merge)?;
            return Err(GitError::Failed {
                message: tr!("git.error.alreadyMerged", &upstream),
            });
        }
        let finish = || -> Result<String, GitError> {
            let failed = |e: std::io::Error| GitError::Failed {
                message: e.to_string(),
            };
            for ((_, bytes), spec) in files.iter().zip(&specs) {
                let file = self.dir.join(spec);
                match bytes {
                    Some(bytes) => {
                        if let Some(folder) = file.parent() {
                            std::fs::create_dir_all(folder).map_err(failed)?;
                        }
                        std::fs::write(&file, bytes).map_err(failed)?;
                    }
                    None => match std::fs::remove_file(&file) {
                        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                            return Err(failed(e));
                        }
                        _ => {}
                    },
                }
            }
            if !specs.is_empty() {
                let mut add = vec!["add", "--all", "--"];
                add.extend(&specs);
                self.run(&add)?;
            }
            let paths = self.unmerged()?;
            if !paths.is_empty() {
                return Err(GitError::Conflict { paths });
            }
            self.run(&["commit", "--quiet", "--no-edit"])?;
            Ok(self.run(&["rev-parse", "HEAD"])?.trim().to_owned())
        };
        finish().inspect_err(|_| {
            let _ = self.output(&["merge", "--abort"]);
        })
    }

    /// Branches here, then those only on the server; each in name order.
    pub fn branches(&self) -> Result<Vec<GitBranch>, GitError> {
        let out = self.run(&[
            "for-each-ref",
            "--format=%(refname)%09%(HEAD)",
            "refs/heads",
            "refs/remotes",
        ])?;
        let mut local = Vec::new();
        let mut remote = Vec::new();
        for line in out.lines() {
            let (name, head) = line.split_once('\t').unwrap_or((line, ""));
            if let Some(name) = name.strip_prefix("refs/heads/") {
                local.push(GitBranch {
                    name: name.into(),
                    is_current: head == "*",
                    is_remote: false,
                });
            } else if let Some((_, name)) = name
                .strip_prefix("refs/remotes/")
                .and_then(|n| n.split_once('/'))
                && name != "HEAD"
            {
                remote.push(name.to_owned());
            }
        }
        remote.sort();
        remote.dedup();
        remote.retain(|name| !local.iter().any(|b| &b.name == name));
        local.sort_by(|a, b| a.name.cmp(&b.name));
        local.extend(remote.into_iter().map(|name| GitBranch {
            name,
            is_current: false,
            is_remote: true,
        }));
        Ok(local)
    }

    /// Checks out branch `name`; a branch only on the server becomes a local one. Uncommitted
    /// changes come along unless the other branch changed the same files.
    pub fn switch(&self, name: &str) -> Result<(), GitError> {
        self.run(&["switch", "--quiet", "--", name]).map(drop)
    }

    /// Makes branch `name` from the current commit and checks it out.
    pub fn create_branch(&self, name: &str) -> Result<(), GitError> {
        let name = name.trim();
        if name.starts_with('-')
            || !self
                .output(&["check-ref-format", "--branch", name])?
                .status
                .success()
        {
            return Err(GitError::BadBranchName { name: name.into() });
        }
        let reference = format!("refs/heads/{name}");
        if self
            .output(&["rev-parse", "--quiet", "--verify", &reference])?
            .status
            .success()
        {
            return Err(GitError::BranchExists { name: name.into() });
        }
        self.run(&["switch", "--quiet", "--create", name]).map(drop)
    }

    /// Who commits are signed with; empty fields are not set.
    pub fn identity(&self) -> Result<GitIdentity, GitError> {
        Ok(GitIdentity {
            name: self.config("user.name")?.unwrap_or_default(),
            email: self.config("user.email")?.unwrap_or_default(),
        })
    }

    fn branch(&self) -> Result<String, GitError> {
        let out = self.output(&["symbolic-ref", "--quiet", "--short", "HEAD"])?;
        if !out.status.success() {
            return Err(GitError::Detached);
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
    }

    /// Files git couldn't merge on its own, relative to the project where they're inside it.
    fn unmerged(&self) -> Result<Vec<String>, GitError> {
        let mut paths: Vec<String> = self
            .run(&["ls-files", "--unmerged", "-z"])?
            .split('\0')
            .filter_map(|line| line.split_once('\t'))
            .map(|(_, path)| self.inside(path).unwrap_or_else(|| path.to_owned()))
            .collect();
        paths.dedup();
        Ok(paths)
    }

    /// The server branch this one syncs with, or why there is none.
    fn upstream_or_error(&self) -> Result<String, GitError> {
        match self.upstream()? {
            Some(upstream) => Ok(upstream),
            None => Err(GitError::NoUpstream {
                branch: self.branch()?,
            }),
        }
    }

    fn has_commits(&self) -> Result<bool, GitError> {
        Ok(self
            .output(&["rev-parse", "--quiet", "--verify", "HEAD"])?
            .status
            .success())
    }

    fn upstream(&self) -> Result<Option<String>, GitError> {
        let out = self.output(&["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"])?;
        Ok(out
            .status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned()))
    }

    fn remotes(&self) -> Result<Vec<String>, GitError> {
        Ok(self.run(&["remote"])?.lines().map(Into::into).collect())
    }

    fn config(&self, key: &str) -> Result<Option<String>, GitError> {
        let out = self.output(&["config", "--get", key])?;
        Ok(out
            .status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
            .filter(|v| !v.is_empty()))
    }

    /// The path relative to the project for a path relative to the top of the repository;
    /// `None` when it is outside the project.
    fn inside(&self, path: &str) -> Option<String> {
        path.strip_prefix(&self.prefix).map(Into::into)
    }

    /// Files of `git log --name-status -z`.
    fn name_status(&self, text: &str) -> Vec<FileChange> {
        let mut out = Vec::new();
        let mut fields = text.split('\0').filter(|f| !f.is_empty());
        while let Some(code) = fields.next() {
            let Some(path) = fields.next() else { break };
            let (kind, from, path) = match code.chars().next() {
                Some('R') => (ChangeKind::Renamed, self.inside(path), fields.next()),
                Some('C') => (ChangeKind::Added, None, fields.next()),
                Some('A') => (ChangeKind::Added, None, Some(path)),
                Some('D') => (ChangeKind::Deleted, None, Some(path)),
                _ => (ChangeKind::Modified, None, Some(path)),
            };
            if let Some(path) = path.and_then(|p| self.inside(p)) {
                out.push(FileChange { path, from, kind });
            }
        }
        out
    }

    fn output(&self, args: &[&str]) -> Result<Output, GitError> {
        self.git.output(&self.dir, args)
    }

    /// Standard output of a command that has to succeed.
    fn run(&self, args: &[&str]) -> Result<String, GitError> {
        let out = self.check(self.output(args)?)?;
        Ok(String::from_utf8_lossy(&out).into_owned())
    }

    /// A command that talks to the server, with the user's GitHub sign-in on offer: JEVision's
    /// own, otherwise GitHub CLI's.
    fn network(&self, args: &[&str]) -> Result<(), GitError> {
        let mut command = self.git.command(&self.dir);
        let token = self.github().ok().and_then(|_| signin::token());
        if let Some(token) = token {
            command
                .args(["-c", GITHUB_CREDENTIAL])
                .env(GITHUB_TOKEN_VARIABLE, token);
        } else if let Some(gh) = &self.git.gh {
            let helper = format!(
                "credential.https://github.com.helper=!'{}' auth git-credential",
                gh.display()
            );
            command.args(["-c", &helper]);
        }
        self.check(self.git.spawn(command.args(args))?).map(drop)
    }

    /// Like [`check`], with the paths in errors relative to the project.
    fn check(&self, out: Output) -> Result<Vec<u8>, GitError> {
        check(out).map_err(|e| match e {
            GitError::LocalChanges { paths } => GitError::LocalChanges {
                paths: paths
                    .into_iter()
                    .map(|p| self.inside(&p).unwrap_or(p))
                    .collect(),
            },
            e => e,
        })
    }
}

/// The program's standard output when it succeeded, else the error its messages describe.
fn check(out: Output) -> Result<Vec<u8>, GitError> {
    if out.status.success() {
        return Ok(out.stdout);
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    Err(classify(&format!("{stderr}\n{stdout}")))
}

/// Recognises what went wrong from git's messages.
fn classify(text: &str) -> GitError {
    let has = |needles: &[&str]| needles.iter().any(|n| text.contains(n));
    if has(&["would be overwritten by"]) {
        return GitError::LocalChanges {
            paths: text
                .lines()
                .skip_while(|l| !l.contains("would be overwritten by"))
                .skip(1)
                .take_while(|l| l.starts_with('\t'))
                .map(|l| l.trim().to_owned())
                .collect(),
        };
    }
    if has(&[
        "Please tell me who you are",
        "Author identity unknown",
        "empty ident name",
        "unable to auto-detect email address",
    ]) {
        return GitError::NoIdentity;
    }
    if has(&[
        "nothing to commit",
        "nothing added to commit",
        "no changes added to commit",
    ]) {
        return GitError::NothingToCommit;
    }
    if has(&[
        "[rejected]",
        "[remote rejected]",
        "fetch first",
        "non-fast-forward",
    ]) {
        return GitError::Rejected;
    }
    if has(&[
        "could not read Username",
        "could not read Password",
        "Authentication failed",
        "terminal prompts disabled",
        "Invalid username or password",
        "Permission denied (publickey",
        "Host key verification failed",
        "returned error: 401",
        "returned error: 403",
    ]) {
        return GitError::SignIn;
    }
    if has(&[
        "Repository not found",
        "does not appear to be a git repository",
    ]) {
        return GitError::RemoteNotFound;
    }
    if has(&[
        "Could not resolve host",
        "Could not resolve hostname",
        "Failed to connect",
        "Connection refused",
        "timed out",
        "Network is unreachable",
        "Could not read from remote repository",
        "unable to access",
    ]) {
        return GitError::Unreachable;
    }
    let message = text
        .lines()
        .map(|l| {
            l.trim()
                .trim_start_matches("fatal: ")
                .trim_start_matches("error: ")
        })
        .filter(|l| !l.is_empty() && !l.starts_with("hint:"))
        .collect::<Vec<_>>()
        .join("\n");
    GitError::Failed { message }
}

/// `origin`, else the first remote there is.
fn default_remote(remotes: Vec<String>) -> Option<String> {
    if remotes.iter().any(|r| r == "origin") {
        Some("origin".into())
    } else {
        remotes.into_iter().next()
    }
}

/// What a `1` entry of `git status --porcelain=v2` means, from its two-letter code.
fn tracked_kind(rest: &str) -> ChangeKind {
    match rest.get(..2) {
        Some(xy) if xy.contains('D') => ChangeKind::Deleted,
        Some(xy) if xy.contains('A') => ChangeKind::Added,
        _ => ChangeKind::Modified,
    }
}

/// The `n`-th space-separated field and everything after it: a path, which may hold spaces.
fn nth_field(text: &str, n: usize) -> &str {
    text.splitn(n + 1, ' ').nth(n).unwrap_or_default()
}

/// A path relative to the project, refused if it could reach outside it.
fn pathspec(path: &str) -> Result<&str, GitError> {
    let inside = Path::new(path)
        .components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
    if !inside {
        return Err(GitError::Failed {
            message: tr!("git.error.outsideProject", path),
        });
    }
    Ok(if path.is_empty() { "." } else { path })
}

/// `git@github.com:acme/qa.git` and `https://github.com/acme/qa.git` → `github.com/acme/qa`.
pub fn display_url(url: &str) -> String {
    let url = url.trim().trim_end_matches('/');
    let url = url.strip_suffix(".git").unwrap_or(url);
    if let Some((_, rest)) = url.split_once("://") {
        let rest = rest.rsplit_once('@').map_or(rest, |(_, host)| host);
        return rest.to_owned();
    }
    match url.split_once(':') {
        Some((host, path)) if !host.contains('/') => {
            let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
            format!("{host}/{}", path.trim_start_matches('/'))
        }
        _ => url.to_owned(),
    }
}

/// `name` in `PATH` or in the usual tool folders, if it can run here.
fn find_tool(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .chain(TOOL_DIRS.iter().map(PathBuf::from))
        .map(|dir| dir.join(name))
        .find(|exe| exe.is_file() && runs_here(exe))
}

/// On macOS `/usr/bin/git` only stands in for the developer tools: without them it asks
/// to install them instead of running.
fn runs_here(exe: &Path) -> bool {
    if !cfg!(target_os = "macos") || exe.parent() != Some(Path::new("/usr/bin")) {
        return true;
    }
    Command::new("/usr/bin/xcode-select")
        .arg("--print-path")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .is_ok_and(|out| {
            out.status.success() && Path::new(String::from_utf8_lossy(&out.stdout).trim()).is_dir()
        })
}
