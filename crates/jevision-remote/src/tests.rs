use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use iroh::{Endpoint, EndpointAddr, EndpointId, SecretKey};
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use serde_json::{Value, json};

use super::*;
use crate::pairing::{code_bytes, pair_transcript};
use crate::store::Store;
use crate::wire::{PROTOCOLS, Protocols};

const EXAMPLE: &str = include_str!("../../../examples/sign-in.flow.json");
const RUN: &str = "20261018-120000-sign-in";
const PICTURE: &[u8] = b"\x89PNG\r\n\x1a\nnot really a picture";

/// A scratch folder, removed with everything in it at the end of the test.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("jevision-remote-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(fs::canonicalize(dir).unwrap())
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A phone key in software, standing in for the Secure Enclave.
struct TestSigner(SigningKey);

impl TestSigner {
    fn new(seed: u8) -> Arc<Self> {
        Arc::new(Self(SigningKey::from_slice(&[seed; 32]).unwrap()))
    }
}

impl DeviceSigner for TestSigner {
    fn public_key(&self) -> Vec<u8> {
        self.0.verifying_key().to_sec1_bytes().to_vec()
    }

    fn sign(&self, message: Vec<u8>) -> Result<Vec<u8>, RemoteError> {
        let signature: Signature = self.0.sign(&message);
        Ok(signature.to_der().as_bytes().to_vec())
    }
}

/// Claims another phone's public key but cannot sign for it.
struct Impostor(Arc<TestSigner>, Arc<TestSigner>);

impl DeviceSigner for Impostor {
    fn public_key(&self) -> Vec<u8> {
        self.0.public_key()
    }

    fn sign(&self, message: Vec<u8>) -> Result<Vec<u8>, RemoteError> {
        self.1.sign(message)
    }
}

/// A run the Mac was asked to start: project root, test, data row and device.
type Started = (String, String, Option<u32>, String);

/// A tool an assistant called: session, device name, project root, tool and arguments.
type Called = (String, String, String, String, Value);

#[derive(Default)]
struct Fake {
    locked: Mutex<bool>,
    busy: Mutex<bool>,
    confirm: Mutex<bool>,
    asked: Mutex<Vec<(String, String, String)>>,
    started: Mutex<Vec<Started>>,
    stopped: Mutex<Vec<String>>,
    active: Mutex<HashMap<String, RunStatus>>,
    hellos: Mutex<Vec<Value>>,
    called: Mutex<Vec<Called>>,
    ended: Mutex<Vec<String>>,
}

impl HostDelegate for Fake {
    fn can_run(&self) -> Result<(), RemoteError> {
        if *self.locked.lock().unwrap() {
            Err(RemoteError::MacLocked)
        } else if *self.busy.lock().unwrap() {
            Err(RemoteError::RunInProgress)
        } else {
            Ok(())
        }
    }

    fn confirm_run(&self, device: Device, project: String, test: String) -> bool {
        self.asked
            .lock()
            .unwrap()
            .push((device.name, project, test));
        *self.confirm.lock().unwrap()
    }

    fn start_run(
        &self,
        project_root: String,
        test: String,
        row: Option<u32>,
        requested_by: Device,
    ) -> Result<String, RemoteError> {
        let run = "20261018-130000-live".to_owned();
        self.active.lock().unwrap().insert(
            run.clone(),
            RunStatus {
                run: run.clone(),
                test: test.clone(),
                state: RunState::Running,
                verdict: None,
                attempt: 1,
                block: Some("open".into()),
                waiting: None,
            },
        );
        self.started
            .lock()
            .unwrap()
            .push((project_root, test, row, requested_by.name));
        Ok(run)
    }

    fn stop_run(&self, _project_root: String, run: String) -> Result<(), RemoteError> {
        self.stopped.lock().unwrap().push(run);
        Ok(())
    }

    fn run_status(&self, _project_root: String, run: String) -> Option<RunStatus> {
        self.active.lock().unwrap().get(&run).cloned()
    }

    fn active_runs(&self, _project_root: String) -> Vec<RunStatus> {
        self.active.lock().unwrap().values().cloned().collect()
    }

    fn assistant_hello(&self, _session: String, _device: Device, params: Value) -> Value {
        self.hellos.lock().unwrap().push(params);
        json!({ "serverInfo": { "name": "jevision" } })
    }

    fn assistant_tools(&self) -> Value {
        json!({ "tools": [{ "name": "list_tests" }] })
    }

    fn assistant_call(
        &self,
        session: String,
        device: Device,
        project_root: String,
        tool: String,
        arguments: Value,
    ) -> Result<Value, AssistantRejection> {
        self.called.lock().unwrap().push((
            session,
            device.name,
            project_root,
            tool.clone(),
            arguments,
        ));
        match tool.as_str() {
            "nothing" => Err(AssistantRejection {
                code: -32602,
                message: "Unknown tool: nothing".into(),
            }),
            "big" => Ok(json!({
                "content": [{ "type": "text", "text": "x".repeat(wire::MAX_MESSAGE + 1) }],
                "isError": false,
            })),
            _ => Ok(json!({ "content": [{ "type": "text", "text": "done" }], "isError": false })),
        }
    }

    fn assistant_ended(&self, session: String) {
        self.ended.lock().unwrap().push(session);
    }
}

/// Direct connections on this computer only: no relays, no discovery services.
fn offline() -> Network {
    Network {
        relay: Relay::Disabled,
        local_discovery: false,
        global_discovery: false,
    }
}

struct Setup {
    host: Host,
    fake: Arc<Fake>,
    scratch: Scratch,
    key: u8,
}

impl Setup {
    async fn new(name: &str) -> Self {
        Self::keyed(name, 42).await
    }

    /// A Mac of its own endpoint id, for what the phone keeps per Mac for the whole process.
    async fn keyed(name: &str, key: u8) -> Self {
        let scratch = Scratch::new(name);
        let project = scratch.0.join("project");
        write(&project, "login/sign-in.flow.json", EXAMPLE);
        write(
            &project,
            "smoke.flow.json",
            EXAMPLE
                .replace("Sign in with valid credentials", "Smoke")
                .as_str(),
        );
        write(&project, "notes.txt", "not a test");
        write(&project, ".hidden/secret.flow.json", EXAMPLE);
        write(&scratch.0, "outside.flow.json", EXAMPLE);
        std::os::unix::fs::symlink(
            scratch.0.join("outside.flow.json"),
            project.join("escape.flow.json"),
        )
        .unwrap();
        let run = project.join(".jevision/runs").join(RUN);
        write(&run, "report.json", REPORT);
        fs::create_dir_all(run.join("shots")).unwrap();
        fs::write(run.join("shots/1-after.png"), PICTURE).unwrap();
        write(&scratch.0, "stolen.png", "outside");

        let fake = Arc::new(Fake::default());
        let host = Host::new(keyed(&scratch, key), fake.clone()).unwrap();
        host.set_projects(vec![Project {
            name: "Demo".into(),
            root: project.to_string_lossy().into_owned(),
        }]);
        host.start().await.unwrap();
        Self {
            host,
            fake,
            scratch,
            key,
        }
    }

    /// Quits the Mac and starts it again on what it saved.
    async fn restart(&mut self) {
        self.host.stop().await;
        self.host = Host::new(keyed(&self.scratch, self.key), self.fake.clone()).unwrap();
        self.host.set_projects(vec![Project {
            name: "Demo".into(),
            root: self.root(),
        }]);
        self.host.start().await.unwrap();
    }

    fn root(&self) -> String {
        self.scratch
            .0
            .join("project")
            .to_string_lossy()
            .into_owned()
    }

    /// A fresh offer that points at this computer's loopback address.
    fn offer(&self, permission: Permission) -> PairingOffer {
        let mut offer = self.host.create_offer(permission).unwrap();
        offer.addrs = loopback(&offer.addrs);
        offer
    }

    async fn pair(&self, permission: Permission, signer: Arc<TestSigner>) -> Client {
        Client::pair(
            self.offer(permission),
            "Anna's iPhone".into(),
            signer,
            offline(),
        )
        .await
        .unwrap()
    }

    /// The link an assistant is paired with, read the way the terminal reads it.
    fn assistant_offer(&self) -> PairingOffer {
        let mut offer = self.host.create_assistant_offer().unwrap();
        offer.addrs = loopback(&offer.addrs);
        PairingOffer::parse_assistant(&offer.to_assistant_url()).unwrap()
    }

    /// The Mac as a phone paired with it saves it, pointed at loopback like [`Setup::offer`].
    fn record(&self) -> HostRecord {
        let status = self.host.status();
        HostRecord {
            host_id: status.endpoint_id,
            name: "Studio Mac".into(),
            device_id: String::new(),
            relay_url: None,
            addrs: loopback(&status.addrs),
        }
    }

    async fn pair_assistant(&self, signer: Arc<TestSigner>) -> AssistantClient {
        AssistantClient::pair(
            self.assistant_offer(),
            "Dev laptop".into(),
            signer,
            offline(),
        )
        .await
        .unwrap()
    }
}

/// The host's own port on 127.0.0.1. macOS asks before a program reaches other devices on
/// the local network, which a test cannot answer; loopback needs no permission.
fn loopback(addrs: &[String]) -> Vec<String> {
    addrs
        .iter()
        .filter_map(|a| a.parse::<std::net::SocketAddr>().ok())
        .filter(|a| a.is_ipv4())
        .map(|a| format!("127.0.0.1:{}", a.port()))
        .collect()
}

/// What the phone saved, pointed at loopback like [`Setup::offer`].
fn saved(client: &Client) -> HostRecord {
    let mut record = client.record();
    record.addrs = loopback(&record.addrs);
    record
}

/// What an assistant saved, pointed at loopback like [`Setup::offer`].
fn assistant_saved(client: &AssistantClient) -> HostRecord {
    let mut record = client.record();
    record.addrs = loopback(&record.addrs);
    record
}

fn config(scratch: &Scratch) -> HostConfig {
    HostConfig {
        secret_key: [42; 32].to_vec(),
        name: "Studio Mac".into(),
        state_dir: scratch.0.join("state").to_string_lossy().into_owned(),
        network: offline(),
    }
}

fn keyed(scratch: &Scratch, key: u8) -> HostConfig {
    HostConfig {
        secret_key: [key; 32].to_vec(),
        ..config(scratch)
    }
}

fn write(dir: &Path, path: &str, text: &str) {
    let file = dir.join(path);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, text).unwrap();
}

const REPORT: &str = r#"{"format":1,"test":"Sign in with valid credentials","file":"login/sign-in.flow.json",
          "app":"com.example.AcmeMail","startedAt":"2026-10-18T12:00:00.000Z","durationMs":900,
          "verdict":"failed","flaky":false,"attempts":[{"number":1,"verdict":"failed","durationMs":900,
          "steps":[{"seq":1,"test":"login/sign-in.flow.json","block":"open","type":"openApp",
          "status":"failed","durationMs":900,"after":"shots/1-after.png",
          "failure":{"code":"run.target.notFound","params":{"block":"open","image":"x.png"}}}]}]}"#;

#[tokio::test(flavor = "multi_thread")]
async fn pairs_and_lists_projects() {
    let setup = Setup::new("pairs").await;
    let client = setup.pair(Permission::Run, TestSigner::new(1)).await;

    let devices = setup.host.devices();
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].name, "Anna's iPhone");
    assert_eq!(devices[0].permission, Permission::Run);
    assert_eq!(devices[0].public_key, hex(&TestSigner::new(1).public_key()));
    let record = client.record();
    assert_eq!(record.device_id, devices[0].id);
    assert_eq!(record.name, "Studio Mac");
    assert_eq!(client.permission(), Permission::Run);
    assert_eq!(
        client.list_projects().await.unwrap(),
        vec![ProjectInfo {
            name: "Demo".into()
        }]
    );
    assert_eq!(setup.host.status().connected, vec![devices[0].id.clone()]);
    assert_eq!(setup.host.activity(10)[0].kind, ActivityKind::Paired);
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_wrong_expired_and_used_codes() {
    let setup = Setup::new("codes").await;
    let offer = setup.offer(Permission::View);

    let mut wrong = offer.clone();
    wrong.code = "00".repeat(16);
    let refused = Client::pair(wrong, "Mallory".into(), TestSigner::new(9), offline()).await;
    assert_eq!(refused.err(), Some(RemoteError::PairingCodeInvalid));

    setup
        .host
        .with_offer(|o| o.expires = Instant::now() - Duration::from_secs(1));
    let expired = Client::pair(offer.clone(), "Anna".into(), TestSigner::new(1), offline()).await;
    assert_eq!(expired.err(), Some(RemoteError::PairingCodeExpired));

    let offer = setup.offer(Permission::View);
    Client::pair(offer.clone(), "Anna".into(), TestSigner::new(1), offline())
        .await
        .unwrap();
    let reused = Client::pair(
        offer.clone(),
        "Mallory".into(),
        TestSigner::new(9),
        offline(),
    )
    .await;
    assert_eq!(reused.err(), Some(RemoteError::PairingCodeUsed));

    setup.host.cancel_offer();
    let cancelled = Client::pair(offer, "Mallory".into(), TestSigner::new(9), offline()).await;
    assert_eq!(cancelled.err(), Some(RemoteError::PairingCodeInvalid));
    assert_eq!(setup.host.devices().len(), 1);
    let failed = setup
        .host
        .activity(10)
        .iter()
        .filter(|a| a.kind == ActivityKind::PairingFailed)
        .count();
    assert_eq!(failed, 4);
}

#[tokio::test(flavor = "multi_thread")]
async fn locks_pairing_after_five_failures() {
    let setup = Setup::new("lockout").await;
    let offer = setup.offer(Permission::Run);
    for i in 0..5 {
        let mut guess = offer.clone();
        guess.code = format!("{i:032x}");
        let refused = Client::pair(guess, "Mallory".into(), TestSigner::new(9), offline()).await;
        assert_eq!(refused.err(), Some(RemoteError::PairingCodeInvalid));
    }
    let locked = Client::pair(offer, "Anna".into(), TestSigner::new(1), offline()).await;
    assert_eq!(locked.err(), Some(RemoteError::PairingLocked));
    assert!(setup.host.devices().is_empty());

    // A new code starts over.
    setup.pair(Permission::Run, TestSigner::new(1)).await;
    assert_eq!(setup.host.devices().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_a_pairing_signed_with_another_key() {
    let setup = Setup::new("forged").await;
    let offer = setup.offer(Permission::Run);
    let impostor = Arc::new(Impostor(TestSigner::new(1), TestSigner::new(2)));
    let refused = Client::pair(offer, "Mallory".into(), impostor, offline()).await;
    assert_eq!(refused.err(), Some(RemoteError::BadSignature));
    assert!(setup.host.devices().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn signs_in_with_a_stored_device() {
    let setup = Setup::new("signin").await;
    let paired = setup.pair(Permission::View, TestSigner::new(1)).await;
    let record = saved(&paired);
    paired.close().await;

    let saved = serde_json::to_string(&record).unwrap();
    let record: HostRecord = serde_json::from_str(&saved).unwrap();
    let client = Client::connect(record, TestSigner::new(1), offline())
        .await
        .unwrap();
    assert_eq!(client.permission(), Permission::View);
    assert_eq!(client.list_projects().await.unwrap().len(), 1);
    assert!(setup.host.devices()[0].last_seen.is_some());
    assert_eq!(setup.host.activity(1)[0].kind, ActivityKind::SignedIn);
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_unknown_devices() {
    let setup = Setup::new("unknown").await;
    let paired = setup.pair(Permission::Run, TestSigner::new(1)).await;
    let record = saved(&paired);

    let stranger = Client::connect(record.clone(), TestSigner::new(2), offline()).await;
    assert_eq!(stranger.err(), Some(RemoteError::NotPaired));
    let impostor = Arc::new(Impostor(TestSigner::new(1), TestSigner::new(2)));
    let forged = Client::connect(record, impostor, offline()).await;
    assert_eq!(forged.err(), Some(RemoteError::BadSignature));
    assert_eq!(setup.host.activity(1)[0].kind, ActivityKind::SignInFailed);
}

#[tokio::test(flavor = "multi_thread")]
async fn revoking_closes_live_connections() {
    let setup = Setup::new("revoke").await;
    let client = setup.pair(Permission::Run, TestSigner::new(1)).await;
    let run = client
        .run_test("Demo".into(), "smoke.flow.json".into(), None)
        .await
        .unwrap();
    let watch = client.watch("Demo".into(), run).await.unwrap();
    assert!(matches!(
        watch.next().await,
        Ok(Some(RunUpdate::Status { .. }))
    ));

    let id = client.record().device_id;
    setup.host.revoke(&id).unwrap();
    assert_eq!(watch.next().await, Err(RemoteError::Revoked));
    assert_eq!(client.list_projects().await, Err(RemoteError::Revoked));
    assert!(setup.host.devices().is_empty());
    assert!(setup.host.status().connected.is_empty());

    let again = Client::connect(saved(&client), TestSigner::new(1), offline()).await;
    assert_eq!(again.err(), Some(RemoteError::NotPaired));
}

#[tokio::test(flavor = "multi_thread")]
async fn view_devices_cannot_run_until_allowed() {
    let setup = Setup::new("view").await;
    let client = setup.pair(Permission::View, TestSigner::new(1)).await;
    let refused = client
        .run_test("Demo".into(), "smoke.flow.json".into(), None)
        .await;
    assert_eq!(refused, Err(RemoteError::NotAllowed));
    let stop = client.stop_run("Demo".into(), RUN.into()).await;
    assert_eq!(stop, Err(RemoteError::NotAllowed));
    assert!(setup.fake.started.lock().unwrap().is_empty());
    let entry = &setup.host.activity(1)[0];
    assert_eq!(entry.kind, ActivityKind::RunStopped);
    assert_eq!(entry.error, Some(RemoteError::NotAllowed));

    setup
        .host
        .set_permission(&client.record().device_id, Permission::Run)
        .unwrap();
    client
        .run_test("Demo".into(), "smoke.flow.json".into(), None)
        .await
        .unwrap();
    let started = setup.fake.started.lock().unwrap().clone();
    assert_eq!(
        started,
        vec![(
            setup.root(),
            "smoke.flow.json".into(),
            None,
            "Anna's iPhone".into()
        )]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_paths_outside_the_project() {
    let setup = Setup::new("paths").await;
    let client = setup.pair(Permission::Run, TestSigner::new(1)).await;
    let demo = || "Demo".to_owned();
    for path in [
        "../outside.flow.json",
        "login/../../outside.flow.json",
        "/etc/passwd.flow.json",
        "escape.flow.json",
    ] {
        assert_eq!(
            client.get_test(demo(), path.into()).await,
            Err(RemoteError::PathOutsideProject),
            "{path}"
        );
        assert_eq!(
            client.run_test(demo(), path.into(), None).await,
            Err(RemoteError::PathOutsideProject),
            "{path}"
        );
    }
    assert_eq!(
        client.report(demo(), "../..".into()).await,
        Err(RemoteError::PathOutsideProject)
    );
    assert_eq!(
        client.report(demo(), format!("{RUN}/../{RUN}")).await,
        Err(RemoteError::PathOutsideProject)
    );
    assert_eq!(
        client
            .picture(demo(), RUN.into(), "../../../../../stolen.png".into())
            .await,
        Err(RemoteError::PathOutsideProject)
    );
    assert_eq!(
        client
            .picture(demo(), RUN.into(), "report.json".into())
            .await,
        Err(RemoteError::PictureNotFound)
    );
    assert_eq!(
        client.list_tests("Elsewhere".into()).await,
        Err(RemoteError::ProjectNotFound)
    );
    assert!(setup.fake.started.lock().unwrap().is_empty());
    assert_eq!(setup.host.activity(1)[0].kind, ActivityKind::Denied);
}

#[tokio::test(flavor = "multi_thread")]
async fn runs_wait_for_confirmation_on_the_mac() {
    let setup = Setup::new("confirm").await;
    let client = setup.pair(Permission::Run, TestSigner::new(1)).await;
    setup.host.set_confirm_runs(true);

    let declined = client
        .run_test("Demo".into(), "smoke.flow.json".into(), None)
        .await;
    assert_eq!(declined, Err(RemoteError::DeclinedOnMac));
    assert!(setup.fake.started.lock().unwrap().is_empty());

    *setup.fake.confirm.lock().unwrap() = true;
    client
        .run_test("Demo".into(), "smoke.flow.json".into(), None)
        .await
        .unwrap();
    let asked = setup.fake.asked.lock().unwrap().clone();
    let question = (
        "Anna's iPhone".to_owned(),
        "Demo".to_owned(),
        "smoke.flow.json".to_owned(),
    );
    assert_eq!(asked, vec![question.clone(), question]);
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_runs_while_the_mac_is_locked_or_busy() {
    let setup = Setup::new("locked").await;
    let client = setup.pair(Permission::Run, TestSigner::new(1)).await;
    *setup.fake.locked.lock().unwrap() = true;
    let locked = client
        .run_test("Demo".into(), "smoke.flow.json".into(), None)
        .await;
    assert_eq!(locked, Err(RemoteError::MacLocked));
    *setup.fake.locked.lock().unwrap() = false;
    *setup.fake.busy.lock().unwrap() = true;
    let busy = client
        .run_test("Demo".into(), "smoke.flow.json".into(), None)
        .await;
    assert_eq!(busy, Err(RemoteError::RunInProgress));
    assert!(setup.fake.started.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn streams_run_events_to_watchers() {
    let setup = Setup::new("events").await;
    let client = setup.pair(Permission::Run, TestSigner::new(1)).await;
    let run = client
        .run_test("Demo".into(), "smoke.flow.json".into(), None)
        .await
        .unwrap();
    let watch = client.watch("Demo".into(), run.clone()).await.unwrap();
    let Ok(Some(RunUpdate::Status { status })) = watch.next().await else {
        panic!("the first update is the status");
    };
    assert_eq!(status.state, RunState::Running);
    assert_eq!(status.test, "smoke.flow.json");

    let events = vec![
        RunEvent::BlockStarted {
            attempt: 1,
            seq: 1,
            test: "smoke.flow.json".into(),
            block: "open".into(),
            block_type: "openApp".into(),
            title: Some("com.example.Mail".into()),
        },
        RunEvent::BlockFinished {
            attempt: 1,
            seq: 1,
            test: "smoke.flow.json".into(),
            block: "open".into(),
            status: StepStatus::Passed,
            duration_ms: 120,
            failure: None,
            branch: None,
            branches: None,
            branch_condition: None,
        },
        RunEvent::BlockFinished {
            attempt: 1,
            seq: 2,
            test: "smoke.flow.json".into(),
            block: "inbox".into(),
            status: StepStatus::Passed,
            duration_ms: 80,
            failure: None,
            branch: Some("elseIf1".into()),
            branches: Some(3),
            branch_condition: Some("Inbox is empty".into()),
        },
        RunEvent::Finished {
            verdict: Verdict::Passed,
            flaky: false,
        },
    ];
    for event in &events {
        setup.host.publish(setup.root(), run.clone(), event.clone());
    }
    for event in events {
        assert_eq!(watch.next().await, Ok(Some(RunUpdate::Event { event })));
    }
    assert_eq!(watch.next().await, Ok(None));

    let status = client.run_status("Demo".into(), run.clone()).await.unwrap();
    assert_eq!(status.state, RunState::Running);
    client.stop_run("Demo".into(), run.clone()).await.unwrap();
    assert_eq!(*setup.fake.stopped.lock().unwrap(), vec![run]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_run_waiting_for_the_screen_reaches_watchers_as_a_status() {
    let setup = Setup::new("waiting").await;
    let client = setup.pair(Permission::Run, TestSigner::new(1)).await;
    let run = client
        .run_test("Demo".into(), "smoke.flow.json".into(), None)
        .await
        .unwrap();
    let watch = client.watch("Demo".into(), run.clone()).await.unwrap();
    let Ok(Some(RunUpdate::Status { mut status })) = watch.next().await else {
        panic!("the first update is the status");
    };
    assert_eq!(status.waiting, None);

    status.waiting = Some(ScreenWait {
        holder: Some(ScreenHolder {
            process: "jevision".into(),
            test: "Sign in".into(),
            since: 1_800_000_000,
        }),
    });
    setup
        .host
        .publish_status(setup.root(), run.clone(), status.clone());
    assert_eq!(
        watch.next().await,
        Ok(Some(RunUpdate::Status {
            status: status.clone()
        }))
    );

    // The run goes on and ends as before: the watch is still open and still closes at the end.
    let started = RunEvent::BlockStarted {
        attempt: 1,
        seq: 1,
        test: "smoke.flow.json".into(),
        block: "open".into(),
        block_type: "openApp".into(),
        title: None,
    };
    setup
        .host
        .publish(setup.root(), run.clone(), started.clone());
    assert_eq!(
        watch.next().await,
        Ok(Some(RunUpdate::Event { event: started }))
    );
    let finished = RunEvent::Finished {
        verdict: Verdict::Passed,
        flaky: false,
    };
    setup.host.publish(setup.root(), run, finished.clone());
    assert_eq!(
        watch.next().await,
        Ok(Some(RunUpdate::Event { event: finished }))
    );
    assert_eq!(watch.next().await, Ok(None));
}

#[test]
fn waiting_for_the_screen_and_the_card_fields_are_additions_older_peers_ignore() {
    let status = |waiting| RunStatus {
        run: "20261018-130000-live".into(),
        test: "smoke.flow.json".into(),
        state: RunState::Running,
        verdict: None,
        attempt: 1,
        block: None,
        waiting,
    };
    // A status of a run that doesn't wait is the one an older Mac sends, and the other way round.
    let plain = serde_json::to_value(status(None)).unwrap();
    assert!(plain.get("waiting").is_none());
    assert_eq!(
        serde_json::from_value::<RunStatus>(plain).unwrap(),
        status(None)
    );
    let waiting = status(Some(ScreenWait { holder: None }));
    let json = serde_json::to_value(&waiting).unwrap();
    assert_eq!(json["waiting"], serde_json::json!({ "holder": null }));
    assert_eq!(serde_json::from_value::<RunStatus>(json).unwrap(), waiting);

    // A summary from an older Mac has no card fields.
    let old = serde_json::json!({
        "name": "Smoke", "description": null, "app": "com.example.App", "start": "open",
        "width": 1.0, "height": 1.0, "blocks": [], "frames": [], "edges": []
    });
    let summary = serde_json::from_value::<TestSummary>(old).unwrap();
    assert_eq!(
        (
            summary.priority,
            summary.test_type,
            summary.status,
            summary.author
        ),
        (None, None, None, None)
    );
}

#[test]
fn a_started_block_shows_what_it_works_with_filled_for_the_run() {
    let scratch = Scratch::new("title");
    write(
        &scratch.0,
        "greet.flow.json",
        r#"{"format":1,"name":"Greet","app":"com.example.App","start":"type","blocks":{
            "type":{"type":"type","text":"Hello, ${data.name}! ${secret.pin} ${var.later}"}}}"#,
    );
    let values = HashMap::from([
        ("data.name".to_owned(), "Boris".to_owned()),
        ("secret.pin".to_owned(), "••••••".to_owned()),
    ]);
    let title = |block| running_title(&scratch.0, "greet.flow.json", block, &values);
    // A value the block doesn't see stays a placeholder, kept on one line as on the test's card.
    assert_eq!(
        title("type"),
        Some(format!(
            "Hello, Boris! •••••• {}",
            jevision_core::unbreakable_placeholders("${var.later}")
        ))
    );
    assert_eq!(title("gone"), None);

    // An older Mac sends no title.
    let old = json!({
        "event": "blockStarted", "attempt": 1, "seq": 1, "test": "greet.flow.json",
        "block": "type", "blockType": "type"
    });
    assert!(matches!(
        serde_json::from_value::<RunEvent>(old).unwrap(),
        RunEvent::BlockStarted { title: None, .. }
    ));
}

#[test]
fn a_finished_step_carries_the_way_an_if_took() {
    let finished =
        |branch: Option<&str>, branches, condition: Option<&str>| RunEvent::BlockFinished {
            attempt: 1,
            seq: 2,
            test: "inbox.flow.json".into(),
            block: "first-screen".into(),
            status: StepStatus::Passed,
            duration_ms: 80,
            failure: None,
            branch: branch.map(Into::into),
            branches,
            branch_condition: condition.map(Into::into),
        };
    let event = finished(Some("elseIf1"), Some(3), Some("Inbox is empty"));
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["branch"], "elseIf1");
    assert_eq!(json["branches"], 3);
    assert_eq!(json["branchCondition"], "Inbox is empty");
    assert_eq!(serde_json::from_value::<RunEvent>(json).unwrap(), event);

    // Other blocks send nothing new, and an event from an older Mac reads as no branch.
    let plain = serde_json::to_value(finished(None, None, None)).unwrap();
    assert!(plain.get("branch").is_none() && plain.get("branchCondition").is_none());
    assert_eq!(
        serde_json::from_value::<RunEvent>(plain).unwrap(),
        finished(None, None, None)
    );
}

#[test]
fn verdicts_and_statuses_from_a_newer_mac_read_as_unknown() {
    let runs: RunList = serde_json::from_value(serde_json::json!({
        "active": [],
        "saved": [{
            "run": "r1", "test": "Inbox", "file": "inbox.flow.json",
            "startedAt": "2026-10-19T08:00:00Z", "durationMs": 5, "verdict": "skipped", "flaky": false
        }]
    }))
    .unwrap();
    assert_eq!(runs.saved[0].verdict, Verdict::Unknown);

    let step: RunEvent = serde_json::from_value(serde_json::json!({
        "event": "blockFinished", "attempt": 1, "seq": 2, "test": "inbox.flow.json",
        "block": "open", "status": "retried", "durationMs": 80, "failure": null
    }))
    .unwrap();
    assert!(matches!(
        step,
        RunEvent::BlockFinished {
            status: StepStatus::Unknown,
            ..
        }
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn reads_saved_runs_reports_and_pictures() {
    let setup = Setup::new("reports").await;
    let client = setup.pair(Permission::View, TestSigner::new(1)).await;
    let runs = client.list_runs("Demo".into()).await.unwrap();
    assert!(runs.active.is_empty());
    assert_eq!(runs.saved.len(), 1);
    assert_eq!(runs.saved[0].run, RUN);
    assert_eq!(runs.saved[0].verdict, Verdict::Failed);

    let report = client.report("Demo".into(), RUN.into()).await.unwrap();
    assert_eq!(report.file, "login/sign-in.flow.json");
    let status = client.run_status("Demo".into(), RUN.into()).await.unwrap();
    assert_eq!(status.state, RunState::Finished);
    assert_eq!(status.verdict, Some(Verdict::Failed));

    let picture = client
        .picture("Demo".into(), RUN.into(), "shots/1-after.png".into())
        .await
        .unwrap();
    assert_eq!(picture, PICTURE);
    assert_eq!(
        client
            .report("Demo".into(), "20991231-000000-none".into())
            .await,
        Err(RemoteError::RunNotFound)
    );
    assert_eq!(setup.host.activity(5)[1].kind, ActivityKind::ReportRead);
}

#[tokio::test(flavor = "multi_thread")]
async fn lists_tests_as_a_tree_and_summarizes_one() {
    let setup = Setup::new("tests").await;
    let client = setup.pair(Permission::View, TestSigner::new(1)).await;
    let tests = client.list_tests("Demo".into()).await.unwrap();
    let paths: Vec<&str> = tests.iter().map(|t| t.path.as_str()).collect();
    assert_eq!(
        paths,
        ["login", "login/sign-in.flow.json", "smoke.flow.json"]
    );
    assert!(tests[0].is_folder);
    assert_eq!(tests[1].name, "Sign in with valid credentials");
    assert_eq!(
        tests[1].last_run.as_ref().map(|r| r.run.as_str()),
        Some(RUN)
    );
    assert_eq!(tests[2].name, "Smoke");
    assert_eq!(tests[2].last_run, None);

    let test = client
        .get_test("Demo".into(), "login/sign-in.flow.json".into())
        .await
        .unwrap();
    assert_eq!(test.start, "open");
    assert_eq!(test.blocks[0].id, "open");
    assert_eq!(test.blocks[0].kind, "openApp");
    let email = test.blocks.iter().find(|b| b.id == "email").unwrap();
    assert_eq!(email.title, "demo@example.com");
    assert!(test.blocks.iter().all(|b| b.area.is_some()));
    assert_eq!(test.priority.as_deref(), Some("critical"));
    assert_eq!(test.test_type.as_deref(), Some("smoke"));
    assert_eq!(test.status.as_deref(), Some("ready"));
    assert_eq!(test.author, None);
    assert!(
        test.edges.iter().any(|e| e.from == "open"
            && e.edge == "next"
            && e.to == "email"
            && !e.points.is_empty())
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn signs_in_again_after_the_connection_drops() {
    let setup = Setup::new("reconnect").await;
    let client = setup.pair(Permission::Run, TestSigner::new(1)).await;
    let run = client
        .run_test("Demo".into(), "smoke.flow.json".into(), None)
        .await
        .unwrap();
    let watch = client.watch("Demo".into(), run.clone()).await.unwrap();
    assert!(matches!(
        watch.next().await,
        Ok(Some(RunUpdate::Status { .. }))
    ));

    // The phone loses its network, for example while it sleeps.
    let first = client.connection().await.unwrap();
    first.close(iroh::endpoint::VarInt::from_u32(wire::CLOSE_DONE), b"");
    assert_eq!(watch.next().await, Err(RemoteError::HostOffline));

    // The next request signs in again on its own; watching again starts from the status.
    assert_eq!(client.list_projects().await.unwrap().len(), 1);
    assert_ne!(
        client.connection().await.unwrap().stable_id(),
        first.stable_id()
    );
    assert_eq!(setup.host.activity(1)[0].kind, ActivityKind::SignedIn);
    let watch = client.watch("Demo".into(), run.clone()).await.unwrap();
    let Ok(Some(RunUpdate::Status { status })) = watch.next().await else {
        panic!("a new watch starts with the status");
    };
    assert_eq!(status.block.as_deref(), Some("open"));
    let finished = RunEvent::Finished {
        verdict: Verdict::Passed,
        flaky: false,
    };
    setup.host.publish(setup.root(), run, finished.clone());
    assert_eq!(
        watch.next().await,
        Ok(Some(RunUpdate::Event { event: finished }))
    );
    assert_eq!(watch.next().await, Ok(None));
}

/// A test that runs once per row of a data table.
fn data_test(file: &str) -> String {
    format!(
        r#"{{"format":1,"name":"Rows","app":"com.example.AcmeMail","start":"rows","blocks":{{
          "rows":{{"type":"useData","file":"{file}","body":"type"}},
          "type":{{"type":"type","text":"${{data.email}}","next":"rows"}}}}}}"#
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn runs_one_data_row_of_a_test() {
    let setup = Setup::new("rows").await;
    let project = setup.scratch.0.join("project");
    write(&project, "data/rows.flow.json", &data_test("users.csv"));
    write(
        &project,
        "data/users.csv",
        "email,name\na@x.io,Ann\nb@x.io,\n",
    );
    write(&setup.scratch.0, "users.csv", "email\nc@x.io\n");
    write(
        &project,
        "data/leak.flow.json",
        &data_test("../../users.csv"),
    );
    let client = setup.pair(Permission::Run, TestSigner::new(1)).await;

    let test = client
        .get_test("Demo".into(), "data/rows.flow.json".into())
        .await
        .unwrap();
    assert_eq!(test.rows, ["Ann", "b@x.io"]);
    let plain = client
        .get_test("Demo".into(), "smoke.flow.json".into())
        .await
        .unwrap();
    assert!(plain.rows.is_empty());
    // A table outside the project stays unread, as the run would refuse it.
    let leak = client
        .get_test("Demo".into(), "data/leak.flow.json".into())
        .await
        .unwrap();
    assert!(leak.rows.is_empty());

    for (path, row) in [
        ("data/rows.flow.json", 2),
        ("smoke.flow.json", 0),
        ("data/leak.flow.json", 0),
    ] {
        assert_eq!(
            client.run_test("Demo".into(), path.into(), Some(row)).await,
            Err(RemoteError::DataRowNotFound),
            "{path} row {row}"
        );
    }
    assert_eq!(
        client
            .run_test("Demo".into(), "missing.flow.json".into(), Some(0))
            .await,
        Err(RemoteError::TestNotFound)
    );
    assert!(setup.fake.started.lock().unwrap().is_empty());

    client
        .run_test("Demo".into(), "data/rows.flow.json".into(), Some(1))
        .await
        .unwrap();
    let started = setup.fake.started.lock().unwrap().clone();
    assert_eq!(
        started,
        vec![(
            setup.root(),
            "data/rows.flow.json".into(),
            Some(1),
            "Anna's iPhone".into()
        )]
    );
}

#[test]
fn requests_without_a_row_run_every_row() {
    let old: wire::Request =
        serde_json::from_str(r#"{"type":"runTest","project":"Demo","path":"a.flow.json"}"#)
            .unwrap();
    assert_eq!(
        old,
        wire::Request::RunTest {
            project: "Demo".into(),
            path: "a.flow.json".into(),
            row: None,
        }
    );
    let one = wire::Request::RunTest {
        project: "Demo".into(),
        path: "a.flow.json".into(),
        row: Some(2),
    };
    assert_eq!(
        serde_json::to_string(&one).unwrap(),
        r#"{"type":"runTest","project":"Demo","path":"a.flow.json","row":2}"#
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_frames_over_the_size_limit() {
    let setup = Setup::new("frames").await;
    let client = setup.pair(Permission::View, TestSigner::new(1)).await;
    let conn = client.connection().await.unwrap();
    let (mut send, mut recv) = conn.open_bi().await.unwrap();
    let huge = (wire::MAX_MESSAGE as u32 + 1).to_be_bytes();
    send.write_all(&huge).await.unwrap();
    let reply: wire::Reply = wire::expect(&mut recv).await.unwrap();
    assert_eq!(reply, wire::Reply::Error(RemoteError::TooLarge));

    let mut send = conn.open_uni().await.unwrap();
    let oversized = vec![b' '; wire::MAX_MESSAGE + 1];
    assert_eq!(
        wire::send_bytes(&mut send, &oversized, wire::MAX_MESSAGE).await,
        Err(RemoteError::TooLarge)
    );
    // The connection still serves requests.
    assert_eq!(client.list_projects().await.unwrap().len(), 1);
}

#[test]
fn pairing_offers_round_trip_through_qr_text() {
    let offer = PairingOffer {
        host_id: iroh::SecretKey::from_bytes(&[3; 32]).public().to_string(),
        host_name: "Anna's Mac & Studio — 2".into(),
        code: "0123456789abcdef0123456789abcdef".into(),
        expires_at: 1_792_000_000,
        relay_url: Some("https://relay.example.com/".into()),
        addrs: vec!["192.168.1.20:52114".into(), "[fd7a:115c::1]:41641".into()],
    };
    let url = offer.to_url();
    assert!(url.starts_with("jevision://pair?v=1&id="));
    assert!(!url.contains(' ') && !url.contains('\''));
    assert_eq!(PairingOffer::parse(&url), Ok(offer.clone()));

    assert_eq!(
        PairingOffer::parse("https://example.com/pair?code=1"),
        Err(RemoteError::OfferWrongScheme)
    );
    let damaged = [
        url.replace("code=0123", "code=zz23"),
        url.replace("v=1", "v=0"),
        url.replace("v=1", "v=x"),
        url.replace(&offer.host_id, "nothex"),
        url.replace("192.168.1.20%3A52114", "not-an-address"),
        url.replace("&name=", "&nom="),
        url.replace("%26", "%2"),
        url[..url.find("&exp=").unwrap()].to_owned(),
    ];
    for text in damaged {
        assert_eq!(
            PairingOffer::parse(&text),
            Err(RemoteError::OfferMalformed),
            "{text}"
        );
    }
}

#[test]
fn error_codes_have_stable_keys() {
    assert_eq!(RemoteError::MacLocked.key(), "remote.error.macLocked");
    assert_eq!(
        serde_json::to_string(&RemoteError::DeclinedOnMac).unwrap(),
        "\"declinedOnMac\""
    );
    assert_eq!(
        RemoteError::OfferWrongScheme.key(),
        RemoteError::OfferMalformed.key()
    );
    let added = [
        (RemoteError::DeviceTooOld, "deviceTooOld"),
        (RemoteError::MacTooOld, "macTooOld"),
        (RemoteError::Unsupported, "unsupported"),
        (RemoteError::DevicesFromNewer, "devicesFromNewer"),
    ];
    for (error, code) in added {
        assert_eq!(serde_json::to_value(error).unwrap(), json!(code));
        assert_eq!(error.key(), format!("remote.error.{code}"));
    }
    // A code a newer JEVision added reads as unknown.
    assert_eq!(
        serde_json::from_value::<RemoteError>(json!("tooEarly")).unwrap(),
        RemoteError::Unknown
    );
}

#[test]
fn a_link_of_a_newer_format_asks_for_a_newer_app() {
    let offer = PairingOffer {
        host_id: iroh::SecretKey::from_bytes(&[3; 32]).public().to_string(),
        host_name: "Studio Mac".into(),
        code: "0123456789abcdef0123456789abcdef".into(),
        expires_at: 1_792_000_000,
        relay_url: None,
        addrs: vec!["192.168.1.20:52114".into()],
    };
    let phone = offer.to_url();
    let assistant = offer.to_assistant_url();
    assert!(assistant.starts_with("jevision://assistant?v=1&"));
    assert_eq!(
        PairingOffer::parse(&phone.replace("v=1", "v=2")),
        Err(RemoteError::DeviceTooOld)
    );
    assert_eq!(
        PairingOffer::parse_assistant(&assistant.replace("v=1", "v=2")),
        Err(RemoteError::DeviceTooOld)
    );
    assert_eq!(
        PairingOffer::parse(&phone.replace("v=1", "v=0")),
        Err(RemoteError::OfferMalformed)
    );
}

#[test]
fn errors_are_told_in_the_language_asked_for() {
    assert_eq!(
        RemoteError::MacLocked.text_in("en"),
        "The Mac’s screen is locked."
    );
    assert_eq!(
        RemoteError::MacLocked.text_in("ru"),
        "Экран Mac заблокирован."
    );
    assert_eq!(
        RemoteError::OfferWrongScheme.text_in("ru"),
        "Это не код подключения JEVision."
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn keeps_devices_and_activity_private_across_launches() {
    use std::os::unix::fs::PermissionsExt;

    let setup = Setup::new("persist").await;
    let client = setup.pair(Permission::Run, TestSigner::new(1)).await;
    let state = setup.scratch.0.join("state");
    let mode = |name: &str| fs::metadata(state.join(name)).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode("devices.json"), 0o600);
    assert_eq!(mode("activity.jsonl"), 0o600);
    assert_eq!(
        fs::metadata(&state).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert!(!state.join("devices.json.tmp").exists());

    let devices = setup.host.devices();
    setup.host.stop().await;
    client.close().await;
    assert!(!setup.host.status().running);
    let host = Host::new(config(&setup.scratch), setup.fake.clone()).unwrap();
    assert_eq!(host.devices(), devices);
    let kinds: Vec<ActivityKind> = host.activity(10).iter().map(|a| a.kind).collect();
    assert_eq!(kinds, [ActivityKind::Paired]);
}

#[test]
fn activity_log_rotates_and_reads_newest_first() {
    let scratch = Scratch::new("rotate");
    let store = Store::open(&scratch.0).unwrap();
    for i in 0..5000 {
        store.log(&Activity {
            time: i,
            device_id: Some("0123456789abcdef0123456789abcdef".into()),
            device_name: Some("Anna's iPhone".into()),
            kind: ActivityKind::RunStarted,
            subject: Some("Demo/login/sign-in.flow.json".into()),
            error: None,
        });
    }
    let current = fs::metadata(scratch.0.join("activity.jsonl"))
        .unwrap()
        .len();
    let old = fs::metadata(scratch.0.join("activity.1.jsonl"))
        .unwrap()
        .len();
    assert!(current < 512 * 1024);
    assert!((512 * 1024..520 * 1024).contains(&old));
    let newest = store.activity(3);
    let times: Vec<i64> = newest.iter().map(|a| a.time).collect();
    assert_eq!(times, [4999, 4998, 4997]);
    assert_eq!(store.activity(100_000).len(), 5000);
}

const ARGUMENT_SECRET: &str = "hunter2-in-an-argument";

/// Waits for something that happens on the Mac's own time.
async fn eventually(check: impl Fn() -> bool) {
    for _ in 0..100 {
        if check() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("it did not happen in time");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_assistant_calls_the_tools_on_the_mac_next_to_a_phone() {
    let setup = Setup::new("assistant").await;
    let phone = setup.pair(Permission::Run, TestSigner::new(1)).await;
    let assistant = setup.pair_assistant(TestSigner::new(5)).await;

    let devices = setup.host.devices();
    let device = devices
        .iter()
        .find(|d| d.kind == DeviceKind::Assistant)
        .unwrap();
    assert_eq!(
        (device.name.as_str(), device.assistant),
        ("Dev laptop", true)
    );
    let paired_phone = devices
        .iter()
        .find(|d| d.kind == DeviceKind::Phone)
        .unwrap();
    assert!(!paired_phone.assistant);

    let who = json!({ "protocolVersion": "2025-11-25", "clientInfo": { "name": "claude-code" } });
    let hello = assistant.hello(who.clone()).await.unwrap();
    assert_eq!(hello["serverInfo"]["name"], "jevision");
    assert_eq!(
        setup.fake.hellos.lock().unwrap()[0]["clientInfo"]["name"],
        "claude-code"
    );
    let tools = assistant.tools().await.unwrap();
    assert_eq!(tools["tools"][0]["name"], "list_tests");
    let arguments = json!({ "folder": ARGUMENT_SECRET });
    let result = assistant
        .call("list_tests".into(), arguments.clone(), None)
        .await
        .unwrap();
    assert_eq!(result["content"][0]["text"], "done");
    {
        let called = setup.fake.called.lock().unwrap();
        let (_, name, root, tool, got) = &called[0];
        assert_eq!(
            (name.as_str(), root.as_str(), tool.as_str()),
            ("Dev laptop", setup.root().as_str(), "list_tests")
        );
        assert_eq!(got, &arguments);
    }

    // The log says which tool was called and by whom, never with what.
    let entry = setup.host.activity(1).remove(0);
    assert_eq!(entry.kind, ActivityKind::AssistantCalled);
    assert_eq!(entry.subject.as_deref(), Some("list_tests"));
    assert_eq!(entry.device_name.as_deref(), Some("Dev laptop"));
    let log = fs::read_to_string(setup.scratch.0.join("state/activity.jsonl")).unwrap();
    assert!(!log.contains(ARGUMENT_SECRET));

    // The MCP server's own errors arrive as they are, and a reply bigger than a phone's
    // messages can be still arrives whole.
    assert_eq!(
        assistant.call("nothing".into(), json!({}), None).await,
        Err(AssistantFailure::Rejected {
            code: -32602,
            message: "Unknown tool: nothing".into()
        })
    );
    let big = assistant.call("big".into(), json!({}), None).await.unwrap();
    assert_eq!(
        big["content"][0]["text"].as_str().unwrap().len(),
        wire::MAX_MESSAGE + 1
    );

    // The phone keeps working on its own protocol, and both show as connected.
    assert_eq!(phone.list_projects().await.unwrap().len(), 1);
    assert_eq!(setup.host.status().connected.len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_assistant_tells_the_mac_who_it_is_again_after_the_connection_drops() {
    let setup = Setup::new("assistant-reconnect").await;
    let assistant = setup.pair_assistant(TestSigner::new(5)).await;
    let who = json!({ "clientInfo": { "name": "cursor" } });
    assistant.hello(who).await.unwrap();

    assistant
        .call("list_tests".into(), json!({}), None)
        .await
        .unwrap();
    let first = assistant.client.connection().await.unwrap();
    first.close(iroh::endpoint::VarInt::from_u32(wire::CLOSE_DONE), b"");
    assistant
        .call("list_tests".into(), json!({}), None)
        .await
        .unwrap();
    let hellos = setup.fake.hellos.lock().unwrap().clone();
    assert_eq!(hellos.len(), 2);
    assert_eq!(hellos[1]["clientInfo"]["name"], "cursor");
    let called = setup.fake.called.lock().unwrap().clone();
    assert_ne!(called[0].0, called[1].0);
    // What the first connection started was stopped when it ended.
    eventually(|| setup.fake.ended.lock().unwrap().contains(&called[0].0)).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_assistants_that_are_not_paired() {
    let setup = Setup::new("assistant-unpaired").await;
    let paired = setup.pair_assistant(TestSigner::new(5)).await;
    let record = assistant_saved(&paired);

    let stranger = AssistantClient::open(record.clone(), TestSigner::new(6), offline())
        .await
        .unwrap();
    assert_eq!(
        stranger.tools().await,
        Err(AssistantFailure::Remote(RemoteError::NotPaired))
    );
    let impostor = Arc::new(Impostor(TestSigner::new(5), TestSigner::new(6)));
    let forged = AssistantClient::open(record, impostor, offline())
        .await
        .unwrap();
    assert_eq!(
        forged.hello(json!({})).await,
        Err(AssistantFailure::Remote(RemoteError::BadSignature))
    );
    assert!(setup.fake.called.lock().unwrap().is_empty());
    assert!(setup.fake.hellos.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn revoking_an_assistant_stops_it_at_once() {
    let setup = Setup::new("assistant-revoke").await;
    let assistant = setup.pair_assistant(TestSigner::new(5)).await;
    assistant
        .call("list_tests".into(), json!({}), None)
        .await
        .unwrap();
    let session = setup.fake.called.lock().unwrap()[0].0.clone();

    setup.host.revoke(&assistant.record().device_id).unwrap();
    assert_eq!(
        assistant.tools().await,
        Err(AssistantFailure::Remote(RemoteError::Revoked))
    );
    eventually(|| setup.fake.ended.lock().unwrap().contains(&session)).await;
    assert!(setup.host.status().connected.is_empty());

    let again = AssistantClient::open(assistant_saved(&assistant), TestSigner::new(5), offline())
        .await
        .unwrap();
    assert_eq!(
        again.tools().await,
        Err(AssistantFailure::Remote(RemoteError::NotPaired))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_right_to_run_does_not_open_the_tools() {
    let setup = Setup::new("assistant-right").await;
    // A phone that may run tests is still not an assistant.
    let phone = setup.pair(Permission::Run, TestSigner::new(1)).await;
    let as_assistant = AssistantClient::open(saved(&phone), TestSigner::new(1), offline())
        .await
        .unwrap();
    assert_eq!(
        as_assistant.tools().await,
        Err(AssistantFailure::Remote(RemoteError::NotPaired))
    );
    // And an assistant is not a phone.
    let assistant = setup.pair_assistant(TestSigner::new(5)).await;
    let as_phone =
        Client::connect(assistant_saved(&assistant), TestSigner::new(5), offline()).await;
    assert_eq!(as_phone.err(), Some(RemoteError::NotPaired));

    // The owner turns the assistant off: nothing opens, and what it started stops.
    let id = assistant.record().device_id;
    assistant
        .call("list_tests".into(), json!({}), None)
        .await
        .unwrap();
    let session = setup.fake.called.lock().unwrap()[0].0.clone();
    setup.host.set_assistant(&id, false).unwrap();
    assert!(!setup.host.devices().iter().any(|d| d.assistant));
    eventually(|| setup.fake.ended.lock().unwrap().contains(&session)).await;
    let off = Err(AssistantFailure::Remote(RemoteError::NotAllowed));
    assert_eq!(assistant.hello(json!({})).await, off);
    assert_eq!(assistant.tools().await, off);
    assert_eq!(
        assistant.call("list_tests".into(), json!({}), None).await,
        off
    );
    assert_eq!(setup.fake.called.lock().unwrap().len(), 1);
    assert_eq!(setup.fake.hellos.lock().unwrap().len(), 0);
    let entry = setup.host.activity(1).remove(0);
    assert_eq!(entry.kind, ActivityKind::Denied);
    assert_eq!(entry.subject.as_deref(), Some("list_tests"));

    setup.host.set_assistant(&id, true).unwrap();
    assert!(assistant.tools().await.is_ok());

    // The levels of a phone and the switch of an assistant are not interchangeable.
    let phone_id = phone.record().device_id;
    assert_eq!(
        setup.host.set_assistant(&phone_id, true),
        Err(RemoteError::NotAllowed)
    );
    assert_eq!(
        setup.host.set_permission(&id, Permission::Run),
        Err(RemoteError::NotAllowed)
    );
    assert_eq!(
        setup.host.set_assistant("nobody", true),
        Err(RemoteError::NotPaired)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_phone_does_not_see_assistant_requests_and_an_assistant_does_not_see_phone_ones() {
    let setup = Setup::new("assistant-protocols").await;
    let phone = setup.pair(Permission::Run, TestSigner::new(1)).await;
    let assistant = setup.pair_assistant(TestSigner::new(5)).await;

    let conn = phone.connection().await.unwrap();
    let (mut send, mut recv) = conn.open_bi().await.unwrap();
    let call = wire::AssistantRequest::Call {
        tool: "list_tests".into(),
        arguments: json!({}),
        project: None,
    };
    wire::send(&mut send, &call).await.unwrap();
    let reply: wire::Reply = wire::expect(&mut recv).await.unwrap();
    assert_eq!(reply, wire::Reply::Error(RemoteError::Unsupported));
    assert!(setup.fake.called.lock().unwrap().is_empty());

    let conn = assistant.client.connection().await.unwrap();
    let (mut send, mut recv) = conn.open_bi().await.unwrap();
    wire::send(&mut send, &wire::Request::ListProjects)
        .await
        .unwrap();
    let reply: wire::AssistantReply = wire::expect(&mut recv).await.unwrap();
    assert_eq!(
        reply,
        wire::AssistantReply::Error {
            error: RemoteError::Unsupported
        }
    );
    // Both connections go on working.
    assert_eq!(phone.list_projects().await.unwrap().len(), 1);
    assert!(assistant.tools().await.is_ok());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_code_pairs_only_the_kind_of_device_it_was_shown_for() {
    let setup = Setup::new("assistant-codes").await;
    let phone_offer = setup.offer(Permission::Run);
    let wrong = AssistantClient::pair(
        phone_offer.clone(),
        "Mallory".into(),
        TestSigner::new(9),
        offline(),
    )
    .await;
    assert_eq!(wrong.err(), Some(RemoteError::PairingCodeInvalid));

    let mut assistant_offer = setup.host.create_assistant_offer().unwrap();
    assistant_offer.addrs = loopback(&assistant_offer.addrs);
    let wrong = Client::pair(
        assistant_offer.clone(),
        "Mallory".into(),
        TestSigner::new(9),
        offline(),
    )
    .await;
    assert_eq!(wrong.err(), Some(RemoteError::PairingCodeInvalid));
    assert!(setup.host.devices().is_empty());

    // The two kinds of link are not mistaken for each other.
    assert_eq!(
        PairingOffer::parse(&assistant_offer.to_assistant_url()),
        Err(RemoteError::OfferWrongScheme)
    );
    assert_eq!(
        PairingOffer::parse_assistant(&phone_offer.to_url()),
        Err(RemoteError::OfferWrongScheme)
    );
    let url = assistant_offer.to_assistant_url();
    assert!(url.starts_with("jevision://assistant?v=1&id="));
    assert_eq!(PairingOffer::parse_assistant(&url), Ok(assistant_offer));
}

#[tokio::test(flavor = "multi_thread")]
async fn checks_what_an_assistant_sends() {
    let setup = Setup::new("assistant-input").await;
    let assistant = setup.pair_assistant(TestSigner::new(5)).await;

    let protocol = Err(AssistantFailure::Remote(RemoteError::Protocol));
    assert_eq!(assistant.hello(json!("claude")).await, protocol);
    for tool in ["", "../etc/passwd", "a b", &"t".repeat(65)] {
        assert_eq!(
            assistant.call(tool.into(), json!({}), None).await,
            protocol,
            "{tool}"
        );
    }
    assert_eq!(
        assistant
            .call("list_tests".into(), json!(["not", "an", "object"]), None)
            .await,
        protocol
    );
    let missing = Err(AssistantFailure::Remote(RemoteError::ProjectNotFound));
    assert_eq!(
        assistant
            .call("list_tests".into(), json!({}), Some("Nowhere".into()))
            .await,
        missing
    );
    assert!(setup.fake.called.lock().unwrap().is_empty());

    // What an assistant says about itself is cut down before the app shows it.
    let title = format!("Claude\u{7}\n{}", "x".repeat(300));
    assistant
        .hello(json!({
            "protocolVersion": "2025-11-25",
            "clientInfo": { "name": "claude-code", "title": title, "extra": "dropped" },
            "capabilities": { "huge": "dropped" },
        }))
        .await
        .unwrap();
    let said = setup.fake.hellos.lock().unwrap()[0].clone();
    assert_eq!(said["protocolVersion"], "2025-11-25");
    assert_eq!(said["clientInfo"]["name"], "claude-code");
    let shown = said["clientInfo"]["title"].as_str().unwrap();
    assert_eq!(shown.chars().count(), 64);
    assert!(shown.starts_with("Claude") && !shown.chars().any(char::is_control));
    assert!(said.get("capabilities").is_none() && said["clientInfo"].get("extra").is_none());

    // A project is named, or the only one is meant.
    setup.host.set_projects(vec![
        Project {
            name: "Demo".into(),
            root: setup.root(),
        },
        Project {
            name: "Other".into(),
            root: setup.root(),
        },
    ]);
    assert_eq!(
        assistant.call("list_tests".into(), json!({}), None).await,
        missing
    );
    assert!(
        assistant
            .call("list_tests".into(), json!({}), Some("Other".into()))
            .await
            .is_ok()
    );
}

#[test]
fn devices_saved_before_assistants_are_phones() {
    let scratch = Scratch::new("old-devices");
    write(
        &scratch.0,
        "devices.json",
        r#"{"devices":[{"id":"0123456789abcdef0123456789abcdef","name":"Anna's iPhone",
          "publicKey":"04ab","permission":"run","pairedAt":1792000000,"lastSeen":null}]}"#,
    );
    let store = Store::open(&scratch.0).unwrap();
    let device = &store.devices()[0];
    assert_eq!(device.kind, DeviceKind::Phone);
    assert!(!device.assistant);
    assert_eq!(device.permission, Permission::Run);
}

/// A test with the notes written as JSON object entries.
fn notes_test(notes: &str) -> String {
    format!(
        r#"{{"format":1,"name":"Notes","app":"com.example.AcmeMail","start":"open","blocks":{{
          "open":{{"type":"openApp"}}}},"canvas":{{"notes":{{{notes}}}}}}}"#
    )
}

fn note(area: Area) -> NoteSummary {
    NoteSummary {
        id: "note".into(),
        text: "Text".into(),
        author: "Anna".into(),
        time: "2026-10-19T10:00:00Z".into(),
        anchor: "block".into(),
        block: Some("open".into()),
        area,
        reply_count: 0,
        replies: Vec::new(),
        resolved: None,
        attachments: Vec::new(),
    }
}

const AREA: Area = Area {
    x: 20.0,
    y: 30.0,
    width: 224.0,
    height: 144.0,
};

#[tokio::test(flavor = "multi_thread")]
async fn a_test_brings_its_notes_to_the_phone_read_only() {
    let setup = Setup::new("notes").await;
    let project = setup.scratch.0.join("project");
    write(
        &project,
        "notes/discussed.flow.json",
        &notes_test(
            r#""note":{"text":"**Heads up**: flaky on CI","author":"Anna","time":"2026-10-19T10:00:00Z",
                "anchor":{"type":"block","block":"open"},
                "attachments":[{"file":"shot.png"},{"link":"https://github.com/o/r/issues/1"}],
                "replies":[{"id":"reply","author":"Boris","time":"2026-10-19T11:00:00Z","text":"Seen it too"},
                           {"id":"reply2","text":"Fixed"}],
                "resolved":{"by":"Boris","time":"2026-10-19T12:00:00Z"},
                "frame":{"x":320,"y":16,"width":224,"height":144}},
               "about-folder":{"text":"All tests here need VPN","anchor":{"type":"folder"},
                "frame":{"x":16,"y":200,"width":224,"height":144}},
               "plain":{"text":"Whole test","frame":{"x":16,"y":400,"width":224,"height":144}}"#,
        ),
    );
    let client = setup.pair(Permission::View, TestSigner::new(1)).await;

    let test = client
        .get_test("Demo".into(), "notes/discussed.flow.json".into())
        .await
        .unwrap();
    assert_eq!(test.notes.len(), 3);
    let by_id = |id: &str| test.notes.iter().find(|n| n.id == id).unwrap();

    let note = by_id("note");
    assert_eq!(note.text, "**Heads up**: flaky on CI");
    assert_eq!(
        (note.author.as_str(), note.time.as_str()),
        ("Anna", "2026-10-19T10:00:00Z")
    );
    assert_eq!(
        (note.anchor.as_str(), note.block.as_deref()),
        ("block", Some("open"))
    );
    assert_eq!(
        note.area,
        Area {
            x: 320.0,
            y: 16.0,
            width: 224.0,
            height: 144.0
        }
    );
    assert_eq!(note.reply_count, 2);
    assert_eq!(note.replies.len(), 2);
    assert_eq!(note.replies[0].author, "Boris");
    assert_eq!(note.replies[1].text, "Fixed");
    let resolved = note.resolved.as_ref().unwrap();
    assert_eq!(
        (resolved.by.as_str(), resolved.time.as_str()),
        ("Boris", "2026-10-19T12:00:00Z")
    );
    let attachments: Vec<(&str, &str)> = note
        .attachments
        .iter()
        .map(|a| (a.kind.as_str(), a.value.as_str()))
        .collect();
    assert_eq!(
        attachments,
        [
            ("file", "shot.png"),
            ("link", "https://github.com/o/r/issues/1")
        ]
    );

    let folder = by_id("about-folder");
    assert_eq!(
        (folder.anchor.as_str(), folder.block.as_deref()),
        ("folder", None)
    );
    assert_eq!(folder.resolved, None);
    let plain = by_id("plain");
    assert_eq!((plain.anchor.as_str(), plain.reply_count), ("test", 0));

    // A test without notes sends none, and the canvas covers the notes.
    let none = client
        .get_test("Demo".into(), "smoke.flow.json".into())
        .await
        .unwrap();
    assert!(none.notes.is_empty());
    assert!(test.height >= 400.0 + 144.0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_flood_of_notes_is_cut_to_the_limits_on_the_mac() {
    let setup = Setup::new("notes-flood").await;
    let project = setup.scratch.0.join("project");
    let replies: Vec<Value> = (0..30)
        .map(|n| json!({ "id": format!("r{n}"), "author": "Anna", "text": format!("Reply {n}") }))
        .collect();
    let mut attachments: Vec<Value> = vec![
        json!({ "link": "javascript:alert(1)" }),
        json!({ "link": "file:///etc/passwd" }),
        json!({ "link": "https://example.com/has space" }),
        json!({ "link": format!("https://example.com/{}", "a".repeat(600)) }),
    ];
    attachments.extend((0..15).map(|n| json!({ "file": format!("shot{n}.png") })));
    let notes: serde_json::Map<String, Value> = (0..120)
        .map(|n| {
            (
                format!("note{n:03}"),
                json!({
                    "text": "é".repeat(8000),
                    "author": "A".repeat(300),
                    "replies": replies,
                    "attachments": attachments,
                    "frame": { "x": 16, "y": 16, "width": 224, "height": 144 },
                }),
            )
        })
        .collect();
    let text = notes_test("");
    let text = text.replace(
        r#""notes":{}"#,
        &format!(r#""notes":{}"#, Value::Object(notes)),
    );
    assert!(
        text.len() < wire::MAX_MESSAGE,
        "the file itself is readable"
    );
    write(&project, "notes/flood.flow.json", &text);
    let client = setup.pair(Permission::View, TestSigner::new(1)).await;

    let test = client
        .get_test("Demo".into(), "notes/flood.flow.json".into())
        .await
        .unwrap();
    assert_eq!(test.notes.len(), wire::MAX_NOTES);
    assert!(serde_json::to_string(&test.notes).unwrap().len() <= wire::MAX_NOTES_JSON);
    for note in &test.notes {
        assert!(note.text.len() <= wire::MAX_NOTE_TEXT);
        assert!(note.text.ends_with('…'), "a cut text says so");
        assert_eq!(note.author.len(), wire::MAX_NOTE_LABEL);
        assert!(note.author.ends_with('…'));
        assert_eq!(note.reply_count, 30);
        assert_eq!(note.replies.len(), wire::MAX_REPLIES);
        // The newest replies stay, oldest first.
        assert_eq!(note.replies[0].id, "r10");
        assert_eq!(note.replies[wire::MAX_REPLIES - 1].id, "r29");
        assert_eq!(note.attachments.len(), wire::MAX_ATTACHMENTS);
        assert!(note.attachments.iter().all(|a| a.kind == "file"));
    }
}

#[test]
fn bounded_notes_limits_whatever_a_sender_writes() {
    // Characters of several bytes are cut between characters.
    let mut long = note(AREA);
    long.text = "日".repeat(5000);
    long.replies = (0..25)
        .map(|n| NoteReplySummary {
            id: format!("r{n}"),
            author: "Boris".into(),
            time: String::new(),
            text: "ü".repeat(2000),
        })
        .collect();
    long.reply_count = 3; // less than there are: the count never shrinks below them
    let [long] = &wire::bounded_notes(vec![long])[..] else {
        panic!("one note stays");
    };
    assert!(long.text.len() <= wire::MAX_NOTE_TEXT && long.text.ends_with('…'));
    assert!(long.text.is_char_boundary(long.text.len() - '…'.len_utf8()));
    assert_eq!(long.reply_count, 25);
    assert_eq!(long.replies.len(), wire::MAX_REPLIES);
    assert_eq!(long.replies[0].id, "r5");
    assert!(
        long.replies
            .iter()
            .all(|r| r.text.len() <= wire::MAX_REPLY_TEXT)
    );

    // Links the phone may open are web addresses and short enough not to be cut; files are
    // names only.
    let mut linked = note(AREA);
    let attach = |kind: &str, value: &str| NoteAttachmentSummary {
        kind: kind.into(),
        value: value.into(),
    };
    linked.attachments = vec![
        attach("link", "https://example.com/a"),
        attach("link", "http://example.com/b"),
        attach("link", "javascript:alert(1)"),
        attach("link", "tel:+100"),
        attach("link", "https://"),
        attach("link", "https://ex ample.com"),
        attach("link", "https://exa\u{0}mple.com"),
        attach("snippet", "from a newer Mac"),
        attach("file", ""),
        attach("file", "shot.png"),
    ];
    let [linked] = &wire::bounded_notes(vec![linked])[..] else {
        panic!("one note stays");
    };
    let kept: Vec<&str> = linked
        .attachments
        .iter()
        .map(|a| a.value.as_str())
        .collect();
    assert_eq!(
        kept,
        ["https://example.com/a", "http://example.com/b", "shot.png"]
    );

    // An anchor of a newer Mac, and a block anchor without a block, read as the test.
    let mut odd = note(AREA);
    odd.anchor = "whiteboard".into();
    let mut headless = note(AREA);
    headless.block = None;
    let mut stray = note(AREA);
    stray.anchor = "folder".into();
    let bounded = wire::bounded_notes(vec![odd, headless, stray]);
    assert_eq!(
        bounded
            .iter()
            .map(|n| (n.anchor.as_str(), n.block.as_deref()))
            .collect::<Vec<_>>(),
        [("test", None), ("test", None), ("folder", None)]
    );

    // The area stays on the canvas and in a sane size; numbers that are no numbers go.
    let sized = |x: f64, y: f64, width: f64, height: f64| {
        note(Area {
            x,
            y,
            width,
            height,
        })
    };
    let bounded = wire::bounded_notes(vec![
        sized(-50.0, 1e12, 1e9, -3.0),
        sized(f64::NAN, 0.0, 10.0, 10.0),
        sized(0.0, f64::INFINITY, 10.0, 10.0),
    ]);
    assert_eq!(bounded.len(), 1);
    assert_eq!(
        bounded[0].area,
        Area {
            x: 0.0,
            y: 1_000_000.0,
            width: 2000.0,
            height: 1.0
        }
    );
}

#[test]
fn bounded_notes_keeps_the_notes_within_one_message() {
    // Heavy notes in a heap: what is kept stays within the budget, the rest is left out.
    let heavy = |n: usize| {
        let mut note = note(AREA);
        note.id = format!("note{n}");
        note.text = "x".repeat(wire::MAX_NOTE_TEXT);
        note.replies = (0..wire::MAX_REPLIES)
            .map(|r| NoteReplySummary {
                id: format!("reply{r}"),
                author: "Anna".into(),
                time: "2026-10-19T10:00:00Z".into(),
                text: "y".repeat(wire::MAX_REPLY_TEXT),
            })
            .collect();
        note.attachments = (0..wire::MAX_ATTACHMENTS)
            .map(|a| NoteAttachmentSummary {
                kind: "link".into(),
                value: format!("https://example.com/{a}/{}", "z".repeat(400)),
            })
            .collect();
        note
    };
    let bounded = wire::bounded_notes((0..wire::MAX_NOTES).map(heavy).collect());
    let size = serde_json::to_string(&bounded).unwrap().len();
    assert!(size <= wire::MAX_NOTES_JSON, "{size} bytes");
    assert!(!bounded.is_empty() && bounded.len() < wire::MAX_NOTES);
    // The ones kept are the first ones.
    assert_eq!(bounded[0].id, "note0");
    assert_eq!(
        bounded.last().unwrap().id,
        format!("note{}", bounded.len() - 1)
    );
    // Cutting what is cut changes nothing.
    assert_eq!(wire::bounded_notes(bounded.clone()), bounded);
}

#[test]
fn notes_are_an_addition_older_peers_ignore() {
    let with_notes = |notes: Vec<NoteSummary>| TestSummary {
        name: "Smoke".into(),
        description: None,
        priority: None,
        test_type: None,
        status: None,
        author: None,
        app: "com.example.App".into(),
        start: "open".into(),
        width: 1.0,
        height: 1.0,
        blocks: Vec::new(),
        frames: Vec::new(),
        edges: Vec::new(),
        rows: Vec::new(),
        notes,
    };

    // A Mac that has no notes to send says what an older Mac says, and a summary from an older
    // Mac reads as one without notes.
    let plain = serde_json::to_value(with_notes(Vec::new())).unwrap();
    assert!(plain.get("notes").is_none());
    let old = json!({
        "name": "Smoke", "description": null, "app": "com.example.App", "start": "open",
        "width": 1.0, "height": 1.0, "blocks": [], "frames": [], "edges": []
    });
    assert_eq!(
        serde_json::from_value::<TestSummary>(old).unwrap(),
        with_notes(Vec::new())
    );

    // A summary with notes, as a phone that predates them reads it: the same fields as before,
    // the notes unknown to it and so ignored.
    #[derive(Debug, PartialEq, serde::Deserialize)]
    struct BeforeNotes {
        name: String,
        app: String,
        start: String,
        width: f64,
        height: f64,
        blocks: Vec<BlockSummary>,
        frames: Vec<FrameSummary>,
        edges: Vec<EdgeSummary>,
        rows: Vec<String>,
    }
    let mut replied = note(AREA);
    replied.reply_count = 1;
    replied.replies = vec![NoteReplySummary {
        id: "reply".into(),
        author: "Boris".into(),
        time: String::new(),
        text: "Seen".into(),
    }];
    replied.resolved = Some(NoteResolvedSummary {
        by: "Boris".into(),
        time: String::new(),
    });
    let summary = with_notes(vec![replied]);
    let json = serde_json::to_value(&summary).unwrap();
    assert_eq!(json["notes"][0]["replyCount"], 1);
    assert_eq!(json["notes"][0]["resolved"]["by"], "Boris");
    assert_eq!(json["notes"][0]["area"]["width"], 224.0);
    let before: BeforeNotes = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(
        (before.name.as_str(), before.start.as_str()),
        ("Smoke", "open")
    );
    assert_eq!(
        serde_json::from_value::<TestSummary>(json.clone()).unwrap(),
        summary
    );

    // A newer Mac may add fields to a note and kinds to anchors and attachments; this phone
    // still reads the test and takes what it knows.
    let mut future = json;
    future["notes"][0]["color"] = json!("yellow");
    future["notes"][0]["anchor"] = json!("whiteboard");
    future["notes"][0]["attachments"] = json!([{ "kind": "snippet", "value": "x", "size": 3 }]);
    let read: TestSummary = serde_json::from_value(future).unwrap();
    let [read] = &wire::bounded_notes(read.notes)[..] else {
        panic!("the note stays");
    };
    assert_eq!(read.anchor, "test");
    assert!(read.attachments.is_empty());
    assert_eq!(read.replies[0].text, "Seen");
}

/// The frames of protocol 1 as it is defined now. A file there is never edited: phones and
/// Macs that speak protocol 1 write and read exactly these.
#[test]
fn protocol_1_frames_read_and_write_back_unchanged() {
    fn again<T: serde::Serialize + serde::de::DeserializeOwned>(frame: &Value) -> Value {
        serde_json::to_value(serde_json::from_value::<T>(frame.clone()).unwrap()).unwrap()
    }
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/protocol-1");
    let mut read = 0;
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let frame: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let back = match name.split('-').next().unwrap() {
            "hello" => again::<wire::Hello>(&frame),
            "host" => again::<wire::HostHello>(&frame),
            "proof" => again::<wire::Proof>(&frame),
            "request" => again::<wire::Request>(&frame),
            "reply" => again::<wire::Reply>(&frame),
            "assistant" if name.starts_with("assistant-request") => {
                again::<wire::AssistantRequest>(&frame)
            }
            "assistant" => again::<wire::AssistantReply>(&frame),
            "watch" => again::<RunUpdate>(&frame),
            _ => panic!("{name} is not a frame"),
        };
        assert_eq!(back, frame, "{name}");
        read += 1;
    }
    assert_eq!(read, 42);
}

/// Sends `hello` as the first frame of a phone, written by hand the way another version of
/// JEVision writes it, and returns the Mac's answer as it came. `hello` gets the phone's
/// endpoint id, which a pairing signs.
async fn first_answer(offer: &PairingOffer, hello: impl FnOnce(EndpointId) -> Value) -> Value {
    let phone = bind(SecretKey::from_bytes(&random()), &offline(), Vec::new())
        .await
        .unwrap();
    let mut addr = EndpointAddr::new(offer.host_id.parse().unwrap());
    for ip in &offer.addrs {
        addr = addr.with_ip_addr(ip.parse().unwrap());
    }
    let conn = phone.connect(addr, ALPN_PHONE).await.unwrap();
    let (mut send, mut recv) = conn.open_bi().await.unwrap();
    wire::send(&mut send, &hello(phone.id())).await.unwrap();
    let answer = wire::expect(&mut recv).await.unwrap();
    phone.close().await;
    answer
}

/// A pairing hello of a phone of version 9.0.0 that speaks `protocols`.
fn pair_hello(
    offer: &PairingOffer,
    phone: EndpointId,
    protocols: Value,
    signer: &TestSigner,
) -> Value {
    let host: EndpointId = offer.host_id.parse().unwrap();
    let key = signer.public_key();
    let code = code_bytes(&offer.code).unwrap();
    let signature = signer
        .sign(pair_transcript(&code, &host, &phone, &key))
        .unwrap();
    json!({
        "type": "pair",
        "protocols": protocols,
        "appVersion": "9.0.0",
        "code": offer.code,
        "deviceName": "Anna's iPhone",
        "deviceKey": hex(&key),
        "signature": hex(&signature),
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn a_newer_mac_speaks_the_protocol_of_an_older_phone() {
    let setup = Setup::new("newer-mac").await;
    setup.host.set_protocols(Protocols { min: 1, max: 2 });
    let offer = setup.offer(Permission::Run);
    let signer = TestSigner::new(1);
    let welcome = first_answer(&offer, |phone| {
        pair_hello(&offer, phone, json!({ "min": 1, "max": 1 }), &signer)
    })
    .await;
    assert_eq!(welcome["type"], "welcome");
    assert_eq!(welcome["protocol"], 1);
    assert_eq!(welcome["appVersion"], APP_VERSION);

    let phone = Client::connect(setup.record(), signer, offline())
        .await
        .unwrap();
    assert_eq!(phone.list_projects().await.unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_older_mac_speaks_the_protocol_of_a_newer_phone() {
    let setup = Setup::new("older-mac").await;
    let offer = setup.offer(Permission::Run);
    let signer = TestSigner::new(1);
    let welcome = first_answer(&offer, |phone| {
        pair_hello(&offer, phone, json!({ "min": 1, "max": 2 }), &signer)
    })
    .await;
    assert_eq!(welcome["type"], "welcome");
    assert_eq!(welcome["protocol"], 1);

    let newer = Protocols { min: 1, max: 2 };
    let record = setup.record();
    let host = record.host_id.parse().unwrap();
    let phone = Client::new(ALPN_PHONE, newer, host, record, signer, &offline())
        .await
        .unwrap();
    assert_eq!(phone.list_projects().await.unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_mac_that_needs_a_newer_phone_says_which_version() {
    let setup = Setup::keyed("device-too-old", 7).await;
    let host_id = setup.host.status().endpoint_id;
    let refused_with = |kind| {
        let row = &setup.host.activity(1)[0];
        assert_eq!(row.kind, kind);
        assert_eq!(row.error, Some(RemoteError::DeviceTooOld));
        assert_eq!(row.subject.as_deref(), Some(APP_VERSION));
        assert_eq!(row.device_name.as_deref(), Some("Anna's iPhone"));
    };

    setup.host.set_protocols(Protocols { min: 2, max: 2 });
    let phone = Client::pair(
        setup.offer(Permission::Run),
        "Anna's iPhone".into(),
        TestSigner::new(1),
        offline(),
    )
    .await;
    assert_eq!(phone.err(), Some(RemoteError::DeviceTooOld));
    assert_eq!(mac_version(&host_id).as_deref(), Some(APP_VERSION));
    refused_with(ActivityKind::PairingFailed);

    // A phone paired before the Mac moved on hears the same when it signs in.
    setup.host.set_protocols(PROTOCOLS);
    let phone = setup.pair(Permission::Run, TestSigner::new(1)).await;
    let record = saved(&phone);
    phone.close().await;
    setup.host.set_protocols(Protocols { min: 2, max: 2 });
    let again = Client::connect(record, TestSigner::new(1), offline()).await;
    assert_eq!(again.err(), Some(RemoteError::DeviceTooOld));
    refused_with(ActivityKind::SignInFailed);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_phone_that_needs_a_newer_mac_says_which_version() {
    let setup = Setup::new("mac-too-old").await;
    let offer = setup.offer(Permission::Run);
    let answer = first_answer(&offer, |phone| {
        pair_hello(
            &offer,
            phone,
            json!({ "min": 2, "max": 2 }),
            &TestSigner::new(1),
        )
    })
    .await;
    // The Mac tells its version only to a phone it is too new for.
    assert_eq!(answer, json!({ "type": "refused", "error": "macTooOld" }));
    let row = &setup.host.activity(1)[0];
    assert_eq!(row.kind, ActivityKind::PairingFailed);
    assert_eq!(row.error, Some(RemoteError::MacTooOld));
    assert_eq!(row.subject.as_deref(), Some("9.0.0"));
    assert_eq!(row.device_name.as_deref(), Some("Anna's iPhone"));
    assert!(setup.host.devices().is_empty());

    let newer = Protocols { min: 2, max: 2 };
    let offer = setup.offer(Permission::Run);
    let name = "Anna's iPhone".to_owned();
    let phone = Client::pair_on(
        ALPN_PHONE,
        newer,
        offer,
        name,
        TestSigner::new(1),
        offline(),
    )
    .await;
    assert_eq!(phone.err(), Some(RemoteError::MacTooOld));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_mac_reads_the_protocols_of_any_first_frame() {
    let setup = Setup::new("first-frame").await;
    let offer = setup.offer(Permission::Run);
    let newer = first_answer(&offer, |_| {
        json!({ "type": "pairWithPasskey", "protocols": { "min": 1, "max": 1 }, "passkey": "x" })
    })
    .await;
    assert_eq!(newer, json!({ "type": "refused", "error": "unsupported" }));
    let backwards = first_answer(
        &offer,
        |_| json!({ "type": "signIn", "protocols": { "min": 2, "max": 1 }, "deviceKey": "04" }),
    )
    .await;
    assert_eq!(backwards, json!({ "type": "refused", "error": "protocol" }));
    // A phone that doesn't say speaks protocol 1.
    let plain = first_answer(&offer, |_| json!({ "type": "signIn", "deviceKey": "04" })).await;
    assert_eq!(plain["type"], "challenge");
    let unreadable = first_answer(
        &offer,
        |_| json!({ "type": "signIn", "protocols": "2", "deviceKey": "04" }),
    )
    .await;
    assert_eq!(unreadable["type"], "challenge");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_phone_of_a_newer_protocol_hears_so_whatever_else_it_sends() {
    let setup = Setup::new("newer-shape").await;
    let offer = setup.offer(Permission::Run);
    let newer = json!({ "min": 2, "max": 2 });
    let unknown = first_answer(
        &offer,
        |_| json!({ "type": "pairWithPasskey", "protocols": newer, "appVersion": "9.0.0" }),
    )
    .await;
    assert_eq!(unknown, json!({ "type": "refused", "error": "macTooOld" }));

    // A phone of protocol 2 that renamed a field of `pair`.
    let signer = TestSigner::new(1);
    let renamed = first_answer(&offer, |phone| {
        let mut hello = pair_hello(&offer, phone, newer.clone(), &signer);
        let key = hello.as_object_mut().unwrap().remove("deviceKey").unwrap();
        hello["deviceKeys"] = json!([key]);
        hello
    })
    .await;
    assert_eq!(renamed, json!({ "type": "refused", "error": "macTooOld" }));
    let row = &setup.host.activity(1)[0];
    assert_eq!(row.kind, ActivityKind::PairingFailed);
    assert_eq!(row.error, Some(RemoteError::MacTooOld));
    assert_eq!(row.subject.as_deref(), Some("9.0.0"));
    assert_eq!(row.device_name.as_deref(), Some("Anna's iPhone"));
}

#[tokio::test(flavor = "multi_thread")]
async fn both_sides_speak_the_newest_protocol_they_share() {
    let setup = Setup::new("newest-shared").await;
    setup.host.set_protocols(Protocols { min: 1, max: 2 });
    let offer = setup.offer(Permission::Run);
    let welcome = first_answer(&offer, |phone| {
        pair_hello(
            &offer,
            phone,
            json!({ "min": 1, "max": 2 }),
            &TestSigner::new(1),
        )
    })
    .await;
    assert_eq!(welcome["type"], "welcome");
    assert_eq!(welcome["protocol"], 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_version_another_device_names_is_kept_only_when_plain() {
    for plain in ["1.4.0", "10.0", "2.0.0-beta.1", "3.1.0-rc-2"] {
        assert!(plain_version(plain), "{plain}");
    }
    let long = format!("1.{}", "0".repeat(31));
    for not in [
        "",
        "9.0 — download it from evil.example",
        "v1.0",
        "1..0",
        "1.0-",
        "1.0+b",
        &long,
    ] {
        assert!(!plain_version(not), "{not}");
    }

    let setup = Setup::new("untrusted-version").await;
    let offer = setup.offer(Permission::Run);
    let answer = first_answer(&offer, |phone| {
        let mut hello = pair_hello(
            &offer,
            phone,
            json!({ "min": 2, "max": 2 }),
            &TestSigner::new(1),
        );
        hello["appVersion"] = json!("9.0 — download it from evil.example");
        hello
    })
    .await;
    assert_eq!(answer["error"], "macTooOld");
    let row = &setup.host.activity(1)[0];
    assert_eq!(row.error, Some(RemoteError::MacTooOld));
    assert_eq!(row.subject, None);

    let mut welcome = newer_welcome("run");
    welcome["appVersion"] = json!("7.0 — download it from evil.example");
    let (_mac, record) = scripted_mac(welcome, vec![]).await;
    let host = record.host_id.clone();
    let phone = Client::connect(record, TestSigner::new(1), offline()).await;
    assert!(phone.is_ok());
    assert_eq!(mac_version(&host), None);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_welcome_of_an_earlier_build_reads_as_protocol_1() {
    let welcome = json!({
        "type": "welcome", "hostName": "Earlier Mac", "deviceId": "d1", "permission": "run",
        "relayUrl": null, "addrs": []
    });
    let (_mac, record) = scripted_mac(welcome, vec![]).await;
    let host = record.host_id.clone();
    let phone = Client::connect(record, TestSigner::new(1), offline()).await;
    assert!(phone.is_ok());
    assert_eq!(mac_version(&host), None);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_mac_answers_requests_it_does_not_know_and_keeps_the_connection() {
    let setup = Setup::new("unknown-request").await;
    let phone = setup.pair(Permission::Run, TestSigner::new(1)).await;
    let conn = phone.connection().await.unwrap();
    let (mut send, mut recv) = conn.open_bi().await.unwrap();
    wire::send(&mut send, &json!({ "type": "teleport", "project": "Demo" }))
        .await
        .unwrap();
    let _ = send.finish();
    let answer: Value = wire::expect(&mut recv).await.unwrap();
    assert_eq!(answer, json!({ "error": "unsupported" }));
    assert_eq!(phone.list_projects().await.unwrap().len(), 1);
    assert_eq!(
        phone.connection().await.unwrap().stable_id(),
        conn.stable_id()
    );

    let assistant = setup.pair_assistant(TestSigner::new(2)).await;
    let conn = assistant.client.connection().await.unwrap();
    let (mut send, mut recv) = conn.open_bi().await.unwrap();
    wire::send(&mut send, &json!({ "type": "listResources" }))
        .await
        .unwrap();
    let _ = send.finish();
    let answer: Value = wire::expect(&mut recv).await.unwrap();
    assert_eq!(answer, json!({ "type": "error", "error": "unsupported" }));
    assert!(assistant.tools().await.is_ok());
    assert_eq!(
        assistant.client.connection().await.unwrap().stable_id(),
        conn.stable_id()
    );
}

/// A Mac that answers with frames written by hand, the way a newer JEVision writes them:
/// `hello` to the phone's first frame, then the frames of `replies`, one list per request.
async fn scripted_mac(hello: Value, replies: Vec<Vec<Value>>) -> (Endpoint, HostRecord) {
    let mac = bind(
        SecretKey::from_bytes(&random()),
        &offline(),
        vec![ALPN_PHONE.to_vec()],
    )
    .await
    .unwrap();
    let addrs: Vec<String> = mac
        .bound_sockets()
        .iter()
        .map(ToString::to_string)
        .collect();
    let record = HostRecord {
        host_id: mac.id().to_string(),
        name: "Newer Mac".into(),
        device_id: String::new(),
        relay_url: None,
        addrs: loopback(&addrs),
    };
    let listener = mac.clone();
    tokio::spawn(async move {
        let Some(incoming) = listener.accept().await else {
            return;
        };
        let Ok(conn) = incoming.await else {
            return;
        };
        for frames in std::iter::once(vec![hello]).chain(replies) {
            let Ok((mut send, mut recv)) = conn.accept_bi().await else {
                return;
            };
            let _ = wire::expect::<Value>(&mut recv).await;
            for frame in frames {
                let _ = wire::send(&mut send, &frame).await;
            }
            let _ = send.finish();
        }
        conn.closed().await;
    });
    (mac, record)
}

fn newer_welcome(permission: &str) -> Value {
    json!({
        "type": "welcome", "protocol": 1, "appVersion": "7.0.0", "hostName": "Newer Mac",
        "deviceId": "d1", "permission": permission, "relayUrl": null, "addrs": []
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn a_phone_reads_what_a_newer_mac_says_as_unknown() {
    let signer = TestSigner::new(1);
    let mut on_protocol_2 = newer_welcome("run");
    on_protocol_2["protocol"] = json!(2);
    let first_answers = [
        (
            json!({ "type": "queued", "position": 3 }),
            RemoteError::Unknown,
        ),
        (
            json!({ "type": "refused", "error": "tooEarly" }),
            RemoteError::Unknown,
        ),
        (on_protocol_2, RemoteError::Protocol),
    ];
    for (hello, error) in first_answers {
        let (_mac, record) = scripted_mac(hello, Vec::new()).await;
        let phone = Client::connect(record, signer.clone(), offline()).await;
        assert_eq!(phone.err(), Some(error));
    }

    let replies = vec![
        vec![json!({ "ok": { "type": "hologram" } })],
        vec![json!({ "error": "tooEarly" })],
    ];
    let (_mac, record) = scripted_mac(newer_welcome("admin"), replies).await;
    let phone = Client::connect(record.clone(), signer, offline())
        .await
        .unwrap();
    assert_eq!(mac_version(&record.host_id).as_deref(), Some("7.0.0"));
    // A level a newer Mac added reads as the least one.
    assert_eq!(phone.permission(), Permission::View);
    assert_eq!(phone.list_projects().await, Err(RemoteError::Unknown));
    assert_eq!(phone.list_projects().await, Err(RemoteError::Unknown));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_watch_skips_what_a_newer_mac_added() {
    let status = json!({
        "run": RUN, "test": "login/sign-in.flow.json", "state": "queued", "verdict": null,
        "attempt": 0, "block": null
    });
    let frames = vec![
        json!({ "ok": { "type": "status", "status": status } }),
        json!({ "type": "progress", "percent": 40 }),
        json!({ "type": "event", "event": { "event": "zoomed", "factor": 2 } }),
        json!({ "type": "event", "event": { "event": "paused", "block": "open", "reason": "phoneCall" } }),
        json!({ "type": "event", "event": { "event": "finished", "verdict": "passed", "flaky": false } }),
    ];
    let (_mac, record) = scripted_mac(newer_welcome("view"), vec![frames]).await;
    let phone = Client::connect(record, TestSigner::new(1), offline())
        .await
        .unwrap();
    let watch = phone.watch("Demo".into(), RUN.into()).await.unwrap();
    let Ok(Some(RunUpdate::Status { status })) = watch.next().await else {
        panic!("a watch starts with the status");
    };
    assert_eq!(status.state, RunState::Unknown);
    let paused = RunEvent::Paused {
        block: "open".into(),
        reason: PauseReason::Unknown,
    };
    assert_eq!(
        watch.next().await,
        Ok(Some(RunUpdate::Event { event: paused }))
    );
    let finished = RunEvent::Finished {
        verdict: Verdict::Passed,
        flaky: false,
    };
    assert_eq!(
        watch.next().await,
        Ok(Some(RunUpdate::Event { event: finished }))
    );
    assert_eq!(watch.next().await, Ok(None));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_watch_stops_at_a_broken_frame_of_a_known_kind() {
    let status = json!({
        "run": RUN, "test": "login/sign-in.flow.json", "state": "queued", "verdict": null,
        "attempt": 0, "block": null
    });
    let frames = vec![
        json!({ "ok": { "type": "status", "status": status } }),
        json!({ "type": "status", "status": { "run": 5 } }),
        json!({ "type": "event", "event": { "event": "paused" } }),
    ];
    let (_mac, record) = scripted_mac(newer_welcome("view"), vec![frames]).await;
    let phone = Client::connect(record, TestSigner::new(1), offline())
        .await
        .unwrap();
    let watch = phone.watch("Demo".into(), RUN.into()).await.unwrap();
    assert!(matches!(
        watch.next().await,
        Ok(Some(RunUpdate::Status { .. }))
    ));
    assert_eq!(watch.next().await, Err(RemoteError::Protocol));
    assert_eq!(watch.next().await, Err(RemoteError::Protocol));
}

#[test]
fn devices_keep_what_a_newer_mac_wrote_in_them() {
    let scratch = Scratch::new("devices-newer-fields");
    let device = |id: &str, permission: &str| {
        json!({
            "id": id, "name": "Anna's iPhone", "publicKey": "04ab", "kind": "phone",
            "permission": permission, "assistant": false, "pairedAt": 1, "lastSeen": null,
            "color": "teal"
        })
    };
    let watch = json!({ "id": "c", "kind": "watch", "name": "Anna's Watch" });
    let file = json!({
        "sync": { "cloud": true },
        "devices": [device("a", "admin"), device("b", "view"), watch]
    });
    write(&scratch.0, "devices.json", &file.to_string());

    let mut store = Store::open(&scratch.0).unwrap();
    let ids: Vec<&str> = store.devices().iter().map(|d| d.id.as_str()).collect();
    assert_eq!(ids, ["a", "b"]);
    assert_eq!(store.devices()[0].permission, Permission::View);
    store
        .change(|devices| {
            devices[0].last_seen = Some(5);
            devices[1].permission = Permission::Run;
        })
        .unwrap();
    let saved: Value =
        serde_json::from_slice(&fs::read(scratch.0.join("devices.json")).unwrap()).unwrap();
    let mut seen = device("a", "admin");
    seen["lastSeen"] = json!(5);
    let expected = json!({
        "format": 1,
        "sync": { "cloud": true },
        "devices": [seen, device("b", "run"), watch]
    });
    assert_eq!(saved, expected);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_devices_file_from_a_newer_mac_is_read_but_never_written() {
    let mut setup = Setup::new("devices-future").await;
    let phone = setup.pair(Permission::Run, TestSigner::new(1)).await;
    phone.close().await;
    let path = setup.scratch.0.join("state/devices.json");
    let mut file: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    file["format"] = json!(2);
    let future = serde_json::to_vec_pretty(&file).unwrap();
    fs::write(&path, &future).unwrap();
    setup.restart().await;

    let stranger = Client::pair(
        setup.offer(Permission::Run),
        "Boris's iPhone".into(),
        TestSigner::new(2),
        offline(),
    )
    .await;
    assert_eq!(stranger.err(), Some(RemoteError::DevicesFromNewer));
    let phone = Client::connect(setup.record(), TestSigner::new(1), offline())
        .await
        .unwrap();
    assert_eq!(phone.list_projects().await.unwrap().len(), 1);
    let id = setup.host.devices()[0].id.clone();
    assert_eq!(setup.host.revoke(&id), Err(RemoteError::DevicesFromNewer));
    assert_eq!(fs::read(&path).unwrap(), future);

    // A file that asks for a newer JEVision is left alone the same way.
    let scratch = Scratch::new("devices-min-version");
    let text = r#"{"minVersion":"99.0.0","devices":{"a":{}}}"#;
    write(&scratch.0, "devices.json", text);
    let mut store = Store::open(&scratch.0).unwrap();
    assert!(store.devices().is_empty());
    assert_eq!(store.change(|_| {}), Err(RemoteError::DevicesFromNewer));
    assert_eq!(
        fs::read_to_string(scratch.0.join("devices.json")).unwrap(),
        text
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_devices_file_that_cannot_be_read_is_set_aside() {
    let mut setup = Setup::new("devices-broken").await;
    let state = setup.scratch.0.join("state");
    write(&state, "devices.unreadable.json", "an older copy");
    write(&state, "devices.json", r#"{"devices": ["#);
    setup.restart().await;

    assert!(setup.host.devices().is_empty());
    assert_eq!(
        fs::read_to_string(state.join("devices.unreadable.json")).unwrap(),
        r#"{"devices": ["#
    );
    let row = &setup.host.activity(1)[0];
    assert_eq!(row.kind, ActivityKind::DevicesUnreadable);
    assert_eq!(row.subject.as_deref(), Some("devices.unreadable.json"));
    assert_eq!(
        (&row.device_id, &row.device_name, row.error),
        (&None, &None, None)
    );
    // Phones pair again.
    setup.pair(Permission::Run, TestSigner::new(1)).await;
    assert_eq!(setup.host.devices().len(), 1);
}
