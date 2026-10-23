//! A batch of thousands of tests must hand out the next test, take in how one did and tell what
//! changed as fast per test as a short batch does: the time is compared with a smaller batch on
//! the same machine and build, never with a fixed budget.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use jevision_core::{Batch, FORMAT_VERSION, Loaded, RunMode, TestOutcome};

/// How much bigger the large batch is than the small one.
const FACTOR: usize = 10;

/// `n` tests in chains of ten, each depending on the one before it; every other one uses the
/// screen. One after another: side by side, the tests waiting for the screen are told again each
/// time it frees up or gets busy, which a test can't tell apart from slow work.
fn batch(n: usize) -> Batch {
    let files: Vec<String> = (0..n).map(|k| format!("t{k}.flow.json")).collect();
    Batch::plan(&files, RunMode::Sequential, |file| {
        let k: usize = file
            .trim_start_matches('t')
            .trim_end_matches(".flow.json")
            .parse()
            .unwrap();
        let block = if k.is_multiple_of(2) {
            r#"{"type": "pressKeys", "keys": "cmd+n"}"#
        } else {
            r#"{"type": "command", "command": "true"}"#
        };
        let mut flow = jevision_core::parse(&format!(
            r#"{{"format": {FORMAT_VERSION}, "name": "T{k}", "app": "com.apple.TextEdit",
                "start": "a", "blocks": {{"a": {block}}}}}"#
        ))
        .unwrap();
        if !k.is_multiple_of(10) {
            flow.depends_on = vec![format!("t{}.flow.json", k - 1)];
        }
        Loaded::Test(Box::new(flow))
    })
}

/// Runs the whole batch as the app does: starts what may start, finishes the test that started
/// first, one in fifty failing, and looks at what changed. Returns how many tests ran.
fn run(mut batch: Batch) -> usize {
    let mut running = VecDeque::new();
    let mut ran = 0;
    loop {
        while let Some(i) = batch.start_next() {
            running.push_back(i);
        }
        let Some(i) = running.pop_front() else {
            break;
        };
        let outcome = if i % 50 == 5 {
            TestOutcome::Failed
        } else {
            TestOutcome::Passed
        };
        batch.finish(i, outcome);
        for j in batch.take_changes() {
            std::hint::black_box(batch.wait(j));
        }
        ran += 1;
    }
    assert!(batch.is_over());
    ran
}

/// The fastest of a few runs of `work` on a fresh copy of `batch`, so a busy machine skews the
/// time as little as possible.
fn fastest(batch: &Batch) -> Duration {
    (0..5)
        .map(|_| {
            let fresh = batch.clone();
            let clock = Instant::now();
            std::hint::black_box(run(fresh));
            clock.elapsed()
        })
        .min()
        .unwrap()
}

#[test]
fn a_batch_of_thousands_of_tests_takes_no_longer_per_test() {
    let (small, large) = (batch(2_000), batch(2_000 * FACTOR));
    // Each failure blocks the four tests after it in its chain.
    assert_eq!(run(large.clone()), 20_000 - 4 * 400);
    let (small_time, large_time) = (fastest(&small), fastest(&large));
    println!("2000 and 20000 tests: {small_time:?} -> {large_time:?}");
    // Work per test grows the time ten times; work over every pair of tests, a hundred times.
    let limit = FACTOR as u32 * 5;
    assert!(
        large_time < small_time.max(Duration::from_millis(1)) * limit,
        "20000 tests took {large_time:?}, more than {limit} times the {small_time:?} of 2000"
    );
}
