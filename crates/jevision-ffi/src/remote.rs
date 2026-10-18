//! Remote access from paired phones. The app turns it on, shows pairing codes and manages the
//! devices; a run a phone asks for starts through the app like any other, and every run on
//! this Mac is reported live to the phones watching it.

use std::path::Path;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use jevision_remote::{self as remote, Activity, ActivityKind, HostStatus, Relay, RemoteError};
use jevision_runner::{PauseReason, Verdict};

use crate::run::{Failure, RunEvent, RunListener, RunOutcome};

/// Time a run a phone asked for has to create its folder before the phone hears it failed.
const START_TIMEOUT: Duration = Duration::from_secs(30);

/// How the app sets up remote access.
#[derive(Debug, Clone, uniffi::Record)]
pub struct RemoteConfig {
    /// 32 bytes from [`generate_remote_key`], kept in the Keychain: phones know the Mac by it.
    pub secret_key: Vec<u8>,
    /// The Mac's name as phones show it.
    pub name: String,
    /// Folder for paired devices and the activity log.
    pub state_dir: String,
    pub relay: Relay,
    /// Find phones on the same Wi-Fi by mDNS.
    pub local_discovery: bool,
    /// Publish the Mac's current addresses, so phones find it after it changed networks.
    pub global_discovery: bool,
}

/// What a paired phone may do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum DevicePermission {
    /// Look at tests, runs and reports.
    View,
    /// Also start and stop runs.
    Run,
}

impl From<DevicePermission> for remote::Permission {
    fn from(p: DevicePermission) -> Self {
        match p {
            DevicePermission::View => Self::View,
            DevicePermission::Run => Self::Run,
        }
    }
}

impl From<remote::Permission> for DevicePermission {
    fn from(p: remote::Permission) -> Self {
        match p {
            remote::Permission::View => Self::View,
            remote::Permission::Run => Self::Run,
        }
    }
}

/// A phone paired with this Mac.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PairedDevice {
    pub id: String,
    /// The name the phone gave itself, such as `Anna's iPhone`.
    pub name: String,
    pub permission: DevicePermission,
    /// Seconds since 1970.
    pub paired_at: i64,
    /// When the phone last signed in, in seconds since 1970.
    pub last_seen: Option<i64>,
}

impl From<remote::Device> for PairedDevice {
    fn from(d: remote::Device) -> Self {
        Self {
            id: d.id,
            name: d.name,
            permission: d.permission.into(),
            paired_at: d.paired_at,
            last_seen: d.last_seen,
        }
    }
}

/// A pairing code to show as a QR code.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PairingCode {
    /// The text of the QR code.
    pub url: String,
    /// When the code stops working, in seconds since 1970.
    pub expires_at: i64,
}

/// A project phones may see, by name; its folder never leaves the Mac.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct RemoteProject {
    pub name: String,
    pub root: String,
}

/// What the app does for remote access. Calls come from background threads, never the main
/// one, and may wait for the main thread.
#[uniffi::export(with_foreign)]
pub trait RemoteDelegate: Send + Sync {
    /// Why a run can't start now: `MacLocked` while the screen is locked, `RunInProgress` while
    /// the app runs a test or waits to; `None` when it can.
    fn run_blocker(&self) -> Option<RemoteError>;
    /// Asks the person at the Mac whether `device` may run `test` of `project` (its name).
    /// Returns `false` when nobody answers in reasonable time.
    fn confirm_run(&self, device: PairedDevice, project: String, test: String) -> bool;
    /// Starts `test`, relative to `project_root`, the way the app starts any run, with the
    /// device's name as [`crate::RunOptions::requested_by`] and the listener from
    /// [`RemoteHost::run_listener`]. `data_row` runs only that row of the test's data table,
    /// counted from 0 and known to exist, as [`crate::RunOptions::data_rows`]; `None` runs
    /// every row. Returns once the run has started, or why it couldn't.
    fn start_run(
        &self,
        project_root: String,
        test: String,
        data_row: Option<u32>,
        requested_by: PairedDevice,
    ) -> Option<RemoteError>;
    /// Stops the run going on, as the emergency stop does.
    fn stop_run(&self);
}

/// A new secret key for the Mac's endpoint, to keep in the Keychain.
#[uniffi::export]
pub fn generate_remote_key() -> Vec<u8> {
    remote::generate_secret_key().to_vec()
}

/// Remote access on this Mac. Nothing listens until [`start`](Self::start).
#[derive(uniffi::Object)]
pub struct RemoteHost {
    host: Arc<remote::Host>,
    runs: Arc<Runs>,
    runtime: tokio::runtime::Runtime,
}

#[uniffi::export]
impl RemoteHost {
    /// Loads the paired devices from `config.state_dir`.
    #[uniffi::constructor]
    pub fn new(
        config: RemoteConfig,
        delegate: Arc<dyn RemoteDelegate>,
    ) -> Result<Arc<Self>, RemoteError> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("jevision-remote")
            .enable_all()
            .build()
            .map_err(|_| RemoteError::HostFailed)?;
        let runs = Arc::new(Runs::default());
        let bridge = Bridge {
            app: delegate,
            runs: Arc::clone(&runs),
        };
        let config = remote::HostConfig {
            secret_key: config.secret_key,
            name: config.name,
            state_dir: config.state_dir,
            network: remote::Network {
                relay: config.relay,
                local_discovery: config.local_discovery,
                global_discovery: config.global_discovery,
            },
        };
        let host = remote::Host::new(config, Arc::new(bridge))?;
        Ok(Arc::new(Self {
            host: Arc::new(host),
            runs,
            runtime,
        }))
    }

    /// Starts accepting phones; blocks until the network is set up.
    pub fn start(&self) -> Result<(), RemoteError> {
        self.runtime.block_on(self.host.start())
    }

    /// Disconnects every phone and stops accepting new ones.
    pub fn stop(&self) {
        self.runtime.block_on(self.host.stop());
    }

    pub fn status(&self) -> HostStatus {
        let _runtime = self.runtime.enter();
        self.host.status()
    }

    /// A new one-time pairing code for a phone that gets `permission`, working for five
    /// minutes. It replaces the code shown before.
    pub fn create_offer(&self, permission: DevicePermission) -> Result<PairingCode, RemoteError> {
        let _runtime = self.runtime.enter();
        let offer = self.host.create_offer(permission.into())?;
        Ok(PairingCode {
            url: offer.to_url(),
            expires_at: offer.expires_at,
        })
    }

    /// Makes the code shown stop working.
    pub fn cancel_offer(&self) {
        self.host.cancel_offer();
    }

    pub fn devices(&self) -> Vec<PairedDevice> {
        self.host.devices().into_iter().map(Into::into).collect()
    }

    pub fn set_permission(
        &self,
        device_id: String,
        permission: DevicePermission,
    ) -> Result<(), RemoteError> {
        self.host.set_permission(&device_id, permission.into())
    }

    /// Forgets the device and disconnects it at once.
    pub fn revoke(&self, device_id: String) -> Result<(), RemoteError> {
        let _runtime = self.runtime.enter();
        self.host.revoke(&device_id)
    }

    /// Whether the person at the Mac approves each run a phone asks for.
    pub fn set_confirm_runs(&self, confirm: bool) {
        self.host.set_confirm_runs(confirm);
    }

    /// The projects phones may see, replacing the previous list.
    pub fn set_projects(&self, projects: Vec<RemoteProject>) {
        self.host.set_projects(
            projects
                .into_iter()
                .map(|p| remote::Project {
                    name: p.name,
                    root: p.root,
                })
                .collect(),
        );
    }

    /// The newest `limit` entries of the activity log, newest first.
    pub fn activity(&self, limit: u32) -> Vec<Activity> {
        self.host.activity(limit)
    }

    /// Hears a run of the project at `project_root` (as given to
    /// [`set_projects`](Self::set_projects)) and reports it to the phones. Pass it every run
    /// the app starts, alongside the app's own listener.
    pub fn run_listener(&self, project_root: String) -> Arc<dyn RunListener> {
        Arc::new(RunReporter {
            host: Arc::clone(&self.host),
            runs: Arc::clone(&self.runs),
            root: project_root,
            run: Mutex::new(None),
        })
    }
}

impl Drop for RemoteHost {
    fn drop(&mut self) {
        self.runtime.block_on(self.host.stop());
    }
}

/// The run going on, shared by the host's requests and the run's listener.
#[derive(Default)]
struct Runs {
    state: Mutex<RunsState>,
    /// Held while a phone's run starts, so two phones can't start one each.
    starting: Mutex<()>,
}

#[derive(Default)]
struct RunsState {
    /// Project root and status of the run going on: the Mac runs one test at a time.
    active: Option<(String, remote::RunStatus)>,
    /// Hears the id of the run a phone asked for, once the run has its folder.
    waiting: Option<Sender<Result<String, RemoteError>>>,
}

impl Runs {
    fn state(&self) -> MutexGuard<'_, RunsState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn active(&self, root: &str, run: &str) -> Option<remote::RunStatus> {
        match &self.state().active {
            Some((r, status)) if r == root && status.run == run => Some(status.clone()),
            _ => None,
        }
    }
}

struct Bridge {
    app: Arc<dyn RemoteDelegate>,
    runs: Arc<Runs>,
}

impl remote::HostDelegate for Bridge {
    fn can_run(&self) -> Result<(), RemoteError> {
        if self.runs.state().active.is_some() {
            return Err(RemoteError::RunInProgress);
        }
        self.app.run_blocker().map_or(Ok(()), Err)
    }

    fn confirm_run(&self, device: remote::Device, project: String, test: String) -> bool {
        self.app.confirm_run(device.into(), project, test)
    }

    fn start_run(
        &self,
        project_root: String,
        test: String,
        row: Option<u32>,
        requested_by: remote::Device,
    ) -> Result<String, RemoteError> {
        let _one = self.runs.starting.lock().unwrap_or_else(|e| e.into_inner());
        let (tx, rx) = mpsc::channel();
        {
            let mut state = self.runs.state();
            if state.active.is_some() {
                return Err(RemoteError::RunInProgress);
            }
            state.waiting = Some(tx);
        }
        let refused = self
            .app
            .start_run(project_root, test, row, requested_by.into());
        let started = match refused {
            Some(e) => Err(e),
            None => rx
                .recv_timeout(START_TIMEOUT)
                .unwrap_or(Err(RemoteError::HostFailed)),
        };
        self.runs.state().waiting = None;
        started
    }

    fn stop_run(&self, project_root: String, run: String) -> Result<(), RemoteError> {
        self.runs
            .active(&project_root, &run)
            .ok_or(RemoteError::RunNotFound)?;
        self.app.stop_run();
        Ok(())
    }

    fn run_status(&self, project_root: String, run: String) -> Option<remote::RunStatus> {
        self.runs.active(&project_root, &run)
    }

    fn active_runs(&self, project_root: String) -> Vec<remote::RunStatus> {
        match &self.runs.state().active {
            Some((root, status)) if *root == project_root => vec![status.clone()],
            _ => Vec::new(),
        }
    }
}

/// Keeps the status of one run and passes its progress to the phones.
struct RunReporter {
    host: Arc<remote::Host>,
    runs: Arc<Runs>,
    root: String,
    /// The run's id, the name of its folder, once it has one.
    run: Mutex<Option<String>>,
}

impl RunReporter {
    fn run(&self) -> Option<String> {
        self.run.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Updates the run's status, if it is the one going on.
    fn update(&self, run: &str, change: impl FnOnce(&mut remote::RunStatus)) {
        if let Some((root, status)) = &mut self.runs.state().active
            && *root == self.root
            && status.run == run
        {
            change(status);
        }
    }

    /// Forgets the run going on, if it is this one, and tells the phones it has finished.
    fn end(&self, run: String, verdict: Verdict, flaky: bool) {
        {
            let mut state = self.runs.state();
            match &state.active {
                Some((root, status)) if *root == self.root && status.run == run => {
                    state.active = None;
                }
                _ => return,
            }
        }
        self.host.publish(
            self.root.clone(),
            run,
            remote::RunEvent::Finished { verdict, flaky },
        );
    }
}

impl RunListener for RunReporter {
    fn event(&self, event: RunEvent) {
        if let RunEvent::RunStarted { file, dir, .. } = &event {
            let run = Path::new(dir)
                .file_name()
                .map_or_else(|| dir.clone(), |n| n.to_string_lossy().into_owned());
            *self.run.lock().unwrap_or_else(|e| e.into_inner()) = Some(run.clone());
            let mut state = self.runs.state();
            state.active = Some((
                self.root.clone(),
                remote::RunStatus {
                    run: run.clone(),
                    test: file.clone(),
                    state: remote::RunState::Running,
                    verdict: None,
                    attempt: 0,
                    block: None,
                },
            ));
            if let Some(waiting) = state.waiting.take() {
                let _ = waiting.send(Ok(run));
            }
            return;
        }
        let Some(run) = self.run() else { return };
        let published = match event {
            RunEvent::RunStarted { .. } | RunEvent::RowStarted { .. } => return,
            RunEvent::AttemptStarted { attempt } => {
                self.update(&run, |s| {
                    s.attempt = attempt;
                    s.state = remote::RunState::Running;
                    s.block = None;
                });
                remote::RunEvent::AttemptStarted { attempt }
            }
            RunEvent::StepStarted {
                attempt,
                seq,
                test,
                block,
                block_type,
                ..
            } => {
                self.update(&run, |s| {
                    s.state = remote::RunState::Running;
                    if s.test == test {
                        s.block = Some(block.clone());
                    }
                });
                remote::RunEvent::BlockStarted {
                    attempt,
                    seq,
                    test,
                    block,
                    block_type,
                }
            }
            RunEvent::StepFinished { attempt, step } => remote::RunEvent::BlockFinished {
                attempt,
                seq: step.seq,
                test: step.test,
                block: step.block,
                status: step.status,
                duration_ms: step.duration_ms,
                failure: step.failure.map(failure),
                branch: step.branch,
                branches: step.branches,
                branch_condition: step.branch_condition,
            },
            RunEvent::Paused { block, reason, .. } => {
                self.update(&run, |s| {
                    s.state = remote::RunState::Paused;
                    s.block = Some(block.clone());
                });
                remote::RunEvent::Paused {
                    block,
                    reason: pause_reason(reason),
                }
            }
            RunEvent::Resumed => {
                self.update(&run, |s| s.state = remote::RunState::Running);
                remote::RunEvent::Resumed
            }
            RunEvent::AttemptFinished {
                attempt,
                verdict,
                failure: f,
            } => remote::RunEvent::AttemptFinished {
                attempt,
                verdict,
                failure: f.map(failure),
            },
            RunEvent::RunFinished { verdict, flaky, .. } => {
                self.end(run, verdict, flaky);
                return;
            }
        };
        self.host.publish(self.root.clone(), run, published);
    }

    fn finished(&self, _outcome: RunOutcome) {
        match self.run() {
            // The report could not be saved: the run still ends for the phones.
            Some(run) => self.end(run, Verdict::Failed, false),
            None => {
                if let Some(waiting) = self.runs.state().waiting.take() {
                    let _ = waiting.send(Err(RemoteError::HostFailed));
                }
            }
        }
    }
}

fn failure(f: Failure) -> jevision_runner::Failure {
    jevision_runner::Failure {
        code: f.code,
        params: f.params.into_iter().collect(),
    }
}

fn pause_reason(reason: PauseReason) -> remote::PauseReason {
    match reason {
        PauseReason::Requested => remote::PauseReason::Requested,
        PauseReason::Step => remote::PauseReason::Step,
        PauseReason::Breakpoint => remote::PauseReason::Breakpoint,
        PauseReason::UserInput => remote::PauseReason::UserInput,
    }
}

#[uniffi::remote(Enum)]
pub enum Relay {
    /// The public relays run by n0.
    Default,
    /// A relay the user runs, such as `https://relay.example.com`.
    Custom { url: String },
    /// Direct connections only: same network or VPN.
    Disabled,
}

#[uniffi::remote(Record)]
pub struct HostStatus {
    pub running: bool,
    /// The Mac's endpoint id, hex.
    pub endpoint_id: String,
    /// The home relay, once one is chosen.
    pub relay_url: Option<String>,
    /// Whether the home relay is connected; phones on other networks need it.
    pub relay_connected: bool,
    /// Direct addresses phones on the same network or VPN can use.
    pub addrs: Vec<String>,
    /// Ids of the devices connected now.
    pub connected: Vec<String>,
}

#[uniffi::remote(Record)]
pub struct Activity {
    /// Seconds since 1970.
    pub time: i64,
    pub device_id: Option<String>,
    pub device_name: Option<String>,
    pub kind: ActivityKind,
    /// What it was about: the project and test or run, `project/path`.
    pub subject: Option<String>,
    /// Why it failed; `None` when it succeeded.
    pub error: Option<RemoteError>,
}

#[uniffi::remote(Enum)]
pub enum ActivityKind {
    Paired,
    PairingFailed,
    SignedIn,
    SignInFailed,
    RunStarted,
    RunStopped,
    ReportRead,
    Denied,
    PermissionChanged,
    Revoked,
}

/// Why something about remote access didn't happen; the app shows the text of its
/// `remote.error.<code>` key.
#[uniffi::remote(Error)]
pub enum RemoteError {
    PairingCodeInvalid,
    PairingCodeExpired,
    PairingCodeUsed,
    PairingLocked,
    BadSignature,
    NotPaired,
    Revoked,
    NotAllowed,
    MacLocked,
    RunInProgress,
    DeclinedOnMac,
    HostOffline,
    ProjectNotFound,
    TestNotFound,
    TestUnreadable,
    RunNotFound,
    PictureNotFound,
    PathOutsideProject,
    TooLarge,
    Protocol,
    OfferWrongScheme,
    OfferMalformed,
    InvalidSecretKey,
    InvalidRelayUrl,
    SignerFailed,
    Network,
    HostNotRunning,
    Storage,
    HostFailed,
    DataRowNotFound,
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::{OnceLock, Weak};
    use std::time::Instant;

    use jevision_remote::{Client, DeviceSigner, PairingOffer, RunUpdate};
    use p256::ecdsa::signature::Signer;
    use p256::ecdsa::{Signature, SigningKey};

    use super::*;
    use crate::run::tests::{Fake, Secrets};
    use crate::{DataRowRange, RunOptions, TestRun};

    struct Phone(SigningKey);

    impl DeviceSigner for Phone {
        fn public_key(&self) -> Vec<u8> {
            self.0.verifying_key().to_sec1_bytes().to_vec()
        }

        fn sign(&self, message: Vec<u8>) -> Result<Vec<u8>, RemoteError> {
            let signature: Signature = self.0.sign(&message);
            Ok(signature.to_der().as_bytes().to_vec())
        }
    }

    /// The app's side without the app: runs start in step mode with fake hands.
    #[derive(Default)]
    struct App {
        host: OnceLock<Weak<RemoteHost>>,
        run: Mutex<Option<Arc<TestRun>>>,
        asked: Mutex<Vec<(String, String, String)>>,
    }

    impl App {
        fn run(&self) -> Arc<TestRun> {
            self.run.lock().unwrap().clone().expect("a run started")
        }
    }

    impl RemoteDelegate for App {
        fn run_blocker(&self) -> Option<RemoteError> {
            None
        }

        fn confirm_run(&self, device: PairedDevice, project: String, test: String) -> bool {
            self.asked
                .lock()
                .unwrap()
                .push((device.name, project, test));
            true
        }

        fn start_run(
            &self,
            project_root: String,
            test: String,
            data_row: Option<u32>,
            requested_by: PairedDevice,
        ) -> Option<RemoteError> {
            let host = self.host.get()?.upgrade()?;
            let run = Arc::new(TestRun::new());
            run.set_step_mode(true);
            run.start(
                project_root.clone(),
                test,
                RunOptions {
                    requested_by: Some(requested_by.name),
                    data_rows: data_row.map(|row| DataRowRange {
                        first: row,
                        last: row,
                    }),
                    ..RunOptions::default()
                },
                Arc::new(Fake::default()),
                Arc::new(Secrets),
                host.run_listener(project_root),
            );
            *self.run.lock().unwrap() = Some(run);
            None
        }

        fn stop_run(&self) {
            self.run().cancel();
        }
    }

    struct Scratch(PathBuf);

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn offline() -> remote::Network {
        remote::Network {
            relay: Relay::Disabled,
            local_discovery: false,
            global_discovery: false,
        }
    }

    /// The host's port on 127.0.0.1: a test can't answer macOS's local network prompt.
    fn loopback(addrs: &[String]) -> Vec<String> {
        addrs
            .iter()
            .filter_map(|a| a.parse::<std::net::SocketAddr>().ok())
            .filter(|a| a.is_ipv4())
            .map(|a| format!("127.0.0.1:{}", a.port()))
            .collect()
    }

    #[test]
    fn a_phone_runs_a_test_through_the_app_and_watches_it_live() {
        let scratch = Scratch(
            std::env::temp_dir().join(format!("jevision-ffi-remote-{}", std::process::id())),
        );
        let project = scratch.0.join("project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("t.flow.json"),
            r#"{"format":1,"name":"T","app":"com.example.App","start":"open","blocks":{
                "open":{"type":"openApp","next":"go"},"go":{"type":"pressKeys","keys":"return"}}}"#,
        )
        .unwrap();
        std::fs::write(
            project.join("rows.flow.json"),
            r#"{"format":1,"name":"Rows","app":"com.example.App","start":"rows","blocks":{
                "rows":{"type":"useData","file":"people.csv","body":"type"},
                "type":{"type":"type","text":"${data.name}","next":"rows"}}}"#,
        )
        .unwrap();
        std::fs::write(project.join("people.csv"), "name\nAnn\nBo\n").unwrap();
        let root = project.to_string_lossy().into_owned();

        let app = Arc::new(App::default());
        let host = RemoteHost::new(
            RemoteConfig {
                secret_key: generate_remote_key(),
                name: "Studio Mac".into(),
                state_dir: scratch.0.join("state").to_string_lossy().into_owned(),
                relay: Relay::Disabled,
                local_discovery: false,
                global_discovery: false,
            },
            app.clone(),
        )
        .unwrap();
        app.host.set(Arc::downgrade(&host)).unwrap();
        host.set_projects(vec![RemoteProject {
            name: "Demo".into(),
            root: root.clone(),
        }]);
        host.set_confirm_runs(true);
        host.start().unwrap();
        assert!(host.status().running);

        let code = host.create_offer(DevicePermission::Run).unwrap();
        let mut offer = PairingOffer::parse(&code.url).unwrap();
        assert_eq!(offer.expires_at, code.expires_at);
        offer.addrs = loopback(&offer.addrs);
        let phone = tokio::runtime::Runtime::new().unwrap();
        let signer = Arc::new(Phone(SigningKey::from_slice(&[7; 32]).unwrap()));
        let client = phone
            .block_on(Client::pair(
                offer,
                "Anna's iPhone".into(),
                signer,
                offline(),
            ))
            .unwrap();
        let devices = host.devices();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].name, "Anna's iPhone");
        assert_eq!(devices[0].permission, DevicePermission::Run);

        let run = phone
            .block_on(client.run_test("Demo".into(), "t.flow.json".into(), None))
            .unwrap();
        assert_eq!(
            *app.asked.lock().unwrap(),
            [(
                "Anna's iPhone".to_owned(),
                "Demo".to_owned(),
                "t.flow.json".to_owned()
            )]
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        while !app.run().is_paused() {
            assert!(Instant::now() < deadline, "the run never paused");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            phone.block_on(client.run_test("Demo".into(), "t.flow.json".into(), None)),
            Err(RemoteError::RunInProgress)
        );

        let watch = phone
            .block_on(client.watch("Demo".into(), run.clone()))
            .unwrap();
        let Ok(Some(RunUpdate::Status { status })) = phone.block_on(watch.next()) else {
            panic!("the first update is the status");
        };
        assert_eq!(status.test, "t.flow.json");
        assert_eq!(status.state, remote::RunState::Paused);
        assert_eq!(status.block.as_deref(), Some("open"));

        app.run().resume();
        let mut events = Vec::new();
        while let Some(update) = phone.block_on(watch.next()).unwrap() {
            let RunUpdate::Event { event } = update else {
                panic!("fell behind: {update:?}");
            };
            events.push(event);
        }
        let blocks: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                remote::RunEvent::BlockFinished { block, status, .. } => {
                    Some((block.as_str(), *status))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            blocks,
            [
                ("open", jevision_runner::StepStatus::Passed),
                ("go", jevision_runner::StepStatus::Passed)
            ]
        );
        assert_eq!(events.first(), Some(&remote::RunEvent::Resumed));
        assert_eq!(
            events.last(),
            Some(&remote::RunEvent::Finished {
                verdict: Verdict::Passed,
                flaky: false
            })
        );

        // Finished runs answer from their saved report, which says who asked for them.
        let report = phone
            .block_on(client.report("Demo".into(), run.clone()))
            .unwrap();
        assert_eq!(report.requested_by.as_deref(), Some("Anna's iPhone"));
        let runs = phone.block_on(client.list_runs("Demo".into())).unwrap();
        assert!(runs.active.is_empty());
        assert_eq!(runs.saved[0].run, run);
        assert!(
            host.activity(10)
                .iter()
                .any(|a| a.kind == ActivityKind::RunStarted)
        );

        // One row of a data table, picked on the phone.
        let rows = phone
            .block_on(client.get_test("Demo".into(), "rows.flow.json".into()))
            .unwrap()
            .rows;
        assert_eq!(rows, ["Ann", "Bo"]);
        let run = phone
            .block_on(client.run_test("Demo".into(), "rows.flow.json".into(), Some(1)))
            .unwrap();
        let watch = phone
            .block_on(client.watch("Demo".into(), run.clone()))
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !app.run().is_paused() {
            assert!(Instant::now() < deadline, "the run never paused");
            std::thread::sleep(Duration::from_millis(10));
        }
        app.run().resume();
        while phone.block_on(watch.next()).unwrap().is_some() {}
        let report = phone
            .block_on(client.report("Demo".into(), run.clone()))
            .unwrap();
        assert_eq!(report.verdict, Verdict::Passed);
        let ran: Vec<_> = report.attempts.iter().map(|a| a.row.clone()).collect();
        assert_eq!(
            ran,
            [Some(jevision_runner::DataRow {
                number: 2,
                label: "Bo".into()
            })]
        );

        host.revoke(devices[0].id.clone()).unwrap();
        assert!(host.devices().is_empty());
        phone.block_on(client.close());
    }
}
