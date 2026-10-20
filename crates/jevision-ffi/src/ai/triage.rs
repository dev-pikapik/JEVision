//! Why a run failed, and the one change most likely to fix it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use jevision_core::{BlockKind, DEFAULT_WAIT_TIMEOUT_MS, Flow, ProjectSettings, START, Slot};
use jevision_runner::{Run, StepStatus};
use jevision_vision::image::Image;
use jevision_vision::{MaskedImage, Message};
use serde_json::{Value, json};

use super::{
    AiError, AiSafeguards, AiSession, AiStep, AiToolCall, Finished, Job, McpProblem, McpServer,
};
use crate::vision::VisionSettings;
use crate::{Edited, Failure, FlakyEvidence, Step, deciding_attempt};

/// Longer side of a picture sent with the question, in pixels.
const PICTURE_SIDE: u32 = 1568;
/// Retries the fix "add retries" sets.
pub const SUGGESTED_RETRIES: u32 = 2;
/// Longest wait the fix "add a wait" sets on a waitFor block.
const LONGEST_WAIT_MS: u64 = 120_000;

/// What to explain.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct FailureRequest {
    /// Folder of the run.
    pub run: String,
    /// Project folder.
    pub project: String,
    /// Screenshots may go only to models on this computer.
    pub local_only: bool,
    /// How the test did in its recent runs, when known.
    pub flaky: Option<FlakyEvidence>,
}

/// What most likely made the run fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FailureCause {
    /// The app does something wrong: the test caught a bug.
    AppBug,
    /// The test fails now and then for no reason in the app, such as timing.
    FlakyTest,
    /// The app changed on purpose and the reference picture shows the old look.
    OutdatedReference,
    /// Something around the test: the Mac, the network, test data, permissions.
    Environment,
}

/// A change the app can make in one click.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FailureFix {
    /// Make the screenshot of the failed check its reference.
    UpdateReference,
    /// Wait for the screen to settle before the failed block, or wait longer in it.
    AddWait,
    /// Try the test again when it fails.
    AddRetries,
    /// Keep the test running but stop its failures from failing the project.
    Quarantine,
    RunAgain,
    /// Nothing to change automatically: look at the block.
    OpenBlock,
}

/// The model's explanation of a failed run.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct FailureExplanation {
    pub cause: FailureCause,
    /// Why, in a few sentences in the person's language.
    pub explanation: String,
    pub fix: FailureFix,
    /// Test file of the run, relative to the project folder.
    pub test: String,
    /// The block that failed.
    pub block: Option<String>,
    /// Screenshots sent with the question, private parts covered.
    pub pictures_sent: u32,
    /// Screenshots kept back: the model reads only text, or they may go only to a model on
    /// this computer.
    pub pictures_withheld: bool,
    pub calls: Vec<AiToolCall>,
    pub unavailable: Vec<McpProblem>,
}

#[uniffi::export]
impl AiSession {
    /// Explains why the run in `request.run` failed, with the tools of `servers`. Reads the
    /// run at once; nothing is sent before [`AiSession::step`].
    #[uniffi::constructor(default(safeguards = None))]
    pub fn explain_failure(
        settings: VisionSettings,
        servers: Vec<McpServer>,
        request: FailureRequest,
        language: String,
        safeguards: Option<AiSafeguards>,
    ) -> Result<Arc<Self>, AiError> {
        let local = settings.client().is_ok_and(|c| c.config().is_local());
        let pictures = settings.pictures && (!request.local_only || local);
        let (job, message) = TriageJob::read(&request, pictures, &language)?;
        Self::new(
            &settings,
            safeguards,
            servers,
            Job::Triage(job),
            vec![message],
        )
    }
}

/// Makes the test wait before block `block`: waits longer when it is a waitFor, otherwise
/// adds a waitFor for a still screen just before it. `None` for an unknown block, or one that
/// nothing leads to.
#[uniffi::export]
pub fn add_wait_before(mut flow: Flow, block: String) -> Option<Edited> {
    let i = flow.blocks.iter().position(|b| b.id == block)?;
    if let BlockKind::WaitFor { timeout_ms, .. } = &mut flow.blocks[i].kind {
        *timeout_ms = (*timeout_ms * 2).clamp(DEFAULT_WAIT_TIMEOUT_MS * 3, LONGEST_WAIT_MS);
        return Some(Edited {
            flow,
            selection: Some(block),
        });
    }
    let slot = if flow.start == block {
        Slot {
            from: String::new(),
            edge: START.to_owned(),
        }
    } else {
        flow.blocks.iter().find_map(|b| {
            let edge = b.kind.edges().into_iter().find(|(_, to)| *to == block)?.0;
            Some(Slot {
                from: b.id.clone(),
                edge: edge.into_owned(),
            })
        })?
    };
    let id = flow.insert_block("waitFor", Some(&slot))?;
    Some(Edited {
        flow,
        selection: Some(id),
    })
}

pub(crate) struct TriageJob {
    test: String,
    block: Option<String>,
    offered: Vec<FailureFix>,
    pictures_sent: u32,
    pictures_withheld: bool,
    language: String,
}

impl TriageJob {
    /// The test and the failed block the run was about.
    pub(super) fn logged_under(&self) -> (&str, &str) {
        (&self.test, self.block.as_deref().unwrap_or_default())
    }

    /// The job and the question about the run, with the screenshots of the failed step.
    pub(super) fn read(
        request: &FailureRequest,
        pictures: bool,
        language: &str,
    ) -> Result<(Self, Message), AiError> {
        let run = Run::load(Path::new(&request.run)).map_err(|e| AiError::NoReport {
            reason: e.to_string(),
        })?;
        let project = Path::new(&request.project);
        let report = crate::RunReport::from(run.report.clone());
        let attempt = deciding_attempt(&report);
        let steps = attempt.as_ref().map_or(&[][..], |a| &a.steps[..]);
        let failed = steps
            .iter()
            .rev()
            .find(|s| s.status == StepStatus::Failed && s.failure.is_some());
        let flow = std::fs::read_to_string(project.join(&report.file))
            .ok()
            .and_then(|text| jevision_core::parse(&text).ok());
        let settings = ProjectSettings::load(project).unwrap_or_default();

        let mut offered = Vec::new();
        if let Some(step) = failed {
            if step.block_type == "check" && step.after.is_some() && step.reference.is_some() {
                offered.push(FailureFix::UpdateReference);
            }
            if flow.as_ref().is_some_and(|f| {
                step.test == report.file && add_wait_before(f.clone(), step.block.clone()).is_some()
            }) {
                offered.push(FailureFix::AddWait);
            }
        }
        let retries = flow
            .as_ref()
            .and_then(|f| f.retries)
            .or(settings.retries)
            .unwrap_or(0);
        if flow.is_some() && retries < SUGGESTED_RETRIES {
            offered.push(FailureFix::AddRetries);
        }
        if settings
            .quarantined(&report.file, &jevision_runner::today())
            .is_none()
        {
            offered.push(FailureFix::Quarantine);
        }
        offered.push(FailureFix::RunAgain);
        if failed.is_some() {
            offered.push(FailureFix::OpenBlock);
        }

        let mut text = format!(
            "A run of the test \"{}\" ({}) of the app {} failed. Tell why.\n",
            report.test, report.file, report.app
        );
        if run.report.attempts.len() > 1 {
            text.push_str(&format!(
                "It was tried {} times.\n",
                run.report.attempts.len()
            ));
        }
        if let Some(e) = &request.flaky {
            text.push_str(&format!(
                "Its recent runs: {} passed, {} passed only on a retry, {} failed.\n",
                e.passed, e.passed_on_retry, e.failed
            ));
        }
        if let Some(env) = &run.report.environment {
            text.push_str(&format!("Environment: {env}.\n"));
        }
        if let Some(reason) = attempt.as_ref().and_then(|a| a.failure.clone()) {
            text.push_str(&format!("Why it stopped: {}\n", english(reason)));
        }
        text.push_str("\nThe steps of the deciding try:\n");
        for step in steps {
            text.push_str(&step_line(step));
        }
        if let Some(flow) = &flow {
            text.push_str(&format!(
                "\nThe test file:\n{}",
                jevision_core::to_canonical_json(flow)
            ));
        }

        let mut message = Message::asked(String::new());
        let mut withheld = false;
        if let Some(step) = failed {
            let shots = [
                (
                    "The screen after the step",
                    step.after.as_ref().or(step.before.as_ref()),
                ),
                ("The reference it was compared with", step.expected.as_ref()),
                ("Where they differ", step.diff.as_ref()),
            ];
            for (label, path) in shots {
                let Some(picture) = path.and_then(|p| picture(&run.dir, p)) else {
                    continue;
                };
                if pictures {
                    message.pictures.push((label.to_owned(), picture));
                } else {
                    withheld = true;
                }
            }
        }
        if withheld {
            text.push_str("\n\nThe screenshots are not shown to you; judge from the steps.");
        }
        message.text = text;

        let job = Self {
            test: report.file.clone(),
            block: failed.map(|s| s.block.clone()),
            offered,
            pictures_sent: message.pictures.len() as u32,
            pictures_withheld: withheld,
            language: language.to_owned(),
        };
        Ok((job, message))
    }

    pub(crate) fn instructions(&self) -> String {
        let fixes: Vec<String> = self
            .offered
            .iter()
            .map(|f| format!("- {}: {}", fix_id(*f), fix_meaning(*f)))
            .collect();
        format!(
            "You explain why a run of a JEVision test failed. A JEVision test checks the user \
interface of a Mac app step by step: it clicks and types, waits for the screen and compares it \
with reference pictures or checks it by meaning. Private parts of the screenshots are painted \
over in grey on purpose.\n\
\n\
Decide what most likely made the run fail:\n\
- appBug: the app does something wrong, and the test caught it.\n\
- flakyTest: the test fails now and then for no fault of the app, such as clicking before the \
screen is ready.\n\
- outdatedReference: the app changed on purpose and the reference picture shows its old look.\n\
- environment: something around the test, such as the Mac, the network, test data or \
permissions.\n\
\n\
Then pick the one change that helps most:\n{}\n\
\n\
\"explanation\" says why, in two to four short sentences a person who doesn't program \
understands, in the language {}. Name the step and what the screenshots show.",
            fixes.join("\n"),
            self.language
        )
    }

    pub(crate) fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "cause": {
                    "type": "string",
                    "enum": ["appBug", "flakyTest", "outdatedReference", "environment"],
                },
                "explanation": { "type": "string" },
                "fix": {
                    "type": "string",
                    "enum": self.offered.iter().map(|f| fix_id(*f)).collect::<Vec<_>>(),
                },
            },
            "required": ["cause", "explanation", "fix"],
        })
    }

    pub(crate) fn finish(
        &mut self,
        answer: &Value,
        calls: &[AiToolCall],
        unavailable: &[McpProblem],
    ) -> Finished {
        let cause = match answer["cause"].as_str() {
            Some("appBug") => FailureCause::AppBug,
            Some("flakyTest") => FailureCause::FlakyTest,
            Some("outdatedReference") => FailureCause::OutdatedReference,
            Some("environment") => FailureCause::Environment,
            _ => {
                return Finished::Fix(
                    "\"cause\" must be appBug, flakyTest, outdatedReference or environment."
                        .to_owned(),
                );
            }
        };
        let explanation = answer["explanation"].as_str().unwrap_or_default().trim();
        if explanation.is_empty() {
            return Finished::Fix("Say why in \"explanation\".".to_owned());
        }
        let picked = answer["fix"]
            .as_str()
            .and_then(|id| self.offered.iter().copied().find(|f| fix_id(*f) == id));
        let fix = picked.unwrap_or_else(|| self.fallback(cause));
        Finished::Done(Box::new(AiStep::Explained {
            explanation: FailureExplanation {
                cause,
                explanation: explanation.to_owned(),
                fix,
                test: self.test.clone(),
                block: self.block.clone(),
                pictures_sent: self.pictures_sent,
                pictures_withheld: self.pictures_withheld,
                calls: calls.to_vec(),
                unavailable: unavailable.to_vec(),
            },
        }))
    }

    /// The change that fits `cause` when the model picked none of those offered.
    fn fallback(&self, cause: FailureCause) -> FailureFix {
        let wanted: &[FailureFix] = match cause {
            FailureCause::AppBug => &[FailureFix::OpenBlock],
            FailureCause::FlakyTest => &[FailureFix::AddWait, FailureFix::AddRetries],
            FailureCause::OutdatedReference => &[FailureFix::UpdateReference],
            FailureCause::Environment => &[FailureFix::RunAgain],
        };
        wanted
            .iter()
            .copied()
            .find(|f| self.offered.contains(f))
            .unwrap_or(FailureFix::RunAgain)
    }
}

fn fix_id(fix: FailureFix) -> &'static str {
    match fix {
        FailureFix::UpdateReference => "updateReference",
        FailureFix::AddWait => "addWait",
        FailureFix::AddRetries => "addRetries",
        FailureFix::Quarantine => "quarantine",
        FailureFix::RunAgain => "runAgain",
        FailureFix::OpenBlock => "openBlock",
    }
}

fn fix_meaning(fix: FailureFix) -> &'static str {
    match fix {
        FailureFix::UpdateReference => {
            "make the screenshot of the failed check its new reference picture"
        }
        FailureFix::AddWait => {
            "wait for the screen to settle before the failed step, or wait longer in it"
        }
        FailureFix::AddRetries => "try the test again when it fails",
        FailureFix::Quarantine => {
            "put the test in quarantine: it keeps running, but its failures don't fail the project"
        }
        FailureFix::RunAgain => "run the test again, as it is",
        FailureFix::OpenBlock => "nothing to change automatically: the person looks at the step",
    }
}

fn step_line(step: &Step) -> String {
    let status = match step.status {
        StepStatus::Passed => "passed",
        StepStatus::Failed => "failed",
        StepStatus::BaselineCreated => "saved its reference",
        StepStatus::Cancelled => "cancelled",
        StepStatus::Unknown => "status unknown to this version",
    };
    let mut line = format!(
        "{}. {} {} ({}): {status}",
        step.seq, step.block_type, step.block, step.test
    );
    if let Some(similarity) = step.similarity {
        line.push_str(&format!(", similarity {:.1}%", similarity * 100.0));
    }
    if let Some(vision) = &step.vision {
        line.push_str(&format!(", the AI check said: {}", vision.reason));
    }
    if let Some(failure) = &step.failure {
        line.push_str(&format!(". {}", english(failure.clone())));
    }
    line.push('\n');
    line
}

/// The failure's sentence in English, the language of the question to the model.
fn english(failure: Failure) -> String {
    jevision_runner::describe_in(jevision_l10n::SOURCE_LANGUAGE, &failure.into())
}

/// The saved screenshot `path` of the run in `dir`, scaled for sending. Reports are saved with
/// private parts already covered; files outside the run folder are not read.
fn picture(dir: &Path, path: &str) -> Option<MaskedImage> {
    let dir = dir.canonicalize().ok()?;
    let file: PathBuf = dir.join(path).canonicalize().ok()?;
    if !file.starts_with(&dir) {
        return None;
    }
    let image = Image::decode(&std::fs::read(file).ok()?).ok()?;
    MaskedImage::view(&image, &[], None, PICTURE_SIDE).ok()
}
