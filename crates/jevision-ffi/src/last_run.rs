//! How each block of a test did in a saved run: the badges on the canvas and the reason the
//! inspector shows for a failed block.

use std::collections::HashMap;

use jevision_core::Flow;
use jevision_runner::{StepStatus, Verdict};

use crate::{Failure, RunReport};

/// How a block did in the last attempt of a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum BlockOutcome {
    Passed,
    Failed,
    /// The block didn't run because a block before it failed or the run was stopped.
    Skipped,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct BlockResult {
    pub block: String,
    pub outcome: BlockOutcome,
    /// Why the block failed. For a loop or a `runTest` block, the reason the step inside it failed.
    pub failure: Option<Failure>,
}

/// How each block of `flow`, saved as `file` relative to the project, did in the last attempt
/// of `report`, in reading order (see [`Flow::blocks_in_flow_order`]). `file` may be a test the run's test ran with a `runTest`
/// block. Blocks the attempt left out for another reason, such as the branch an `if` didn't
/// take, have no result, and neither do blocks the test no longer has.
#[uniffi::export]
pub fn block_results(flow: Flow, file: String, report: RunReport) -> Vec<BlockResult> {
    let Some(attempt) = report.attempts.last() else {
        return Vec::new();
    };
    let blocks: HashMap<&str, &jevision_core::Block> =
        flow.blocks.iter().map(|b| (b.id.as_str(), b)).collect();
    let steps: Vec<_> = attempt
        .steps
        .iter()
        .filter(|s| s.test == file && blocks.contains_key(s.block.as_str()))
        .collect();

    let mut results: HashMap<&str, BlockResult> = HashMap::new();
    // Where the attempt stopped: blocks that failed or were stopped, with the branch they took.
    let mut stops: Vec<(&str, Option<&str>)> = Vec::new();
    for step in &steps {
        let outcome = match step.status {
            StepStatus::Passed | StepStatus::BaselineCreated => BlockOutcome::Passed,
            StepStatus::Failed => BlockOutcome::Failed,
            StepStatus::Cancelled => BlockOutcome::Skipped,
        };
        if outcome != BlockOutcome::Passed {
            stops.push((&step.block, step.branch.as_deref()));
        }
        let failure = (outcome == BlockOutcome::Failed)
            .then(|| step.failure.clone().or_else(|| attempt.failure.clone()))
            .flatten();
        let entry = results
            .entry(step.block.as_str())
            .or_insert_with(|| BlockResult {
                block: step.block.clone(),
                outcome,
                failure: None,
            });
        // A failure in any pass through a loop counts; a stop doesn't undo a pass.
        if outcome == BlockOutcome::Failed || entry.outcome == BlockOutcome::Skipped {
            entry.outcome = outcome;
            entry.failure = failure;
        }
    }
    // Stopped between two blocks, such as while paused: the rest goes on from the last one.
    if stops.is_empty()
        && attempt.verdict != Verdict::Passed
        && let Some(last) = steps.last()
    {
        stops.push((&last.block, last.branch.as_deref()));
    }

    let mut queue: Vec<&str> = Vec::new();
    for (block, branch) in stops {
        queue.extend(
            blocks[block]
                .kind
                .edges()
                .into_iter()
                .filter(|(edge, _)| branch.is_none_or(|b| b == *edge))
                .map(|(_, to)| to),
        );
    }
    while let Some(id) = queue.pop() {
        let Some(block) = blocks.get(id) else {
            continue;
        };
        if results.contains_key(id) {
            continue;
        }
        results.insert(
            id,
            BlockResult {
                block: id.to_owned(),
                outcome: BlockOutcome::Skipped,
                failure: None,
            },
        );
        queue.extend(block.kind.edges().into_iter().map(|(_, to)| to));
    }

    flow.blocks_in_flow_order()
        .into_iter()
        .filter_map(|b| results.remove(b.id.as_str()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Attempt, Step};

    const FILE: &str = "t.flow.json";

    fn flow() -> Flow {
        jevision_core::parse(
            r#"{"format":1,"name":"T","app":"com.example.App","start":"open","blocks":{
              "open":{"type":"openApp","next":"if"},
              "if":{"type":"if","reference":{"image":"refs/on.png"},"then":"yes","else":"no"},
              "yes":{"type":"pressKeys","keys":"cmd+s","next":"end"},
              "no":{"type":"pressKeys","keys":"cmd+w","next":"end"},
              "end":{"type":"pressKeys","keys":"cmd+q"}}}"#,
        )
        .unwrap()
    }

    fn step(test: &str, block: &str, status: StepStatus, branch: Option<&str>) -> Step {
        Step {
            seq: 0,
            test: test.into(),
            block: block.into(),
            block_type: String::new(),
            status,
            duration_ms: 0,
            before: None,
            after: None,
            expected: None,
            diff: None,
            reference: None,
            similarity: None,
            vision: None,
            branch: branch.map(Into::into),
            failure: (status == StepStatus::Failed).then(|| failure("run.keys.failed")),
            video_at_ms: None,
        }
    }

    fn failure(code: &str) -> Failure {
        Failure {
            code: code.into(),
            params: HashMap::new(),
        }
    }

    fn report(attempts: Vec<(Verdict, Vec<Step>)>) -> RunReport {
        RunReport {
            test: "T".into(),
            file: FILE.into(),
            app: "com.example.App".into(),
            started_at: "2026-10-18T10:00:00Z".into(),
            duration_ms: 0,
            verdict: attempts.last().map_or(Verdict::Failed, |a| a.0),
            flaky: false,
            baselines_created: Vec::new(),
            requested_by: None,
            environment: None,
            video: None,
            attempts: attempts
                .into_iter()
                .enumerate()
                .map(|(i, (verdict, steps))| Attempt {
                    number: i as u32 + 1,
                    verdict,
                    duration_ms: 0,
                    failure: (verdict == Verdict::Failed).then(|| failure("run.attempt")),
                    steps,
                })
                .collect(),
        }
    }

    fn outcomes(results: &[BlockResult]) -> Vec<(&str, BlockOutcome)> {
        results
            .iter()
            .map(|r| (r.block.as_str(), r.outcome))
            .collect()
    }

    #[test]
    fn a_failure_skips_what_comes_after_it_but_not_the_branch_not_taken() {
        let report = report(vec![(
            Verdict::Failed,
            vec![
                step(FILE, "open", StepStatus::Passed, None),
                step(FILE, "if", StepStatus::Passed, Some("then")),
                step(FILE, "yes", StepStatus::Failed, None),
            ],
        )]);
        let results = block_results(flow(), FILE.into(), report);
        assert_eq!(
            outcomes(&results),
            [
                ("open", BlockOutcome::Passed),
                ("if", BlockOutcome::Passed),
                ("yes", BlockOutcome::Failed),
                ("end", BlockOutcome::Skipped),
            ]
        );
        assert_eq!(results[2].failure, Some(failure("run.keys.failed")));
        assert_eq!(results[0].failure, None);
    }

    #[test]
    fn only_the_last_attempt_counts() {
        let all = vec![
            step(FILE, "open", StepStatus::Passed, None),
            step(FILE, "if", StepStatus::Passed, Some("else")),
            step(FILE, "no", StepStatus::Passed, None),
            step(FILE, "end", StepStatus::Passed, None),
        ];
        let report = report(vec![
            (
                Verdict::Failed,
                vec![step(FILE, "open", StepStatus::Failed, None)],
            ),
            (Verdict::Passed, all),
        ]);
        let results = block_results(flow(), FILE.into(), report);
        assert_eq!(
            outcomes(&results),
            [
                ("open", BlockOutcome::Passed),
                ("if", BlockOutcome::Passed),
                ("end", BlockOutcome::Passed),
                ("no", BlockOutcome::Passed),
            ]
        );
    }

    #[test]
    fn a_stop_between_blocks_skips_the_rest_of_the_branch_taken() {
        let report = report(vec![(
            Verdict::Cancelled,
            vec![
                step(FILE, "open", StepStatus::Passed, None),
                step(FILE, "if", StepStatus::Passed, Some("else")),
            ],
        )]);
        assert_eq!(
            outcomes(&block_results(flow(), FILE.into(), report)),
            [
                ("open", BlockOutcome::Passed),
                ("if", BlockOutcome::Passed),
                ("end", BlockOutcome::Skipped),
                ("no", BlockOutcome::Skipped),
            ]
        );
    }

    #[test]
    fn a_block_failed_inside_takes_the_attempts_reason() {
        let mut outer = step(FILE, "open", StepStatus::Failed, None);
        outer.failure = None;
        let report = report(vec![(
            Verdict::Failed,
            vec![
                step("other.flow.json", "open", StepStatus::Failed, None),
                outer,
            ],
        )]);
        let results = block_results(flow(), FILE.into(), report);
        assert_eq!(results[0].outcome, BlockOutcome::Failed);
        assert_eq!(results[0].failure, Some(failure("run.attempt")));
        assert_eq!(results.len(), 5, "every block after the failure is skipped");
    }

    #[test]
    fn steps_of_another_test_or_of_removed_blocks_are_left_out() {
        let report = report(vec![(
            Verdict::Passed,
            vec![
                step("other.flow.json", "if", StepStatus::Passed, None),
                step(FILE, "gone", StepStatus::Passed, None),
                step(FILE, "open", StepStatus::Passed, None),
            ],
        )]);
        assert_eq!(
            outcomes(&block_results(flow(), FILE.into(), report)),
            [("open", BlockOutcome::Passed)]
        );
    }
}
