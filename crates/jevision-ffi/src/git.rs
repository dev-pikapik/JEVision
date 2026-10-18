//! Git for the open project, and what changed between two versions of a test or a picture.
//! Paths are relative to the project folder.

use std::path::Path;
use std::sync::OnceLock;

use jevision_core::{
    BlockChange, BlockChangeKind, ConflictKind, ConflictPart, Flow, FlowDiff, MergeChoice,
    MergeConflict,
};
use jevision_git::{
    ChangeKind, ConflictVersions, FileChange, Git, GitBranch, GitCommit, GitError, GitHub,
    GitHubAccount, GitIdentity, GitStatus, PullRequest, PullRequestChanges, PullRequestDraft, Repo,
};
use jevision_runner::PictureDiff;

use crate::FlowError;

/// The git installed on this Mac, looked for again until there is one.
fn git() -> Result<Git, GitError> {
    static FOUND: OnceLock<Git> = OnceLock::new();
    if let Some(git) = FOUND.get() {
        return Ok(git.clone());
    }
    let git = Git::find().ok_or(GitError::NotInstalled)?;
    Ok(FOUND.get_or_init(|| git).clone())
}

fn repo(project: &str) -> Result<Repo, GitError> {
    git()?
        .open(Path::new(project))?
        .ok_or(GitError::NotRepository)
}

/// Whether git is installed on this Mac.
#[uniffi::export]
pub fn git_installed() -> bool {
    git().is_ok()
}

/// Where the project stands in git; `None` when git doesn't track it.
#[uniffi::export]
pub fn git_status(project: String) -> Result<Option<GitStatus>, GitError> {
    git()?
        .open(Path::new(&project))?
        .map(|repo| repo.status())
        .transpose()
}

/// Starts tracking the project with a new repository.
#[uniffi::export]
pub fn git_start_tracking(project: String) -> Result<GitStatus, GitError> {
    git()?.init(Path::new(&project))?.status()
}

/// Commits that changed a test or a folder (empty `path` for the whole project), newest first.
#[uniffi::export]
pub fn git_history(project: String, path: String) -> Result<Vec<GitCommit>, GitError> {
    repo(&project)?.history(&path)
}

/// A file as it was in `commit`; `None` when it wasn't there.
#[uniffi::export]
pub fn git_file(
    project: String,
    commit: String,
    path: String,
) -> Result<Option<Vec<u8>>, GitError> {
    repo(&project)?.file(&commit, &path)
}

/// Writes `path` with the contents `source` had in `commit` (its name then, if renamed since).
#[uniffi::export]
pub fn git_restore(
    project: String,
    commit: String,
    source: String,
    path: String,
) -> Result<(), GitError> {
    repo(&project)?.restore(&commit, &source, &path)
}

/// Commits the changes to `paths` alone with exactly `message`; returns the commit's id.
#[uniffi::export]
pub fn git_commit(
    project: String,
    paths: Vec<String>,
    message: String,
) -> Result<String, GitError> {
    repo(&project)?.commit(&paths, &message)
}

/// Sends the branch's commits to the server.
#[uniffi::export]
pub fn git_push(project: String) -> Result<(), GitError> {
    repo(&project)?.push()
}

/// Brings in the server's commits. On a conflict nothing changes.
#[uniffi::export]
pub fn git_pull(project: String) -> Result<(), GitError> {
    repo(&project)?.pull()
}

/// A file that conflicted on pull: where the two sides parted, here and on the server.
#[uniffi::export]
pub fn git_conflict_versions(project: String, path: String) -> Result<ConflictVersions, GitError> {
    repo(&project)?.conflict_versions(&path)
}

/// A file that conflicted, as the user settled it; no contents removes it.
#[derive(uniffi::Record)]
pub struct SettledFile {
    pub path: String,
    pub contents: Option<Vec<u8>>,
}

/// Brings in the server's commits with the conflicted files as settled and commits the merge;
/// returns its id. On any failure nothing changes.
#[uniffi::export]
pub fn git_finish_pull(project: String, files: Vec<SettledFile>) -> Result<String, GitError> {
    let files: Vec<(String, Option<Vec<u8>>)> =
        files.into_iter().map(|f| (f.path, f.contents)).collect();
    repo(&project)?.finish_pull(&files)
}

/// Branches here, then those only on the server.
#[uniffi::export]
pub fn git_branches(project: String) -> Result<Vec<GitBranch>, GitError> {
    repo(&project)?.branches()
}

#[uniffi::export]
pub fn git_switch_branch(project: String, name: String) -> Result<(), GitError> {
    repo(&project)?.switch(&name)
}

/// Makes a branch from the current commit and switches to it.
#[uniffi::export]
pub fn git_create_branch(project: String, name: String) -> Result<(), GitError> {
    repo(&project)?.create_branch(&name)
}

/// Who commits are signed with; empty fields are not set.
#[uniffi::export]
pub fn git_identity(project: String) -> Result<GitIdentity, GitError> {
    repo(&project)?.identity()
}

/// Signs the user's commits in every repository with this name and email.
#[uniffi::export]
pub fn git_set_identity(project: String, identity: GitIdentity) -> Result<(), GitError> {
    git()?.set_identity(Path::new(&project), &identity)
}

/// Who GitHub CLI is signed in to GitHub as.
#[uniffi::export]
pub fn github_account() -> Result<GitHubAccount, GitError> {
    git()?.github()?.account()
}

/// The project's repository, once it is known to be on GitHub, and GitHub signed in.
fn on_github(project: &str) -> Result<(Repo, GitHub), GitError> {
    let repo = repo(project)?;
    repo.github()?;
    Ok((repo, git()?.github()?))
}

/// Open pull requests of the project's repository, the latest updated first.
#[uniffi::export]
pub fn github_pull_requests(project: String) -> Result<Vec<PullRequest>, GitError> {
    let (repo, hub) = on_github(&project)?;
    repo.pull_requests(&hub)
}

/// The files of the project a pull request changes, downloading its commits when needed.
#[uniffi::export]
pub fn github_pull_request_changes(
    project: String,
    pull_request: PullRequest,
) -> Result<PullRequestChanges, GitError> {
    repo(&project)?.pull_request_changes(&pull_request.base_commit, &pull_request.head_commit)
}

/// What a pull request from the current branch would start from.
#[uniffi::export]
pub fn github_pull_request_draft(project: String) -> Result<PullRequestDraft, GitError> {
    let (repo, hub) = on_github(&project)?;
    repo.pull_request_draft(&hub)
}

/// Proposes the current branch for `base`, pushing it first when `push` and it has commits
/// that aren't on GitHub. The title and description are exactly the user's.
#[uniffi::export]
pub fn github_create_pull_request(
    project: String,
    base: String,
    title: String,
    description: String,
    push: bool,
) -> Result<PullRequest, GitError> {
    let (repo, hub) = on_github(&project)?;
    if title.trim().is_empty() {
        return Err(GitError::EmptyMessage);
    }
    if push {
        repo.push()?;
    }
    repo.create_pull_request(&hub, &base, &title, &description)
}

/// What changed between two versions of a test and its canvas; a missing version has no blocks.
#[uniffi::export]
pub fn diff_flows(before: Option<Flow>, after: Option<Flow>) -> FlowDiff {
    jevision_core::diff(before.as_ref(), after.as_ref())
}

/// What both versions of a test changed differently since `base`, block by block.
#[uniffi::export]
pub fn merge_conflicts(base: Option<Flow>, mine: Flow, theirs: Flow) -> Vec<MergeConflict> {
    jevision_core::merge_conflicts(base.as_ref(), &mine, &theirs)
}

/// Both versions of a test combined, with each conflict settled by the choice at its position.
#[uniffi::export]
pub fn merge_flows(
    base: Option<Flow>,
    mine: Flow,
    theirs: Flow,
    choices: Vec<MergeChoice>,
) -> Result<Flow, FlowError> {
    jevision_core::merge_flows(base.as_ref(), &mine, &theirs, &choices)
        .map_err(|e| FlowError::Invalid { issues: e.issues })
}

/// How two versions of a picture differ; `None` when they can't be compared pixel by pixel.
#[uniffi::export]
pub fn diff_pictures(before: Vec<u8>, after: Vec<u8>) -> Option<PictureDiff> {
    jevision_runner::picture_diff(&before, &after)
}

#[uniffi::remote(Error)]
pub enum GitError {
    NotInstalled,
    NotRepository,
    NoCommits,
    NoRemote,
    NoUpstream { branch: String },
    Detached,
    SignIn,
    RemoteNotFound,
    Unreachable,
    Rejected,
    Conflict { paths: Vec<String> },
    LocalChanges { paths: Vec<String> },
    NoIdentity,
    EmptyMessage,
    NothingToCommit,
    BadBranchName { name: String },
    BranchExists { name: String },
    NotInCommit { path: String },
    NoGitHubCli,
    GitHubSignedOut,
    NotGitHub,
    NoPermission,
    NotPushed { branch: String },
    PullRequestExists,
    NothingToPropose { base: String },
    Failed { message: String },
}

#[uniffi::remote(Enum)]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    Conflicted,
}

#[uniffi::remote(Record)]
pub struct FileChange {
    pub path: String,
    pub from: Option<String>,
    pub kind: ChangeKind,
}

#[uniffi::remote(Record)]
pub struct GitStatus {
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub has_commits: bool,
    pub remote: Option<String>,
    pub github: Option<String>,
    pub changes: Vec<FileChange>,
}

#[uniffi::remote(Record)]
pub struct GitCommit {
    pub id: String,
    pub short_id: String,
    pub parent: Option<String>,
    pub author: String,
    pub time: i64,
    pub summary: String,
    pub files: Vec<FileChange>,
}

#[uniffi::remote(Record)]
pub struct GitBranch {
    pub name: String,
    pub is_current: bool,
    pub is_remote: bool,
}

#[uniffi::remote(Record)]
pub struct GitIdentity {
    pub name: String,
    pub email: String,
}

#[uniffi::remote(Record)]
pub struct GitHubAccount {
    pub login: String,
    pub name: String,
}

#[uniffi::remote(Record)]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    pub author: String,
    pub url: String,
    pub head: String,
    pub head_commit: String,
    pub base: String,
    pub base_commit: String,
    pub draft: bool,
    pub updated: i64,
}

#[uniffi::remote(Record)]
pub struct PullRequestDraft {
    pub branch: String,
    pub base: String,
    pub bases: Vec<String>,
    pub unpushed: bool,
    pub existing: Option<PullRequest>,
}

#[uniffi::remote(Record)]
pub struct PullRequestChanges {
    pub before: String,
    pub after: String,
    pub files: Vec<FileChange>,
}

#[uniffi::remote(Enum)]
pub enum BlockChangeKind {
    Added,
    Removed,
    Changed,
}

#[uniffi::remote(Record)]
pub struct BlockChange {
    pub id: String,
    pub kind: BlockChangeKind,
    pub fields: Vec<String>,
}

#[uniffi::remote(Record)]
pub struct FlowDiff {
    pub blocks: Vec<BlockChange>,
    pub test_fields: Vec<String>,
    pub notes: Vec<BlockChange>,
    pub sections: Vec<BlockChange>,
}

#[uniffi::remote(Record)]
pub struct ConflictVersions {
    pub base: Option<Vec<u8>>,
    pub mine: Option<Vec<u8>>,
    pub theirs: Option<Vec<u8>>,
}

#[uniffi::remote(Enum)]
pub enum ConflictPart {
    Test,
    Block,
    Note,
    Frame,
}

#[uniffi::remote(Enum)]
pub enum ConflictKind {
    Changed,
    Link,
    RemovedHere,
    RemovedThere,
    AddedBoth,
}

#[uniffi::remote(Enum)]
pub enum MergeChoice {
    Mine,
    Theirs,
    Both,
}

#[uniffi::remote(Record)]
pub struct MergeConflict {
    pub part: ConflictPart,
    pub id: String,
    pub field: Option<String>,
    pub kind: ConflictKind,
    pub can_keep_both: bool,
}

#[uniffi::remote(Record)]
pub struct PictureDiff {
    pub similarity: f64,
    pub differing: u64,
    pub png: Vec<u8>,
}
