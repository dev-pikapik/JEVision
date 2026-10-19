//! A stand-in for the Mac app, for trying the iPhone app without one.
//!
//! Serves a small project from a new temporary folder, prints a pairing link that points at
//! this computer's loopback address, and plays every run a phone starts block by block instead
//! of driving the Mac: `login/sign-in.flow.json` fails at its last block, everything else
//! passes. Phones pair with permission to run. Stop it with Ctrl-C.
//!
//! ```sh
//! cargo run -p jevision-remote --example test-host -- [--step-ms 400] [--link-file PATH]
//! xcrun simctl openurl <simulator> "$(cat PATH)"
//! ```
//!
//! The iPhone Simulator shares the Mac's network, so it reaches `127.0.0.1` directly.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use jevision_core::{BlockKind, Compare, Condition};
use jevision_remote::{
    Device, Failure, Host, HostConfig, HostDelegate, Network, Permission, Project, Relay,
    RemoteError, RunEvent, RunState, RunStatus, StepStatus, Verdict,
};
use serde_json::json;

const SIGN_IN: &str = include_str!("../../../examples/sign-in.flow.json");
const PEOPLE: &str = r#"{"format":1,"name":"Greet every person","app":"com.apple.TextEdit",
  "description":"Types a greeting for each person in people.csv.","start":"open","blocks":{
    "open":{"type":"openApp","next":"ready"},
    "ready":{"type":"if","reference":{"image":"","compare":"meaning","expect":"An empty document"},
      "then":"rows"},
    "rows":{"type":"useData","file":"people.csv","body":"type"},
    "type":{"type":"type","text":"Hello, ${data.name}!","next":"rows"}}}"#;
const PEOPLE_CSV: &str =
    "name,email\nAnna,anna@example.com\nBoris,boris@example.com\nChen,chen@example.com\n";
const FAILING: &str = "login/sign-in.flow.json";

fn main() {
    let mut step = Duration::from_millis(400);
    let mut link_file = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match (arg.as_str(), args.next()) {
            ("--step-ms", Some(ms)) => match ms.parse() {
                Ok(ms) => step = Duration::from_millis(ms),
                Err(_) => usage(),
            },
            ("--link-file", Some(path)) => link_file = Some(PathBuf::from(path)),
            _ => usage(),
        }
    }

    let folder = std::env::temp_dir().join(format!("jevision-test-host-{}", std::process::id()));
    let project = folder.join("Acme Mail");
    write(&project, FAILING, SIGN_IN);
    write(&project, "people.flow.json", PEOPLE);
    write(&project, "people.csv", PEOPLE_CSV);
    let root = project.to_string_lossy().into_owned();

    let player = Arc::new_cyclic(|me| Player {
        me: me.clone(),
        host: OnceLock::new(),
        step,
        runs: Mutex::new(HashMap::new()),
        stopping: Mutex::new(HashSet::new()),
    });
    let host = Arc::new(
        Host::new(
            HostConfig {
                secret_key: jevision_remote::generate_secret_key().to_vec(),
                name: "Test Mac".into(),
                state_dir: folder.join("state").to_string_lossy().into_owned(),
                network: Network {
                    relay: Relay::Disabled,
                    local_discovery: false,
                    global_discovery: false,
                },
            },
            player.clone(),
        )
        .expect("the host is set up"),
    );
    let _ = player.host.set(Arc::downgrade(&host));
    host.set_projects(vec![Project {
        name: "Acme Mail".into(),
        root: root.clone(),
    }]);
    // A finished run, so the phone has history before it starts one.
    let earlier = run_id(FAILING);
    player.track(&earlier, FAILING);
    player.play(&root, FAILING, None, "Mac", &earlier, Duration::ZERO);

    let runtime = tokio::runtime::Runtime::new().expect("the runtime starts");
    runtime.block_on(host.start()).expect("the host listens");
    let mut offer = host
        .create_offer(Permission::Run)
        .expect("the host makes a code");
    offer.addrs = offer
        .addrs
        .iter()
        .filter_map(|a| a.parse::<std::net::SocketAddr>().ok())
        .filter(|a| a.is_ipv4())
        .map(|a| format!("127.0.0.1:{}", a.port()))
        .collect();
    let link = offer.to_url();
    if let Some(path) = link_file {
        std::fs::write(path, &link).expect("the link file is written");
    }
    println!("{link}");
    eprintln!("Serving {root}. The link works once, for five minutes. Ctrl-C stops.");
    runtime.block_on(std::future::pending::<()>());
}

fn usage() -> ! {
    eprintln!("usage: test-host [--step-ms N] [--link-file PATH]");
    std::process::exit(2);
}

/// Plays runs instead of driving the Mac.
struct Player {
    me: Weak<Player>,
    host: OnceLock<Weak<Host>>,
    step: Duration,
    /// Runs going on, by id.
    runs: Mutex<HashMap<String, RunStatus>>,
    /// Runs a phone asked to stop.
    stopping: Mutex<HashSet<String>>,
}

impl HostDelegate for Player {
    fn can_run(&self) -> Result<(), RemoteError> {
        if self.runs.lock().unwrap().is_empty() {
            Ok(())
        } else {
            Err(RemoteError::RunInProgress)
        }
    }

    fn confirm_run(&self, _device: Device, _project: String, _test: String) -> bool {
        true
    }

    fn start_run(
        &self,
        project_root: String,
        test: String,
        row: Option<u32>,
        requested_by: Device,
    ) -> Result<String, RemoteError> {
        let player = self.me.upgrade().ok_or(RemoteError::HostFailed)?;
        let run = run_id(&test);
        self.track(&run, &test);
        let id = run.clone();
        std::thread::spawn(move || {
            player.play(
                &project_root,
                &test,
                row,
                &requested_by.name,
                &id,
                player.step,
            );
        });
        Ok(run)
    }

    fn stop_run(&self, _project_root: String, run: String) -> Result<(), RemoteError> {
        if !self.runs.lock().unwrap().contains_key(&run) {
            return Err(RemoteError::RunNotFound);
        }
        self.stopping.lock().unwrap().insert(run);
        Ok(())
    }

    fn run_status(&self, _project_root: String, run: String) -> Option<RunStatus> {
        self.runs.lock().unwrap().get(&run).cloned()
    }

    fn active_runs(&self, _project_root: String) -> Vec<RunStatus> {
        self.runs.lock().unwrap().values().cloned().collect()
    }
}

impl Player {
    fn track(&self, run: &str, test: &str) {
        self.runs.lock().unwrap().insert(
            run.to_owned(),
            RunStatus {
                run: run.to_owned(),
                test: test.to_owned(),
                state: RunState::Running,
                verdict: None,
                attempt: 0,
                block: None,
            },
        );
    }

    fn update(&self, run: &str, change: impl FnOnce(&mut RunStatus)) {
        if let Some(status) = self.runs.lock().unwrap().get_mut(run) {
            change(status);
        }
    }

    fn publish(&self, root: &str, run: &str, event: RunEvent) {
        if let Some(host) = self.host.get().and_then(Weak::upgrade) {
            host.publish(root.to_owned(), run.to_owned(), event);
        }
    }

    /// Plays a run to its end, a block every `step`, and saves its report the way the runner
    /// does.
    fn play(&self, root: &str, test: &str, row: Option<u32>, by: &str, run: &str, step: Duration) {
        let started_at = rfc3339(SystemTime::now());
        let clock = Instant::now();
        let text = std::fs::read_to_string(Path::new(root).join(test)).expect("the test is read");
        let flow = jevision_core::parse(&text).expect("the test parses");
        let blocks: Vec<(String, &str)> = flow
            .blocks_in_flow_order()
            .into_iter()
            .map(|b| (b.id.clone(), b.kind.type_name()))
            .collect();
        let rows: Vec<Option<(u32, String)>> = match (table_rows(root, test, &flow), row) {
            (Some(labels), Some(row)) => vec![Some((row + 1, labels[row as usize].clone()))],
            (Some(labels), None) => (1..).zip(labels).map(Some).collect(),
            (None, _) => vec![None],
        };

        let mut attempts = Vec::new();
        let mut verdict = Verdict::Passed;
        for row in rows {
            let attempt = 1;
            self.update(run, |s| s.attempt = attempt);
            self.publish(root, run, RunEvent::AttemptStarted { attempt });
            let attempt_clock = Instant::now();
            let mut steps = Vec::new();
            let mut outcome = (Verdict::Passed, None);
            for (index, (block, kind)) in blocks.iter().enumerate() {
                if self.stopping.lock().unwrap().contains(run) {
                    outcome = (Verdict::Cancelled, None);
                    break;
                }
                let seq = index as u32 + 1;
                self.update(run, |s| s.block = Some(block.clone()));
                self.publish(
                    root,
                    run,
                    RunEvent::BlockStarted {
                        attempt,
                        seq,
                        test: test.to_owned(),
                        block: block.clone(),
                        block_type: (*kind).to_owned(),
                    },
                );
                std::thread::sleep(step);
                let failure = (test == FAILING && index + 1 == blocks.len()).then(|| {
                    Failure::new("run.check.mismatch")
                        .with("block", block)
                        .with("image", "refs/message-list.png")
                        .with("similarity", "0.82")
                        .with("threshold", "0.9")
                });
                let status = if failure.is_some() {
                    StepStatus::Failed
                } else {
                    StepStatus::Passed
                };
                let ms = step.as_millis() as u64;
                // Every `if` takes its first way, Yes or Condition 1.
                let branch =
                    flow.blocks
                        .iter()
                        .find(|b| &b.id == block)
                        .and_then(|b| match &b.kind {
                            BlockKind::If {
                                condition, else_if, ..
                            } => Some((
                                else_if.len() as u32 + 2,
                                match condition {
                                    Condition::Screen(reference) => match &reference.compare {
                                        Compare::Meaning { expect, .. } => expect.clone(),
                                        _ => reference.image.clone(),
                                    },
                                    Condition::Value(check) => check.of.clone(),
                                },
                            )),
                            _ => None,
                        });
                self.publish(
                    root,
                    run,
                    RunEvent::BlockFinished {
                        attempt,
                        seq,
                        test: test.to_owned(),
                        block: block.clone(),
                        status,
                        duration_ms: ms,
                        failure: failure.clone(),
                        branch: branch.as_ref().map(|_| "then".to_owned()),
                        branches: branch.as_ref().map(|b| b.0),
                        branch_condition: branch.as_ref().map(|b| b.1.clone()),
                    },
                );
                steps.push(json!({
                    "seq": seq, "test": test, "block": block, "type": kind,
                    "status": status, "durationMs": ms, "failure": failure,
                    "branch": branch.as_ref().map(|_| "then"), "branches": branch.as_ref().map(|b| b.0),
                    "branchCondition": branch.as_ref().map(|b| &b.1),
                }));
                if failure.is_some() {
                    outcome = (Verdict::Failed, failure);
                    break;
                }
            }
            let (attempt_verdict, failure) = outcome;
            self.publish(
                root,
                run,
                RunEvent::AttemptFinished {
                    attempt,
                    verdict: attempt_verdict,
                    failure: failure.clone(),
                },
            );
            attempts.push(json!({
                "number": attempt, "verdict": attempt_verdict,
                "durationMs": attempt_clock.elapsed().as_millis() as u64, "failure": failure,
                "row": row.map(|(number, label)| json!({"number": number, "label": label})),
                "steps": steps,
            }));
            if attempt_verdict != Verdict::Passed {
                verdict = attempt_verdict;
            }
            if attempt_verdict == Verdict::Cancelled {
                break;
            }
        }

        let report = json!({
            "format": 1, "test": flow.name, "file": test, "app": flow.app,
            "startedAt": started_at, "durationMs": clock.elapsed().as_millis() as u64,
            "verdict": verdict, "flaky": false, "requestedBy": by, "attempts": attempts,
        });
        let dir = Path::new(root).join(".jevision/runs").join(run);
        std::fs::create_dir_all(&dir).expect("the run folder is made");
        std::fs::write(dir.join("report.json"), report.to_string()).expect("the report is saved");
        self.runs.lock().unwrap().remove(run);
        self.stopping.lock().unwrap().remove(run);
        self.publish(
            root,
            run,
            RunEvent::Finished {
                verdict,
                flaky: false,
            },
        );
    }
}

/// Labels of the rows of the test's data table, when it has one.
fn table_rows(root: &str, test: &str, flow: &jevision_core::Flow) -> Option<Vec<String>> {
    let file = jevision_core::primary_data_file(flow)?;
    let path = Path::new(root).join(test).parent()?.join(file);
    let table = jevision_core::DataTable::load(&path).ok()?;
    Some((0..table.rows.len()).map(|row| table.label(row)).collect())
}

/// A run folder name like the runner's, such as `20261018-130000123-sign-in`.
fn run_id(test: &str) -> String {
    let stem = Path::new(test)
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.strip_suffix(".flow.json"))
        .unwrap_or("test");
    let digits: String = rfc3339(SystemTime::now())
        .chars()
        .filter(char::is_ascii_digit)
        .collect();
    format!("{}-{}-{stem}", &digits[..8], &digits[8..])
}

/// UTC time in RFC 3339 with milliseconds, such as `2026-10-18T13:00:00.123Z`.
fn rfc3339(time: SystemTime) -> String {
    let since = time.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = since.as_secs();
    let (days, rest) = ((secs / 86_400) as i64, secs % 86_400);
    // Civil date from days since 1970, after Howard Hinnant's `civil_from_days`.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rest / 3600,
        rest / 60 % 60,
        rest % 60,
        since.subsec_millis()
    )
}

fn write(dir: &Path, path: &str, text: &str) {
    let file = dir.join(path);
    std::fs::create_dir_all(file.parent().expect("files are in folders"))
        .expect("the project folder is made");
    std::fs::write(file, text).expect("the project is written");
}
