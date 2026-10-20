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

/// First message of a connection, phone to Mac. Keys and signatures are hex.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum Hello {
    /// Pairs a new phone with the code of a [`crate::PairingOffer`].
    Pair {
        code: String,
        device_name: String,
        device_key: String,
        signature: String,
    },
    /// Signs in a paired phone; the Mac answers with a challenge.
    SignIn { device_key: String },
}

/// Mac to phone during the handshake.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum HostHello {
    Challenge { nonce: String },
    Welcome(Welcome),
    Refused { error: RemoteError },
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
    pub host_name: String,
    pub device_id: String,
    pub permission: Permission,
    /// The Mac's current addresses, so the phone's saved record stays fresh.
    pub relay_url: Option<String>,
    pub addrs: Vec<String>,
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
}

impl Request {
    #[cfg(feature = "host")]
    pub(crate) fn needs(&self) -> Permission {
        match self {
            Self::RunTest { .. } | Self::StopRun { .. } => Permission::Run,
            _ => Permission::View,
        }
    }

    #[cfg(feature = "client")]
    /// Asking again after a lost connection cannot do anything twice.
    pub(crate) fn is_idempotent(&self) -> bool {
        !matches!(self, Self::RunTest { .. })
    }
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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Reply {
    Ok(Response),
    Error(RemoteError),
}

/// Assistant to Mac on [`crate::ALPN_V2`], after the same handshake phones use. What goes in
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
fn cut(text: &mut String, max: usize) {
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
}

/// What a watched run sends: its full status first and again whenever the phone fell behind,
/// events in between.
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
