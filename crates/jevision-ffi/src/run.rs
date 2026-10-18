//! Running tests from the native apps: the platform layer, secrets and progress are callback
//! interfaces the app implements; [`TestRun`] runs a test on its own thread and controls it.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use jevision_core::{MouseButton, Point};
use jevision_runner::{
    PauseReason, RunControl, Runner, Screenshot, StepStatus, Verdict, VisionAnswer, VisionJudge,
};

use crate::vision::VisionSettings;

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
    fn open_app(&self, app: String) -> Result<(), HandsError>;
    /// Opens files or web addresses in `app`, or in the system default application.
    fn open_items(
        &self,
        items: Vec<Openable>,
        app: Option<String>,
        new_instance: bool,
    ) -> Result<(), HandsError>;
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
    ) -> Result<(), HandsError>;
    /// The text may hold secret values: never log it.
    fn type_text(&self, text: String) -> Result<(), HandsError>;
    fn press_keys(&self, keys: String) -> Result<(), HandsError>;
    /// Whether a person moved the mouse or typed since the last call; the object's own
    /// synthetic input does not count.
    fn user_input_detected(&self) -> bool;
}

/// Values for `${secret.NAME}`. Asked once per secret before the first block runs.
#[uniffi::export(with_foreign)]
pub trait SecretSource: Send + Sync {
    fn secret(&self, name: String) -> Option<String>;
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
    /// The model for checks by meaning; without one they fail with a clear message.
    pub vision: Option<VisionSettings>,
    /// Checks by meaning may only use a model on this Mac.
    pub local_only: bool,
}

/// One test run on a background thread. Control methods work from any thread and take effect
/// at once; a run waiting for the screen wakes up on [`cancel`](Self::cancel).
#[derive(uniffi::Object)]
pub struct TestRun {
    control: RunControl,
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
                let outcome = run(
                    &project,
                    &test,
                    options,
                    hands,
                    secrets,
                    &thread_listener,
                    control,
                );
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

fn run(
    project: &str,
    test: &str,
    options: RunOptions,
    hands: Arc<dyn Hands>,
    secrets: Arc<dyn SecretSource>,
    listener: &Arc<dyn RunListener>,
    control: RunControl,
) -> RunOutcome {
    let hands = HandsAdapter(hands);
    let secrets = SecretAdapter(secrets);
    let events = |e: &jevision_runner::Event| listener.event(e.clone().into());
    let vision = options.vision.and_then(|v| v.client().ok());
    let runner = Runner {
        hands: &hands,
        secrets: &secrets,
        events: &events,
        control,
        vision: vision.as_ref().map(|c| c as &dyn VisionJudge),
    };
    let options = jevision_runner::RunOptions {
        start_at: options.start_at,
        retries: options.retries,
        local_only: options.local_only,
        ..Default::default()
    };
    match runner.run(Path::new(project), Path::new(test), &options) {
        Ok(run) => {
            let html_path = run.dir.join("report.html");
            let html = std::fs::write(&html_path, jevision_runner::html(&run.report, &run.dir))
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
        Err(e) => RunOutcome::Failed {
            reason: e.to_string(),
        },
    }
}

struct HandsAdapter(Arc<dyn Hands>);

impl jevision_runner::Hands for HandsAdapter {
    fn open_app(&self, app: &str) -> Result<(), jevision_runner::HandsError> {
        Ok(self.0.open_app(app.to_owned())?)
    }

    fn open(
        &self,
        items: &[jevision_runner::Openable],
        app: Option<&str>,
        new_instance: bool,
    ) -> Result<(), jevision_runner::HandsError> {
        let items = items
            .iter()
            .map(|item| match item {
                jevision_runner::Openable::File(p) => Openable::File { path: path(p) },
                jevision_runner::Openable::Url(url) => Openable::Url { url: url.clone() },
            })
            .collect();
        Ok(self
            .0
            .open_items(items, app.map(str::to_owned), new_instance)?)
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
    ) -> Result<(), jevision_runner::HandsError> {
        Ok(self.0.click(app.to_owned(), at, button, double)?)
    }

    fn type_text(&self, text: &str) -> Result<(), jevision_runner::HandsError> {
        Ok(self.0.type_text(text.to_owned())?)
    }

    fn press_keys(&self, keys: &str) -> Result<(), jevision_runner::HandsError> {
        Ok(self.0.press_keys(keys.to_owned())?)
    }

    fn user_input_detected(&self) -> bool {
        self.0.user_input_detected()
    }
}

struct SecretAdapter(Arc<dyn SecretSource>);

impl jevision_runner::SecretSource for SecretAdapter {
    fn secret(&self, name: &str) -> Option<String> {
        self.0.secret(name.to_owned())
    }
}

fn path(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// How a run ended.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
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
    RunFinished {
        verdict: Verdict,
        flaky: bool,
        report: String,
    },
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
    pub attempts: Vec<Attempt>,
}

impl From<jevision_runner::RunReport> for RunReport {
    fn from(r: jevision_runner::RunReport) -> Self {
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
        }
    }
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Attempt {
    pub number: u32,
    pub verdict: Verdict,
    pub duration_ms: u64,
    pub failure: Option<Failure>,
    pub steps: Vec<Step>,
}

impl From<jevision_runner::Attempt> for Attempt {
    fn from(a: jevision_runner::Attempt) -> Self {
        Self {
            number: a.number,
            verdict: a.verdict,
            duration_ms: a.duration_ms,
            failure: a.failure.map(Into::into),
            steps: a.steps.into_iter().map(Into::into).collect(),
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
            vision: s.vision,
            branch: s.branch,
            failure: s.failure.map(Into::into),
        }
    }
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
}

#[uniffi::remote(Record)]
pub struct VisionAnswer {
    /// Provider name, such as `Anthropic`.
    pub provider: String,
    pub model: String,
    pub passed: bool,
    /// The model's explanation, in a sentence.
    pub reason: String,
}

#[uniffi::remote(Enum)]
pub enum Verdict {
    Passed,
    Failed,
    Cancelled,
}

#[uniffi::remote(Enum)]
pub enum StepStatus {
    Passed,
    Failed,
    BaselineCreated,
    Cancelled,
}

#[uniffi::remote(Enum)]
pub enum PauseReason {
    Requested,
    Step,
    Breakpoint,
    UserInput,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use std::sync::mpsc::{Receiver, Sender, channel};
    use std::time::{Duration, Instant};

    const SECRET: &str = "s3cr3t-pa55";

    #[derive(Default)]
    struct Fake {
        log: Mutex<Vec<String>>,
    }

    impl Hands for Fake {
        fn open_app(&self, app: String) -> Result<(), HandsError> {
            self.note(format!("open {app}"));
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
        fn click(&self, _: String, _: Point, _: MouseButton, _: bool) -> Result<(), HandsError> {
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
        fn user_input_detected(&self) -> bool {
            false
        }
    }

    impl Fake {
        fn note(&self, entry: String) {
            self.log.lock().unwrap().push(entry);
        }
    }

    struct Secrets;

    impl SecretSource for Secrets {
        fn secret(&self, name: String) -> Option<String> {
            (name == "PASSWORD").then(|| SECRET.to_owned())
        }
    }

    enum Message {
        Event(RunEvent),
        Finished(RunOutcome),
    }

    struct Listener(Mutex<Sender<Message>>);

    impl RunListener for Listener {
        fn event(&self, event: RunEvent) {
            let _ = self.0.lock().unwrap().send(Message::Event(event));
        }
        fn finished(&self, outcome: RunOutcome) {
            let _ = self.0.lock().unwrap().send(Message::Finished(outcome));
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
            let (tx, rx) = channel();
            run.start(
                path(&self.0),
                "t.flow.json".into(),
                RunOptions::default(),
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
                Ok(Message::Event(e)) => events.push(e),
                Ok(Message::Finished(outcome)) => return (events, outcome),
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
    fn cancel_ends_a_paused_run_at_once() {
        let p = Project::new(
            "cancel",
            r#""open":{"type":"openApp","next":"go"},"go":{"type":"pressKeys","keys":"return"}"#,
        );
        let run = TestRun::new();
        run.set_step_mode(true);
        let rx = p.start(&run, Arc::new(Fake::default()));
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Message::Event(RunEvent::RunStarted { .. })) => {}
            _ => panic!("expected RunStarted"),
        }
        let next = || match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Message::Event(e)) => e,
            _ => panic!("expected an event"),
        };
        assert_eq!(next(), RunEvent::AttemptStarted { attempt: 1 });
        assert_eq!(
            next(),
            RunEvent::Paused {
                block: "open".into(),
                reason: PauseReason::Step
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
}
