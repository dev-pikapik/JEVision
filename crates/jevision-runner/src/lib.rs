//! Runs JEVision tests: walks a test's flowchart block by block, drives the application under
//! test through a platform layer ([`Hands`]), compares screenshots with reference images and
//! writes a [`RunReport`] with screenshots into the project's `.jevision/runs` folder.
//!
//! The crate is platform independent. Everything that touches the operating system — launching
//! applications, synthetic input, screen capture, finding an image on screen — sits behind
//! [`Hands`]; secrets come from a [`SecretSource`]; checks by meaning ask a [`VisionJudge`];
//! progress goes to an [`EventSink`]; a [`RunControl`] cancels, pauses and steps a run from
//! another thread.

mod app_settings;
mod bug;
mod ci;
mod control;
mod env;
mod home;
mod import;
mod last_results;
mod machine;
mod packages;
mod report;
mod run;
mod saved;
mod screen;
mod simulator;
mod system;
mod usage;
mod words;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

pub use app_settings::{default_saved_settings, put_back_left_settings};
pub use bug::{
    BugContext, BugDraft, BugPicture, bug_report, bug_report_in, describe_block, describe_block_in,
    mask_secrets, picture_caption_in, run_flows,
};
pub use ci::CiOrigin;
pub use control::{PauseReason, RunControl};
pub use import::{import_ci_artifact, imported_ci_artifacts};
pub use jevision_core::{DataError, DataTable};
pub use jevision_core::{
    ElementNode, Modifiers, MouseButton, Point, ScrollDirection, ScrollUnit, WindowPosition,
    WindowSize,
};
pub use jevision_vision::image::{PictureDiff, picture_diff};
pub use jevision_vision::{
    Client as VisionClient, Config as VisionConfig, Judgement, Located, MaskedImage, PixelArea,
    Provider, Question, Search, TIMEOUT, Usage, VideoMask, VideoWindow, VisionError, VisionJudge,
    video_mask,
};
pub use last_results::{LAST_RESULTS_FORMAT, verdict_before};
pub use packages::{StagedSet, discard_staged, stage_source_set, zip_package};
pub use report::{
    Attempt, DataRow, Failure, REPORT_FILE, REPORT_FORMAT, RetryVerdict, RowResult, RunReport,
    Step, StepStatus, UsedPackage, Verdict, VisionAnswer, branch_text, branch_text_in, html,
    html_in, junit_xml, junit_xml_in, nothing_checked_in,
};
pub use run::{
    Run, RunOptions, Runner, VIDEO_FILE, VideoMode, content_hash, create_run_dir, rfc3339, today,
    vision_failure,
};
pub use saved::{PruneSummary, preview_prune_runs, prune_runs};
pub use screen::{
    ScreenHolder, ScreenLock, default_screen_lock, describe_screen_holder,
    describe_screen_holder_in,
};
pub use simulator::{
    SIMULATOR_APP, Simctl, SimulatorApp, SimulatorDevice, SimulatorHands, Xcrun, installed_apps,
    pick_device, simulator_apps, simulator_devices,
};
pub use usage::{
    DaySpend, Limit, LimitState, Price, ProviderSpend, Spend, UsageBook, UsageRecord, UsageSummary,
    WARN_SHARE, default_prices, money, price_of,
};
pub use words::{
    Phrase, describe, describe_in, describe_vision, describe_vision_in, phrase, vision_phrase,
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
    /// Launches the application (or finds it running) and brings it to the front, as `start`
    /// says; returns once it can take input.
    fn open_app(
        &self,
        app: &str,
        start: AppStart,
        arguments: &[String],
        environment: &HashMap<String, String>,
    ) -> Result<(), HandsError>;
    /// Whether the application runs in the App Sandbox, as its code signature says. macOS then
    /// keeps the app's data in a container of its own and gives it a home folder there, whatever
    /// it is started with.
    fn sandboxed(&self, app: &str) -> Result<bool, HandsError>;
    /// Gives the application's front window this size and position, and checks that the window
    /// took them: [`HandsError::WindowFixed`] if the window cannot be changed,
    /// [`HandsError::WindowDiffers`] if it ended up somewhere else or at another size.
    fn arrange_window(
        &self,
        app: &str,
        size: Option<WindowSize>,
        position: Option<WindowPosition>,
    ) -> Result<(), HandsError>;
    /// Opens files or web addresses in `app`, or in the system default application.
    fn open(
        &self,
        items: &[Openable],
        app: Option<&str>,
        new_instance: bool,
    ) -> Result<(), HandsError>;
    /// Closes what [`Hands::open`] opened for these items: the windows showing the files, and
    /// the application if that call started it. A document with unsaved changes may stay open.
    fn close(&self, items: &[Openable], app: Option<&str>) -> Result<(), HandsError>;
    /// Brings the application to the front so typed keys reach it.
    fn activate(&self, app: &str) -> Result<(), HandsError>;
    /// Asks the application to quit, as the user would.
    fn quit(&self, app: &str) -> Result<(), HandsError>;
    /// Captures the application's front window.
    fn capture(&self, app: &str) -> Result<Screenshot, HandsError>;
    /// The elements of the application's front window and of the menus it has open, as
    /// accessibility reports them, an element before what is inside it; frames in fractions of
    /// the front window. [`HandsError::PermissionDenied`] with `accessibility` without that
    /// permission.
    fn elements(&self, app: &str) -> Result<Vec<ElementNode>, HandsError>;
    /// Finds the PNG `template` in the application's front window and returns its centre, in
    /// points from the window's top-left corner, or
    /// `None` when no place is at least `min_similarity` (0–1) alike.
    fn locate(
        &self,
        app: &str,
        template: &[u8],
        min_similarity: f64,
    ) -> Result<Option<Point>, HandsError>;
    /// Clicks at `at` with the keys of `modifiers` held down.
    fn click(
        &self,
        app: &str,
        at: Point,
        button: MouseButton,
        double: bool,
        modifiers: Modifiers,
    ) -> Result<(), HandsError>;
    /// Moves the pointer to `at` and leaves it there, with the keys of `modifiers` held down
    /// while it moves.
    fn hover(&self, app: &str, at: Point, modifiers: Modifiers) -> Result<(), HandsError>;
    /// Presses `button` at the first point of `path`, moves through the others holding it down,
    /// and lets go at the last, with the keys of `modifiers` held down all the while. The button
    /// and the keys are let go even when the run is cancelled.
    fn drag(
        &self,
        app: &str,
        path: &[Point],
        button: MouseButton,
        modifiers: Modifiers,
    ) -> Result<(), HandsError>;
    /// Moves the pointer to `at` and turns the scroll wheel by `amount` lines or pixels; `down`
    /// brings what is further down into sight. The keys of `modifiers` are held down while the
    /// wheel turns: with ⌘ most apps zoom instead of scrolling.
    fn scroll(
        &self,
        app: &str,
        at: Point,
        direction: ScrollDirection,
        amount: u32,
        unit: ScrollUnit,
        modifiers: Modifiers,
    ) -> Result<(), HandsError>;
    /// Types text into the front application. The text may hold secret values: never log it.
    fn type_text(&self, text: &str) -> Result<(), HandsError>;
    /// Presses one key combination such as `cmd+shift+s`.
    fn press_keys(&self, keys: &str) -> Result<(), HandsError>;
    /// The text on the clipboard; empty when it holds none. The text may be secret: never log it.
    fn clipboard(&self) -> Result<String, HandsError>;
    /// Replaces what is on the clipboard with `text`. The text may hold secret values: never log
    /// it.
    fn set_clipboard(&self, text: &str) -> Result<(), HandsError>;
    /// The picture on the clipboard as PNG; `None` when it holds none.
    fn clipboard_image(&self) -> Result<Option<Vec<u8>>, HandsError>;
    /// The files copied to the clipboard, as absolute paths; empty when it holds none.
    fn clipboard_files(&self) -> Result<Vec<String>, HandsError>;
    /// Replaces what is on the clipboard with the PNG picture `png`.
    fn set_clipboard_image(&self, png: &[u8]) -> Result<(), HandsError>;
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

/// How [`Hands::open_app`] starts an application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppStart {
    /// Finds it running, or launches it as usual.
    AsUsual,
    /// A copy that is already running is first asked to quit ([`HandsError::StillRunning`] if it
    /// will not); the new one starts without bringing back the windows of its last session.
    Fresh,
    /// As [`AppStart::Fresh`], and the new copy, never one that was running, gets the empty
    /// folder `home` (an absolute path) as its home folder: `HOME` and `CFFIXED_USER_HOME` point
    /// at it, so the app finds none of its settings and files and keeps what it writes there.
    CleanHome { home: String },
}

/// Something [`Hands::open`] opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Openable {
    /// An absolute path: a fresh copy of the test's file, made for this run.
    File(PathBuf),
    /// An `http(s)://` address.
    Url(String),
}

/// Why the operating system layer could not do something. A run turns it into a failure that
/// names the block; its `Display` is the same sentence without the block, in the process language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandsError {
    /// A system permission is missing; holds its name, e.g. `screenRecording`, `accessibility`.
    PermissionDenied(String),
    AppNotFound(String),
    NoWindow(String),
    CannotOpen(String),
    /// The screen is locked: there is nothing to look at, and keys would go to the lock screen.
    ScreenLocked,
    /// The application would not quit when asked to start over, e.g. a question about unsaved
    /// changes is waiting for an answer.
    StillRunning(String),
    /// The window does not let its size or position be changed.
    WindowFixed(String),
    /// The window took another size or position than asked, e.g. because of its own limits.
    /// `what` is `size` or `position`; `wanted` and `actual` are written `900 × 600` for a size
    /// and `40, 60` for a position.
    WindowDiffers {
        app: String,
        what: String,
        wanted: String,
        actual: String,
    },
    /// What the platform said, as it said it.
    Failed(String),
}

impl std::fmt::Display for HandsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use jevision_l10n::tr;
        let text = match self {
            Self::PermissionDenied(p) => tr!(
                "run.hands.permissionDenied",
                words::permission_name(&jevision_l10n::language(), p)
            ),
            Self::AppNotFound(app) => tr!("mcp.hands.appNotFound", app),
            Self::NoWindow(app) => tr!("mcp.hands.noWindow", app),
            Self::CannotOpen(item) => tr!("mcp.hands.cannotOpen", item),
            Self::ScreenLocked => tr!("mcp.hands.screenLocked"),
            Self::StillRunning(app) => tr!("mcp.hands.stillRunning", app),
            Self::WindowFixed(app) => tr!("mcp.hands.windowFixed", app),
            Self::WindowDiffers {
                app,
                what,
                wanted,
                actual,
            } if what == "position" => tr!("mcp.hands.windowDiffers.position", app, wanted, actual),
            Self::WindowDiffers {
                app,
                wanted,
                actual,
                ..
            } => tr!("mcp.hands.windowDiffers.size", app, wanted, actual),
            Self::Failed(reason) => reason.clone(),
        };
        f.write_str(&text)
    }
}

impl std::error::Error for HandsError {}

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
    /// `timeout_ms` is the longest the block waits, when it waits; time the run is paused
    /// doesn't count against it.
    StepStarted {
        attempt: u32,
        seq: u32,
        test: String,
        block: String,
        block_type: String,
        variables: BTreeMap<String, String>,
        timeout_ms: Option<u64>,
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
    /// The run acts on the screen and another run has it; this run waits for its turn. Sent
    /// again if the screen changes hands to another run that is also ahead. `holder` is `None`
    /// while the lock file does not say who has it.
    WaitingForScreen {
        holder: Option<ScreenHolder>,
    },
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
