//! Messages between the Mac and the phone and how they travel.
//!
//! Every message is a frame: a 4-byte big-endian length, then that many bytes of JSON. A
//! picture is the one exception: its PNG bytes follow the reply as a frame of their own. A
//! connection starts with a handshake on its first stream; after that every request opens a
//! stream of its own and gets one reply, or a reply and a run of updates for [`Request::Watch`].

use iroh::endpoint::{ReadExactError, RecvStream, SendStream};
use jevision_runner::{Failure, RunReport, StepStatus, Verdict};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

use crate::{Permission, RemoteError};

/// Largest JSON message. Reports of long runs are the biggest ones.
pub(crate) const MAX_MESSAGE: usize = 4 << 20;
/// Largest picture.
pub(crate) const MAX_PICTURE: usize = 16 << 20;
/// Largest reply to an assistant: a screenshot travels inside it as base64 text.
pub(crate) const MAX_ASSISTANT_REPLY: usize = 32 << 20;

/// Connection close codes.
pub(crate) const CLOSE_DONE: u32 = 0;
pub(crate) const CLOSE_REVOKED: u32 = 1;
#[cfg(feature = "host")]
pub(crate) const CLOSE_REFUSED: u32 = 2;
/// The owner turned the assistant off while it was connected.
pub(crate) const CLOSE_DISABLED: u32 = 3;

/// The protocol versions this app speaks. A version changes only when a message changes in a
/// way the other side would misread; added fields need none.
pub(crate) const PROTOCOLS: Protocols = Protocols { min: 1, max: 1 };

/// A range of protocol versions, both ends included.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Protocols {
    pub min: u32,
    pub max: u32,
}

/// A device that doesn't say speaks protocol 1.
impl Default for Protocols {
    fn default() -> Self {
        Self { min: 1, max: 1 }
    }
}

impl Protocols {
    /// The newest version both the Mac (`self`) and the phone speak.
    #[cfg(feature = "host")]
    pub(crate) fn agree(self, phone: Protocols) -> Result<u32, RemoteError> {
        if phone.min > phone.max {
            return Err(RemoteError::Protocol);
        }
        if self.min.max(phone.min) <= self.max.min(phone.max) {
            Ok(self.max.min(phone.max))
        } else if phone.max < self.min {
            Err(RemoteError::DeviceTooOld)
        } else {
            Err(RemoteError::MacTooOld)
        }
    }
}

/// First message of a connection, phone to Mac: the versions the phone speaks, then what it
/// wants. The Mac reads the versions first and on their own, so a newer phone may change the
/// rest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Hello {
    #[serde(default)]
    pub protocols: Protocols,
    /// The phone app's version, so the Mac can say which to update.
    #[serde(default)]
    pub app_version: String,
    #[serde(flatten)]
    pub kind: HelloKind,
}

/// What the phone wants. Keys and signatures are hex.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum HelloKind {
    /// Pairs a new phone with the code of a [`crate::PairingOffer`].
    Pair {
        code: String,
        device_name: String,
        device_key: String,
        signature: String,
    },
    /// Signs in a paired phone; the Mac answers with a challenge.
    SignIn { device_key: String },
    /// Something a newer phone wants.
    #[serde(other)]
    Unknown,
}

/// Mac to phone during the handshake.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum HostHello {
    Challenge {
        nonce: String,
    },
    Welcome(Welcome),
    /// `app_version` is the Mac's, sent only with [`RemoteError::DeviceTooOld`]: a stranger
    /// learns it only when it needs it to update.
    Refused {
        error: RemoteError,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        app_version: Option<String>,
    },
    /// An answer of a newer Mac.
    #[serde(other)]
    Unknown,
}

/// The phone's answer to a challenge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Proof {
    pub signature: String,
}

/// The handshake succeeded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Welcome {
    /// The protocol version the Mac chose.
    #[serde(default = "first_protocol")]
    pub protocol: u32,
    /// The Mac app's version.
    #[serde(default)]
    pub app_version: String,
    pub host_name: String,
    pub device_id: String,
    #[serde(flatten, with = "granted")]
    pub permission: Permission,
    /// The Mac's current addresses, so the phone's saved record stays fresh.
    pub relay_url: Option<String>,
    pub addrs: Vec<String>,
}

fn first_protocol() -> u32 {
    1
}

/// [`Permission`] as the welcome writes it: `edit` as `run` with `"edit": true`, which a phone
/// that predates editing reads as `run`.
mod granted {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use crate::Permission;

    #[derive(Serialize, Deserialize)]
    struct Granted {
        permission: Permission,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        edit: bool,
    }

    pub(super) fn serialize<S: Serializer>(p: &Permission, s: S) -> Result<S::Ok, S::Error> {
        let edit = *p == Permission::Edit;
        let permission = if edit { Permission::Run } else { *p };
        Granted { permission, edit }.serialize(s)
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Permission, D::Error> {
        let granted = Granted::deserialize(d)?;
        Ok(match granted.permission {
            Permission::Run if granted.edit => Permission::Edit,
            permission => permission,
        })
    }
}

/// Phone to Mac after the handshake. `project` is a project name, `path` a test file relative
/// to the project folder and `run` the name of a run folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum Request {
    ListProjects,
    ListTests {
        project: String,
    },
    GetTest {
        project: String,
        path: String,
    },
    /// `row` runs only that row of the test's data table, counted from 0; `None` runs every
    /// row. Phones that predate data rows leave it out.
    RunTest {
        project: String,
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        row: Option<u32>,
    },
    StopRun {
        project: String,
        run: String,
    },
    GetRunStatus {
        project: String,
        run: String,
    },
    /// Replies with the run's status, then sends [`RunUpdate`]s until the run finishes.
    Watch {
        project: String,
        run: String,
    },
    ListRuns {
        project: String,
    },
    GetReport {
        project: String,
        run: String,
    },
    /// `picture` is an image path from the run's report, relative to the run folder.
    GetPicture {
        project: String,
        run: String,
        picture: String,
    },
    /// Makes one change to the test, if the file is still at `revision`, the
    /// [`TestSummary::revision`] the phone read. Macs that predate editing answer
    /// [`RemoteError::Unsupported`].
    EditTest {
        project: String,
        path: String,
        revision: String,
        edit: TestEdit,
    },
    /// A request of a newer phone; the Mac answers [`RemoteError::Unsupported`].
    #[serde(other)]
    Unknown,
}

impl Request {
    #[cfg(feature = "host")]
    pub(crate) fn needs(&self) -> Permission {
        match self {
            Self::EditTest { .. } => Permission::Edit,
            Self::RunTest { .. } | Self::StopRun { .. } => Permission::Run,
            _ => Permission::View,
        }
    }

    #[cfg(feature = "client")]
    /// Asking again after a lost connection cannot do anything twice.
    pub(crate) fn is_idempotent(&self) -> bool {
        !matches!(self, Self::RunTest { .. } | Self::EditTest { .. })
    }
}

/// One change to a test from a phone. The Mac makes it the way its editor does, checks the
/// block still reads and saves the file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "change",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum TestEdit {
    /// Sets the setting `field` of block `block`, one of its [`BlockSummary::fields`], to
    /// `value` written as the phone shows it: `true` or `false` for a switch, digits for a
    /// number. An empty value takes an optional setting away.
    SetField {
        block: String,
        field: String,
        value: String,
    },
    /// Adds a block of type `block_type`, one of [`TestSummary::palette`], after block
    /// `after` (on the `then` branch of an `if`, after a loop rather than in it), or at the
    /// end of the test without one.
    AddBlock {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        after: Option<String>,
        block_type: String,
    },
    /// Removes the block and joins what led to it with what came after it; its notes stay as
    /// notes about the test.
    RemoveBlock { block: String },
    /// Gives the test a new name; the file keeps its own.
    Rename { name: String },
    /// A change of a newer phone; the Mac answers [`RemoteError::Unsupported`].
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum Response {
    Projects {
        projects: Vec<ProjectInfo>,
    },
    Tests {
        tests: Vec<TestEntry>,
    },
    Test {
        test: Box<TestSummary>,
    },
    RunStarted {
        run: String,
    },
    RunStopped,
    Status {
        status: RunStatus,
    },
    Runs {
        runs: RunList,
    },
    Report {
        report: Box<RunReport>,
    },
    /// The PNG bytes follow in the next frame.
    Picture {
        size: u64,
    },
    /// The test as it is after the change. `select` is the block the phone shows next: the one
    /// it added, or the one after the block it removed.
    Edited {
        test: Box<TestSummary>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        select: Option<String>,
    },
    /// An answer of a newer Mac.
    #[serde(other)]
    Unknown,
}

/// The answer to a [`Request`]. It keeps these two shapes for good: a new answer is a new
/// [`Response`] type and a new failure a new [`RemoteError`] code, both of which an older
/// side reads as unknown.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Reply {
    Ok(Response),
    Error(RemoteError),
}

/// Assistant to Mac on [`crate::ALPN_ASSISTANT`], after the same handshake phones use. What goes in
/// and out is what the local MCP server takes and returns, so an assistant sees the same
/// tools on both.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum AssistantRequest {
    /// The MCP `initialize` request's params; the reply is its result.
    Hello { params: Value },
    /// The MCP `tools/list` result.
    Tools,
    /// The MCP `tools/call` result of `tool`. `project` is the name of a project the Mac
    /// shares; without it the Mac uses the only one it shares.
    Call {
        tool: String,
        arguments: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project: Option<String>,
    },
    /// A request of a newer assistant; the Mac answers [`RemoteError::Unsupported`].
    #[serde(other)]
    Unknown,
}

impl AssistantRequest {
    #[cfg(feature = "client")]
    /// Asking again after a lost connection cannot do anything twice.
    pub(crate) fn is_idempotent(&self) -> bool {
        !matches!(self, Self::Call { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum AssistantReply {
    Ok {
        result: Value,
    },
    /// The MCP server's own error: unknown tool, invalid params.
    Rejected {
        code: i64,
        message: String,
    },
    Error {
        error: RemoteError,
    },
    /// An answer of a newer Mac.
    #[serde(other)]
    Unknown,
}

/// A project the Mac shares with phones. Its folder stays on the Mac.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectInfo {
    pub name: String,
}

/// A test or a folder of tests. Entries come sorted by path, every folder right before what
/// it holds, so the list reads as a tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestEntry {
    /// Relative to the project folder, with `/` between folders.
    pub path: String,
    /// The test's name, or the folder's.
    pub name: String,
    pub is_folder: bool,
    pub last_run: Option<LastRun>,
}

/// The newest saved run of a test.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LastRun {
    pub run: String,
    pub verdict: Verdict,
    /// UTC time, RFC 3339.
    pub started_at: String,
}

/// A test as the phone draws it, read-only. Geometry is in canvas points, the same layout the
/// Mac editor shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestSummary {
    pub name: String,
    pub description: Option<String>,
    /// The card fields as written in the test file: `critical`, `high`, `medium`, `low`.
    /// Absent when the test has none and from Macs that don't send them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
    /// `smoke`, `regression` or `acceptance`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_type: Option<String>,
    /// `draft`, `ready` or `outdated`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// Who wrote the test or looks after it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// Bundle id of the application under test.
    pub app: String,
    pub start: String,
    pub width: f64,
    pub height: f64,
    /// In reading order.
    pub blocks: Vec<BlockSummary>,
    /// Frames around loops and their bodies.
    pub frames: Vec<FrameSummary>,
    pub edges: Vec<EdgeSummary>,
    /// What each row of the test's data table is called, in order, when the whole test runs
    /// once per row; a run can be limited to one of them. Empty without such a table.
    #[serde(default)]
    pub rows: Vec<String>,
    /// The sticky notes of the test as the Mac editor shows them, read-only; the Mac sends at
    /// most a hundred. Absent from Macs that don't send them, and phones that predate notes
    /// ignore it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<NoteSummary>,
    /// Which version of the file this is, sent back with every change so none overwrites
    /// what changed on the Mac meanwhile. Empty from Macs that predate editing.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub revision: String,
    /// The block types the Mac can add to the test, in the order of its library.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub palette: Vec<PaletteEntry>,
}

/// A block type of the Mac's library.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaletteEntry {
    /// The block's `type`, such as `click`.
    pub block_type: String,
    /// Its group in the library: `actions`, `waiting`, `checks`, `logic`, `subTests`, `data`.
    pub group: String,
}

/// A setting of a block a phone can change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockField {
    /// Where it is in the block, such as `text` or `target.description`.
    pub path: String,
    pub kind: FieldKind,
    /// As the phone shows it: `true` or `false` for a switch; empty when not set.
    #[serde(default)]
    pub value: String,
    /// The values a [`FieldKind::Choice`] takes, as the test file writes them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<String>,
}

/// How a phone edits a setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FieldKind {
    Flag,
    Number,
    Choice,
    /// A time such as `2s` or `500ms`.
    Duration,
    /// Text of several lines, such as a script.
    LongText,
    /// One line of text. A kind from a newer Mac reads as this one.
    #[serde(other)]
    Text,
}

/// A problem the Mac finds in a block, as a key of the shared catalog with its values, so the
/// phone words it in its own language.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Problem {
    pub code: String,
    /// The test still runs, but probably not as meant.
    #[serde(default)]
    pub warning: bool,
    #[serde(default)]
    pub params: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BlockSummary {
    pub id: String,
    /// The block's `type`, such as `click`; the base of its localization keys.
    pub kind: String,
    /// The block's main parameter as written in the test: an app, an image, keys, text with
    /// placeholders, a count. Empty when the kind says it all.
    pub title: String,
    /// `None` for a block the layout leaves out.
    pub area: Option<Area>,
    /// The settings a phone that may edit can change. Absent from Macs that predate editing.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<BlockField>,
    /// What is wrong with the block. Absent from Macs that predate editing.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub problems: Vec<Problem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrameSummary {
    pub block: String,
    pub area: Area,
}

/// A connector from block `from` along its edge `edge` (`next`, `then`, `else`, `body`) to
/// block `to`, drawn through `points` with the arrow at the last one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EdgeSummary {
    pub from: String,
    pub edge: String,
    pub to: String,
    pub points: Vec<Position>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Area {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Position {
    pub x: f64,
    pub y: f64,
}

/// Largest number of notes of a test that reach the phone.
pub(crate) const MAX_NOTES: usize = 100;
/// Largest note and reply text, in bytes.
pub(crate) const MAX_NOTE_TEXT: usize = 4000;
pub(crate) const MAX_REPLY_TEXT: usize = 1000;
/// Replies of a note that reach the phone: the newest ones.
pub(crate) const MAX_REPLIES: usize = 20;
pub(crate) const MAX_ATTACHMENTS: usize = 10;
/// Largest file name or web address of an attachment, in bytes.
pub(crate) const MAX_ATTACHMENT_VALUE: usize = 512;
/// Largest author name, time and id, in bytes.
pub(crate) const MAX_NOTE_LABEL: usize = 64;
/// Largest width and height of a note, in canvas points.
const MAX_NOTE_SIDE: f64 = 2000.0;
/// Largest corner of a note, in canvas points.
const MAX_NOTE_CORNER: f64 = 1_000_000.0;
/// Largest size of all notes of a test together as JSON, so a test with many long notes
/// still fits a message. Notes that don't fit are left out.
pub(crate) const MAX_NOTES_JSON: usize = 1 << 20;

/// A sticky note of a test, read-only. Its text is Markdown. Files that come with a note stay
/// on the Mac; only their names travel. A phone limits whatever it receives with
/// `bounded_notes` before drawing it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteSummary {
    pub id: String,
    pub text: String,
    /// Who wrote the note; empty when unknown.
    pub author: String,
    /// When it was written, RFC 3339; empty when unknown.
    pub time: String,
    /// What the note is about: `test`, `block` or `folder`. Anything else reads as `test`.
    pub anchor: String,
    /// The block a note about a block is about.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block: Option<String>,
    /// Where the note sits on the canvas, in the same points as the blocks.
    pub area: Area,
    /// How many replies the note has, also when only the newest are in `replies`.
    pub reply_count: u32,
    /// The discussion under the note, oldest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub replies: Vec<NoteReplySummary>,
    /// Set when the discussion is settled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved: Option<NoteResolvedSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<NoteAttachmentSummary>,
}

/// One message under a note.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteReplySummary {
    pub id: String,
    pub author: String,
    /// RFC 3339; empty when unknown.
    pub time: String,
    pub text: String,
}

/// Who settled the discussion under a note, and when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteResolvedSummary {
    pub by: String,
    /// RFC 3339; empty when unknown.
    pub time: String,
}

/// A file or a link that comes with a note.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteAttachmentSummary {
    /// `file` or `link`.
    pub kind: String,
    /// The file name, which stays on the Mac, or the web address.
    pub value: String,
}

/// `text` cut to at most `max` bytes at a character boundary, with `…` where it was cut.
pub(crate) fn cut(text: &mut String, max: usize) {
    if text.len() <= max {
        return;
    }
    let mut end = max.saturating_sub('…'.len_utf8());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text.push('…');
}

/// An address the phone may open: `http` or `https`, with a host and no spaces.
fn is_web_address(address: &str) -> bool {
    let rest = address
        .strip_prefix("https://")
        .or_else(|| address.strip_prefix("http://"));
    rest.is_some_and(|r| {
        r.chars().next().is_some_and(char::is_alphanumeric)
            && !address.chars().any(|c| c.is_whitespace() || c.is_control())
    })
}

/// `notes` as a phone may take them, whoever sent them: at most [`MAX_NOTES`] notes within
/// [`MAX_NOTES_JSON`] bytes of JSON altogether, each with its text, replies (the newest ones), attachments
/// and labels cut to their limits, links other than web addresses and attachments of unknown
/// kinds dropped, an unknown anchor read as `test` and the area kept on the canvas. Notes
/// without a usable area go. Cutting again changes nothing.
pub(crate) fn bounded_notes(notes: Vec<NoteSummary>) -> Vec<NoteSummary> {
    let mut left = MAX_NOTES_JSON;
    let mut kept = Vec::new();
    for note in notes.into_iter().take(MAX_NOTES) {
        let Some(note) = bounded_note(note) else {
            continue;
        };
        let size = serde_json::to_string(&note).map_or(usize::MAX, |json| json.len());
        if size > left {
            break;
        }
        left -= size;
        kept.push(note);
    }
    kept
}

fn bounded_note(mut note: NoteSummary) -> Option<NoteSummary> {
    let area = &mut note.area;
    if ![area.x, area.y, area.width, area.height]
        .iter()
        .all(|v| v.is_finite())
    {
        return None;
    }
    area.x = area.x.clamp(0.0, MAX_NOTE_CORNER);
    area.y = area.y.clamp(0.0, MAX_NOTE_CORNER);
    area.width = area.width.clamp(1.0, MAX_NOTE_SIDE);
    area.height = area.height.clamp(1.0, MAX_NOTE_SIDE);

    for label in [&mut note.id, &mut note.author, &mut note.time] {
        cut(label, MAX_NOTE_LABEL);
    }
    cut(&mut note.text, MAX_NOTE_TEXT);
    if note.anchor == "block" {
        if let Some(block) = &mut note.block {
            cut(block, MAX_NOTE_LABEL);
        } else {
            note.anchor = "test".into();
        }
    } else {
        if note.anchor != "folder" {
            note.anchor = "test".into();
        }
        note.block = None;
    }

    note.reply_count = note
        .reply_count
        .max(note.replies.len().try_into().unwrap_or(u32::MAX));
    let skipped = note.replies.len().saturating_sub(MAX_REPLIES);
    note.replies.drain(..skipped);
    for reply in &mut note.replies {
        for label in [&mut reply.id, &mut reply.author, &mut reply.time] {
            cut(label, MAX_NOTE_LABEL);
        }
        cut(&mut reply.text, MAX_REPLY_TEXT);
    }
    if let Some(resolved) = &mut note.resolved {
        cut(&mut resolved.by, MAX_NOTE_LABEL);
        cut(&mut resolved.time, MAX_NOTE_LABEL);
    }
    // A cut address would lead somewhere else, so a long link goes instead of being cut.
    note.attachments.retain(|a| match a.kind.as_str() {
        "file" => !a.value.is_empty(),
        "link" => a.value.len() <= MAX_ATTACHMENT_VALUE && is_web_address(&a.value),
        _ => false,
    });
    note.attachments.truncate(MAX_ATTACHMENTS);
    for attachment in &mut note.attachments {
        cut(&mut attachment.value, MAX_ATTACHMENT_VALUE);
    }
    Some(note)
}

/// Where a run stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunStatus {
    pub run: String,
    /// Test file, relative to the project folder.
    pub test: String,
    pub state: RunState,
    /// Set once the run has finished.
    pub verdict: Option<Verdict>,
    /// The attempt going on or the last one, from 1; 0 before the first starts.
    pub attempt: u32,
    /// The block running or waiting.
    pub block: Option<String>,
    /// Set while the run waits for its turn at the screen; a run that acts on the screen
    /// while another has it. Absent otherwise and from Macs that don't send it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiting: Option<ScreenWait>,
}

/// A run that waits for the screen. It goes to the phones as a new status of the run, which
/// an older phone shows like any other.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenWait {
    /// Who has the screen; absent while the lock does not say.
    pub holder: Option<ScreenHolder>,
}

/// The run that has the screen: another JEVision window, the command line or the assistant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenHolder {
    /// The program, such as `JEVision` or `jevision`.
    pub process: String,
    /// Name of the test that runs.
    pub test: String,
    /// When that run took the screen, in seconds since 1970.
    pub since: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RunState {
    Running,
    Paused,
    Finished,
    /// A state from a newer Mac.
    #[serde(other)]
    Unknown,
}

/// A run saved in the project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSummary {
    pub run: String,
    /// Name of the test.
    pub test: String,
    /// Test file, relative to the project folder.
    pub file: String,
    /// UTC time, RFC 3339.
    pub started_at: String,
    pub duration_ms: u64,
    pub verdict: Verdict,
    pub flaky: bool,
}

/// Runs of a project: those going on now and the saved ones, newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunList {
    pub active: Vec<RunStatus>,
    pub saved: Vec<RunSummary>,
}

/// Progress of a run, as the Mac app reports it to [`crate::Host::publish`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "event",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum RunEvent {
    AttemptStarted {
        attempt: u32,
    },
    /// `test` is the file the block belongs to; it differs from the run's test inside `runTest`.
    BlockStarted {
        attempt: u32,
        seq: u32,
        test: String,
        block: String,
        block_type: String,
        /// What the block works with, its placeholders filled with the values of this run, such
        /// as the data row's: `Hello, Anna!`. Absent from Macs that don't send it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
    },
    BlockFinished {
        attempt: u32,
        seq: u32,
        test: String,
        block: String,
        status: StepStatus,
        duration_ms: u64,
        failure: Option<Failure>,
        /// The way out an `if` took: `then`, `elseIf1`, `elseIf2`… or `else`. Absent for other
        /// blocks and from Macs that don't send it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        branch: Option<String>,
        /// How many ways out the `if` had.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        branches: Option<u32>,
        /// What the condition of the way taken looks for. Absent for `else`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        branch_condition: Option<String>,
    },
    Paused {
        block: String,
        reason: PauseReason,
    },
    Resumed,
    AttemptFinished {
        attempt: u32,
        verdict: Verdict,
        failure: Option<Failure>,
    },
    /// The last event of a run; its report is saved by now.
    Finished {
        verdict: Verdict,
        flaky: bool,
    },
}

/// Why a run waits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PauseReason {
    Requested,
    Step,
    Breakpoint,
    /// A person used the mouse or keyboard while the test was driving the Mac.
    UserInput,
    /// A reason from a newer Mac.
    #[serde(other)]
    Unknown,
}

/// What a watched run sends: its full status first and again whenever the phone fell behind,
/// events in between. An update of a kind a newer Mac added is skipped by the phone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum RunUpdate {
    Status { status: RunStatus },
    Event { event: RunEvent },
}

/// A frame of a watch as the phone reads it: an update, or one of a kind a newer Mac added,
/// which it skips. A frame of a known kind that doesn't read is an error.
#[cfg(feature = "client")]
pub(crate) enum WatchFrame {
    Update(RunUpdate),
    Newer,
}

#[cfg(feature = "client")]
impl<'de> Deserialize<'de> for WatchFrame {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        /// Only the kinds of a frame, to tell a newer one from a broken one.
        #[derive(Deserialize)]
        #[serde(tag = "type", rename_all = "camelCase")]
        enum FrameKind {
            Status,
            Event {
                event: EventKind,
            },
            #[serde(other)]
            Newer,
        }
        #[derive(Deserialize)]
        #[serde(tag = "event", rename_all = "camelCase")]
        enum EventKind {
            AttemptStarted,
            BlockStarted,
            BlockFinished,
            Paused,
            Resumed,
            AttemptFinished,
            Finished,
            #[serde(other)]
            Newer,
        }

        let frame = Value::deserialize(deserializer)?;
        match RunUpdate::deserialize(&frame) {
            Ok(update) => Ok(Self::Update(update)),
            Err(error) => match FrameKind::deserialize(&frame) {
                Ok(
                    FrameKind::Newer
                    | FrameKind::Event {
                        event: EventKind::Newer,
                    },
                ) => Ok(Self::Newer),
                _ => Err(serde::de::Error::custom(error)),
            },
        }
    }
}

pub(crate) async fn send<T: Serialize>(
    stream: &mut SendStream,
    msg: &T,
) -> Result<(), RemoteError> {
    send_within(stream, msg, MAX_MESSAGE).await
}

pub(crate) async fn send_within<T: Serialize>(
    stream: &mut SendStream,
    msg: &T,
    limit: usize,
) -> Result<(), RemoteError> {
    let bytes = serde_json::to_vec(msg).map_err(|_| RemoteError::Protocol)?;
    send_bytes(stream, &bytes, limit).await
}

/// The next message, or `None` when the other side finished the stream.
pub(crate) async fn recv<T: DeserializeOwned>(
    stream: &mut RecvStream,
) -> Result<Option<T>, RemoteError> {
    recv_within(stream, MAX_MESSAGE).await
}

pub(crate) async fn recv_within<T: DeserializeOwned>(
    stream: &mut RecvStream,
    limit: usize,
) -> Result<Option<T>, RemoteError> {
    match recv_bytes(stream, limit).await? {
        Some(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| RemoteError::Protocol),
        None => Ok(None),
    }
}

/// Like [`recv`], but a finished stream is a protocol error.
pub(crate) async fn expect<T: DeserializeOwned>(stream: &mut RecvStream) -> Result<T, RemoteError> {
    recv(stream).await?.ok_or(RemoteError::Protocol)
}

pub(crate) async fn expect_within<T: DeserializeOwned>(
    stream: &mut RecvStream,
    limit: usize,
) -> Result<T, RemoteError> {
    recv_within(stream, limit)
        .await?
        .ok_or(RemoteError::Protocol)
}

pub(crate) async fn send_bytes(
    stream: &mut SendStream,
    bytes: &[u8],
    limit: usize,
) -> Result<(), RemoteError> {
    if bytes.len() > limit {
        return Err(RemoteError::TooLarge);
    }
    let len = (bytes.len() as u32).to_be_bytes();
    stream
        .write_all(&len)
        .await
        .map_err(|_| RemoteError::HostOffline)?;
    stream
        .write_all(bytes)
        .await
        .map_err(|_| RemoteError::HostOffline)
}

/// Reads one frame; a length over `limit` is refused before anything is allocated.
pub(crate) async fn recv_bytes(
    stream: &mut RecvStream,
    limit: usize,
) -> Result<Option<Vec<u8>>, RemoteError> {
    let mut len = [0; 4];
    match stream.read_exact(&mut len).await {
        Ok(()) => {}
        Err(ReadExactError::FinishedEarly(0)) => return Ok(None),
        Err(ReadExactError::FinishedEarly(_)) => return Err(RemoteError::Protocol),
        Err(ReadExactError::ReadError(_)) => return Err(RemoteError::HostOffline),
    }
    let len = u32::from_be_bytes(len) as usize;
    if len > limit {
        return Err(RemoteError::TooLarge);
    }
    let mut bytes = vec![0; len];
    match stream.read_exact(&mut bytes).await {
        Ok(()) => Ok(Some(bytes)),
        Err(ReadExactError::FinishedEarly(_)) => Err(RemoteError::Protocol),
        Err(ReadExactError::ReadError(_)) => Err(RemoteError::HostOffline),
    }
}
