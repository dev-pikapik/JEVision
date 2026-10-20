//! Running tests from the native apps: the platform layer, secrets and progress are callback
//! interfaces the app implements; [`TestRun`] runs a test on its own thread and controls it.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use jevision_core::{
    Blocker, Modifiers, MouseButton, Point, Rect, ScrollDirection, ScrollUnit, WindowPosition,
    WindowSize,
};
use jevision_runner::{
    PauseReason, PixelArea, RetryVerdict, RunControl, Runner, Screenshot, StepStatus, Verdict,
    VideoMask, VideoMode, VideoWindow, VisionJudge,
};

use crate::vision::{AiUsageSettings, NamedJudge, ProviderSettings, VisionSettings};

/// Why the operating system layer could not do something.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum HandsError {
    /// `permission` is `screenRecording` or `accessibility`.
    #[error("permission `{permission}` is not granted")]
    PermissionDenied { permission: String },
    #[error("application `{app}` is not installed")]
    AppNotFound { app: String },
    #[error("application `{app}` has no open window")]
    NoWindow { app: String },
    #[error("cannot open `{item}`")]
    CannotOpen { item: String },
    /// The screen is locked: nothing to look at, and keys would go to the lock screen.
    #[error("the screen is locked")]
    ScreenLocked,
    /// The application did not quit when asked to, so it cannot start over.
    #[error("application `{app}` is still running")]
    StillRunning { app: String },
    /// The window does not let its size or position be changed.
    #[error("the window of `{app}` cannot be resized or moved")]
    WindowFixed { app: String },
    /// The window took another size or position than asked. `what` is `size` or `position`,
    /// `wanted` and `actual` are written `900 × 600` and `40, 60`.
    #[error("the {what} of the window of `{app}` should be {wanted} but is {actual}")]
    WindowDiffers {
        app: String,
        what: String,
        wanted: String,
        actual: String,
    },
    #[error("{reason}")]
    Failed { reason: String },
}

impl From<uniffi::UnexpectedUniFFICallbackError> for HandsError {
    fn from(e: uniffi::UnexpectedUniFFICallbackError) -> Self {
        Self::Failed { reason: e.reason }
    }
}

impl From<HandsError> for jevision_runner::HandsError {
    fn from(e: HandsError) -> Self {
        match e {
            HandsError::PermissionDenied { permission } => Self::PermissionDenied(permission),
            HandsError::AppNotFound { app } => Self::AppNotFound(app),
            HandsError::NoWindow { app } => Self::NoWindow(app),
            HandsError::CannotOpen { item } => Self::CannotOpen(item),
            HandsError::ScreenLocked => Self::ScreenLocked,
            HandsError::StillRunning { app } => Self::StillRunning(app),
            HandsError::WindowFixed { app } => Self::WindowFixed(app),
            HandsError::WindowDiffers {
                app,
                what,
                wanted,
                actual,
            } => Self::WindowDiffers {
                app,
                what,
                wanted,
                actual,
            },
            HandsError::Failed { reason } => Self::Failed(reason),
        }
    }
}

/// Something [`Hands::open_items`] opens.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum Openable {
    /// An absolute path: a fresh copy of the test's file, made for this run.
    File { path: String },
    /// An `http(s)://` address.
    Url { url: String },
}

/// Operating-system side of a run, implemented by the app. Points and screenshots are relative
/// to the front window of the given application. Calls come from the run's thread, never the
/// main one; long ones should stop early once the run is cancelled.
#[uniffi::export(with_foreign)]
pub trait Hands: Send + Sync {
    /// Opens the application and brings it to the front. With `fresh` a copy that is running
    /// is first asked to quit (`StillRunning` if it will not) and the new one does not bring
    /// back the windows of its last session.
    fn open_app(&self, app: String, fresh: bool) -> Result<(), HandsError>;
    /// Gives the front window of the application this size and position, and checks that it
    /// took them: `WindowFixed` if the window cannot be changed, `WindowDiffers` if it ended
    /// up elsewhere or at another size.
    fn arrange_window(
        &self,
        app: String,
        size: Option<WindowSize>,
        position: Option<WindowPosition>,
    ) -> Result<(), HandsError>;
    /// Opens files or web addresses in `app`, or in the system default application.
    fn open_items(
        &self,
        items: Vec<Openable>,
        app: Option<String>,
        new_instance: bool,
    ) -> Result<(), HandsError>;
    /// Closes what `open_items` opened for these items: the windows showing the files, and the
    /// application if that call started it.
    fn close_items(&self, items: Vec<Openable>, app: Option<String>) -> Result<(), HandsError>;
    fn activate(&self, app: String) -> Result<(), HandsError>;
    fn quit(&self, app: String) -> Result<(), HandsError>;
    fn capture(&self, app: String) -> Result<Screenshot, HandsError>;
    fn locate(
        &self,
        app: String,
        template: Vec<u8>,
        min_similarity: f64,
    ) -> Result<Option<Point>, HandsError>;
    fn click(
        &self,
        app: String,
        at: Point,
        button: MouseButton,
        double_click: bool,
        modifiers: Modifiers,
    ) -> Result<(), HandsError>;
    /// Moves the pointer to `at` and leaves it there.
    fn hover(&self, app: String, at: Point) -> Result<(), HandsError>;
    /// Presses `button` at the first point of `path`, moves through the others holding it down,
    /// and lets go at the last, with the keys of `modifiers` held down all the while. The button
    /// and the keys are let go even when the run is cancelled on the way.
    fn drag(
        &self,
        app: String,
        path: Vec<Point>,
        button: MouseButton,
        modifiers: Modifiers,
    ) -> Result<(), HandsError>;
    /// Moves the pointer to `at` and turns the scroll wheel by `amount` lines or pixels; `down`
    /// brings what is further down into sight.
    fn scroll(
        &self,
        app: String,
        at: Point,
        direction: ScrollDirection,
        amount: u32,
        unit: ScrollUnit,
    ) -> Result<(), HandsError>;
    /// The text may hold secret values: never log it.
    fn type_text(&self, text: String) -> Result<(), HandsError>;
    fn press_keys(&self, keys: String) -> Result<(), HandsError>;
    /// The text on the clipboard, empty when it holds none. The text may be secret: never log it.
    fn clipboard(&self) -> Result<String, HandsError>;
    /// Replaces what is on the clipboard with `text`, which may hold secret values: never log it.
    fn set_clipboard(&self, text: String) -> Result<(), HandsError>;
    /// Whether a person moved the mouse or typed since the last call; the object's own
    /// synthetic input does not count.
    fn user_input_detected(&self) -> bool;
    /// Starts recording the app's windows into an `.mp4` at `path`. The video's first moment is
    /// the moment of this call; the app may not be running yet. Every frame has `hide`
    /// (fractions of a window) and the password fields painted over in each of the app's
    /// windows; [`video_mask`] works out where.
    fn start_video(&self, app: String, path: String, hide: Vec<Rect>) -> Result<(), HandsError>;
    /// Stops the recording and finishes the file; a recording without a picture leaves none.
    fn stop_video(&self) -> Result<(), HandsError>;
}

#[uniffi::remote(Record)]
pub struct VideoWindow {
    pub bounds: Rect,
    pub hide: Vec<Rect>,
}

#[uniffi::remote(Record)]
pub struct PixelArea {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[uniffi::remote(Record)]
pub struct VideoMask {
    pub areas: Vec<PixelArea>,
    pub luma: u8,
    pub chroma_blue: u8,
    pub chroma_red: u8,
}

/// What to paint over a `width`×`height` video frame showing the parts `shown` of the screen
/// (points, the same as the windows' `bounds`): the windows' `hide` areas in whole, even
/// pixels, and the colour screenshots are covered with, in the frame's video-range Y′CbCr.
#[uniffi::export]
pub fn video_mask(
    width: u32,
    height: u32,
    shown: Vec<Rect>,
    windows: Vec<VideoWindow>,
) -> VideoMask {
    jevision_runner::video_mask(width, height, &shown, &windows)
}

/// Values for `${secret.NAME}`. Asked once per secret before the first block runs.
#[uniffi::export(with_foreign)]
pub trait SecretSource: Send + Sync {
    /// `environment` is the run's environment: a secret may have a value of its own there.
    fn secret(&self, name: String, environment: Option<String>) -> Option<String>;
}

/// Receives a run's progress on the run's thread: every [`RunEvent`], then [`RunOutcome`].
#[uniffi::export(with_foreign)]
pub trait RunListener: Send + Sync {
    fn event(&self, event: RunEvent);
    fn finished(&self, outcome: RunOutcome);
}

/// How a run starts.
#[derive(Debug, Clone, Default, uniffi::Record)]
pub struct RunOptions {
    /// Block to start from instead of the test's `start` block.
    pub start_at: Option<String>,
    /// Overrides the test's `retries`.
    pub retries: Option<u32>,
    /// Environment from the project's `jevision.environments.json` for `${env.NAME}`; it also
    /// picks the secrets of that environment.
    #[uniffi(default = None)]
    pub environment: Option<String>,
    /// Rows of the data table to run; `None` runs every row. A test whose first `useData`
    /// block names a table runs once per row.
    #[uniffi(default = None)]
    pub data_rows: Option<DataRowRange>,
    /// The model for checks by meaning; without one they fail with a clear message.
    pub vision: Option<VisionSettings>,
    /// Checks by meaning may only use a model on this Mac.
    pub local_only: bool,
    /// Name of the paired device that asked for the run, saved in the report.
    #[uniffi(default = None)]
    pub requested_by: Option<String>,
    /// Name of the schedule that started the run, saved in the report.
    #[uniffi(default = None)]
    pub schedule: Option<String>,
    /// Asked instead of `vision` when it can't be reached, is busy or has used up its limit.
    #[uniffi(default = None)]
    pub fallback: Option<VisionSettings>,
    /// Settings of the providers that blocks need besides `vision`'s: those checks by meaning
    /// name, and JEV, which finds elements a test describes in words.
    #[uniffi(default = None)]
    pub providers: Option<Arc<dyn ProviderSettings>>,
    /// Where AI calls are logged and the spending limits they are held to; `None` logs
    /// nothing and limits nothing.
    #[uniffi(default = None)]
    pub usage: Option<AiUsageSettings>,
    /// Whether to record a video of the app, and which videos to keep.
    pub video: VideoMode,
    /// Videos of the project's runs older than this many days are deleted after the run;
    /// `None` keeps them all.
    #[uniffi(default = None)]
    pub keep_videos_days: Option<u32>,
    /// The test is in the run only because other tests in it depend on it.
    #[uniffi(default = false)]
    pub added_as_dependency: bool,
}

/// One test run on a background thread. Control methods work from any thread and take effect
/// at once; a run waiting for the screen wakes up on [`cancel`](Self::cancel).
#[derive(uniffi::Object)]
pub struct TestRun {
    pub(crate) control: RunControl,
}

#[uniffi::export]
impl TestRun {
    #[uniffi::constructor]
    pub fn new() -> Self {
        Self {
            control: RunControl::new(),
        }
    }

    /// Runs the test file `test` (relative to `project`, or absolute) on a new thread and
    /// returns at once. Saves `report.json`, `report.html` and `junit.xml` in a new folder under
    /// `<project>/.jevision/runs`.
    pub fn start(
        &self,
        project: String,
        test: String,
        options: RunOptions,
        hands: Arc<dyn Hands>,
        secrets: Arc<dyn SecretSource>,
        listener: Arc<dyn RunListener>,
    ) {
        let control = self.control.clone();
        let thread_listener = listener.clone();
        let spawned = std::thread::Builder::new()
            .name("jevision-run".into())
            .spawn(move || {
                let events = |e: &jevision_runner::Event| thread_listener.event(e.clone().into());
                let outcome = run(&project, &test, options, hands, secrets, &events, control);
                thread_listener.finished(outcome);
            });
        if let Err(e) = spawned {
            listener.finished(RunOutcome::Failed {
                reason: e.to_string(),
            });
        }
    }

    /// Emergency stop: the run ends before the next action and is reported as cancelled.
    pub fn cancel(&self) {
        self.control.cancel();
    }

    pub fn is_cancelled(&self) -> bool {
        self.control.is_cancelled()
    }

    /// Pauses before the next block, or between two looks at the screen while waiting.
    pub fn pause(&self) {
        self.control.pause();
    }

    /// Leaves step mode and continues until the next breakpoint.
    pub fn resume(&self) {
        self.control.resume();
    }

    /// Runs one block and pauses before the next; turns step mode on.
    pub fn step(&self) {
        self.control.step();
    }

    /// With step mode on before the run starts, it pauses before the first block.
    pub fn set_step_mode(&self, on: bool) {
        self.control.set_step_mode(on);
    }

    /// Ids of blocks to pause before.
    pub fn set_breakpoints(&self, blocks: Vec<String>) {
        self.control.set_breakpoints(blocks);
    }

    pub fn is_paused(&self) -> bool {
        self.control.is_paused()
    }
}

impl Default for TestRun {
    fn default() -> Self {
        Self::new()
    }
}

/// Runs `test` on this thread and saves its reports; `events` hears the progress.
pub(crate) fn run(
    project: &str,
    test: &str,
    options: RunOptions,
    hands: Arc<dyn Hands>,
    secrets: Arc<dyn SecretSource>,
    events: &dyn jevision_runner::EventSink,
    control: RunControl,
) -> RunOutcome {
    let hands = HandsAdapter(hands);
    let secrets = SecretAdapter(secrets, options.environment.clone());
    let vision = options.vision.clone().and_then(|v| v.client().ok());
    let fallback = options.fallback.clone().and_then(|v| v.client().ok());
    let providers = options
        .providers
        .as_ref()
        .map(NamedJudge::every)
        .unwrap_or_default();
    let providers: Vec<&dyn VisionJudge> = providers.iter().map(|c| c as _).collect();
    let usage = options.usage.clone().map(AiUsageSettings::book);
    let locator = options
        .providers
        .as_ref()
        .map(|p| NamedJudge::new(jevision_runner::Provider::Jev, p));
    let runner = Runner {
        hands: &hands,
        secrets: &secrets,
        events,
        control,
        vision: vision.as_ref().map(|c| c as &dyn VisionJudge),
        fallback: fallback.as_ref().map(|c| c as &dyn VisionJudge),
        providers: &providers,
        usage: usage.as_ref(),
        locator: locator.as_ref().map(|c| c as &dyn VisionJudge),
    };
    match runner.run(
        Path::new(project),
        Path::new(test),
        &runner_options(&options),
    ) {
        Ok(run) => finished(run),
        Err(e) => RunOutcome::Failed {
            reason: e.to_string(),
        },
    }
}

fn runner_options(options: &RunOptions) -> jevision_runner::RunOptions {
    jevision_runner::RunOptions {
        start_at: options.start_at.clone(),
        retries: options.retries,
        environment: options.environment.clone(),
        rows: options
            .data_rows
            .as_ref()
            .map(|r| r.first as usize..=r.last as usize),
        local_only: options.local_only,
        requested_by: options.requested_by.clone(),
        schedule: options.schedule.clone(),
        video: options.video,
        keep_videos_for: options
            .keep_videos_days
            .map(|days| std::time::Duration::from_secs(u64::from(days) * 24 * 60 * 60)),
        added_as_dependency: options.added_as_dependency,
        screen_lock: screen_lock(),
        ..Default::default()
    }
}

/// Every run takes the Mac's screen lock, which is how the app, the command line and the
/// assistant keep out of each other's way. This crate's own tests don't: they would wait for
/// a person's run and leave a file in their library.
fn screen_lock() -> Option<std::path::PathBuf> {
    if cfg!(test) {
        None
    } else {
        jevision_runner::default_screen_lock()
    }
}

/// Saves `report.html` and `junit.xml` next to the report of `run`.
fn finished(run: jevision_runner::Run) -> RunOutcome {
    let html_path = run.dir.join("report.html");
    let html = std::fs::write(
        &html_path,
        jevision_runner::html(&run.report, &run.dir, run.report.video.as_deref()),
    )
    .ok()
    .map(|()| path(&html_path));
    // For CI: a missing file only costs the export, not the run.
    let _ = std::fs::write(
        run.dir.join("junit.xml"),
        jevision_runner::junit_xml(std::slice::from_ref(&run.report)),
    );
    RunOutcome::Finished {
        dir: path(&run.dir),
        html,
        report: run.report.into(),
    }
}

/// Saves the test file `test` of the folder `project` as blocked: it didn't run because
/// `blocker` didn't pass. The run shows in the history like any other.
#[uniffi::export]
pub fn save_blocked_run(
    project: String,
    test: String,
    blocker: Blocker,
    options: RunOptions,
) -> RunOutcome {
    match jevision_runner::Run::blocked(
        Path::new(&project),
        Path::new(&test),
        blocker,
        &runner_options(&options),
    ) {
        Ok(run) => finished(run),
        Err(e) => RunOutcome::Failed {
            reason: e.to_string(),
        },
    }
}

struct HandsAdapter(Arc<dyn Hands>);

impl jevision_runner::Hands for HandsAdapter {
    fn open_app(&self, app: &str, fresh: bool) -> Result<(), jevision_runner::HandsError> {
        Ok(self.0.open_app(app.to_owned(), fresh)?)
    }

    fn arrange_window(
        &self,
        app: &str,
        size: Option<WindowSize>,
        position: Option<WindowPosition>,
    ) -> Result<(), jevision_runner::HandsError> {
        Ok(self.0.arrange_window(app.to_owned(), size, position)?)
    }

    fn open(
        &self,
        items: &[jevision_runner::Openable],
        app: Option<&str>,
        new_instance: bool,
    ) -> Result<(), jevision_runner::HandsError> {
        Ok(self
            .0
            .open_items(openables(items), app.map(str::to_owned), new_instance)?)
    }

    fn close(
        &self,
        items: &[jevision_runner::Openable],
        app: Option<&str>,
    ) -> Result<(), jevision_runner::HandsError> {
        Ok(self
            .0
            .close_items(openables(items), app.map(str::to_owned))?)
    }

    fn activate(&self, app: &str) -> Result<(), jevision_runner::HandsError> {
        Ok(self.0.activate(app.to_owned())?)
    }

    fn quit(&self, app: &str) -> Result<(), jevision_runner::HandsError> {
        Ok(self.0.quit(app.to_owned())?)
    }

    fn capture(&self, app: &str) -> Result<Screenshot, jevision_runner::HandsError> {
        Ok(self.0.capture(app.to_owned())?)
    }

    fn locate(
        &self,
        app: &str,
        template: &[u8],
        min_similarity: f64,
    ) -> Result<Option<Point>, jevision_runner::HandsError> {
        Ok(self
            .0
            .locate(app.to_owned(), template.to_vec(), min_similarity)?)
    }

    fn click(
        &self,
        app: &str,
        at: Point,
        button: MouseButton,
        double: bool,
        modifiers: Modifiers,
    ) -> Result<(), jevision_runner::HandsError> {
        Ok(self
            .0
            .click(app.to_owned(), at, button, double, modifiers)?)
    }

    fn hover(&self, app: &str, at: Point) -> Result<(), jevision_runner::HandsError> {
        Ok(self.0.hover(app.to_owned(), at)?)
    }

    fn drag(
        &self,
        app: &str,
        path: &[Point],
        button: MouseButton,
        modifiers: Modifiers,
    ) -> Result<(), jevision_runner::HandsError> {
        Ok(self
            .0
            .drag(app.to_owned(), path.to_vec(), button, modifiers)?)
    }

    fn scroll(
        &self,
        app: &str,
        at: Point,
        direction: ScrollDirection,
        amount: u32,
        unit: ScrollUnit,
    ) -> Result<(), jevision_runner::HandsError> {
        Ok(self.0.scroll(app.to_owned(), at, direction, amount, unit)?)
    }

    fn type_text(&self, text: &str) -> Result<(), jevision_runner::HandsError> {
        Ok(self.0.type_text(text.to_owned())?)
    }

    fn press_keys(&self, keys: &str) -> Result<(), jevision_runner::HandsError> {
        Ok(self.0.press_keys(keys.to_owned())?)
    }

    fn clipboard(&self) -> Result<String, jevision_runner::HandsError> {
        Ok(self.0.clipboard()?)
    }

    fn set_clipboard(&self, text: &str) -> Result<(), jevision_runner::HandsError> {
        Ok(self.0.set_clipboard(text.to_owned())?)
    }

    fn user_input_detected(&self) -> bool {
        self.0.user_input_detected()
    }

    fn start_video(
        &self,
        app: &str,
        file: &Path,
        hide: &[Rect],
    ) -> Result<(), jevision_runner::HandsError> {
        Ok(self
            .0
            .start_video(app.to_owned(), path(file), hide.to_vec())?)
    }

    fn stop_video(&self) -> Result<(), jevision_runner::HandsError> {
        Ok(self.0.stop_video()?)
    }
}

struct SecretAdapter(Arc<dyn SecretSource>, Option<String>);

impl jevision_runner::SecretSource for SecretAdapter {
    fn secret(&self, name: &str) -> Option<String> {
        self.0.secret(name.to_owned(), self.1.clone())
    }
}

fn openables(items: &[jevision_runner::Openable]) -> Vec<Openable> {
    items
        .iter()
        .map(|item| match item {
            jevision_runner::Openable::File(p) => Openable::File { path: path(p) },
            jevision_runner::Openable::Url(url) => Openable::Url { url: url.clone() },
        })
        .collect()
}

fn path(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// How a run ended.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
#[allow(
    clippy::large_enum_variant,
    reason = "made once per run and lowered to the app at once; UniFFI enums can't hold a Box"
)]
pub enum RunOutcome {
    /// The run folder, its `report.html` (absent if it could not be written) and the report.
    Finished {
        dir: String,
        html: Option<String>,
        report: RunReport,
    },
    /// The run folder or report could not be written.
    Failed { reason: String },
}

/// Progress of a run, in the order things happen. Secret values are masked.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
#[expect(
    clippy::large_enum_variant,
    reason = "UniFFI passes enum fields by value; a box would not cross the boundary"
)]
pub enum RunEvent {
    RunStarted {
        test: String,
        file: String,
        dir: String,
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
    /// `test` is the file the block belongs to; it differs from the run's test inside `runTest`.
    StepStarted {
        attempt: u32,
        seq: u32,
        test: String,
        block: String,
        block_type: String,
        variables: HashMap<String, String>,
    },
    StepFinished {
        attempt: u32,
        step: Step,
    },
    /// `variables` are the values visible to `block`, secrets masked.
    Paused {
        block: String,
        reason: PauseReason,
        variables: HashMap<String, String>,
    },
    Resumed,
    /// The run acts on the screen and another run has it, so this one waits its turn; the next
    /// event is `StepStarted` once the screen is free. `holder` is `None` while the lock does
    /// not say who has the screen.
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
        report: String,
    },
}

/// The run that has the screen: another JEVision window, the command line or the assistant.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ScreenHolder {
    /// The program, such as `JEVision` or `jevision`.
    pub process: String,
    pub pid: u32,
    /// Name of the test that runs.
    pub test: String,
    /// When that run took the screen, in seconds since 1970.
    pub since: u64,
}

impl From<jevision_runner::ScreenHolder> for ScreenHolder {
    fn from(h: jevision_runner::ScreenHolder) -> Self {
        Self {
            process: h.process,
            pid: h.pid,
            test: h.test,
            since: h.since,
        }
    }
}

impl From<jevision_runner::Event> for RunEvent {
    fn from(e: jevision_runner::Event) -> Self {
        use jevision_runner::Event as E;
        match e {
            E::RunStarted {
                test,
                file,
                dir,
                max_attempts,
            } => Self::RunStarted {
                test,
                file,
                dir: path(&dir),
                max_attempts,
            },
            E::RowStarted { row, rows, label } => Self::RowStarted { row, rows, label },
            E::AttemptStarted { attempt } => Self::AttemptStarted { attempt },
            E::StepStarted {
                attempt,
                seq,
                test,
                block,
                block_type,
                variables,
            } => Self::StepStarted {
                attempt,
                seq,
                test,
                block,
                block_type,
                variables: variables.into_iter().collect(),
            },
            E::StepFinished { attempt, step } => Self::StepFinished {
                attempt,
                step: (*step).into(),
            },
            E::Paused {
                block,
                reason,
                variables,
            } => Self::Paused {
                block,
                reason,
                variables: variables.into_iter().collect(),
            },
            E::Resumed => Self::Resumed,
            E::WaitingForScreen { holder } => Self::WaitingForScreen {
                holder: holder.map(Into::into),
            },
            E::AttemptFinished {
                attempt,
                verdict,
                failure,
            } => Self::AttemptFinished {
                attempt,
                verdict,
                failure: failure.map(Into::into),
            },
            E::RunFinished {
                verdict,
                flaky,
                report,
            } => Self::RunFinished {
                verdict,
                flaky,
                report: path(&report),
            },
        }
    }
}

/// Outcome of a whole run, as saved in `report.json`.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct RunReport {
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
    pub flaky: bool,
    pub baselines_created: Vec<String>,
    /// The run passed, but no block looked at the screen, so it couldn't have caught a wrong
    /// result.
    #[uniffi(default = false)]
    pub nothing_checked: bool,
    /// Name of the paired device that started the run from afar; `None` for a run started on
    /// this Mac.
    #[uniffi(default = None)]
    pub requested_by: Option<String>,
    /// Name of the schedule that started the run; `None` for a run someone started.
    #[uniffi(default = None)]
    pub schedule: Option<String>,
    /// Environment of the project the run used.
    #[uniffi(default = None)]
    pub environment: Option<String>,
    /// Video of the run, relative to the run folder.
    #[uniffi(default = None)]
    pub video: Option<String>,
    /// SHA-256 of the test file the run read, in hex.
    #[uniffi(default = None)]
    pub file_hash: Option<String>,
    /// The test was in the project's quarantine: its failure doesn't fail a batch of runs.
    #[uniffi(default = false)]
    pub quarantined: bool,
    #[uniffi(default = None)]
    pub quarantine_reason: Option<String>,
    /// The run ran in a virtual machine; `None` where that couldn't be told.
    #[uniffi(default = None)]
    pub virtual_machine: Option<bool>,
    /// Device pixels per point of the display the run looked at: 2 on Retina.
    #[uniffi(default = None)]
    pub display_scale: Option<f64>,
    /// Why the test didn't run: a test it depends on failed or couldn't run.
    #[uniffi(default = None)]
    pub blocked: Option<Blocker>,
    /// The test ran only because other tests in the batch depend on it.
    #[uniffi(default = false)]
    pub added_as_dependency: bool,
    pub attempts: Vec<Attempt>,
    /// One result per row of the test's data table, in the order they ran; empty for a test
    /// without one.
    #[uniffi(default = [])]
    pub rows: Vec<RowResult>,
}

/// What the retries of a run say about its result.
#[uniffi::remote(Enum)]
pub enum RetryVerdict {
    PassedOnRetry { retry: u32 },
    FailedDifferently { attempts: u32 },
    FailedEveryTime { attempts: u32 },
}

/// What the retries of the run say: flaky or a real failure; `None` for a run tried once or
/// stopped.
#[uniffi::export]
pub fn retry_verdict(report: &RunReport) -> Option<RetryVerdict> {
    if report.rows.is_empty() {
        return RetryVerdict::of(report.verdict, report.flaky, report.attempts.len());
    }
    // Each data row has retries of its own: the row that decided the run speaks for it.
    let row = report
        .rows
        .iter()
        .find(|r| r.verdict != Verdict::Passed)
        .or_else(|| report.rows.iter().find(|r| r.flaky))?;
    RetryVerdict::of(row.verdict, row.flaky, row.attempts as usize)
}

/// The attempt that decided the run's verdict: its last one, or with a data table the last
/// attempt of the first row that didn't pass.
#[uniffi::export]
pub fn deciding_attempt(report: &RunReport) -> Option<Attempt> {
    report
        .attempts
        .chunk_by(|a, b| a.row == b.row)
        .filter_map(<[Attempt]>::last)
        .find(|a| a.verdict != Verdict::Passed)
        .or(report.attempts.last())
        .cloned()
}

/// English sentence for a retry verdict, for the command line.
#[uniffi::export]
pub fn describe_retry_verdict(verdict: RetryVerdict) -> String {
    verdict.describe()
}

impl From<jevision_runner::RunReport> for RunReport {
    fn from(r: jevision_runner::RunReport) -> Self {
        let rows = r.rows().into_iter().map(Into::into).collect();
        Self {
            rows,
            test: r.test,
            file: r.file,
            app: r.app,
            started_at: r.started_at,
            duration_ms: r.duration_ms,
            verdict: r.verdict,
            flaky: r.flaky,
            baselines_created: r.baselines_created,
            nothing_checked: r.nothing_checked,
            requested_by: r.requested_by,
            schedule: r.schedule,
            environment: r.environment,
            video: r.video,
            file_hash: r.file_hash,
            quarantined: r.quarantined,
            quarantine_reason: r.quarantine_reason,
            virtual_machine: r.virtual_machine,
            display_scale: r.display_scale,
            blocked: r.blocked,
            added_as_dependency: r.added_as_dependency,
            attempts: r.attempts.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Attempt {
    /// Counted anew for each data row.
    pub number: u32,
    pub verdict: Verdict,
    pub duration_ms: u64,
    pub failure: Option<Failure>,
    pub steps: Vec<Step>,
    /// Row of the test's data table the attempt ran with.
    #[uniffi(default = None)]
    pub row: Option<DataRow>,
}

/// Rows of a data table, counted from 0, `first` to `last` inclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct DataRowRange {
    pub first: u32,
    pub last: u32,
}

/// A row of the test's data table, as a run names it.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DataRow {
    /// Counted from 1.
    pub number: u32,
    /// The row's value in its `name` column, or else in its first; may be empty.
    pub label: String,
}

impl From<jevision_runner::DataRow> for DataRow {
    fn from(r: jevision_runner::DataRow) -> Self {
        Self {
            number: r.number,
            label: r.label,
        }
    }
}

/// How one data row of a run went: the verdict of its last attempt.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct RowResult {
    pub row: DataRow,
    pub verdict: Verdict,
    pub flaky: bool,
    pub attempts: u32,
    /// Why the row's last attempt failed.
    pub failure: Option<Failure>,
}

impl From<jevision_runner::RowResult> for RowResult {
    fn from(r: jevision_runner::RowResult) -> Self {
        Self {
            row: r.row.into(),
            verdict: r.verdict,
            flaky: r.flaky,
            attempts: u32::try_from(r.attempts).unwrap_or(u32::MAX),
            failure: r.failure.map(Into::into),
        }
    }
}

impl From<jevision_runner::Attempt> for Attempt {
    fn from(a: jevision_runner::Attempt) -> Self {
        Self {
            number: a.number,
            verdict: a.verdict,
            duration_ms: a.duration_ms,
            failure: a.failure.map(Into::into),
            steps: a.steps.into_iter().map(Into::into).collect(),
            row: a.row.map(Into::into),
        }
    }
}

/// One block as it ran. Image paths are relative to the run folder.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Step {
    pub seq: u32,
    pub test: String,
    pub block: String,
    pub block_type: String,
    pub status: StepStatus,
    pub duration_ms: u64,
    pub before: Option<String>,
    pub after: Option<String>,
    pub expected: Option<String>,
    pub diff: Option<String>,
    /// Reference file of a `check`, relative to the project folder.
    pub reference: Option<String>,
    pub similarity: Option<f64>,
    /// What the vision model said, for a comparison by meaning.
    pub vision: Option<VisionAnswer>,
    pub branch: Option<String>,
    pub failure: Option<Failure>,
    /// When the block started, in milliseconds from the start of the run's video.
    #[uniffi(default = None)]
    pub video_at_ms: Option<u64>,
    /// The screenshot exactly as a vision model got it.
    #[uniffi(default = None)]
    pub sent: Option<String>,
    /// Private areas of `sent` were covered before sending.
    #[uniffi(default = false)]
    pub sent_masked: bool,
    /// Where the vision model found the elements the block describes in words, in fractions of
    /// `sent`.
    #[uniffi(default = [])]
    pub found: Vec<Rect>,
    /// How many ways out the `if` had: two are Yes and No, more are Condition 1, 2… and Otherwise.
    #[uniffi(default = None)]
    pub branches: Option<u32>,
    /// What the condition of the way taken looks for: what the screen should show, or the name
    /// of the reference image. Absent for `else`.
    #[uniffi(default = None)]
    pub branch_condition: Option<String>,
    /// Values the block saved for the blocks after it, by name. Secrets are masked.
    #[uniffi(default = [])]
    pub outputs: Vec<jevision_core::NamedValue>,
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
            failure: s.failure.map(Into::into),
            video_at_ms: s.video_at_ms,
            sent: s.sent,
            sent_masked: s.sent_masked,
            found: s.found,
            branches: s.branches,
            branch_condition: s.branch_condition,
            outputs: s
                .outputs
                .into_iter()
                .map(|(name, value)| jevision_core::NamedValue { name, value })
                .collect(),
        }
    }
}

/// Which way an `if` step went, in English, for the command line: `Took “Condition 2: Inbox is
/// empty”`, `Took “Otherwise”`. `None` for other blocks.
#[uniffi::export]
pub fn branch_text(step: &Step) -> Option<String> {
    jevision_runner::branch_text(
        step.branch.as_deref()?,
        step.branches,
        step.branch_condition.as_deref(),
    )
}

/// Why a run failed: a localization key (`run.waitFor.timeout`) and the values its text uses.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Failure {
    pub code: String,
    pub params: HashMap<String, String>,
}

impl From<jevision_runner::Failure> for Failure {
    fn from(f: jevision_runner::Failure) -> Self {
        Self {
            code: f.code,
            params: f.params.into_iter().collect(),
        }
    }
}

/// English sentence for a failure, for when the app has no translation of its code.
#[uniffi::export]
pub fn describe_failure(failure: Failure) -> String {
    jevision_runner::describe(&jevision_runner::Failure {
        code: failure.code,
        params: failure.params.into_iter().collect(),
    })
}

#[uniffi::remote(Record)]
pub struct Screenshot {
    /// PNG image in device pixels.
    pub png: Vec<u8>,
    /// Device pixels per point: 2 on Retina displays.
    pub scale: f64,
    /// The window was drawn in dark appearance.
    pub dark: bool,
    /// Password fields in the window, in fractions of it from its top-left corner.
    pub secure: Vec<Rect>,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct VisionAnswer {
    /// Provider name, such as `Anthropic`.
    pub provider: String,
    pub model: String,
    pub passed: bool,
    /// The model's explanation, in a sentence.
    pub reason: String,
    /// Tokens the call used, as the provider counted them.
    #[uniffi(default = 0)]
    pub input_tokens: u64,
    #[uniffi(default = 0)]
    pub output_tokens: u64,
    /// What the call cost in US dollars, when the model has a price.
    #[uniffi(default = None)]
    pub cost: Option<f64>,
    /// A spending limit is close or used up: `run.vision.limitNear` or `run.vision.limitReached`.
    #[uniffi(default = None)]
    pub warning: Option<Failure>,
    /// Why the fallback provider answered instead of the main one.
    #[uniffi(default = None)]
    pub fallback_reason: Option<Failure>,
}

impl From<jevision_runner::VisionAnswer> for VisionAnswer {
    fn from(a: jevision_runner::VisionAnswer) -> Self {
        Self {
            provider: a.provider,
            model: a.model,
            passed: a.passed,
            reason: a.reason,
            input_tokens: a.input_tokens,
            output_tokens: a.output_tokens,
            cost: a.cost,
            warning: a.warning.map(Into::into),
            fallback_reason: a.fallback_reason.map(Into::into),
        }
    }
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
    BaselineCreated,
    Cancelled,
    Unknown,
}

#[uniffi::remote(Enum)]
pub enum VideoMode {
    Off,
    /// Records every run and keeps the video only when the run failed.
    OnFailure,
    Always,
}

#[uniffi::remote(Enum)]
pub enum PauseReason {
    Requested,
    Step,
    Breakpoint,
    UserInput,
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use std::sync::mpsc::{Receiver, Sender, channel};
    use std::time::{Duration, Instant};

    const SECRET: &str = "s3cr3t-pa55";

    #[derive(Default)]
    pub(crate) struct Fake {
        log: Mutex<Vec<String>>,
        clipboard: Mutex<String>,
    }

    impl Hands for Fake {
        fn open_app(&self, app: String, _: bool) -> Result<(), HandsError> {
            self.note(format!("open {app}"));
            Ok(())
        }
        fn arrange_window(
            &self,
            _: String,
            _: Option<WindowSize>,
            _: Option<WindowPosition>,
        ) -> Result<(), HandsError> {
            Ok(())
        }
        fn open_items(
            &self,
            _: Vec<Openable>,
            _: Option<String>,
            _: bool,
        ) -> Result<(), HandsError> {
            Ok(())
        }
        fn close_items(&self, _: Vec<Openable>, _: Option<String>) -> Result<(), HandsError> {
            Ok(())
        }
        fn activate(&self, _: String) -> Result<(), HandsError> {
            Ok(())
        }
        fn quit(&self, _: String) -> Result<(), HandsError> {
            Ok(())
        }
        fn capture(&self, _: String) -> Result<Screenshot, HandsError> {
            Err(HandsError::PermissionDenied {
                permission: "screenRecording".into(),
            })
        }
        fn locate(&self, _: String, _: Vec<u8>, _: f64) -> Result<Option<Point>, HandsError> {
            Ok(None)
        }
        fn click(
            &self,
            _: String,
            _: Point,
            button: MouseButton,
            double: bool,
            modifiers: Modifiers,
        ) -> Result<(), HandsError> {
            self.note(format!("click {button:?} {double} {}", holding(modifiers)));
            Ok(())
        }
        fn hover(&self, _: String, _: Point) -> Result<(), HandsError> {
            Ok(())
        }
        fn drag(
            &self,
            _: String,
            _: Vec<Point>,
            button: MouseButton,
            modifiers: Modifiers,
        ) -> Result<(), HandsError> {
            self.note(format!("drag {button:?} {}", holding(modifiers)));
            Ok(())
        }
        fn scroll(
            &self,
            _: String,
            _: Point,
            _: ScrollDirection,
            _: u32,
            _: ScrollUnit,
        ) -> Result<(), HandsError> {
            Ok(())
        }
        fn type_text(&self, text: String) -> Result<(), HandsError> {
            self.note(format!("type {text}"));
            Ok(())
        }
        fn press_keys(&self, keys: String) -> Result<(), HandsError> {
            if keys == "cmd+q" {
                return Err(HandsError::PermissionDenied {
                    permission: "accessibility".into(),
                });
            }
            self.note(format!("keys {keys}"));
            Ok(())
        }
        fn clipboard(&self) -> Result<String, HandsError> {
            Ok(self.clipboard.lock().unwrap().clone())
        }
        fn set_clipboard(&self, text: String) -> Result<(), HandsError> {
            *self.clipboard.lock().unwrap() = text;
            Ok(())
        }
        fn user_input_detected(&self) -> bool {
            false
        }

        fn start_video(&self, app: String, path: String, _: Vec<Rect>) -> Result<(), HandsError> {
            self.note(format!("video {app}"));
            std::fs::write(path, b"movie").map_err(|e| HandsError::Failed {
                reason: e.to_string(),
            })
        }

        fn stop_video(&self) -> Result<(), HandsError> {
            self.note("video end".to_owned());
            Ok(())
        }
    }

    impl Fake {
        fn note(&self, entry: String) {
            self.log.lock().unwrap().push(entry);
        }
    }

    fn holding(modifiers: Modifiers) -> String {
        modifiers.names().join("+")
    }

    pub(crate) struct Secrets;

    impl SecretSource for Secrets {
        fn secret(&self, name: String, _: Option<String>) -> Option<String> {
            (name == "PASSWORD").then(|| SECRET.to_owned())
        }
    }

    enum Message {
        Event(Box<RunEvent>),
        Finished(Box<RunOutcome>),
    }

    struct Listener(Mutex<Sender<Message>>);

    impl RunListener for Listener {
        fn event(&self, event: RunEvent) {
            let _ = self.0.lock().unwrap().send(Message::Event(Box::new(event)));
        }
        fn finished(&self, outcome: RunOutcome) {
            let _ = self
                .0
                .lock()
                .unwrap()
                .send(Message::Finished(Box::new(outcome)));
        }
    }

    struct Project(PathBuf);

    impl Project {
        fn new(name: &str, blocks: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("jevision-ffi-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("t.flow.json"),
                format!(
                    r#"{{"format":1,"name":"T","app":"com.example.App","start":"open","blocks":{{{blocks}}}}}"#
                ),
            )
            .unwrap();
            Self(dir)
        }

        fn start(&self, run: &TestRun, hands: Arc<Fake>) -> Receiver<Message> {
            self.start_with(run, hands, RunOptions::default())
        }

        fn start_with(
            &self,
            run: &TestRun,
            hands: Arc<Fake>,
            options: RunOptions,
        ) -> Receiver<Message> {
            let (tx, rx) = channel();
            run.start(
                path(&self.0),
                "t.flow.json".into(),
                options,
                hands,
                Arc::new(Secrets),
                Arc::new(Listener(Mutex::new(tx))),
            );
            rx
        }
    }

    impl Drop for Project {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Events until the outcome, failing if the run takes longer than `limit`.
    fn wait(rx: &Receiver<Message>, limit: Duration) -> (Vec<RunEvent>, RunOutcome) {
        let mut events = Vec::new();
        let deadline = Instant::now() + limit;
        loop {
            match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(Message::Event(e)) => events.push(*e),
                Ok(Message::Finished(outcome)) => return (events, *outcome),
                Err(e) => panic!("run did not finish: {e}"),
            }
        }
    }

    #[test]
    fn runs_in_the_background_and_writes_reports() {
        let p = Project::new(
            "pass",
            r#""open":{"type":"openApp","next":"pw"},
               "pw":{"type":"type","text":"${secret.PASSWORD}","next":"go"},
               "go":{"type":"pressKeys","keys":"return"}"#,
        );
        let hands = Arc::new(Fake::default());
        let run = TestRun::new();
        let rx = p.start(&run, hands.clone());
        let (events, outcome) = wait(&rx, Duration::from_secs(10));
        let RunOutcome::Finished { dir, html, report } = outcome else {
            panic!("{outcome:?}")
        };
        assert_eq!(report.verdict, Verdict::Passed);
        assert!(Path::new(&dir).starts_with(std::fs::canonicalize(&p.0).unwrap()));
        let html = std::fs::read_to_string(html.unwrap()).unwrap();
        assert!(html.contains("Passed") && !html.contains(SECRET));
        let junit = std::fs::read_to_string(Path::new(&dir).join("junit.xml")).unwrap();
        assert!(junit.contains("<testcase") && !junit.contains(SECRET));
        assert_eq!(
            *hands.log.lock().unwrap(),
            [
                "open com.example.App",
                &format!("type {SECRET}"),
                "keys return"
            ]
        );
        assert!(matches!(events.first(), Some(RunEvent::RunStarted { .. })));
        assert!(matches!(
            events.last(),
            Some(RunEvent::RunFinished {
                verdict: Verdict::Passed,
                ..
            })
        ));
        let started: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                RunEvent::StepStarted { block, .. } => Some(block.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(started, ["open", "pw", "go"]);
        assert!(!format!("{events:?}").contains(SECRET));
    }

    #[test]
    fn the_clipboard_is_the_apps_to_fill_and_read() {
        let p = Project::new(
            "clipboard",
            r#""open":{"type":"openApp","next":"put"},
               "put":{"type":"clipboard","action":"put","text":"copied ${secret.PASSWORD}","next":"read"},
               "read":{"type":"clipboard","action":"read","saveAs":"text","next":"type"},
               "type":{"type":"type","text":"${var.text}"}"#,
        );
        let hands = Arc::new(Fake::default());
        let rx = p.start(&TestRun::new(), hands.clone());
        let (_, outcome) = wait(&rx, Duration::from_secs(10));
        let RunOutcome::Finished { report, .. } = outcome else {
            panic!("{outcome:?}")
        };
        assert_eq!(report.verdict, Verdict::Passed);
        assert_eq!(
            *hands.log.lock().unwrap(),
            [
                "open com.example.App".to_owned(),
                format!("type copied {SECRET}"),
            ]
        );
    }

    #[test]
    fn a_run_without_described_elements_never_reads_provider_settings() {
        let p = Project::new(
            "providers",
            r#""open":{"type":"openApp","next":"go"},
               "go":{"type":"click","target":{"point":{"x":0.5,"y":0.5}}}"#,
        );
        let asked = Arc::new(crate::vision::tests::Asked::default());
        let options = RunOptions {
            providers: Some(asked.clone()),
            ..RunOptions::default()
        };
        let rx = p.start_with(&TestRun::new(), Arc::new(Fake::default()), options);
        wait(&rx, Duration::from_secs(10));
        assert!(asked.0.lock().unwrap().is_empty());
    }

    #[test]
    fn cancel_ends_a_paused_run_at_once() {
        let p = Project::new(
            "cancel",
            r#""open":{"type":"openApp","next":"go"},"go":{"type":"pressKeys","keys":"return"}"#,
        );
        let run = TestRun::new();
        run.set_step_mode(true);
        let rx = p.start(&run, Arc::new(Fake::default()));
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Message::Event(e)) if matches!(*e, RunEvent::RunStarted { .. }) => {}
            _ => panic!("expected RunStarted"),
        }
        let next = || match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Message::Event(e)) => *e,
            _ => panic!("expected an event"),
        };
        assert_eq!(next(), RunEvent::AttemptStarted { attempt: 1 });
        assert_eq!(
            next(),
            RunEvent::Paused {
                block: "open".into(),
                reason: PauseReason::Step,
                variables: HashMap::new(),
            }
        );
        assert!(run.is_paused());
        let clock = Instant::now();
        run.cancel();
        let (_, outcome) = wait(&rx, Duration::from_secs(2));
        assert!(clock.elapsed() < Duration::from_millis(500));
        let RunOutcome::Finished { report, .. } = outcome else {
            panic!("{outcome:?}")
        };
        assert_eq!(report.verdict, Verdict::Cancelled);
        assert!(run.is_cancelled());
    }

    #[test]
    fn hands_errors_become_localizable_failures() {
        let p = Project::new("denied", r#""open":{"type":"pressKeys","keys":"cmd+q"}"#);
        let rx = p.start(&TestRun::new(), Arc::new(Fake::default()));
        let (_, outcome) = wait(&rx, Duration::from_secs(10));
        let RunOutcome::Finished { report, .. } = outcome else {
            panic!("{outcome:?}")
        };
        let failure = report.attempts[0].failure.clone().unwrap();
        assert_eq!(failure.code, "run.hands.permissionDenied");
        assert_eq!(failure.params["permission"], "accessibility");
        assert_eq!(failure.params["block"], "open");
        assert_eq!(
            describe_failure(failure),
            "Block open: permission accessibility is not granted."
        );
    }

    #[test]
    fn a_missing_project_folder_fails_to_start() {
        let (tx, rx) = channel();
        TestRun::new().start(
            "/nonexistent/jevision".into(),
            "t.flow.json".into(),
            RunOptions::default(),
            Arc::new(Fake::default()),
            Arc::new(Secrets),
            Arc::new(Listener(Mutex::new(tx))),
        );
        let (events, outcome) = wait(&rx, Duration::from_secs(5));
        assert!(events.is_empty());
        assert!(matches!(outcome, RunOutcome::Failed { .. }));
    }

    #[test]
    fn waiting_for_the_screen_names_who_has_it() {
        let holder = jevision_runner::ScreenHolder {
            process: "jevision".to_owned(),
            pid: 31,
            test: "Checkout".to_owned(),
            since: 1_700_000_000,
        };
        let event = RunEvent::from(jevision_runner::Event::WaitingForScreen {
            holder: Some(holder),
        });
        assert_eq!(
            event,
            RunEvent::WaitingForScreen {
                holder: Some(ScreenHolder {
                    process: "jevision".to_owned(),
                    pid: 31,
                    test: "Checkout".to_owned(),
                    since: 1_700_000_000,
                })
            }
        );
        assert_eq!(
            RunEvent::from(jevision_runner::Event::WaitingForScreen { holder: None }),
            RunEvent::WaitingForScreen { holder: None }
        );
    }
}
