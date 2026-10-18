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

use crate::{Permission, RemoteError};

/// Largest JSON message. Reports of long runs are the biggest ones.
pub(crate) const MAX_MESSAGE: usize = 4 << 20;
/// Largest picture.
pub(crate) const MAX_PICTURE: usize = 16 << 20;

/// Connection close codes.
pub(crate) const CLOSE_DONE: u32 = 0;
pub(crate) const CLOSE_REVOKED: u32 = 1;
#[cfg(feature = "host")]
pub(crate) const CLOSE_REFUSED: u32 = 2;

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
    RunTest {
        project: String,
        path: String,
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
        test: TestSummary,
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
    let bytes = serde_json::to_vec(msg).map_err(|_| RemoteError::Protocol)?;
    send_bytes(stream, &bytes, MAX_MESSAGE).await
}

/// The next message, or `None` when the other side finished the stream.
pub(crate) async fn recv<T: DeserializeOwned>(
    stream: &mut RecvStream,
) -> Result<Option<T>, RemoteError> {
    match recv_bytes(stream, MAX_MESSAGE).await? {
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
