//! Runs JEVision tests: walks a test's flowchart block by block, drives the application under
//! test through a platform layer ([`Hands`]), compares screenshots with reference images and
//! writes a [`RunReport`] with screenshots into the project's `.jevision/runs` folder.
//!
//! The crate is platform independent. Everything that touches the operating system — launching
//! applications, synthetic input, screen capture, finding an image on screen — sits behind
//! [`Hands`]; secrets come from a [`SecretSource`]; checks by meaning ask a [`VisionJudge`];
//! progress goes to an [`EventSink`]; a [`RunControl`] cancels, pauses and steps a run from
//! another thread.

mod control;
mod env;
mod machine;
mod report;
mod run;
mod usage;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

pub use control::{PauseReason, RunControl};
pub use jevision_core::{DataError, DataTable};
pub use jevision_core::{MouseButton, Point, ScrollDirection, ScrollUnit};
pub use jevision_vision::image::{PictureDiff, picture_diff};
pub use jevision_vision::{
    Client as VisionClient, Config as VisionConfig, Judgement, MaskedImage, PixelArea, Provider,
    Question, TIMEOUT, Usage, VideoMask, VideoWindow, VisionError, VisionJudge, video_mask,
};
pub use report::{
    Attempt, DataRow, Failure, NOTHING_CHECKED, REPORT_FORMAT, RetryVerdict, RowResult, RunReport,
    Step, StepStatus, Verdict, VisionAnswer, branch_text, describe, describe_vision, html,
    junit_xml,
};
pub use run::{Run, RunOptions, Runner, VIDEO_FILE, VideoMode, prune_videos, vision_failure};
pub use usage::{
    DaySpend, Limit, LimitState, Price, ProviderSpend, Spend, UsageBook, UsageRecord, UsageSummary,
    WARN_SHARE, default_prices, money, price_of,
};

/// Shown instead of a secret value wherever a secret would otherwise appear.
pub const SECRET_MASK: &str = "••••";

/// Operating-system side of a run: one implementation per platform.
///
/// Points and screenshots are relative to the front window of the given application, so tests
/// do not depend on window position or on which display the window is on. Methods may block
/// until the action is done; long-running ones (typing a long text) should stop early once the
/// [`RunControl`] the application created for the run reports [`RunControl::is_cancelled`].
pub trait Hands: Send + Sync {
    /// Launches the application (or finds it running) and brings it to the front; returns once
    /// it can take input.
    fn open_app(&self, app: &str) -> Result<(), HandsError>;
    /// Opens files or web addresses in `app`, or in the system default application.
    fn open(
        &self,
        items: &[Openable],
        app: Option<&str>,
        new_instance: bool,
    ) -> Result<(), HandsError>;
    /// Brings the application to the front so typed keys reach it.
    fn activate(&self, app: &str) -> Result<(), HandsError>;
    /// Asks the application to quit, as the user would.
    fn quit(&self, app: &str) -> Result<(), HandsError>;
    /// Captures the application's front window.
    fn capture(&self, app: &str) -> Result<Screenshot, HandsError>;
    /// Finds the PNG `template` in the application's front window and returns its centre, in
    /// points from the window's top-left corner, or
    /// `None` when no place is at least `min_similarity` (0–1) alike.
    fn locate(
        &self,
        app: &str,
        template: &[u8],
        min_similarity: f64,
    ) -> Result<Option<Point>, HandsError>;
    fn click(
        &self,
        app: &str,
        at: Point,
        button: MouseButton,
        double: bool,
    ) -> Result<(), HandsError>;
    /// Moves the pointer to `at` and leaves it there.
    fn hover(&self, app: &str, at: Point) -> Result<(), HandsError>;
    /// Presses the left button at the first point of `path`, moves through the others holding
    /// it down, and lets go at the last. The button is let go even when the run is cancelled.
    fn drag(&self, app: &str, path: &[Point]) -> Result<(), HandsError>;
    /// Moves the pointer to `at` and turns the scroll wheel by `amount` lines or pixels; `down`
    /// brings what is further down into sight.
    fn scroll(
        &self,
        app: &str,
        at: Point,
        direction: ScrollDirection,
        amount: u32,
        unit: ScrollUnit,
    ) -> Result<(), HandsError>;
    /// Types text into the front application. The text may hold secret values: never log it.
    fn type_text(&self, text: &str) -> Result<(), HandsError>;
    /// Presses one key combination such as `cmd+shift+s`.
    fn press_keys(&self, keys: &str) -> Result<(), HandsError>;
    /// Whether a person moved the mouse or typed since the last call; synthetic input sent by
    /// this object does not count. A `true` pauses the run.
    fn user_input_detected(&self) -> bool;
    /// Starts recording a video of the application's windows into the file at `path`. The
    /// video's first moment is the moment of this call, so times measured from it line up.
    /// The application may not be running yet: its windows appear once it opens them.
    /// Every frame has `hide` (fractions of a window, from its top-left corner) and the password
    /// fields painted over in each of the application's windows, as screenshots do.
    fn start_video(
        &self,
        app: &str,
        path: &Path,
        hide: &[jevision_core::Rect],
    ) -> Result<(), HandsError>;
    /// Stops the recording and finishes the file. A recording that caught no picture leaves
    /// no file.
    fn stop_video(&self) -> Result<(), HandsError>;
}

/// A captured window.
#[derive(Debug, Clone, PartialEq)]
pub struct Screenshot {
    /// PNG image in device pixels.
    pub png: Vec<u8>,
    /// Device pixels per point: 2 on Retina displays.
    pub scale: f64,
    /// The window was drawn in dark appearance.
    pub dark: bool,
    /// Password fields in the window, in fractions of it from its top-left corner. They are
    /// covered before the screenshot is saved or sent anywhere.
    pub secure: Vec<jevision_core::Rect>,
}

/// Something [`Hands::open`] opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Openable {
    /// An absolute path: a fresh copy of the test's file, made for this run.
    File(PathBuf),
    /// An `http(s)://` address.
    Url(String),
}

/// Why the operating system layer could not do something.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HandsError {
    /// A system permission is missing; holds its name, e.g. `screenRecording`, `accessibility`.
    #[error("permission `{0}` is not granted")]
    PermissionDenied(String),
    #[error("application `{0}` is not installed")]
    AppNotFound(String),
    #[error("application `{0}` has no open window")]
    NoWindow(String),
    #[error("cannot open `{0}`")]
    CannotOpen(String),
    /// The screen is locked: there is nothing to look at, and keys would go to the lock screen.
    #[error("the screen is locked")]
    ScreenLocked,
    #[error("{0}")]
    Failed(String),
}

/// Values for `${secret.NAME}` placeholders. Values are used only for typing; reports, events
/// and logs show [`SECRET_MASK`] instead.
pub trait SecretSource: Send + Sync {
    fn secret(&self, name: &str) -> Option<String>;
}

impl SecretSource for HashMap<String, String> {
    fn secret(&self, name: &str) -> Option<String> {
        self.get(name).cloned()
    }
}

/// Receives progress while a run goes on, on the thread that runs it.
pub trait EventSink: Send + Sync {
    fn send(&self, event: &Event);
}

impl<F: Fn(&Event) + Send + Sync> EventSink for F {
    fn send(&self, event: &Event) {
        self(event)
    }
}

/// Progress of a run, in the order things happen.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(
    tag = "event",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Event {
    /// `dir` is the run folder; screenshot paths in steps are relative to it.
    RunStarted {
        test: String,
        file: String,
        dir: PathBuf,
        max_attempts: u32,
    },
    /// A row of the test's data table starts, before its first attempt. `row` counts from 1.
    RowStarted {
        row: u32,
        rows: u32,
        label: String,
    },
    AttemptStarted {
        attempt: u32,
    },
    /// A block starts. `test` is the file the block belongs to (it differs from the run's test
    /// inside `runTest`); `variables` are the values visible to it, secrets masked.
    StepStarted {
        attempt: u32,
        seq: u32,
        test: String,
        block: String,
        block_type: String,
        variables: BTreeMap<String, String>,
    },
    StepFinished {
        attempt: u32,
        step: Box<Step>,
    },
    Paused {
        block: String,
        reason: PauseReason,
        /// Values visible to `block`, secrets masked: what a person debugging the test looks at.
        variables: BTreeMap<String, String>,
    },
    Resumed,
    AttemptFinished {
        attempt: u32,
        verdict: Verdict,
        failure: Option<Failure>,
    },
    RunFinished {
        verdict: Verdict,
        flaky: bool,
        report: PathBuf,
    },
}
