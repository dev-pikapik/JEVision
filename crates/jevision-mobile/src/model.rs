//! What the phone shows: the remote protocol's types, exposed to Swift.
//!
//! Most are remote declarations that mirror the type in `jevision-remote` field by field (the
//! compiler rejects any mismatch). Run events and reports carry failure parameters in a sorted
//! map, which the bindings cannot express, so those are converted copies.

use std::collections::HashMap;

pub use jevision_remote::{
    Area, BlockSummary, EdgeSummary, FrameSummary, HostRecord, LastRun, Network,
    NoteAttachmentSummary, NoteReplySummary, NoteResolvedSummary, NoteSummary, PairingOffer,
    PauseReason, Permission, Position, ProjectInfo, Relay, RunList, RunState, RunStatus,
    RunSummary, ScreenHolder, ScreenWait, StepStatus, TestEntry, TestSummary, Verdict,
};

/// A paired Mac as the phone saves it, with [`crate::host_record_to_json`].
#[uniffi::remote(Record)]
pub struct HostRecord {
    /// The Mac's endpoint id, hex.
    pub host_id: String,
    pub name: String,
    /// This phone's id on that Mac.
    pub device_id: String,
    pub relay_url: Option<String>,
    pub addrs: Vec<String>,
}

/// What the Mac's pairing QR code holds. The code works once, for five minutes.
#[uniffi::remote(Record)]
pub struct PairingOffer {
    /// The Mac's endpoint id, hex.
    pub host_id: String,
    /// The Mac's name as its owner sees it.
    pub host_name: String,
    /// The one-time code, 32 hex digits.
    pub code: String,
    /// When the code stops working, in seconds since 1970.
    pub expires_at: i64,
    pub relay_url: Option<String>,
    pub addrs: Vec<String>,
}

/// How the phone reaches the Mac.
#[uniffi::remote(Record)]
pub struct Network {
    pub relay: Relay,
    /// Find the Mac on the same Wi-Fi by mDNS. Needs the multicast networking entitlement and
    /// the local network permission.
    pub local_discovery: bool,
    /// Look up the Mac's current addresses through the n0 DNS service.
    pub global_discovery: bool,
}

/// Relay servers used when no direct path works. They forward ciphertext only.
#[uniffi::remote(Enum)]
pub enum Relay {
    /// The public relays run by n0.
    Default,
    /// A relay the user runs, such as `https://relay.example.com`.
    Custom { url: String },
    /// Direct connections only: same network, VPN, or reachable addresses.
    Disabled,
}

/// What a paired phone may do.
#[uniffi::remote(Enum)]
pub enum Permission {
    /// List projects and tests, look at tests, runs, reports and pictures.
    View,
    /// Everything `view` allows, plus starting and stopping runs.
    Run,
}

/// A project the Mac shares with phones.
#[uniffi::remote(Record)]
pub struct ProjectInfo {
    pub name: String,
}

/// A test or a folder of tests. Entries come sorted by path, every folder right before what
/// it holds, so the list reads as a tree.
#[uniffi::remote(Record)]
pub struct TestEntry {
    /// Relative to the project folder, with `/` between folders.
    pub path: String,
    /// The test's name, or the folder's.
    pub name: String,
    pub is_folder: bool,
    pub last_run: Option<LastRun>,
}

/// The newest saved run of a test.
#[uniffi::remote(Record)]
pub struct LastRun {
    pub run: String,
    pub verdict: Verdict,
    /// UTC time, RFC 3339.
    pub started_at: String,
}

/// A test as the phone draws it, read-only, in canvas points.
#[uniffi::remote(Record)]
pub struct TestSummary {
    pub name: String,
    pub description: Option<String>,
    /// The card fields as the test file writes them: `critical`, `high`, `medium`, `low`.
    pub priority: Option<String>,
    /// `smoke`, `regression` or `acceptance`.
    pub test_type: Option<String>,
    /// `draft`, `ready` or `outdated`.
    pub status: Option<String>,
    /// Who wrote the test or looks after it.
    pub author: Option<String>,
    /// Bundle id of the application under test.
    pub app: String,
    /// Id of the first block.
    pub start: String,
    pub width: f64,
    pub height: f64,
    /// In reading order.
    pub blocks: Vec<BlockSummary>,
    /// Frames around loops and their bodies.
    pub frames: Vec<FrameSummary>,
    pub edges: Vec<EdgeSummary>,
    /// What each row of the test's data table is called, in order, when the whole test runs
    /// once per row; pass a row's index to [`crate::RemoteConnection::run_test`] to run only
    /// it. Empty without such a table.
    pub rows: Vec<String>,
    /// The sticky notes on the canvas, read-only, already limited in number and length.
    pub notes: Vec<NoteSummary>,
}

/// A sticky note of a test. `text` is Markdown. Files that come with it stay on the Mac.
#[uniffi::remote(Record)]
pub struct NoteSummary {
    pub id: String,
    pub text: String,
    /// Who wrote the note; empty when unknown.
    pub author: String,
    /// RFC 3339; empty when unknown.
    pub time: String,
    /// What the note is about: `test`, `block` or `folder`.
    pub anchor: String,
    /// The block a note about a block is about.
    pub block: Option<String>,
    /// Where the note sits on the canvas, in the same points as the blocks.
    pub area: Area,
    /// How many replies the note has, also when only the newest are in `replies`.
    pub reply_count: u32,
    /// Oldest first.
    pub replies: Vec<NoteReplySummary>,
    /// Set when the discussion is settled.
    pub resolved: Option<NoteResolvedSummary>,
    pub attachments: Vec<NoteAttachmentSummary>,
}

#[uniffi::remote(Record)]
pub struct NoteReplySummary {
    pub id: String,
    pub author: String,
    /// RFC 3339; empty when unknown.
    pub time: String,
    pub text: String,
}

#[uniffi::remote(Record)]
pub struct NoteResolvedSummary {
    pub by: String,
    /// RFC 3339; empty when unknown.
    pub time: String,
}

/// A file or a link that comes with a note.
#[uniffi::remote(Record)]
pub struct NoteAttachmentSummary {
    /// `file` or `link`.
    pub kind: String,
    /// The file name, which stays on the Mac, or the web address to open.
    pub value: String,
}

#[uniffi::remote(Record)]
pub struct BlockSummary {
    pub id: String,
    /// The block's type, such as `click`; the base of its localization keys.
    pub kind: String,
    /// The block's main parameter as written in the test. Empty when the kind says it all.
    pub title: String,
    /// `nil` for a block the layout leaves out.
    pub area: Option<Area>,
}

#[uniffi::remote(Record)]
pub struct FrameSummary {
    pub block: String,
    pub area: Area,
}

/// A connector from block `from` along its edge `edge` (`next`, `then`, `else`, `body`) to
/// block `to`, drawn through `points` with the arrow at the last one.
#[uniffi::remote(Record)]
pub struct EdgeSummary {
    pub from: String,
    pub edge: String,
    pub to: String,
    pub points: Vec<Position>,
}

#[uniffi::remote(Record)]
pub struct Area {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[uniffi::remote(Record)]
pub struct Position {
    pub x: f64,
    pub y: f64,
}

/// Where a run stands.
#[uniffi::remote(Record)]
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
    /// Set while the run waits for its turn at the screen.
    pub waiting: Option<ScreenWait>,
}

/// A run that waits for the screen.
#[uniffi::remote(Record)]
pub struct ScreenWait {
    /// Who has the screen; `nil` while the lock does not say.
    pub holder: Option<ScreenHolder>,
}

/// The run that has the screen.
#[uniffi::remote(Record)]
pub struct ScreenHolder {
    /// The program, such as `JEVision` or `jevision`.
    pub process: String,
    /// Name of the test that runs.
    pub test: String,
    /// When that run took the screen, in seconds since 1970.
    pub since: u64,
}

#[uniffi::remote(Enum)]
pub enum RunState {
    Running,
    Paused,
    Finished,
    /// A state from a newer Mac.
    Unknown,
}

/// A run saved in the project.
#[uniffi::remote(Record)]
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
#[uniffi::remote(Record)]
pub struct RunList {
    pub active: Vec<RunStatus>,
    pub saved: Vec<RunSummary>,
}

#[uniffi::remote(Enum)]
pub enum Verdict {
    Passed,
    Failed,
    Cancelled,
    Blocked,
    Unknown,
}

#[uniffi::remote(Enum)]
pub enum StepStatus {
    Passed,
    Failed,
    /// There was no reference yet; the screenshot became the reference.
    BaselineCreated,
    Cancelled,
    Unknown,
}

/// Why a run waits.
#[uniffi::remote(Enum)]
pub enum PauseReason {
    Requested,
    Step,
    Breakpoint,
    /// A person used the mouse or keyboard while the test was driving the Mac.
    UserInput,
    /// A reason from a newer Mac.
    Unknown,
}

/// A vision model's verdict on a screen.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct VisionAnswer {
    /// Provider name, such as `Anthropic`.
    pub provider: String,
    pub model: String,
    pub passed: bool,
    /// The model's explanation, in a sentence.
    pub reason: String,
}

impl From<jevision_runner::VisionAnswer> for VisionAnswer {
    fn from(a: jevision_runner::VisionAnswer) -> Self {
        Self {
            provider: a.provider,
            model: a.model,
            passed: a.passed,
            reason: a.reason,
        }
    }
}

/// Why a block or a run failed: a localization key (`run.waitFor.timeout`) and the values its
/// text uses.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Failure {
    pub code: String,
    pub params: HashMap<String, String>,
}

impl From<jevision_remote::Failure> for Failure {
    fn from(f: jevision_remote::Failure) -> Self {
        Self {
            code: f.code,
            params: f.params.into_iter().collect(),
        }
    }
}

/// What a watched run sends: its full status first and again whenever the phone fell behind,
/// events in between.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum RunUpdate {
    Status { status: RunStatus },
    Event { event: RunEvent },
}

impl From<jevision_remote::RunUpdate> for RunUpdate {
    fn from(update: jevision_remote::RunUpdate) -> Self {
        match update {
            jevision_remote::RunUpdate::Status { status } => Self::Status { status },
            jevision_remote::RunUpdate::Event { event } => Self::Event {
                event: event.into(),
            },
        }
    }
}

/// Progress of a run.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
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
        /// as the data row's: `Hello, Anna!`. `None` from Macs that don't send it.
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
        /// The way out an `if` took: `then`, `elseIf1`, `elseIf2`… or `else`.
        branch: Option<String>,
        /// How many ways out the `if` had.
        branches: Option<u32>,
        /// What the condition of the way taken looks for. Absent for `else`.
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

impl From<jevision_remote::RunEvent> for RunEvent {
    fn from(event: jevision_remote::RunEvent) -> Self {
        use jevision_remote::RunEvent as E;
        match event {
            E::AttemptStarted { attempt } => Self::AttemptStarted { attempt },
            E::BlockStarted {
                attempt,
                seq,
                test,
                block,
                block_type,
                title,
            } => Self::BlockStarted {
                attempt,
                seq,
                test,
                block,
                block_type,
                title,
            },
            E::BlockFinished {
                attempt,
                seq,
                test,
                block,
                status,
                duration_ms,
                failure,
                branch,
                branches,
                branch_condition,
            } => Self::BlockFinished {
                attempt,
                seq,
                test,
                block,
                status,
                duration_ms,
                failure: failure.map(Into::into),
                branch,
                branches,
                branch_condition,
            },
            E::Paused { block, reason } => Self::Paused { block, reason },
            E::Resumed => Self::Resumed,
            E::AttemptFinished {
                attempt,
                verdict,
                failure,
            } => Self::AttemptFinished {
                attempt,
                verdict,
                failure: failure.map(Into::into),
            },
            E::Finished { verdict, flaky } => Self::Finished { verdict, flaky },
        }
    }
}

/// Outcome of a whole run, as saved on the Mac.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct RunReport {
    /// Name of the test.
    pub test: String,
    /// Test file, relative to the project folder.
    pub file: String,
    /// Bundle id of the application under test.
    pub app: String,
    /// UTC time, RFC 3339.
    pub started_at: String,
    pub duration_ms: u64,
    /// Result of the last attempt.
    pub verdict: Verdict,
    /// Attempts disagreed: a failure was followed by a pass, or attempts failed differently.
    pub flaky: bool,
    /// References saved by this run because they did not exist, relative to the project folder.
    pub baselines_created: Vec<String>,
    pub attempts: Vec<Attempt>,
    /// Name of the paired device that started the run; `None` when it was started on the Mac.
    pub requested_by: Option<String>,
    /// Name of the test that didn't pass, so this one, depending on it, didn't run.
    #[uniffi(default = None)]
    pub blocked_by: Option<String>,
}

impl From<jevision_remote::RunReport> for RunReport {
    fn from(r: jevision_remote::RunReport) -> Self {
        Self {
            test: r.test,
            file: r.file,
            app: r.app,
            started_at: r.started_at,
            duration_ms: r.duration_ms,
            verdict: r.verdict,
            flaky: r.flaky,
            baselines_created: r.baselines_created,
            attempts: r.attempts.into_iter().map(Into::into).collect(),
            requested_by: r.requested_by,
            blocked_by: r.blocked.map(|b| b.cause_name),
        }
    }
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Attempt {
    /// 1 for the first run, 2 for the first retry and so on.
    pub number: u32,
    pub verdict: Verdict,
    pub duration_ms: u64,
    pub failure: Option<Failure>,
    /// Row of the test's data table the attempt ran with; absent for a test without one.
    pub row: Option<DataRow>,
    /// In the order the blocks started.
    pub steps: Vec<Step>,
}

/// A row of a test's data table, as a run names it.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DataRow {
    /// Counted from 1.
    pub number: u32,
    /// The row's value in its `name` column, or else in its first; may be empty.
    pub label: String,
}

impl From<jevision_runner::Attempt> for Attempt {
    fn from(a: jevision_runner::Attempt) -> Self {
        Self {
            number: a.number,
            verdict: a.verdict,
            duration_ms: a.duration_ms,
            failure: a.failure.map(Into::into),
            row: a.row.map(|row| DataRow {
                number: row.number,
                label: row.label,
            }),
            steps: a.steps.into_iter().map(Into::into).collect(),
        }
    }
}

/// One block as it ran. Pictures are paths relative to the run folder; fetch them with
/// `RemoteConnection.picture`.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Step {
    pub seq: u32,
    /// Test file the block belongs to.
    pub test: String,
    pub block: String,
    pub block_type: String,
    pub status: StepStatus,
    pub duration_ms: u64,
    pub before: Option<String>,
    pub after: Option<String>,
    /// Copy of the reference the screen was compared with.
    pub expected: Option<String>,
    pub diff: Option<String>,
    /// Reference file of a `check`, relative to the project folder.
    pub reference: Option<String>,
    /// Share of compared pixels that matched the reference, 0–1.
    pub similarity: Option<f64>,
    /// What the vision model said, for a comparison by meaning.
    pub vision: Option<VisionAnswer>,
    /// Edge an `if` block followed: `then`, `elseIf1`, `elseIf2`… or `else`.
    pub branch: Option<String>,
    /// How many ways out the `if` had: two are Yes and No, more are Condition 1, 2… and Otherwise.
    pub branches: Option<u32>,
    /// What the condition of the way taken looks for: what the screen should show, or the name
    /// of the reference image. Absent for `else`.
    pub branch_condition: Option<String>,
    pub failure: Option<Failure>,
}

impl From<jevision_runner::Step> for Step {
    fn from(s: jevision_runner::Step) -> Self {
        Self {
            seq: s.seq,
            test: s.test,
            block: s.block,
            block_type: s.block_type,
            status: s.status,
            duration_ms: s.duration_ms,
            before: s.before,
            after: s.after,
            expected: s.expected,
            diff: s.diff,
            reference: s.reference,
            similarity: s.similarity,
            vision: s.vision.map(Into::into),
            branch: s.branch,
            branches: s.branches,
            branch_condition: s.branch_condition,
            failure: s.failure.map(Into::into),
        }
    }
}
