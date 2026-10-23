//! Runs of several tests: Run All, a folder, the tests chosen or the command line. A test can
//! depend on others (`dependsOn`): they run first, are added to the run when they weren't
//! chosen, and when one of them doesn't pass, the test is blocked instead of run, naming the
//! test that really failed.
//!
//! A batch runs its tests one after another, or side by side up to a limit. Tests that use the
//! screen take turns either way: a run drives the mouse and keyboard and compares what it sees,
//! so two of them on one screen would spoil each other. They share one lock on the screen;
//! only tests that never touch it can run next to one that does.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use jevision_l10n::{language, tr_in};
use serde::{Deserialize, Serialize};

use crate::issue::Issue;
use crate::model::{BlockKind, Condition, Flow, ScriptLanguage};

/// How the tests of a batch run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    /// One after another, in order.
    Sequential,
    /// Up to `limit` at a time, as dependencies and the screen allow.
    Parallel { limit: u32 },
}

/// Parallel runs a batch may have at a time, at most.
pub const MAX_PARALLEL: u32 = 16;
/// Parallel runs of a folder set to parallel without a limit.
pub const DEFAULT_PARALLEL: u32 = 4;

/// Why a test of a batch is blocked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Blocker {
    /// The test's own dependency that didn't pass, relative to the project folder.
    pub dependency: String,
    /// The test behind it, relative to the project folder: the dependency itself, or a test
    /// further down the chain of dependencies.
    pub cause: String,
    /// Name of `cause`; its file when the test can't be read or doesn't exist.
    pub cause_name: String,
    pub reason: BlockReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum BlockReason {
    /// `cause` ran and failed.
    Failed,
    /// `cause` couldn't run: it can't be read, has errors, or the Mac refused it.
    NotRun,
    /// `cause` is a dependency that doesn't exist.
    Missing,
    /// The tests depend on each other in a circle; `tests` goes round it and back to the first.
    Cycle { tests: Vec<String> },
    /// A reason from a newer JEVision.
    #[serde(other)]
    Unknown,
}

impl Blocker {
    /// [`Self::describe_in`] in the process language.
    pub fn describe(&self) -> String {
        self.describe_in(&language())
    }

    /// `Blocked: Login failed` in `lang`, for the command line and the reports.
    pub fn describe_in(&self, lang: &str) -> String {
        match &self.reason {
            BlockReason::Failed => tr_in!(lang, "batch.blocked.failed", &self.cause_name),
            BlockReason::NotRun => tr_in!(lang, "batch.blocked.notRun", &self.cause_name),
            BlockReason::Missing => tr_in!(lang, "batch.blocked.missing", &self.cause),
            BlockReason::Cycle { tests } => {
                tr_in!(lang, "batch.blocked.cycle", tests.join(" → "))
            }
            BlockReason::Unknown => tr_in!(lang, "batch.blocked.unknown", &self.cause_name),
        }
    }
}

/// How a test of a batch did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestOutcome {
    Passed,
    Failed,
    /// Couldn't start; the tests that depend on it are blocked.
    NotRun,
    /// Stopped by a person, or skipped because the batch was stopped.
    Cancelled,
}

/// Where a test of a batch is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestState {
    Waiting,
    Running,
    Done(TestOutcome),
    Blocked(Blocker),
}

/// What a waiting test waits for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaitingFor {
    /// A dependency that hasn't passed yet, relative to the project folder.
    Dependency { test: String },
    /// The test before it, in a batch that runs one test at a time.
    Turn,
    /// The test running now that uses the screen.
    Screen,
    /// A free place: the batch runs as many tests as it may.
    Limit,
}

/// One test of a batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchTest {
    /// Relative to the project folder, with `/` between folders.
    pub file: String,
    /// Name saved in the test; its file when it can't be read.
    pub name: String,
    /// Tests of the batch this one waits for, relative to the project folder.
    pub depends_on: Vec<String>,
    /// Not chosen: in the batch because a chosen test depends on it.
    pub added: bool,
    pub uses_screen: bool,
    pub state: TestState,
}

/// The tests of one run of several, in the order they may run, and how far they got.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Batch {
    tests: Vec<BatchTest>,
    mode: RunMode,
}

/// A test file as the batch sees it.
pub enum Loaded {
    Test(Box<Flow>),
    /// There, but not a test this build can read.
    Unreadable,
    Missing,
}

impl Batch {
    /// Plans the tests `chosen`, relative to the folder `project`, with the tests they depend
    /// on.
    pub fn new(project: &Path, chosen: &[String], mode: RunMode) -> Self {
        Self::plan(chosen, mode, |file| load(project, file))
    }

    /// [`Batch::new`] with the test files read by `read`.
    pub fn plan(chosen: &[String], mode: RunMode, read: impl FnMut(&str) -> Loaded) -> Self {
        let mut planner = Planner {
            read,
            loaded: HashMap::new(),
            nodes: HashMap::new(),
            visiting: Vec::new(),
            order: Vec::new(),
        };
        let chosen_set: HashSet<&String> = chosen.iter().collect();
        for file in chosen {
            planner.visit(file);
        }
        let mut tests: Vec<BatchTest> = Vec::with_capacity(planner.order.len());
        let mut blocked = Vec::new();
        for file in std::mem::take(&mut planner.order) {
            let uses_screen = planner.uses_screen(&file, &mut HashSet::new());
            let node = planner
                .nodes
                .remove(&file)
                .expect("every planned test has a node");
            if let Some(blocker) = node.problem {
                blocked.push((tests.len(), blocker));
            }
            tests.push(BatchTest {
                added: !chosen_set.contains(&file),
                name: node.name,
                depends_on: node.depends_on,
                file,
                uses_screen,
                state: TestState::Waiting,
            });
        }
        let mut batch = Self { tests, mode };
        for (i, blocker) in blocked {
            if batch.tests[i].state == TestState::Waiting {
                batch.tests[i].state = TestState::Blocked(blocker);
                batch.block_dependents(i);
            }
        }
        batch
    }

    pub fn tests(&self) -> &[BatchTest] {
        &self.tests
    }

    pub fn mode(&self) -> RunMode {
        self.mode
    }

    /// Position of the test `file` in the batch.
    pub fn position(&self, file: &str) -> Option<usize> {
        self.tests.iter().position(|t| t.file == file)
    }

    /// A test that may start now, marked as running; `None` while every test left waits.
    pub fn start_next(&mut self) -> Option<usize> {
        let running: Vec<&BatchTest> = self
            .tests
            .iter()
            .filter(|t| t.state == TestState::Running)
            .collect();
        let limit = match self.mode {
            RunMode::Sequential => 1,
            RunMode::Parallel { limit } => limit.clamp(1, MAX_PARALLEL) as usize,
        };
        if running.len() >= limit {
            return None;
        }
        let screen_busy = running.iter().any(|t| t.uses_screen);
        let i = (0..self.tests.len()).find(|&i| {
            let t = &self.tests[i];
            t.state == TestState::Waiting
                && self.dependencies_passed(t)
                && !(t.uses_screen && screen_busy)
        })?;
        self.tests[i].state = TestState::Running;
        Some(i)
    }

    /// Records how the test at `i` did; returns the tests that are blocked by it.
    pub fn finish(&mut self, i: usize, outcome: TestOutcome) -> Vec<usize> {
        self.tests[i].state = TestState::Done(outcome);
        match outcome {
            TestOutcome::Failed | TestOutcome::NotRun => self.block_dependents(i),
            TestOutcome::Passed | TestOutcome::Cancelled => Vec::new(),
        }
    }

    /// Skips the tests still waiting, as cancelled; the ones running finish on their own.
    pub fn stop(&mut self) {
        for t in &mut self.tests {
            if t.state == TestState::Waiting {
                t.state = TestState::Done(TestOutcome::Cancelled);
            }
        }
    }

    /// No test is waiting or running.
    pub fn is_over(&self) -> bool {
        !self
            .tests
            .iter()
            .any(|t| matches!(t.state, TestState::Waiting | TestState::Running))
    }

    /// What the waiting test at `i` waits for; `None` when it isn't waiting.
    pub fn wait(&self, i: usize) -> Option<WaitingFor> {
        let t = &self.tests[i];
        if t.state != TestState::Waiting {
            return None;
        }
        if let Some(dependency) = t.depends_on.iter().find(|d| !self.passed(d)) {
            return Some(WaitingFor::Dependency {
                test: dependency.clone(),
            });
        }
        if self.mode == RunMode::Sequential {
            return Some(WaitingFor::Turn);
        }
        let screen_busy = self
            .tests
            .iter()
            .any(|o| o.state == TestState::Running && o.uses_screen);
        Some(if t.uses_screen && screen_busy {
            WaitingFor::Screen
        } else {
            WaitingFor::Limit
        })
    }

    fn passed(&self, file: &str) -> bool {
        self.tests
            .iter()
            .any(|t| t.file == file && t.state == TestState::Done(TestOutcome::Passed))
    }

    fn dependencies_passed(&self, t: &BatchTest) -> bool {
        t.depends_on.iter().all(|d| self.passed(d))
    }

    /// Blocks every waiting test that depends on the test at `i`, and the ones depending on
    /// those; returns them in batch order.
    fn block_dependents(&mut self, i: usize) -> Vec<usize> {
        let mut blocked = Vec::new();
        let mut queue = vec![i];
        while let Some(i) = queue.pop() {
            let failed = &self.tests[i];
            let blocker = match &failed.state {
                TestState::Done(outcome) => Blocker {
                    dependency: failed.file.clone(),
                    cause: failed.file.clone(),
                    cause_name: failed.name.clone(),
                    reason: if *outcome == TestOutcome::Failed {
                        BlockReason::Failed
                    } else {
                        BlockReason::NotRun
                    },
                },
                TestState::Blocked(b) => Blocker {
                    dependency: failed.file.clone(),
                    ..b.clone()
                },
                TestState::Waiting | TestState::Running => continue,
            };
            let file = failed.file.clone();
            for j in 0..self.tests.len() {
                let t = &mut self.tests[j];
                if t.state == TestState::Waiting && t.depends_on.contains(&file) {
                    t.state = TestState::Blocked(blocker.clone());
                    blocked.push(j);
                    queue.push(j);
                }
            }
        }
        blocked.sort_unstable();
        blocked
    }
}

fn load(project: &Path, file: &str) -> Loaded {
    match std::fs::read_to_string(project.join(file)) {
        Ok(text) => {
            crate::parse(&text).map_or(Loaded::Unreadable, |flow| Loaded::Test(Box::new(flow)))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Loaded::Missing,
        Err(_) => Loaded::Unreadable,
    }
}

struct Node {
    name: String,
    depends_on: Vec<String>,
    problem: Option<Blocker>,
}

struct Planner<R> {
    read: R,
    loaded: HashMap<String, Loaded>,
    nodes: HashMap<String, Node>,
    /// Tests being planned, each depending on the next.
    visiting: Vec<String>,
    /// Planned tests, every one after the tests it depends on.
    order: Vec<String>,
}

impl<R: FnMut(&str) -> Loaded> Planner<R> {
    fn read(&mut self, file: &str) -> &Loaded {
        if !self.loaded.contains_key(file) {
            let loaded = (self.read)(file);
            self.loaded.insert(file.to_owned(), loaded);
        }
        &self.loaded[file]
    }

    fn flow(&mut self, file: &str) -> Option<&Flow> {
        match self.read(file) {
            Loaded::Test(flow) => Some(flow),
            Loaded::Unreadable | Loaded::Missing => None,
        }
    }

    /// Plans `file` after the tests it depends on.
    fn visit(&mut self, file: &str) {
        if let Some(at) = self.visiting.iter().position(|f| f == file) {
            let mut tests = self.visiting[at..].to_vec();
            tests.push(file.to_owned());
            for member in &self.visiting[at..] {
                let node = self
                    .nodes
                    .get_mut(member)
                    .expect("a visited test has a node");
                node.problem.get_or_insert_with(|| Blocker {
                    dependency: file.to_owned(),
                    cause: file.to_owned(),
                    cause_name: file.to_owned(),
                    reason: BlockReason::Cycle {
                        tests: tests.clone(),
                    },
                });
            }
            return;
        }
        if self.nodes.contains_key(file) {
            return;
        }
        let (name, written) = match self.flow(file) {
            Some(flow) => (flow.name.clone(), flow.depends_on.clone()),
            None => (file.to_owned(), Vec::new()),
        };
        let mut node = Node {
            name,
            depends_on: Vec::new(),
            problem: None,
        };
        let mut found = Vec::new();
        for path in &written {
            let Some(dependency) = resolve_test(file, path) else {
                node.problem.get_or_insert_with(|| missing(path));
                continue;
            };
            if matches!(self.read(&dependency), Loaded::Missing) {
                node.problem.get_or_insert_with(|| missing(&dependency));
            } else if !found.contains(&dependency) {
                found.push(dependency);
            }
        }
        node.depends_on = found.clone();
        self.nodes.insert(file.to_owned(), node);
        self.visiting.push(file.to_owned());
        for dependency in found {
            self.visit(&dependency);
        }
        self.visiting.pop();
        self.order.push(file.to_owned());
    }

    /// The test, or a test it runs, has a block that acts on the screen or looks at it. Commands,
    /// requests, files, databases, pauses, dates, loops, conditions on values and scripts don't,
    /// except AppleScript, which usually drives apps through their windows. The clipboard does:
    /// tests side by side would fight over it. A test that can't be read counts as one that does.
    fn uses_screen(&mut self, file: &str, seen: &mut HashSet<String>) -> bool {
        if !seen.insert(file.to_owned()) {
            return false;
        }
        let Some(flow) = self.flow(file) else {
            return true;
        };
        let mut runs = Vec::new();
        for block in &flow.blocks {
            match &block.kind {
                BlockKind::RunTest { test, .. } => match resolve_test(file, test) {
                    Some(test) => runs.push(test),
                    None => return true,
                },
                BlockKind::Script { language, .. } if *language != ScriptLanguage::AppleScript => {}
                BlockKind::Command { .. }
                | BlockKind::Http { .. }
                | BlockKind::CheckFile { .. }
                | BlockKind::Database { .. }
                | BlockKind::Pause { .. }
                | BlockKind::DateTime { .. }
                | BlockKind::Repeat { .. }
                | BlockKind::UseData { .. } => {}
                BlockKind::If {
                    condition: Condition::Value(_),
                    else_if,
                    ..
                } if else_if
                    .iter()
                    .all(|way| matches!(way.condition, Condition::Value(_))) => {}
                _ => return true,
            }
        }
        runs.into_iter().any(|test| self.uses_screen(&test, seen))
    }
}

fn missing(dependency: &str) -> Blocker {
    Blocker {
        dependency: dependency.to_owned(),
        cause: dependency.to_owned(),
        cause_name: dependency.to_owned(),
        reason: BlockReason::Missing,
    }
}

/// `path`, relative to the folder of the test `file` (relative to the project), as a path
/// relative to the project; `None` when it leads out of the project.
pub fn resolve_test(file: &str, path: &str) -> Option<String> {
    if path.starts_with('/') {
        return None;
    }
    let mut parts: Vec<&str> = file.split('/').collect();
    parts.pop();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

/// Problems with the dependencies of `flow`, saved as the test `file` of the folder `project`:
/// tests that don't exist, the test itself, and a circle of tests depending on each other.
pub fn dependency_issues(flow: &Flow, project: &Path, file: &str) -> Vec<Issue> {
    dependency_issues_with(flow, file, |f| load(project, f))
}

fn dependency_issues_with(
    flow: &Flow,
    file: &str,
    mut read: impl FnMut(&str) -> Loaded,
) -> Vec<Issue> {
    let mut out = Vec::new();
    let mut targets = Vec::new();
    for path in &flow.depends_on {
        let issue = |code| Issue::error(code).with("field", "dependsOn");
        match resolve_test(file, path) {
            Some(target) if target == file => out.push(issue("flow.dependency.self")),
            Some(target) if !matches!(read(&target), Loaded::Missing) => targets.push(target),
            _ => out.push(issue("flow.dependency.missing").with("test", path)),
        }
    }
    // The shortest way back to this test, through the files as saved.
    let mut came_from: HashMap<String, String> = HashMap::new();
    let mut queue: std::collections::VecDeque<String> = targets.into_iter().collect();
    for t in &queue {
        came_from.insert(t.clone(), file.to_owned());
    }
    while let Some(current) = queue.pop_front() {
        let Loaded::Test(next) = read(&current) else {
            continue;
        };
        for path in &next.depends_on {
            let Some(target) = resolve_test(&current, path) else {
                continue;
            };
            if target == file {
                let mut cycle = vec![current.clone()];
                while let Some(before) = came_from.get(cycle.last().expect("never empty")) {
                    cycle.push(before.clone());
                    if before == file {
                        break;
                    }
                }
                cycle.reverse();
                cycle.push(file.to_owned());
                out.push(
                    Issue::error("flow.dependency.cycle")
                        .with("field", "dependsOn")
                        .with("cycle", cycle.join(" → ")),
                );
                return out;
            }
            if !came_from.contains_key(&target) {
                came_from.insert(target.clone(), current.clone());
                queue.push_back(target);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_paths_from_the_test_folder() {
        assert_eq!(
            resolve_test("a.flow.json", "b.flow.json").as_deref(),
            Some("b.flow.json")
        );
        assert_eq!(
            resolve_test("smoke/a.flow.json", "../shared/./login.flow.json").as_deref(),
            Some("shared/login.flow.json")
        );
        assert_eq!(resolve_test("a.flow.json", "../outside.flow.json"), None);
        assert_eq!(resolve_test("a.flow.json", "/etc/x.flow.json"), None);
    }

    /// A test with one click block, depending on `deps`.
    fn test(name: &str, deps: &[&str]) -> Flow {
        let mut flow = crate::parse(&format!(
            r#"{{"format": {}, "name": "{name}", "app": "com.apple.TextEdit", "start": "a",
                "blocks": {{"a": {{"type": "pressKeys", "keys": "cmd+n"}}}}}}"#,
            crate::FORMAT_VERSION
        ))
        .unwrap();
        flow.depends_on = deps.iter().map(|d| d.to_string()).collect();
        flow
    }

    fn project(files: &[(&str, Flow)]) -> impl FnMut(&str) -> Loaded + use<> {
        let files: HashMap<String, Flow> = files
            .iter()
            .map(|(f, flow)| (f.to_string(), flow.clone()))
            .collect();
        move |file| match file {
            "broken.flow.json" => Loaded::Unreadable,
            _ => files
                .get(file)
                .cloned()
                .map_or(Loaded::Missing, |flow| Loaded::Test(Box::new(flow))),
        }
    }

    fn chosen(files: &[&str]) -> Vec<String> {
        files.iter().map(|f| f.to_string()).collect()
    }

    fn files(batch: &Batch) -> Vec<&str> {
        batch.tests().iter().map(|t| t.file.as_str()).collect()
    }

    fn shop() -> impl FnMut(&str) -> Loaded {
        project(&[
            ("login.flow.json", test("Login", &[])),
            ("cart/add.flow.json", test("Add", &["../login.flow.json"])),
            ("cart/pay.flow.json", test("Pay", &["add.flow.json"])),
            ("about.flow.json", test("About", &[])),
        ])
    }

    #[test]
    fn dependencies_run_first_and_missing_ones_are_added() {
        let batch = Batch::plan(
            &chosen(&["cart/pay.flow.json", "about.flow.json"]),
            RunMode::Sequential,
            shop(),
        );
        assert_eq!(
            files(&batch),
            [
                "login.flow.json",
                "cart/add.flow.json",
                "cart/pay.flow.json",
                "about.flow.json"
            ]
        );
        let added: Vec<bool> = batch.tests().iter().map(|t| t.added).collect();
        assert_eq!(added, [true, true, false, false]);
        assert_eq!(batch.tests()[2].depends_on, ["cart/add.flow.json"]);
        assert!(batch.tests().iter().all(|t| t.uses_screen));
    }

    #[test]
    fn a_failed_dependency_blocks_the_chain_with_the_real_cause() {
        let mut batch = Batch::plan(
            &chosen(&["cart/pay.flow.json", "about.flow.json"]),
            RunMode::Sequential,
            shop(),
        );
        assert_eq!(batch.start_next(), Some(0));
        assert_eq!(batch.start_next(), None, "one at a time");
        assert_eq!(
            batch.wait(1),
            Some(WaitingFor::Dependency {
                test: "login.flow.json".into()
            })
        );
        assert_eq!(batch.finish(0, TestOutcome::Failed), [1, 2]);
        let TestState::Blocked(pay) = &batch.tests()[2].state else {
            panic!("pay is blocked")
        };
        assert_eq!(pay.dependency, "cart/add.flow.json");
        assert_eq!(pay.cause, "login.flow.json");
        assert_eq!(pay.describe_in("en"), "Blocked: Login failed");
        assert_eq!(
            pay.describe_in("ru"),
            "Заблокирован: тест «Login» не прошёл"
        );
        assert_eq!(batch.start_next(), Some(3), "independent tests still run");
        batch.finish(3, TestOutcome::Passed);
        assert!(batch.is_over());
    }

    #[test]
    fn a_blocker_of_a_newer_jevision_reads_and_describes() {
        let blocker: Blocker = serde_json::from_value(serde_json::json!({
            "dependency": "login.flow.json",
            "cause": "login.flow.json",
            "causeName": "Login",
            "reason": {"kind": "quarantined", "until": "2027-01-01"},
        }))
        .unwrap();
        assert_eq!(blocker.reason, BlockReason::Unknown);
        assert_eq!(blocker.describe_in("en"), "Blocked by Login");
        assert_eq!(
            blocker.describe_in("ru"),
            "Заблокирован из-за теста «Login»"
        );
    }

    #[test]
    fn a_dependency_that_couldnt_start_or_is_missing_blocks_too() {
        let read = project(&[
            ("a.flow.json", test("A", &["broken.flow.json"])),
            ("b.flow.json", test("B", &["gone.flow.json"])),
        ]);
        let mut batch = Batch::plan(
            &chosen(&["a.flow.json", "b.flow.json"]),
            RunMode::Sequential,
            read,
        );
        assert_eq!(
            files(&batch),
            ["broken.flow.json", "a.flow.json", "b.flow.json"]
        );
        let TestState::Blocked(b) = &batch.tests()[2].state else {
            panic!("b is blocked before anything runs")
        };
        assert_eq!(b.describe_in("en"), "Blocked: gone.flow.json doesn’t exist");
        assert_eq!(batch.start_next(), Some(0));
        assert_eq!(batch.finish(0, TestOutcome::NotRun), [1]);
        let TestState::Blocked(a) = &batch.tests()[1].state else {
            panic!("a is blocked")
        };
        assert_eq!(
            a.describe_in("en"),
            "Blocked: broken.flow.json couldn’t run"
        );
        assert!(batch.is_over());
    }

    #[test]
    fn a_cycle_blocks_its_tests_and_names_the_circle() {
        let read = project(&[
            ("a.flow.json", test("A", &["b.flow.json"])),
            ("b.flow.json", test("B", &["a.flow.json"])),
            ("c.flow.json", test("C", &["a.flow.json"])),
        ]);
        let batch = Batch::plan(&chosen(&["c.flow.json"]), RunMode::Sequential, read);
        assert!(
            batch
                .tests()
                .iter()
                .all(|t| matches!(t.state, TestState::Blocked(_)))
        );
        let TestState::Blocked(c) = &batch.tests()[2].state else {
            unreachable!()
        };
        assert_eq!(
            c.describe_in("en"),
            "Blocked: the tests depend on each other in a circle: a.flow.json → b.flow.json → a.flow.json"
        );
    }

    #[test]
    fn parallel_runs_share_the_screen_and_respect_the_limit() {
        let mut quiet = test("Quiet", &[]);
        quiet.blocks = vec![crate::Block {
            id: "a".into(),
            kind: BlockKind::RunTest {
                test: "q2.flow.json".into(),
                package: None,
                inputs: Default::default(),
                next: None,
            },
        }];
        let mut q2 = quiet.clone();
        q2.blocks.clear();
        let read = project(&[
            ("s1.flow.json", test("S1", &[])),
            ("s2.flow.json", test("S2", &[])),
            ("q1.flow.json", quiet.clone()),
            ("q2.flow.json", q2),
            ("q3.flow.json", quiet),
        ]);
        let all = chosen(&[
            "s1.flow.json",
            "s2.flow.json",
            "q1.flow.json",
            "q3.flow.json",
        ]);
        let mut batch = Batch::plan(&all, RunMode::Parallel { limit: 3 }, read);
        let screens: Vec<bool> = batch.tests().iter().map(|t| t.uses_screen).collect();
        assert_eq!(screens, [true, true, false, false]);
        assert_eq!(batch.start_next(), Some(0));
        assert_eq!(batch.start_next(), Some(2), "s2 waits for the screen");
        assert_eq!(batch.start_next(), Some(3));
        assert_eq!(batch.start_next(), None, "three at a time");
        assert_eq!(batch.wait(1), Some(WaitingFor::Screen));
        batch.finish(2, TestOutcome::Passed);
        assert_eq!(batch.start_next(), None, "the screen is still taken");
        batch.finish(0, TestOutcome::Passed);
        assert_eq!(batch.start_next(), Some(1));
    }

    /// A test with one command block, which never touches the screen, depending on `deps`.
    fn quiet(name: &str, deps: &[&str]) -> Flow {
        let mut flow = crate::parse(&format!(
            r#"{{"format": {}, "name": "{name}", "app": "com.apple.TextEdit", "start": "a",
                "blocks": {{"a": {{"type": "command", "command": "true"}}}}}}"#,
            crate::FORMAT_VERSION
        ))
        .unwrap();
        flow.depends_on = deps.iter().map(|d| d.to_string()).collect();
        flow
    }

    #[test]
    fn parallel_runs_wait_for_dependencies_however_many_places_are_free() {
        let read = project(&[
            ("base.flow.json", quiet("Base", &[])),
            ("a.flow.json", quiet("A", &["base.flow.json"])),
            ("b.flow.json", quiet("B", &["base.flow.json"])),
            ("c.flow.json", quiet("C", &[])),
        ]);
        let all = chosen(&["a.flow.json", "b.flow.json", "c.flow.json"]);
        let mut batch = Batch::plan(&all, RunMode::Parallel { limit: 4 }, read);
        assert_eq!(
            files(&batch),
            [
                "base.flow.json",
                "a.flow.json",
                "b.flow.json",
                "c.flow.json"
            ]
        );
        assert_eq!(batch.start_next(), Some(0));
        assert_eq!(batch.start_next(), Some(3), "c needs nobody");
        assert_eq!(batch.start_next(), None, "a and b wait for base");
        assert_eq!(
            batch.wait(1),
            Some(WaitingFor::Dependency {
                test: "base.flow.json".into()
            })
        );
        batch.finish(3, TestOutcome::Passed);
        assert_eq!(batch.start_next(), None, "base is still running");
        batch.finish(0, TestOutcome::Passed);
        assert_eq!(batch.start_next(), Some(1));
        assert_eq!(batch.start_next(), Some(2));
    }

    #[test]
    fn a_failed_dependency_blocks_its_tests_while_the_others_go_on() {
        let read = project(&[
            ("base.flow.json", quiet("Base", &[])),
            ("a.flow.json", quiet("A", &["base.flow.json"])),
            ("b.flow.json", quiet("B", &["a.flow.json"])),
            ("c.flow.json", quiet("C", &[])),
        ]);
        let all = chosen(&["b.flow.json", "c.flow.json"]);
        let mut batch = Batch::plan(&all, RunMode::Parallel { limit: 3 }, read);
        assert_eq!(batch.start_next(), Some(0));
        assert_eq!(batch.start_next(), Some(3));
        assert_eq!(batch.finish(0, TestOutcome::Failed), [1, 2]);
        let TestState::Blocked(blocker) = &batch.tests()[2].state else {
            panic!("b should be blocked");
        };
        assert_eq!(blocker.describe(), "Blocked: Base failed");
        assert_eq!(batch.start_next(), None);
        batch.finish(3, TestOutcome::Passed);
        assert!(batch.is_over());
    }

    #[test]
    fn a_parallel_plan_keeps_its_rules_at_every_step() {
        let read = || {
            project(&[
                ("setup.flow.json", quiet("Setup", &[])),
                ("login.flow.json", test("Login", &["setup.flow.json"])),
                ("inbox.flow.json", test("Inbox", &["login.flow.json"])),
                ("api.flow.json", quiet("Api", &["setup.flow.json"])),
                ("clock.flow.json", quiet("Clock", &[])),
                ("files.flow.json", quiet("Files", &["clock.flow.json"])),
                ("paste.flow.json", test("Paste", &[])),
            ])
        };
        let all = chosen(&[
            "inbox.flow.json",
            "api.flow.json",
            "files.flow.json",
            "paste.flow.json",
        ]);
        for limit in 1..=6 {
            let mut batch = Batch::plan(&all, RunMode::Parallel { limit }, read());
            assert_eq!(batch.tests().len(), 7);
            let mut running: Vec<usize> = Vec::new();
            let mut started = Vec::new();
            while !batch.is_over() {
                while let Some(i) = batch.start_next() {
                    let t = &batch.tests()[i];
                    assert!(
                        t.depends_on
                            .iter()
                            .all(|d| batch.tests()[batch.position(d).unwrap()].state
                                == TestState::Done(TestOutcome::Passed)),
                        "{} started before its dependencies passed",
                        t.file
                    );
                    running.push(i);
                    started.push(i);
                    assert!(running.len() <= limit as usize, "limit {limit} exceeded");
                    assert!(
                        running
                            .iter()
                            .filter(|&&r| batch.tests()[r].uses_screen)
                            .count()
                            <= 1,
                        "two tests on the screen at limit {limit}"
                    );
                }
                assert!(!running.is_empty(), "the plan got stuck at limit {limit}");
                // The test that started first finishes first.
                batch.finish(running.remove(0), TestOutcome::Passed);
            }
            started.sort_unstable();
            assert_eq!(started, (0..7).collect::<Vec<_>>(), "every test ran once");
        }
    }

    #[test]
    fn stopping_a_parallel_run_skips_the_waiting_tests_and_lets_the_running_ones_end() {
        let read = project(&[
            ("a.flow.json", quiet("A", &[])),
            ("b.flow.json", quiet("B", &[])),
            ("c.flow.json", quiet("C", &["a.flow.json"])),
            ("d.flow.json", quiet("D", &[])),
        ]);
        let all = chosen(&["a.flow.json", "b.flow.json", "c.flow.json", "d.flow.json"]);
        let mut batch = Batch::plan(&all, RunMode::Parallel { limit: 2 }, read);
        assert_eq!(batch.start_next(), Some(0));
        assert_eq!(batch.start_next(), Some(1));
        batch.stop();
        assert!(!batch.is_over(), "a and b are still running");
        assert_eq!(batch.start_next(), None);
        batch.finish(0, TestOutcome::Cancelled);
        batch.finish(1, TestOutcome::Cancelled);
        assert!(batch.is_over());
        assert!(
            batch.tests()[2..]
                .iter()
                .all(|t| t.state == TestState::Done(TestOutcome::Cancelled))
        );
    }

    #[test]
    fn only_blocks_on_the_screen_keep_a_test_from_running_beside_others() {
        let flow = |blocks: &str| {
            crate::parse(&format!(
                r#"{{"format": {}, "name": "T", "app": "com.apple.TextEdit", "start": "a",
                    "blocks": {{{blocks}}}}}"#,
                crate::FORMAT_VERSION
            ))
            .unwrap()
        };
        let read = project(&[
            (
                "quiet.flow.json",
                flow(
                    r#""a": {"type": "script", "language": "python3", "code": "print(1)", "next": "b"},
                    "b": {"type": "command", "command": "true", "next": "c"},
                    "c": {"type": "http", "url": "https://x.test", "next": "d"},
                    "d": {"type": "checkFile", "path": "out.txt", "until": "exists", "next": "e"},
                    "e": {"type": "database", "file": "a.sqlite", "query": "select 1", "next": "f"},
                    "f": {"type": "repeat", "times": 2, "body": "g", "next": "h"},
                    "g": {"type": "pause", "duration": "1s"},
                    "h": {"type": "useData", "file": "users.csv", "body": "i", "next": "j"},
                    "i": {"type": "if", "value": {"of": "${data.name}", "is": "equals", "to": "x"}},
                    "j": {"type": "dateTime", "saveAs": "today", "next": "k"},
                    "k": {"type": "runTest", "test": "sub/setup.flow.json"}"#,
                ),
            ),
            (
                "sub/setup.flow.json",
                flow(r#""a": {"type": "command", "command": "true"}"#),
            ),
            (
                "apple.flow.json",
                flow(r#""a": {"type": "script", "language": "appleScript", "code": "beep"}"#),
            ),
            (
                "looks.flow.json",
                flow(
                    r#""a": {"type": "if", "value": {"of": "1", "is": "equals", "to": "1"},
                        "elseIf": [{"reference": {"image": "refs/a.png"}}]}"#,
                ),
            ),
            (
                "calls.flow.json",
                flow(r#""a": {"type": "runTest", "test": "s1.flow.json"}"#),
            ),
            (
                "clip.flow.json",
                flow(r#""a": {"type": "clipboard", "action": "read", "saveAs": "x"}"#),
            ),
            ("s1.flow.json", test("S1", &[])),
        ]);
        let all = chosen(&[
            "quiet.flow.json",
            "apple.flow.json",
            "looks.flow.json",
            "calls.flow.json",
            "clip.flow.json",
        ]);
        let batch = Batch::plan(&all, RunMode::Parallel { limit: 5 }, read);
        let screens: Vec<bool> = batch.tests().iter().map(|t| t.uses_screen).collect();
        assert_eq!(screens, [false, true, true, true, true]);
    }

    #[test]
    fn stopping_skips_the_rest_without_blocking() {
        let mut batch = Batch::plan(
            &chosen(&["cart/pay.flow.json"]),
            RunMode::Sequential,
            shop(),
        );
        assert_eq!(batch.start_next(), Some(0));
        batch.stop();
        assert_eq!(batch.finish(0, TestOutcome::Cancelled), Vec::<usize>::new());
        assert!(
            batch.tests()[1..]
                .iter()
                .all(|t| t.state == TestState::Done(TestOutcome::Cancelled))
        );
        assert!(batch.is_over());
    }

    #[test]
    fn issues_name_missing_tests_and_the_circle() {
        let read = || {
            project(&[
                ("a.flow.json", test("A", &["sub/b.flow.json"])),
                ("sub/b.flow.json", test("B", &["c.flow.json"])),
                ("sub/c.flow.json", test("C", &["../a.flow.json"])),
            ])
        };
        let a = test("A", &["sub/b.flow.json", "nope.flow.json", "a.flow.json"]);
        let issues = dependency_issues_with(&a, "a.flow.json", read());
        let codes: Vec<&str> = issues.iter().map(|i| i.code.as_str()).collect();
        assert_eq!(
            codes,
            [
                "flow.dependency.missing",
                "flow.dependency.self",
                "flow.dependency.cycle"
            ]
        );
        assert_eq!(
            issues[2].param("cycle"),
            Some("a.flow.json → sub/b.flow.json → sub/c.flow.json → a.flow.json")
        );
        assert!(
            dependency_issues_with(&test("B", &["c.flow.json"]), "sub/x.flow.json", read())
                .is_empty()
        );
    }
}
