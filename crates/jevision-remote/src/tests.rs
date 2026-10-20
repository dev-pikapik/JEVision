use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use serde_json::{Value, json};

use super::*;
use crate::store::Store;

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
}

impl Setup {
    async fn new(name: &str) -> Self {
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
        let host = Host::new(config(&scratch), fake.clone()).unwrap();
        host.set_projects(vec![Project {
            name: "Demo".into(),
            root: project.to_string_lossy().into_owned(),
        }]);
        host.start().await.unwrap();
        Self {
            host,
            fake,
            scratch,
        }
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
        url.replace("v=1", "v=2"),
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
    assert_eq!(reply, wire::Reply::Error(RemoteError::Protocol));
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
            error: RemoteError::Protocol
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
    assert!(url.starts_with("jevision://assistant?v=2&id="));
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
