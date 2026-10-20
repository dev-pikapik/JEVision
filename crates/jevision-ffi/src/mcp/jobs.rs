//! Runs started by assistants: one at a time, its tests one after another on a thread of its
//! own, so an assistant can ask how it goes while it runs.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

use jevision_l10n::tr;
use jevision_runner::{Event, PauseReason, Verdict, describe_screen_holder};
use serde_json::{Value, json};

use super::{AssistantHost, hands_message};
use crate::{DataRowRange, RunOutcome, TestRun, describe_failure};

/// Every run started since the server started, oldest first.
#[derive(Default)]
pub(super) struct Jobs {
    all: Mutex<Vec<Arc<Job>>>,
    threads: Mutex<Vec<JoinHandle<()>>>,
}

/// How the tests of a job run, as the assistant asked.
#[derive(Clone, Default)]
pub(super) struct RunChoices {
    /// Overrides the tests' `retries`.
    pub retries: Option<u32>,
    /// Environment of the project to run in.
    pub environment: Option<String>,
    /// Rows of the tests' data tables to run; `None` for all of them.
    pub rows: Option<DataRowRange>,
}

impl Jobs {
    /// Starts running `tests` (relative to `project`) unless a run is still going.
    pub(super) fn start(
        &self,
        project: &Path,
        tests: Vec<String>,
        choices: RunChoices,
        host: Arc<dyn AssistantHost>,
    ) -> Result<Arc<Job>, String> {
        let mut all = lock(&self.all);
        if let Some(going) = all.iter().find(|j| !j.lock().finished) {
            return Err(tr!("mcp.run.stillGoing", &going.id));
        }
        let job = Arc::new(Job {
            id: (all.len() + 1).to_string(),
            tests,
            state: Mutex::default(),
            changed: Condvar::new(),
        });
        let worker = job.clone();
        let project = project.to_owned();
        let thread = std::thread::Builder::new()
            .name("jevision-assistant-run".into())
            .spawn(move || worker.work(&project, &choices, &host))
            .map_err(|e| tr!("mcp.run.cannotStart", e.to_string()))?;
        all.push(job.clone());
        lock(&self.threads).push(thread);
        Ok(job)
    }

    /// The run with `id`, or the newest one.
    pub(super) fn get(&self, id: Option<&str>) -> Result<Arc<Job>, String> {
        let all = lock(&self.all);
        let found = match id {
            Some(id) => all.iter().find(|j| j.id == id),
            None => all.last(),
        };
        found.cloned().ok_or_else(|| match id {
            Some(id) => tr!("mcp.run.notFound", id),
            None => tr!("mcp.run.none"),
        })
    }

    pub(super) fn stop_all(&self) {
        for job in lock(&self.all).iter() {
            job.stop();
        }
    }

    /// Waits until every run has saved its reports.
    pub(super) fn wait_all(&self) {
        for thread in std::mem::take(&mut *lock(&self.threads)) {
            let _ = thread.join();
        }
    }
}

pub(super) struct Job {
    pub(super) id: String,
    /// Test files, relative to the project folder.
    tests: Vec<String>,
    state: Mutex<State>,
    changed: Condvar,
}

#[derive(Default)]
struct State {
    /// One per test that ended, in order.
    results: Vec<Value>,
    /// The test running now and where it is.
    now: Option<Now>,
    run: Option<Arc<TestRun>>,
    /// The tests not started yet won't start.
    stopped: bool,
    finished: bool,
}

#[derive(Default)]
struct Now {
    /// Data row being run, counted from 1, with how many the run goes through; 0 without one.
    row: u32,
    label: String,
    attempt: u32,
    step: u32,
    block: String,
    paused: bool,
    /// Who has the screen while this test waits for it.
    waiting: Option<String>,
}

impl Job {
    fn lock(&self) -> MutexGuard<'_, State> {
        lock(&self.state)
    }

    fn update(&self, change: impl FnOnce(&mut State)) {
        change(&mut self.lock());
        self.changed.notify_all();
    }

    /// Stops the test that is running, which then ends as cancelled, and skips the rest.
    pub(super) fn stop(&self) {
        let run = {
            let mut state = self.lock();
            state.stopped = true;
            state.run.clone()
        };
        if let Some(run) = run {
            run.cancel();
        }
    }

    /// How the run goes, after waiting up to `wait` for it to end.
    pub(super) fn status(&self, wait: Duration) -> Value {
        let state = self.lock();
        let (state, _) = self
            .changed
            .wait_timeout_while(state, wait, |s| !s.finished)
            .unwrap_or_else(|e| e.into_inner());
        let mut tests = state.results.clone();
        if let Some(now) = &state.now {
            let mut running = json!({
                "path": self.tests[tests.len()],
                "verdict": "running",
                "attempt": now.attempt,
                "step": now.step,
                "block": now.block,
            });
            if now.row > 0 {
                running["row"] = json!(now.row);
                running["rowLabel"] = json!(now.label);
            }
            if now.paused {
                running["paused"] = json!(tr!("mcp.run.paused"));
            }
            if let Some(waiting) = &now.waiting {
                running["waiting"] = json!(waiting);
            }
            tests.push(running);
        }
        for path in &self.tests[tests.len()..] {
            tests.push(json!({ "path": path, "verdict": "waiting" }));
        }
        let count = |verdict: &str| {
            state
                .results
                .iter()
                .filter(|r| r["verdict"] == verdict)
                .count()
        };
        json!({
            "run": self.id,
            "status": if state.finished { "finished" } else { "running" },
            "passed": count("passed"),
            "failed": count("failed"),
            "cancelled": count("cancelled"),
            "notRun": count("notRun"),
            "errors": count("error"),
            "tests": tests,
        })
    }

    fn work(&self, project: &Path, choices: &RunChoices, host: &Arc<dyn AssistantHost>) {
        for file in &self.tests {
            let run = Arc::new(TestRun::new());
            let skip = {
                let mut state = self.lock();
                if !state.stopped {
                    state.run = Some(run.clone());
                    state.now = Some(Now::default());
                }
                state.stopped
            };
            let result = if skip {
                json!({ "path": file, "verdict": "notRun" })
            } else {
                self.changed.notify_all();
                self.run_one(project, file, &run, choices, host)
            };
            // Stopped with Esc Esc or stop_run: the rest don't run either.
            let cancelled = result["verdict"] == "cancelled";
            self.update(|state| {
                state.results.push(result);
                state.run = None;
                state.now = None;
                state.stopped |= cancelled;
            });
        }
        self.update(|state| state.finished = true);
    }

    fn run_one(
        &self,
        project: &Path,
        file: &str,
        run: &Arc<TestRun>,
        choices: &RunChoices,
        host: &Arc<dyn AssistantHost>,
    ) -> Value {
        let hands = match host.start_run(run.clone()) {
            Ok(hands) => hands,
            Err(e) => {
                return json!({ "path": file, "verdict": "error", "error": hands_message(&e) });
            }
        };
        let mut options = host.run_options();
        options.retries = choices.retries;
        options.environment = choices.environment.clone();
        options.data_rows = choices.rows;
        let events = |event: &Event| self.progress(event, run, host);
        let outcome = crate::run::run(
            &project.to_string_lossy(),
            &project.join(file).to_string_lossy(),
            options,
            hands,
            host.secrets(),
            &events,
            run.control.clone(),
        );
        host.run_ended();
        outcome_json(file, outcome)
    }

    fn progress(&self, event: &Event, run: &Arc<TestRun>, host: &Arc<dyn AssistantHost>) {
        let mut user_paused = false;
        self.update(|state| {
            let Some(now) = &mut state.now else { return };
            match event {
                Event::RowStarted { row, label, .. } => {
                    now.row = *row;
                    now.label.clone_from(label);
                }
                Event::AttemptStarted { attempt } => now.attempt = *attempt,
                Event::StepStarted {
                    attempt,
                    seq,
                    block,
                    ..
                } => {
                    now.attempt = *attempt;
                    now.step = *seq;
                    now.block.clone_from(block);
                    now.waiting = None;
                }
                Event::WaitingForScreen { holder } => {
                    now.waiting = Some(describe_screen_holder(holder.as_ref()));
                }
                Event::Paused { reason, .. } => {
                    now.paused = true;
                    user_paused = *reason == PauseReason::UserInput;
                }
                Event::Resumed => now.paused = false,
                _ => {}
            }
        });
        if user_paused {
            host.run_paused(run.clone());
        }
    }
}

fn outcome_json(file: &str, outcome: RunOutcome) -> Value {
    match outcome {
        RunOutcome::Finished { dir, html, report } => {
            let failure = crate::run::deciding_attempt(&report)
                .and_then(|a| a.failure)
                .map(describe_failure);
            json!({
                "path": file,
                "verdict": verdict_name(report.verdict),
                "report": PathBuf::from(&dir).file_name().map(|n| n.to_string_lossy().into_owned()),
                "failure": failure,
                "flaky": report.flaky,
                "quarantined": report.quarantined,
                "attempts": report.attempts.len(),
                "rows": report.rows.iter().map(|r| json!({
                    "row": r.row.number,
                    "label": r.row.label,
                    "verdict": verdict_name(r.verdict),
                    "flaky": r.flaky,
                    "attempts": r.attempts,
                    "failure": r.failure.clone().map(describe_failure),
                })).collect::<Vec<_>>(),
                "durationMs": report.duration_ms,
                "newReferences": report.baselines_created,
                "html": html,
            })
        }
        RunOutcome::Failed { reason } => {
            json!({ "path": file, "verdict": "error", "error": tr!("mcp.run.notSaved", reason) })
        }
    }
}

pub(super) fn verdict_name(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Passed => "passed",
        Verdict::Failed => "failed",
        Verdict::Cancelled => "cancelled",
        Verdict::Blocked => "blocked",
        Verdict::Unknown => "unknown",
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}
