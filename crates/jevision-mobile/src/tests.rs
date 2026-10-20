use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use jevision_remote::{Device, Host, HostConfig, HostDelegate, Project};
use p256::ecdsa::signature::Signer as _;
use p256::ecdsa::{Signature, SigningKey};

use super::*;

const EXAMPLE: &str = include_str!("../../../examples/sign-in.flow.json");
const SAVED_RUN: &str = "20261018-120000-sign-in";
const LIVE_RUN: &str = "20261018-130000-live";
const PICTURE: &[u8] = b"\x89PNG\r\n\x1a\nnot really a picture";
const REPORT: &str = r#"{"format":1,"test":"Sign in with valid credentials","file":"login/sign-in.flow.json",
    "app":"com.example.AcmeMail","startedAt":"2026-10-18T12:00:00.000Z","durationMs":900,
    "verdict":"failed","flaky":false,"attempts":[{"number":1,"verdict":"failed","durationMs":900,
    "failure":{"code":"run.target.notFound","params":{"block":"open","image":"x.png"}},
    "row":{"number":2,"label":"Bo"},"steps":[{"seq":1,"test":"login/sign-in.flow.json","block":"open","type":"openApp",
    "status":"failed","durationMs":900,"after":"shots/1-after.png",
    "failure":{"code":"run.target.notFound","params":{"block":"open","image":"x.png"}}}]}]}"#;

/// A scratch folder, removed with everything in it at the end of the test.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("jevision-mobile-{}-{name}", std::process::id()));
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

/// A phone key in software, standing in for the Secure Enclave key of the Swift app.
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

/// The person cancelled Face ID.
struct Cancelled(Arc<TestSigner>);

impl DeviceSigner for Cancelled {
    fn public_key(&self) -> Vec<u8> {
        self.0.public_key()
    }

    fn sign(&self, _message: Vec<u8>) -> Result<Vec<u8>, RemoteError> {
        Err(RemoteError::SignerFailed)
    }
}

/// The Mac app: starts a run on request and remembers what it was asked.
#[derive(Default)]
struct Fake {
    active: Mutex<HashMap<String, RunStatus>>,
    stopped: Mutex<Vec<String>>,
    rows: Mutex<Vec<Option<u32>>>,
}

impl HostDelegate for Fake {
    fn can_run(&self) -> Result<(), jevision_remote::RemoteError> {
        Ok(())
    }

    fn confirm_run(&self, _device: Device, _project: String, _test: String) -> bool {
        true
    }

    fn start_run(
        &self,
        _project_root: String,
        test: String,
        row: Option<u32>,
        _requested_by: Device,
    ) -> Result<String, jevision_remote::RemoteError> {
        self.rows.lock().unwrap().push(row);
        let status = RunStatus {
            run: LIVE_RUN.into(),
            test,
            state: RunState::Running,
            verdict: None,
            attempt: 1,
            block: Some("open".into()),
            waiting: None,
        };
        self.active.lock().unwrap().insert(LIVE_RUN.into(), status);
        Ok(LIVE_RUN.into())
    }

    fn stop_run(
        &self,
        _project_root: String,
        run: String,
    ) -> Result<(), jevision_remote::RemoteError> {
        self.stopped.lock().unwrap().push(run);
        Ok(())
    }

    fn run_status(&self, _project_root: String, run: String) -> Option<RunStatus> {
        self.active.lock().unwrap().get(&run).cloned()
    }

    fn active_runs(&self, _project_root: String) -> Vec<RunStatus> {
        self.active.lock().unwrap().values().cloned().collect()
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
            "people.flow.json",
            r#"{"format":1,"name":"People","app":"com.example.AcmeMail","start":"rows","blocks":{
              "rows":{"type":"useData","file":"people.csv","body":"type"},
              "type":{"type":"type","text":"${data.name}","next":"rows"}}}"#,
        );
        write(&project, "people.csv", "name\nAnn\nBo\n");
        let run = project.join(".jevision/runs").join(SAVED_RUN);
        write(&run, "report.json", REPORT);
        fs::create_dir_all(run.join("shots")).unwrap();
        fs::write(run.join("shots/1-after.png"), PICTURE).unwrap();

        let fake = Arc::new(Fake::default());
        let config = HostConfig {
            secret_key: [42; 32].to_vec(),
            name: "Studio Mac".into(),
            state_dir: scratch.0.join("state").to_string_lossy().into_owned(),
            network: offline(),
        };
        let host = Host::new(config, fake.clone()).unwrap();
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

    /// The text of the Mac's QR code, pointed at this computer's loopback address.
    fn qr(&self, permission: Permission) -> String {
        let mut offer = self.host.create_offer(permission).unwrap();
        offer.addrs = loopback(&offer.addrs);
        offer.to_url()
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

fn write(dir: &Path, path: &str, text: &str) {
    let file = dir.join(path);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, text).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn pairs_runs_and_reads_reports_over_loopback() {
    let setup = Setup::new("flow").await;
    let offer = parse_pairing_offer(setup.qr(Permission::Run)).unwrap();
    assert_eq!(offer.host_name, "Studio Mac");
    let phone =
        RemoteConnection::pair(offer, "Anna's iPhone".into(), TestSigner::new(1), offline())
            .await
            .unwrap();
    assert_eq!(phone.permission(), Permission::Run);
    assert_eq!(phone.record().device_id, setup.host.devices()[0].id);
    assert_eq!(
        phone.list_projects().await.unwrap(),
        vec![ProjectInfo {
            name: "Demo".into()
        }]
    );

    let tests = phone.list_tests("Demo".into()).await.unwrap();
    assert_eq!(tests[1].path, "login/sign-in.flow.json");
    assert_eq!(
        tests[1].last_run.as_ref().map(|r| r.verdict),
        Some(Verdict::Failed)
    );
    let test = phone
        .get_test("Demo".into(), "login/sign-in.flow.json".into())
        .await
        .unwrap();
    assert_eq!(
        (test.blocks[0].id.as_str(), test.blocks[0].kind.as_str()),
        ("open", "openApp")
    );
    assert!(
        test.edges
            .iter()
            .any(|e| e.from == "open" && e.edge == "next")
    );

    let run = phone
        .run_test("Demo".into(), "login/sign-in.flow.json".into(), None)
        .await
        .unwrap();
    assert_eq!(run, LIVE_RUN);
    let watcher = phone.watch("Demo".into(), run.clone()).await.unwrap();
    let Ok(Some(RunUpdate::Status { status })) = watcher.next().await else {
        panic!("the first update is the status");
    };
    assert_eq!(status.state, RunState::Running);
    let failure = jevision_remote::Failure::new("run.target.notFound").with("block", "open");
    for event in [
        jevision_remote::RunEvent::BlockStarted {
            attempt: 1,
            seq: 1,
            test: "login/sign-in.flow.json".into(),
            block: "open".into(),
            block_type: "openApp".into(),
        },
        jevision_remote::RunEvent::BlockFinished {
            attempt: 1,
            seq: 1,
            test: "login/sign-in.flow.json".into(),
            block: "open".into(),
            status: StepStatus::Failed,
            duration_ms: 120,
            failure: Some(failure),
            branch: None,
            branches: None,
            branch_condition: None,
        },
        jevision_remote::RunEvent::Finished {
            verdict: Verdict::Failed,
            flaky: false,
        },
    ] {
        setup.host.publish(setup.root(), run.clone(), event);
    }
    assert!(matches!(
        watcher.next().await,
        Ok(Some(RunUpdate::Event {
            event: RunEvent::BlockStarted { seq: 1, .. }
        }))
    ));
    let Ok(Some(RunUpdate::Event {
        event:
            RunEvent::BlockFinished {
                failure: Some(failure),
                ..
            },
    })) = watcher.next().await
    else {
        panic!("the block failed");
    };
    assert_eq!(failure.code, "run.target.notFound");
    assert_eq!(failure.params["block"], "open");
    assert_eq!(
        watcher.next().await,
        Ok(Some(RunUpdate::Event {
            event: RunEvent::Finished {
                verdict: Verdict::Failed,
                flaky: false
            }
        }))
    );
    assert_eq!(watcher.next().await, Ok(None));

    phone.stop_run("Demo".into(), run.clone()).await.unwrap();
    assert_eq!(*setup.fake.stopped.lock().unwrap(), vec![run.clone()]);
    let runs = phone.list_runs("Demo".into()).await.unwrap();
    assert_eq!(runs.active[0].run, run);
    assert_eq!(runs.saved[0].run, SAVED_RUN);

    let report = phone.report("Demo".into(), SAVED_RUN.into()).await.unwrap();
    assert_eq!(report.test, "Sign in with valid credentials");
    assert_eq!(report.duration_ms, 900);
    let attempt = &report.attempts[0];
    assert_eq!(attempt.failure.as_ref().unwrap().params["image"], "x.png");
    assert_eq!(
        attempt.row,
        Some(DataRow {
            number: 2,
            label: "Bo".into()
        })
    );
    let after = attempt.steps[0].after.clone().unwrap();
    let picture = phone
        .picture("Demo".into(), SAVED_RUN.into(), after)
        .await
        .unwrap();
    assert_eq!(picture, PICTURE);

    // The app saves the record, quits, and connects again on the next launch.
    let json = host_record_to_json(phone.record());
    phone.close().await;
    drop(phone);
    let mut record = host_record_from_json(json).unwrap();
    record.addrs = loopback(&record.addrs);
    let phone = RemoteConnection::connect(record, TestSigner::new(1), offline())
        .await
        .unwrap();
    assert_eq!(phone.list_projects().await.unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn reports_errors_with_their_stable_codes() {
    let setup = Setup::new("errors").await;
    let viewer = RemoteConnection::pair(
        parse_pairing_offer(setup.qr(Permission::View)).unwrap(),
        "Anna's iPhone".into(),
        TestSigner::new(1),
        offline(),
    )
    .await
    .unwrap();
    let refused = viewer
        .run_test("Demo".into(), "login/sign-in.flow.json".into(), None)
        .await;
    assert_eq!(refused, Err(RemoteError::NotAllowed));
    assert_eq!(RemoteError::NotAllowed.key(), "remote.error.notAllowed");
    assert_eq!(
        viewer
            .report("Demo".into(), "20991231-000000-none".into())
            .await,
        Err(RemoteError::RunNotFound)
    );

    let cancelled = RemoteConnection::pair(
        parse_pairing_offer(setup.qr(Permission::Run)).unwrap(),
        "Ben's iPhone".into(),
        Arc::new(Cancelled(TestSigner::new(2))),
        offline(),
    )
    .await;
    assert_eq!(cancelled.err(), Some(RemoteError::SignerFailed));

    assert_eq!(
        parse_pairing_offer("https://example.com".into()).err(),
        Some(RemoteError::OfferWrongScheme)
    );
    assert_eq!(
        host_record_from_json("{}".into()).err(),
        Some(RemoteError::Storage)
    );
    assert_eq!(RemoteError::HostOffline.key(), "remote.error.hostOffline");
    assert_eq!(RemoteError::MacLocked.to_string(), "the Mac is locked");
}

#[tokio::test(flavor = "multi_thread")]
async fn runs_one_row_of_a_data_table() {
    let setup = Setup::new("rows").await;
    let phone = RemoteConnection::pair(
        parse_pairing_offer(setup.qr(Permission::Run)).unwrap(),
        "Anna's iPhone".into(),
        TestSigner::new(1),
        offline(),
    )
    .await
    .unwrap();
    let test = phone
        .get_test("Demo".into(), "people.flow.json".into())
        .await
        .unwrap();
    assert_eq!(test.rows, ["Ann", "Bo"]);
    let missing = phone
        .run_test("Demo".into(), "people.flow.json".into(), Some(2))
        .await;
    assert_eq!(missing, Err(RemoteError::DataRowNotFound));
    assert_eq!(
        RemoteError::DataRowNotFound.key(),
        "remote.error.dataRowNotFound"
    );
    phone
        .run_test("Demo".into(), "people.flow.json".into(), Some(1))
        .await
        .unwrap();
    assert_eq!(*setup.fake.rows.lock().unwrap(), [Some(1)]);
}
