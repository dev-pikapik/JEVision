//! Notes of a test: what they are about, replies, files and links, mentions, the history of a note
//! in git and the notes of a folder. Paths are relative to the project folder.

use std::path::{Component, Path};

use jevision_core::{
    Attachment, Flow, IssueRef, Note, NoteAnchor, NoteEvent, NoteEventKind, NoteVersion,
    TEST_SUFFIX, UnusedNoteFiles,
};
use jevision_git::{GitError, GitHubRepo, IssueState, IssueStatus};

use crate::git::{git, repo};

/// The most versions of a test read to tell the history of a note: older ones are left out, and a
/// note first seen in the oldest one is told as written there.
const MAX_VERSIONS: usize = 500;

/// A test after an edit of a note, and the reply it made.
#[derive(uniffi::Record)]
pub struct ReplyAdded {
    pub flow: Flow,
    pub reply: String,
}

/// Makes note `note` about `anchor`. `None` for an unknown note and a block that isn't there.
#[uniffi::export]
pub fn note_set_anchor(mut flow: Flow, note: String, anchor: NoteAnchor) -> Option<Flow> {
    flow.set_note_anchor(&note, anchor).then_some(flow)
}

/// The notes that would lose the block they are about if the blocks `blocks` were removed, which
/// takes along more blocks than those when branches only they led to are left behind.
#[uniffi::export]
pub fn notes_left_by_removing(flow: Flow, blocks: Vec<String>) -> Vec<String> {
    flow.notes_left_by_removing(&blocks)
}

/// Adds a reply written by `author` at `time` (RFC 3339) to note `note`. `None` for an unknown
/// note and an empty text.
#[uniffi::export]
pub fn note_add_reply(
    mut flow: Flow,
    note: String,
    author: String,
    time: String,
    text: String,
) -> Option<ReplyAdded> {
    let reply = flow.add_reply(&note, &author, &time, &text)?;
    Some(ReplyAdded { flow, reply })
}

/// Changes the text of a reply. `None` for an unknown reply and an empty text.
#[uniffi::export]
pub fn note_edit_reply(mut flow: Flow, note: String, reply: String, text: String) -> Option<Flow> {
    flow.edit_reply(&note, &reply, &text).then_some(flow)
}

#[uniffi::export]
pub fn note_delete_reply(mut flow: Flow, note: String, reply: String) -> Option<Flow> {
    flow.delete_reply(&note, &reply).then_some(flow)
}

/// Marks the discussion under a note as settled by `by` at `time` (RFC 3339). `None` for an unknown
/// note and one that is settled already.
#[uniffi::export]
pub fn note_resolve(mut flow: Flow, note: String, by: String, time: String) -> Option<Flow> {
    flow.resolve_note(&note, &by, &time).then_some(flow)
}

/// Opens a settled discussion again.
#[uniffi::export]
pub fn note_reopen(mut flow: Flow, note: String) -> Option<Flow> {
    flow.reopen_note(&note).then_some(flow)
}

/// Adds a file (by its name in the note's folder) or a web address to a note. `None` for what
/// can't be one, and for what the note has already.
#[uniffi::export]
pub fn note_attach(mut flow: Flow, note: String, attachment: Attachment) -> Option<Flow> {
    flow.attach(&note, attachment).then_some(flow)
}

/// Takes an attachment off a note; its file stays where it is.
#[uniffi::export]
pub fn note_detach(mut flow: Flow, note: String, attachment: Attachment) -> Option<Flow> {
    flow.detach(&note, &attachment).then_some(flow)
}

/// The folder next to `test` that holds the files that come with its notes. It moves, and goes
/// to the trash, with the test.
#[uniffi::export]
pub fn note_assets_folder(test: String) -> String {
    jevision_core::assets_folder(&test)
}

/// The folder that holds the files of note `note` of `test`; `None` when the id can't be a folder
/// name.
#[uniffi::export]
pub fn note_files_folder(test: String, note: String) -> Option<String> {
    jevision_core::note_files_folder(&test, &note)
}

/// Where the attached file `name` of `note` is; `None` for names that could lead elsewhere.
#[uniffi::export]
pub fn note_file_path(test: String, note: String, name: String) -> Option<String> {
    jevision_core::note_file_path(&test, &note, &name)
}

/// The files in the notes folder of `test` that no note of `flow` holds any more, and their size.
#[uniffi::export]
pub fn note_unused_files(test: String, flow: Flow) -> UnusedNoteFiles {
    jevision_core::unused_note_files(&test, &flow)
}

/// The name to save a file given as `wanted` under in a note's folder where `taken` names are
/// already.
#[uniffi::export]
pub fn note_file_name(wanted: String, taken: Vec<String>) -> String {
    jevision_core::attachment_file_name(&wanted, &taken)
}

/// Whether `address` is a web page, the only kind of link a note keeps.
#[uniffi::export]
pub fn note_is_web_address(address: String) -> bool {
    jevision_core::is_web_address(&address)
}

/// The names to offer for the mention being typed at the end of `text`, best first.
#[uniffi::export]
pub fn note_mention_suggestions(text: String, names: Vec<String>) -> Vec<String> {
    jevision_core::mention_suggestions(&text, &names)
}

/// `text` with the mention being typed at its end completed with `name`.
#[uniffi::export]
pub fn note_complete_mention(text: String, name: String) -> String {
    jevision_core::complete_mention(&text, &name)
}

/// `text` with each `@name` turned into a Markdown link a view can draw as a mention.
#[uniffi::export]
pub fn note_mentions_as_links(text: String, names: Vec<String>) -> String {
    jevision_core::mentions_as_links(&text, &names)
}

/// The scheme of the links [`note_mentions_as_links`] makes.
#[uniffi::export]
pub fn note_mention_scheme() -> String {
    jevision_core::MENTION_SCHEME.to_owned()
}

/// The issue or pull request a web address points to on github.com.
#[uniffi::export]
pub fn note_github_issue(address: String) -> Option<IssueRef> {
    jevision_core::github_issue_ref(&address)
}

/// The title and state of an issue or pull request, when the user is signed in to GitHub
/// (`GitHubSignedOut` otherwise). Asks GitHub every time it is called.
#[uniffi::export]
pub fn note_github_issue_status(
    owner: String,
    repo: String,
    number: u64,
) -> Result<IssueStatus, GitError> {
    // Checks the names, which end up in the address asked for.
    let repo = GitHubRepo::from_url(&format!("https://github.com/{owner}/{repo}"))
        .ok_or(GitError::NotGitHub)?;
    git()?.github()?.issue_status(&repo, number)
}

/// The people who committed to the project, the one git signs with first.
#[uniffi::export]
pub fn git_authors(project: String) -> Result<Vec<String>, GitError> {
    repo(&project)?.authors()
}

/// Whether the project is tracked by git and Git LFS is installed.
#[uniffi::export]
pub fn git_lfs_available(project: String) -> bool {
    repo(&project).is_ok_and(|repo| repo.lfs_available())
}

/// Whether Git LFS already takes the file at `path`.
#[uniffi::export]
pub fn git_lfs_tracks(project: String, path: String) -> Result<bool, GitError> {
    repo(&project)?.lfs_tracks(&path)
}

/// Makes Git LFS take the file at `path`, writing a line for it to `.gitattributes`.
#[uniffi::export]
pub fn git_lfs_track(project: String, path: String) -> Result<(), GitError> {
    repo(&project)?.lfs_track(&path)
}

/// Drops the lines of `.gitattributes` that made Git LFS take only the files at `paths`, now gone.
#[uniffi::export]
pub fn git_lfs_untrack(project: String, paths: Vec<String>) -> Result<(), GitError> {
    repo(&project)?.lfs_untrack(&paths)
}

/// What happened to note `note` of `test` over the history of the test file in git, the newest
/// change first, with what isn't committed yet on top. Empty when git doesn't track the project.
#[uniffi::export]
pub fn note_history(
    project: String,
    test: String,
    note: String,
) -> Result<Vec<NoteEvent>, GitError> {
    let repo = match git().and_then(|git| git.open(Path::new(&project))) {
        Ok(Some(repo)) => repo,
        Ok(None) | Err(GitError::NotInstalled) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut commits = repo.history(&test)?;
    commits.truncate(MAX_VERSIONS);
    commits.reverse();
    let mut versions = Vec::with_capacity(commits.len() + 1);
    for commit in &commits {
        // The file is followed across renames: its path in each commit is the one that commit
        // changed.
        let path = commit
            .files
            .first()
            .map_or(test.as_str(), |f| f.path.as_str());
        let file = repo
            .file(&commit.id, path)?
            .and_then(|bytes| String::from_utf8(bytes).ok());
        versions.push(NoteVersion {
            commit: commit.short_id.clone(),
            author: commit.author.clone(),
            time: commit.time,
            file,
        });
    }
    let current = std::fs::read_to_string(Path::new(&project).join(&test)).ok();
    if versions.last().map(|v| &v.file) != Some(&current)
        && (current.is_some() || !versions.is_empty())
    {
        versions.push(NoteVersion {
            commit: String::new(),
            author: repo.identity().map(|i| i.name).unwrap_or_default(),
            time: 0,
            file: current,
        });
    }
    Ok(jevision_core::note_timeline(&note, &versions))
}

/// A note that is about the folder its test is in.
#[derive(uniffi::Record)]
pub struct FolderNote {
    /// The test file the note is in.
    pub test: String,
    pub name: String,
    pub note: Note,
}

/// The notes about `folder` ("" for the project folder): those in the tests directly in it that
/// are about the folder, tests in name order.
#[uniffi::export]
pub fn folder_notes(project: String, folder: String) -> Vec<FolderNote> {
    let relative = Path::new(&folder);
    if !relative
        .components()
        .all(|c| matches!(c, Component::Normal(_)))
    {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(Path::new(&project).join(relative)) else {
        return Vec::new();
    };
    let mut files: Vec<String> = entries
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|name| name.ends_with(TEST_SUFFIX))
        .collect();
    files.sort();
    let mut found = Vec::new();
    for file in files {
        let path = relative.join(&file);
        let Ok(text) = std::fs::read_to_string(Path::new(&project).join(&path)) else {
            continue;
        };
        let Ok(flow) = jevision_core::parse(&text) else {
            continue;
        };
        let test = path.to_string_lossy().into_owned();
        for note in flow.canvas.notes {
            if note.anchor == NoteAnchor::Folder {
                found.push(FolderNote {
                    test: test.clone(),
                    name: flow.name.clone(),
                    note,
                });
            }
        }
    }
    found
}

#[uniffi::remote(Record)]
pub struct UnusedNoteFiles {
    pub paths: Vec<String>,
    pub size: u64,
}

#[uniffi::remote(Record)]
pub struct IssueRef {
    pub owner: String,
    pub repo: String,
    pub number: u64,
}

#[uniffi::remote(Enum)]
pub enum NoteEventKind {
    Created,
    Edited,
    AnchoredToTest,
    AnchoredToFolder,
    AnchoredToBlock,
    Attached,
    Detached,
    Replied,
    ReplyEdited,
    ReplyRemoved,
    Resolved,
    Reopened,
    Removed,
}

#[uniffi::remote(Record)]
pub struct NoteEvent {
    pub kind: NoteEventKind,
    pub detail: String,
    pub author: String,
    pub commit: String,
    pub time: i64,
}

#[uniffi::remote(Enum)]
pub enum IssueState {
    Open,
    Closed,
    Merged,
}

#[uniffi::remote(Record)]
pub struct IssueStatus {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub state: IssueState,
    pub is_pull_request: bool,
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use jevision_core::{CanvasItem, Point};

    use super::*;

    fn flow() -> Flow {
        jevision_core::parse(
            r#"{"format":1,"name":"Sign in","app":"com.example.App","start":"a",
                "blocks":{"a":{"type":"pressKeys","keys":"a"}}}"#,
        )
        .unwrap()
    }

    fn sh(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .current_dir(dir)
            .args([
                "-c",
                "user.name=Ann Tester",
                "-c",
                "user.email=ann@example.com",
            ])
            .args([
                "-c",
                "commit.gpgsign=false",
                "-c",
                "init.defaultBranch=main",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn a_note_tells_its_history_and_folders_list_their_notes() {
        let dir = std::env::temp_dir().join(format!("jevision-ffi-notes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("login")).unwrap();
        let project = dir.to_str().unwrap().to_owned();
        let test = "login/sign-in.flow.json".to_owned();
        let save = |flow: &Flow| {
            std::fs::write(dir.join(&test), jevision_core::to_canonical_json(flow)).unwrap()
        };

        assert_eq!(
            note_history(project.clone(), test.clone(), "note".into()),
            Ok(Vec::new()),
            "no git"
        );
        sh(&dir, &["init", "-q"]);

        let edit = crate::add_note(
            flow(),
            "Check it".into(),
            "Ann Tester".into(),
            "t".into(),
            Point { x: 0.0, y: 0.0 },
            NoteAnchor::Folder,
        );
        let [CanvasItem::Note { id }] = edit.items.as_slice() else {
            panic!("a note was added");
        };
        let note = id.clone();
        save(&edit.flow);
        sh(&dir, &["add", "."]);
        sh(&dir, &["commit", "-qm", "Note"]);
        let reply = note_add_reply(
            edit.flow,
            note.clone(),
            "Bob".into(),
            "t".into(),
            "Done".into(),
        )
        .unwrap();
        save(&reply.flow);
        sh(&dir, &["commit", "-qam", "Reply"]);
        let settled =
            note_resolve(reply.flow, note.clone(), "Ann Tester".into(), "t".into()).unwrap();
        save(&settled);

        let events = note_history(project.clone(), test.clone(), note.clone()).unwrap();
        let kinds: Vec<_> = events
            .iter()
            .map(|e| (e.kind, e.author.as_str(), e.time == 0))
            .collect();
        assert_eq!(
            kinds,
            [
                (NoteEventKind::Resolved, "Ann Tester", true),
                (NoteEventKind::Replied, "Bob", false),
                (NoteEventKind::Created, "Ann Tester", false),
            ]
        );
        assert!(
            note_history(project.clone(), test.clone(), "other".into())
                .unwrap()
                .is_empty()
        );

        let notes = folder_notes(project.clone(), "login".into());
        assert_eq!(
            (notes.len(), notes[0].test.as_str(), notes[0].name.as_str()),
            (1, "login/sign-in.flow.json", "Sign in")
        );
        assert!(folder_notes(project.clone(), "".into()).is_empty());
        assert!(folder_notes(project.clone(), "../login".into()).is_empty());

        assert!(
            git_authors(project.clone())
                .unwrap()
                .contains(&"Ann Tester".to_owned())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn issue_status_refuses_names_that_are_not_a_repository() {
        for (owner, repo) in [("a/b", "c"), ("a", "../c"), ("", "c"), ("a", "c?x=1")] {
            assert_eq!(
                note_github_issue_status(owner.into(), repo.into(), 1),
                Err(GitError::NotGitHub),
                "{owner}/{repo}"
            );
        }
    }
}
