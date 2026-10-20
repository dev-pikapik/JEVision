//! Runs of several tests in the order their dependencies allow: the app and the command line
//! ask the batch which test may start, run it and say how it did.

use std::path::Path;
use std::sync::Mutex;

use jevision_core::{
    Batch, BlockReason, Blocker, Flow, Issue, RunMode, TestOutcome, TestState, WaitingFor,
};

#[uniffi::remote(Enum)]
pub enum RunMode {
    Sequential,
    Parallel { limit: u32 },
}

#[uniffi::remote(Record)]
pub struct Blocker {
    pub dependency: String,
    pub cause: String,
    pub cause_name: String,
    pub reason: BlockReason,
}

#[uniffi::remote(Enum)]
pub enum BlockReason {
    Failed,
    NotRun,
    Missing,
    Cycle { tests: Vec<String> },
}

#[uniffi::remote(Enum)]
pub enum TestOutcome {
    Passed,
    Failed,
    NotRun,
    Cancelled,
}

#[uniffi::remote(Enum)]
pub enum WaitingFor {
    Dependency { test: String },
    Turn,
    Screen,
    Limit,
}

/// Where a test of a batch is.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum BatchState {
    Waiting { reason: WaitingFor },
    Running,
    Done { outcome: TestOutcome },
    Blocked { blocker: Blocker },
}

/// One test of a batch.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct BatchEntry {
    /// Relative to the project folder.
    pub file: String,
    pub name: String,
    /// Tests of the batch this one waits for, relative to the project folder.
    pub depends_on: Vec<String>,
    /// Not chosen: in the batch because a chosen test depends on it.
    pub added: bool,
    pub uses_screen: bool,
    pub state: BatchState,
}

/// The tests of one run of several, with the tests they depend on placed before them.
#[derive(uniffi::Object)]
pub struct TestBatch {
    batch: Mutex<Batch>,
}

#[uniffi::export]
impl TestBatch {
    /// Plans the tests `tests`, relative to the project folder `project`, with the tests they
    /// depend on.
    #[uniffi::constructor]
    pub fn new(project: String, tests: Vec<String>, mode: RunMode) -> Self {
        Self {
            batch: Mutex::new(Batch::new(Path::new(&project), &tests, mode)),
        }
    }

    /// The tests in the order they may run.
    pub fn entries(&self) -> Vec<BatchEntry> {
        let batch = self.lock();
        batch
            .tests()
            .iter()
            .enumerate()
            .map(|(i, t)| BatchEntry {
                file: t.file.clone(),
                name: t.name.clone(),
                depends_on: t.depends_on.clone(),
                added: t.added,
                uses_screen: t.uses_screen,
                state: match &t.state {
                    TestState::Waiting => BatchState::Waiting {
                        reason: batch.wait(i).unwrap_or(WaitingFor::Turn),
                    },
                    TestState::Running => BatchState::Running,
                    TestState::Done(outcome) => BatchState::Done { outcome: *outcome },
                    TestState::Blocked(blocker) => BatchState::Blocked {
                        blocker: blocker.clone(),
                    },
                },
            })
            .collect()
    }

    pub fn mode(&self) -> RunMode {
        self.lock().mode()
    }

    /// Position of a test that may start now, marked as running; `None` while every test left
    /// waits for another.
    pub fn start_next(&self) -> Option<u32> {
        self.lock().start_next().map(index)
    }

    /// Records how the test at `position` did; returns the tests it blocked.
    pub fn finish(&self, position: u32, outcome: TestOutcome) -> Vec<u32> {
        let mut batch = self.lock();
        if position as usize >= batch.tests().len() {
            return Vec::new();
        }
        batch
            .finish(position as usize, outcome)
            .into_iter()
            .map(index)
            .collect()
    }

    /// Skips the tests still waiting; the ones running finish on their own.
    pub fn stop(&self) {
        self.lock().stop();
    }

    /// No test is waiting or running.
    pub fn is_over(&self) -> bool {
        self.lock().is_over()
    }
}

impl TestBatch {
    fn lock(&self) -> std::sync::MutexGuard<'_, Batch> {
        self.batch
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn index(i: usize) -> u32 {
    u32::try_from(i).unwrap_or(u32::MAX)
}

/// The sentence for a blocked test in the current language, such as `Blocked: Login failed`.
#[uniffi::export]
pub fn describe_blocker(blocker: &Blocker) -> String {
    blocker.describe()
}

/// The most tests a batch runs side by side, whatever limit it is given.
#[uniffi::export]
pub fn max_parallel_runs() -> u32 {
    jevision_core::MAX_PARALLEL
}

/// Problems with the tests `flow` depends on, saved as the test `file` (relative to the folder
/// `project`): tests that don't exist, the test itself, and tests depending on each other in a
/// circle.
#[uniffi::export]
pub fn dependency_issues(flow: &Flow, project: String, file: String) -> Vec<Issue> {
    jevision_core::dependency_issues(flow, Path::new(&project), &file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_batch_runs_the_dependency_first_and_blocks_on_its_failure() {
        let dir = std::env::temp_dir().join(format!("jevision-ffi-batch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let test = |name: &str, depends: &str| {
            format!(
                r#"{{"format":1,"name":"{name}","app":"com.apple.TextEdit","dependsOn":[{depends}],"start":"a","blocks":{{"a":{{"type":"openApp"}}}}}}"#
            )
        };
        std::fs::write(dir.join("login.flow.json"), test("Login", "")).unwrap();
        std::fs::write(
            dir.join("inbox.flow.json"),
            test("Inbox", r#""login.flow.json""#),
        )
        .unwrap();
        let batch = TestBatch::new(
            dir.to_string_lossy().into_owned(),
            vec!["inbox.flow.json".into()],
            RunMode::Sequential,
        );
        let entries = batch.entries();
        assert_eq!(entries[0].file, "login.flow.json");
        assert!(entries[0].added && !entries[1].added);
        assert_eq!(
            entries[1].state,
            BatchState::Waiting {
                reason: WaitingFor::Dependency {
                    test: "login.flow.json".into()
                }
            }
        );
        assert_eq!(batch.start_next(), Some(0));
        assert_eq!(batch.finish(0, TestOutcome::Failed), vec![1]);
        let BatchState::Blocked { blocker } = &batch.entries()[1].state else {
            panic!("the dependent is blocked");
        };
        assert_eq!(describe_blocker(blocker), "Blocked: Login failed");
        assert!(batch.is_over());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
