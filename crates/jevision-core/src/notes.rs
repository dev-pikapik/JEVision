//! Notes: what they are about, the discussion under them, the files that come with them, the
//! people they mention and how a note changed over the history of its test.

use std::collections::HashSet;

use crate::decode::parse;
use crate::model::{
    Attachment, AttachmentKind, Flow, Note, NoteAnchor, Reply, Resolved, TEST_SUFFIX,
};

/// Ending of the folder that holds the files that come with the notes of a test.
pub const NOTE_ASSETS_SUFFIX: &str = ".assets";
/// Folder of that folder with one more folder for each note.
const NOTES_FOLDER: &str = "notes";
/// The longest file name given to an attachment, in bytes.
const MAX_NAME: usize = 120;
/// How many names [`mention_suggestions`] offers at most.
const MAX_SUGGESTIONS: usize = 6;
/// Scheme of the address a mention is turned into by [`mentions_as_links`].
pub const MENTION_SCHEME: &str = "jevision-mention";

impl Flow {
    fn note_mut(&mut self, id: &str) -> Option<&mut Note> {
        self.canvas.notes.iter_mut().find(|n| n.id == id)
    }

    /// Makes the notes about the blocks in `gone` notes about the test, or removes them with
    /// `delete`.
    pub(crate) fn let_go_of_notes(&mut self, gone: &HashSet<String>, delete: bool) {
        let about_gone =
            |n: &Note| matches!(&n.anchor, NoteAnchor::Block { id } if gone.contains(id));
        if delete {
            self.canvas.notes.retain(|n| !about_gone(n));
        } else {
            for n in self.canvas.notes.iter_mut().filter(|n| about_gone(n)) {
                n.anchor = NoteAnchor::Test;
            }
        }
    }

    /// The notes that lose their block when the blocks `ids` are removed, which takes along more
    /// blocks than those when branches only they led to are left unreachable.
    pub fn notes_left_by_removing(&self, ids: &[String]) -> Vec<String> {
        let mut after = self.clone();
        for id in ids {
            after.remove_block(id);
        }
        let there = |flow: &Flow, id: &str| flow.blocks.iter().any(|b| b.id == id);
        self.canvas
            .notes
            .iter()
            .filter(|n| {
                matches!(&n.anchor, NoteAnchor::Block { id } if there(self, id) && !there(&after, id))
            })
            .map(|n| n.id.clone())
            .collect()
    }

    /// Makes note `note` about `anchor`. Refuses, returning `false`, an unknown note and a
    /// block that isn't there.
    pub fn set_note_anchor(&mut self, note: &str, anchor: NoteAnchor) -> bool {
        if let NoteAnchor::Block { id } = &anchor {
            if !self.blocks.iter().any(|b| &b.id == id) {
                return false;
            }
        }
        self.note_mut(note).map(|n| n.anchor = anchor).is_some()
    }

    /// Adds a reply to note `note`; returns its id. Refuses an unknown note and an empty text.
    pub fn add_reply(
        &mut self,
        note: &str,
        author: &str,
        time: &str,
        text: &str,
    ) -> Option<String> {
        let text = text.trim();
        let note = self.note_mut(note).filter(|_| !text.is_empty())?;
        let id = unused(note.replies.iter().map(|r| r.id.as_str()), "reply");
        note.replies.push(Reply {
            id: id.clone(),
            author: author.to_owned(),
            time: time.to_owned(),
            text: text.to_owned(),
        });
        Some(id)
    }

    /// Changes the text of a reply; refuses an empty text.
    pub fn edit_reply(&mut self, note: &str, reply: &str, text: &str) -> bool {
        let text = text.trim();
        let reply = self
            .note_mut(note)
            .and_then(|n| n.replies.iter_mut().find(|r| r.id == reply))
            .filter(|_| !text.is_empty());
        reply.map(|r| r.text = text.to_owned()).is_some()
    }

    pub fn delete_reply(&mut self, note: &str, reply: &str) -> bool {
        let Some(note) = self.note_mut(note) else {
            return false;
        };
        let before = note.replies.len();
        note.replies.retain(|r| r.id != reply);
        note.replies.len() != before
    }

    /// Marks the discussion under a note as settled by `by`. Refuses an unknown note and one
    /// that is settled already.
    pub fn resolve_note(&mut self, note: &str, by: &str, time: &str) -> bool {
        match self.note_mut(note) {
            Some(n) if n.resolved.is_none() => {
                n.resolved = Some(Resolved {
                    by: by.to_owned(),
                    time: time.to_owned(),
                });
                true
            }
            _ => false,
        }
    }

    /// Opens a settled discussion again.
    pub fn reopen_note(&mut self, note: &str) -> bool {
        self.note_mut(note)
            .and_then(|n| n.resolved.take())
            .is_some()
    }

    /// Adds an attachment to a note. Refuses an unknown note, one that has it already, a file
    /// name that isn't one name in the note's folder and a link that isn't a web address.
    pub fn attach(&mut self, note: &str, attachment: Attachment) -> bool {
        let fine = match attachment.kind {
            AttachmentKind::File => is_safe_name(&attachment.value),
            AttachmentKind::Link => is_web_address(&attachment.value),
        };
        match self.note_mut(note) {
            Some(n) if fine && !n.attachments.contains(&attachment) => {
                n.attachments.push(attachment);
                true
            }
            _ => false,
        }
    }

    /// Removes an attachment from a note. The file stays where it is; whoever removes it has
    /// the folder of [`note_files_folder`] to clean up.
    pub fn detach(&mut self, note: &str, attachment: &Attachment) -> bool {
        let Some(note) = self.note_mut(note) else {
            return false;
        };
        let before = note.attachments.len();
        note.attachments.retain(|a| a != attachment);
        note.attachments.len() != before
    }
}

/// `base`, or `base2`, `base3`… — the first one not in `taken`.
fn unused<'a>(taken: impl Iterator<Item = &'a str>, base: &str) -> String {
    let taken: HashSet<&str> = taken.collect();
    std::iter::once(base.to_owned())
        .chain((2..).map(|n| format!("{base}{n}")))
        .find(|id| !taken.contains(id.as_str()))
        .expect("an unused id always exists")
}

/// Whether `name` can be one folder or file name inside the project without leading anywhere
/// else or hiding: no slash, no leading dot, no control characters.
pub fn is_safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && !name.starts_with('.')
        && !name
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':'))
}

/// Whether `address` is a web page: `http` or `https`, a host and nothing that breaks a line.
pub fn is_web_address(address: &str) -> bool {
    let rest = address
        .strip_prefix("https://")
        .or_else(|| address.strip_prefix("http://"));
    rest.is_some_and(|r| {
        r.chars().next().is_some_and(|c| c.is_alphanumeric())
            && !address.chars().any(|c| c.is_whitespace() || c.is_control())
    })
}

/// Folder, relative to the project and next to the test file, that holds the files that come
/// with the notes of `test`: `login/sign-in.flow.json` has `login/sign-in.assets`. Renaming or
/// deleting the test moves or deletes the folder with it.
pub fn assets_folder(test: &str) -> String {
    let stem = test.strip_suffix(TEST_SUFFIX).unwrap_or(test);
    format!("{stem}{NOTE_ASSETS_SUFFIX}")
}

/// Folder that holds the files of note `note` of `test`; `None` when the id can't be a folder
/// name.
pub fn note_files_folder(test: &str, note: &str) -> Option<String> {
    is_safe_name(note).then(|| format!("{}/{NOTES_FOLDER}/{note}", assets_folder(test)))
}

/// Where the attached file `name` of note `note` of `test` is, relative to the project; `None`
/// for a note id or a file name that can't be one name in a folder.
pub fn note_file_path(test: &str, note: &str, name: &str) -> Option<String> {
    let folder = note_files_folder(test, note)?;
    is_safe_name(name).then(|| format!("{folder}/{name}"))
}

/// The name to save a file given as `wanted` under in a note's folder: the last part of it, with
/// what can't be in a name replaced, no longer than needed and numbered (`photo 2.png`) when
/// `taken` has it already.
pub fn attachment_file_name(wanted: &str, taken: &[String]) -> String {
    let last = wanted.rsplit(['/', '\\']).next().unwrap_or_default();
    let clean: String = last
        .chars()
        .map(|c| {
            if c.is_control() || r#":*?[]"<>|"#.contains(c) {
                '-'
            } else {
                c
            }
        })
        .collect();
    let clean = clean.trim().trim_start_matches('.').trim();
    let clean = if clean.is_empty() { "file" } else { clean };
    let (stem, ext) = match clean.rfind('.') {
        Some(dot) if dot > 0 && clean.len() - dot <= 12 => clean.split_at(dot),
        _ => (clean, ""),
    };
    let room = MAX_NAME.saturating_sub(ext.len() + 4);
    let stem: String = stem
        .chars()
        .scan(0, |len, c| {
            *len += c.len_utf8();
            (*len <= room).then_some(c)
        })
        .collect();
    let taken: HashSet<String> = taken.iter().map(|t| t.to_lowercase()).collect();
    std::iter::once(format!("{stem}{ext}"))
        .chain((2..).map(|n| format!("{stem} {n}{ext}")))
        .find(|name| !taken.contains(&name.to_lowercase()))
        .expect("an unused name always exists")
}

/// A GitHub issue or pull request a web address points to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueRef {
    pub owner: String,
    pub repo: String,
    pub number: u64,
}

/// The issue or pull request at `address`, such as `https://github.com/owner/repo/issues/12`.
pub fn github_issue_ref(address: &str) -> Option<IssueRef> {
    let rest = address
        .strip_prefix("https://")
        .or_else(|| address.strip_prefix("http://"))?;
    let rest = rest
        .strip_prefix("github.com/")
        .or_else(|| rest.strip_prefix("www.github.com/"))?;
    let rest = rest.split(['?', '#']).next()?;
    let mut parts = rest.trim_end_matches('/').split('/');
    let (owner, repo, kind, number) = (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
    let name = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    if parts.next().is_some() || !matches!(kind, "issues" | "pull") || !name(owner) || !name(repo) {
        return None;
    }
    Some(IssueRef {
        owner: owner.to_owned(),
        repo: repo.to_owned(),
        number: number.parse().ok().filter(|n| *n > 0)?,
    })
}

/// The mention being typed at the end of `text`: where its `@` is and what follows it. A
/// mention starts at the start of the text or after a space and has no line break in it.
fn open_mention(text: &str) -> Option<(usize, &str)> {
    let at = text.rfind('@')?;
    let typed = &text[at + 1..];
    let after_space = text[..at]
        .chars()
        .next_back()
        .is_none_or(char::is_whitespace);
    (after_space && typed.chars().count() <= 40 && !typed.contains('\n')).then_some((at, typed))
}

/// The names to offer for the mention being typed at the end of `text`, best first: names
/// that start with what was typed after the `@`, then names where a word starts with it. Empty
/// when no mention is being typed or nothing fits.
pub fn mention_suggestions(text: &str, names: &[String]) -> Vec<String> {
    let Some((_, typed)) = open_mention(text) else {
        return Vec::new();
    };
    let typed = typed.to_lowercase();
    let lower = |n: &String| n.to_lowercase();
    let starts: Vec<&String> = names
        .iter()
        .filter(|n| lower(n).starts_with(&typed))
        .collect();
    let words = names.iter().filter(|n| {
        !lower(n).starts_with(&typed) && lower(n).split_whitespace().any(|w| w.starts_with(&typed))
    });
    let mut out: Vec<String> = Vec::new();
    for name in starts.into_iter().chain(words) {
        if !out.contains(name) {
            out.push(name.clone());
        }
        if out.len() == MAX_SUGGESTIONS {
            break;
        }
    }
    out
}

/// `text` with the mention being typed at its end completed to `@name `.
pub fn complete_mention(text: &str, name: &str) -> String {
    match open_mention(text) {
        Some((at, _)) => format!("{}@{name} ", &text[..at]),
        None => text.to_owned(),
    }
}

/// `text` with each `@name` turned into a Markdown link to `jevision-mention:name`, so that
/// it is drawn as a mention. `names` are the people the project knows, who may have spaces in
/// their names; any other `@word` is a mention too. An `@` inside a word, such as in an email
/// address, and inside code is left alone.
pub fn mentions_as_links(text: &str, names: &[String]) -> String {
    let mut names: Vec<&String> = names.iter().filter(|n| !n.is_empty()).collect();
    names.sort_by_key(|n| std::cmp::Reverse(n.chars().count()));
    let mut out = String::with_capacity(text.len());
    let mut in_code = false;
    let mut previous = None;
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        if c == '`' {
            in_code = !in_code;
        }
        let starts = c == '@' && !in_code && previous.is_none_or(|p: char| !p.is_alphanumeric());
        let mention = starts.then(|| mention_at(rest, &names)).flatten();
        match mention {
            Some(len) => {
                let name = &rest[1..len];
                let encoded: String = name
                    .bytes()
                    .map(|b| match b {
                        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => {
                            (b as char).to_string()
                        }
                        _ => format!("%{b:02X}"),
                    })
                    .collect();
                out.push_str(&format!("[@{name}]({MENTION_SCHEME}:{encoded})"));
                previous = name.chars().next_back();
                rest = &rest[len..];
            }
            None => {
                out.push(c);
                previous = Some(c);
                rest = &rest[c.len_utf8()..];
            }
        }
    }
    out
}

/// How long the mention at the start of `rest`, which begins with `@`, is in bytes, `@`
/// included: a known name, else the word after the `@`.
fn mention_at(rest: &str, names: &[&String]) -> Option<usize> {
    let after = &rest[1..];
    let ends_word = |len: usize| {
        after[len..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_alphanumeric() && c != '_')
    };
    for name in names {
        let Some(head) = after.get(..name.len()) else {
            continue;
        };
        if head.to_lowercase() == name.to_lowercase() && ends_word(name.len()) {
            return Some(1 + name.len());
        }
    }
    let word: usize = after
        .char_indices()
        .take_while(|(_, c)| c.is_alphanumeric() || matches!(c, '_' | '.' | '-'))
        .map(|(i, c)| i + c.len_utf8())
        .last()?;
    let word = after[..word].trim_end_matches(['.', '-']).len();
    (word > 0).then_some(1 + word)
}

/// One saved version of a test file, for [`note_timeline`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteVersion {
    /// What the version is called, such as a short commit id; empty for what isn't committed.
    pub commit: String,
    /// Who saved it.
    pub author: String,
    /// When, in seconds since 1970; 0 when not known.
    pub time: i64,
    /// The text of the test file; `None` where the test wasn't there.
    pub file: Option<String>,
}

/// What happened to a note.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteEventKind {
    /// Written; the detail is its text.
    Created,
    /// The text changed; the detail is the new text.
    Edited,
    AnchoredToTest,
    AnchoredToFolder,
    /// Made about a block; the detail is the block's id.
    AnchoredToBlock,
    /// A file or link added; the detail is its name or address.
    Attached,
    Detached,
    /// A reply added; the detail is its text.
    Replied,
    ReplyEdited,
    ReplyRemoved,
    Resolved,
    Reopened,
    /// The note is gone.
    Removed,
}

/// One change to a note, made by whoever saved the version it came with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteEvent {
    pub kind: NoteEventKind,
    pub detail: String,
    /// Who made the change: whoever saved the version, or whoever the note says wrote the reply
    /// or settled the discussion.
    pub author: String,
    /// The version it came with.
    pub commit: String,
    pub time: i64,
}

/// What happened to note `note` over `versions` of its test, oldest version first, as a list
/// with the newest change first. A note's first version tells only that it was written. A
/// version that doesn't read as a test is passed over; so is one that left the note as it was.
/// Moving or resizing a note is not a change.
pub fn note_timeline(note: &str, versions: &[NoteVersion]) -> Vec<NoteEvent> {
    let mut groups: Vec<Vec<NoteEvent>> = Vec::new();
    let mut before: Option<Note> = None;
    for version in versions {
        let now = match &version.file {
            None => None,
            Some(text) => match parse(text) {
                Ok(flow) => flow.canvas.notes.into_iter().find(|n| n.id == note),
                Err(_) => continue,
            },
        };
        let mut events = Vec::new();
        let mut add = |kind, detail: &str, author: &str| {
            events.push(NoteEvent {
                kind,
                detail: detail.to_owned(),
                author: if author.is_empty() {
                    &version.author
                } else {
                    author
                }
                .to_owned(),
                commit: version.commit.clone(),
                time: version.time,
            });
        };
        match (&before, &now) {
            (None, Some(now)) => add(NoteEventKind::Created, &now.text, ""),
            (Some(was), Some(now)) => changes(was, now, &mut add),
            (Some(_), None) => add(NoteEventKind::Removed, "", ""),
            (None, None) => {}
        }
        if !events.is_empty() {
            groups.push(events);
        }
        before = now;
    }
    groups.into_iter().rev().flatten().collect()
}

fn changes(was: &Note, now: &Note, add: &mut impl FnMut(NoteEventKind, &str, &str)) {
    if was.text != now.text {
        add(NoteEventKind::Edited, &now.text, "");
    }
    if was.anchor != now.anchor {
        match &now.anchor {
            NoteAnchor::Test => add(NoteEventKind::AnchoredToTest, "", ""),
            NoteAnchor::Folder => add(NoteEventKind::AnchoredToFolder, "", ""),
            NoteAnchor::Block { id } => add(NoteEventKind::AnchoredToBlock, id, ""),
        }
    }
    for a in now
        .attachments
        .iter()
        .filter(|a| !was.attachments.contains(a))
    {
        add(NoteEventKind::Attached, &a.value, "");
    }
    for a in was
        .attachments
        .iter()
        .filter(|a| !now.attachments.contains(a))
    {
        add(NoteEventKind::Detached, &a.value, "");
    }
    for r in &now.replies {
        match was.replies.iter().find(|old| old.id == r.id) {
            None => add(NoteEventKind::Replied, &r.text, &r.author),
            Some(old) if old.text != r.text => add(NoteEventKind::ReplyEdited, &r.text, ""),
            Some(_) => {}
        }
    }
    for r in was
        .replies
        .iter()
        .filter(|old| !now.replies.iter().any(|r| r.id == old.id))
    {
        add(NoteEventKind::ReplyRemoved, &r.text, "");
    }
    match (&was.resolved, &now.resolved) {
        (None, Some(r)) => add(NoteEventKind::Resolved, "", &r.by),
        (Some(_), None) => add(NoteEventKind::Reopened, "", ""),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Point, to_canonical_json};

    fn flow() -> Flow {
        parse(
            r#"{"format":1,"name":"T","app":"com.example.App","start":"a",
                "blocks":{"a":{"type":"pressKeys","keys":"a","next":"b"},
                          "b":{"type":"pressKeys","keys":"b"}}}"#,
        )
        .unwrap()
    }

    fn with_note() -> (Flow, String) {
        let mut f = flow();
        let CanvasItem::Note { id } = f.add_note(
            "Check **twice**",
            "Anna",
            "2026-10-18T09:30:00Z",
            Point { x: 0.0, y: 0.0 },
            NoteAnchor::Test,
        ) else {
            unreachable!()
        };
        (f, id)
    }

    use crate::CanvasItem;

    #[test]
    fn replies_resolving_and_attachments_are_edited_in_the_core() {
        let (mut f, n) = with_note();
        assert_eq!(
            f.add_reply(&n, "Bob", "t1", " Agreed "),
            Some("reply".into())
        );
        assert_eq!(f.add_reply(&n, "Bob", "t2", "More"), Some("reply2".into()));
        assert_eq!(f.add_reply(&n, "Bob", "t3", "  "), None);
        assert_eq!(f.add_reply("none", "Bob", "t3", "x"), None);
        assert!(f.edit_reply(&n, "reply2", "Even more"));
        assert!(f.delete_reply(&n, "reply"));
        assert!(!f.delete_reply(&n, "reply"));
        assert_eq!(f.add_reply(&n, "Bob", "t4", "Again"), Some("reply".into()));

        assert!(f.resolve_note(&n, "Bob", "t5"));
        assert!(!f.resolve_note(&n, "Eve", "t6"), "settled once");
        assert!(f.reopen_note(&n));
        assert!(!f.reopen_note(&n));

        let file = Attachment {
            kind: AttachmentKind::File,
            value: "shot.png".into(),
        };
        assert!(f.attach(&n, file.clone()));
        assert!(!f.attach(&n, file.clone()), "no twice");
        for bad in ["../x", "a/b.png", ".hidden", "", "c:d"] {
            let a = Attachment {
                kind: AttachmentKind::File,
                value: bad.into(),
            };
            assert!(!f.attach(&n, a), "{bad}");
        }
        let link = |value: &str| Attachment {
            kind: AttachmentKind::Link,
            value: value.into(),
        };
        assert!(f.attach(&n, link("https://github.com/o/r/issues/1")));
        assert!(!f.attach(&n, link("file:///etc/passwd")));
        assert!(!f.attach(&n, link("javascript:alert(1)")));
        assert!(f.detach(&n, &file));
        assert!(!f.detach(&n, &file));

        let saved = parse(&to_canonical_json(&f)).unwrap();
        assert_eq!(saved, f);
        let note = &saved.canvas.notes[0];
        assert_eq!(note.replies.len(), 2);
        assert_eq!(note.attachments, [link("https://github.com/o/r/issues/1")]);
    }

    #[test]
    fn a_note_about_a_block_moves_with_it_and_survives_its_removal() {
        let (mut f, n) = with_note();
        assert!(!f.set_note_anchor(&n, NoteAnchor::Block { id: "zzz".into() }));
        assert!(f.set_note_anchor(&n, NoteAnchor::Block { id: "b".into() }));
        let b = CanvasItem::Block { id: "b".into() };
        f.canvas.notes[0].rect.x = 500.0;
        f.canvas.notes[0].rect.y = 500.0;
        assert!(f.move_items(std::slice::from_ref(&b), 32.0, 64.0, false));
        let (moved, block) = (&f.canvas.notes[0].rect, &f.canvas.positions["b"]);
        let (dx, dy) = (moved.x - 500.0, moved.y - 500.0);
        assert!(dx > 0.0 && dy > 0.0, "the note followed its block");
        assert_eq!((block.x - dx, block.y - dy), {
            let mut g = flow();
            g.pin_blocks();
            (g.canvas.positions["b"].x, g.canvas.positions["b"].y)
        });

        assert_eq!(
            f.notes_left_by_removing(&["a".into()]),
            Vec::<String>::new()
        );
        assert_eq!(
            f.notes_left_by_removing(&["b".into()]),
            std::slice::from_ref(&n)
        );
        let mut kept = f.clone();
        kept.remove_block("b");
        assert_eq!(kept.canvas.notes[0].anchor, NoteAnchor::Test);
        let mut gone = f.clone();
        gone.remove_block_with_notes("b");
        assert!(gone.canvas.notes.is_empty());
        let mut items = f.clone();
        items.remove_items_with_notes(std::slice::from_ref(&b));
        assert!(items.canvas.notes.is_empty());
        f.remove_items(&[b]);
        assert_eq!(f.canvas.notes[0].anchor, NoteAnchor::Test);
    }

    #[test]
    fn files_of_a_note_live_next_to_its_test_and_never_escape() {
        assert_eq!(
            assets_folder("login/sign-in.flow.json"),
            "login/sign-in.assets"
        );
        assert_eq!(assets_folder("a.flow.json"), "a.assets");
        assert_eq!(
            note_file_path("login/sign-in.flow.json", "note", "shot.png").as_deref(),
            Some("login/sign-in.assets/notes/note/shot.png")
        );
        assert_eq!(note_file_path("a.flow.json", "../x", "shot.png"), None);
        assert_eq!(note_file_path("a.flow.json", "n", "../shot.png"), None);
        assert_eq!(note_files_folder("a.flow.json", "").as_deref(), None);

        let taken = vec!["photo.png".to_owned(), "Photo 2.png".to_owned()];
        assert_eq!(
            attachment_file_name("/Users/me/a/photo.png", &taken),
            "photo 3.png"
        );
        assert_eq!(attachment_file_name("PHOTO.PNG", &taken), "PHOTO 3.PNG");
        assert_eq!(attachment_file_name("..hidden:name", &[]), "hidden-name");
        assert_eq!(attachment_file_name("a*b?[1].png", &[]), "a-b--1-.png");
        assert_eq!(attachment_file_name("", &[]), "file");
        assert_eq!(attachment_file_name("a.tar.gz", &[]), "a.tar.gz");
        let long = format!("{}.pdf", "я".repeat(300));
        let name = attachment_file_name(&long, &[]);
        assert!(name.len() <= MAX_NAME && name.ends_with(".pdf") && is_safe_name(&name));
    }

    #[test]
    fn github_issues_are_told_from_other_links() {
        let r = |a: &str| github_issue_ref(a).map(|r| (r.owner, r.repo, r.number));
        assert_eq!(
            r("https://github.com/dev-pikapik/JEVision/issues/12"),
            Some(("dev-pikapik".into(), "JEVision".into(), 12))
        );
        assert_eq!(
            r("https://github.com/o/r.js/pull/3/?x=1#c"),
            Some(("o".into(), "r.js".into(), 3))
        );
        for no in [
            "https://github.com/o/r",
            "https://github.com/o/r/issues",
            "https://github.com/o/r/issues/0",
            "https://github.com/o/r/issues/x",
            "https://github.com/o/r/issues/1/files",
            "https://gitlab.com/o/r/issues/1",
            "https://github.com.evil.org/o/r/issues/1",
            "ftp://github.com/o/r/issues/1",
        ] {
            assert_eq!(r(no), None, "{no}");
        }
    }

    #[test]
    fn mentions_are_offered_completed_and_drawn() {
        let names: Vec<String> = ["Anna Petrova", "Bob", "Annabel", "Carl Anders"]
            .map(String::from)
            .into();
        assert_eq!(
            mention_suggestions("hi @an", &names),
            ["Anna Petrova", "Annabel", "Carl Anders"]
        );
        assert_eq!(mention_suggestions("@", &names).len(), 4);
        assert!(
            mention_suggestions("mail@an", &names).is_empty(),
            "inside a word"
        );
        assert!(mention_suggestions("hi @bob\nnext", &names).is_empty());
        assert!(mention_suggestions("no mention", &names).is_empty());
        assert_eq!(
            complete_mention("hi @an", "Anna Petrova"),
            "hi @Anna Petrova "
        );
        assert_eq!(complete_mention("none", "Bob"), "none");

        assert_eq!(
            mentions_as_links("@Anna Petrova look, and @Bob. mail a@b.org `@code`", &names),
            "[@Anna Petrova](jevision-mention:Anna%20Petrova) look, and [@Bob](jevision-mention:Bob). mail a@b.org `@code`"
        );
        assert_eq!(
            mentions_as_links("@stranger!", &names),
            "[@stranger](jevision-mention:stranger)!"
        );
        assert_eq!(mentions_as_links("a lone @ here", &names), "a lone @ here");
        assert_eq!(
            mentions_as_links("@Анна", &names),
            "[@Анна](jevision-mention:%D0%90%D0%BD%D0%BD%D0%B0)"
        );
    }

    fn version(commit: &str, author: &str, time: i64, flow: Option<&Flow>) -> NoteVersion {
        NoteVersion {
            commit: commit.into(),
            author: author.into(),
            time,
            file: flow.map(to_canonical_json),
        }
    }

    #[test]
    fn the_timeline_of_a_note_follows_the_versions_of_its_test() {
        let (mut f, n) = with_note();
        let v1 = f.clone();
        f.canvas.notes[0].rect.x += 40.0;
        let moved = f.clone();
        f.canvas.notes[0].text = "Check twice".into();
        let v2 = f.clone();
        f.add_reply(&n, "Bob", "t", "Agreed");
        f.attach(
            &n,
            Attachment {
                kind: AttachmentKind::File,
                value: "shot.png".into(),
            },
        );
        f.set_note_anchor(&n, NoteAnchor::Block { id: "b".into() });
        let v3 = f.clone();
        f.resolve_note(&n, "Anna", "t");
        f.detach(
            &n,
            &Attachment {
                kind: AttachmentKind::File,
                value: "shot.png".into(),
            },
        );
        let v4 = f.clone();
        f.canvas.notes.clear();
        let v5 = f.clone();

        let versions = [
            version("c0", "Eve", 5, None),
            version("c1", "Anna", 10, Some(&v1)),
            version("c1b", "Anna", 11, Some(&moved)),
            NoteVersion {
                commit: "bad".into(),
                author: "?".into(),
                time: 12,
                file: Some("{".into()),
            },
            version("c2", "Anna", 20, Some(&v2)),
            version("c3", "Bob", 30, Some(&v3)),
            version("c4", "Anna", 40, Some(&v4)),
            version("c5", "Cy", 50, Some(&v5)),
        ];
        let events: Vec<String> = note_timeline(&n, &versions)
            .iter()
            .map(|e| format!("{:?}|{}|{}|{}", e.kind, e.detail, e.author, e.commit))
            .collect();
        assert_eq!(
            events,
            [
                "Removed||Cy|c5",
                "Detached|shot.png|Anna|c4",
                "Resolved||Anna|c4",
                "AnchoredToBlock|b|Bob|c3",
                "Attached|shot.png|Bob|c3",
                "Replied|Agreed|Bob|c3",
                "Edited|Check twice|Anna|c2",
                "Created|Check **twice**|Anna|c1",
            ]
        );
        assert!(note_timeline("nothing", &versions).is_empty());
        // Without history there is nothing to list.
        assert!(note_timeline(&n, &[]).is_empty());
    }
}
