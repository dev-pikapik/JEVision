//! A test drafted from a description in words.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use jevision_core::{FORMAT_VERSION, Flow, Issue, Severity, describe, parse, validate_in};
use jevision_vision::Message;
use serde_json::{Value, json};

use super::{AiError, AiSession, AiStep, AiToolCall, Finished, Job, McpProblem, McpServer};
use crate::vision::VisionSettings;

/// What to draft.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct DraftRequest {
    /// The test in the person's words.
    pub description: String,
    /// Bundle id of the app under test.
    pub app: String,
    /// Name of the test; empty lets the model name it.
    pub name: String,
    /// The test open now, which the draft replaces; `None` for an empty one.
    pub current: Option<Flow>,
    /// Folder of the test file, for the files the test reads.
    pub folder: String,
    /// A draft to rework, and what to change in it.
    pub previous: Option<Flow>,
    pub feedback: String,
    /// Screenshots may go only to models on this computer: no checks by meaning in the draft
    /// unless the model is local.
    pub local_only: bool,
}

/// A drafted test, not saved yet.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct TestDraft {
    pub flow: Flow,
    /// What the test does, in a sentence or two.
    pub summary: String,
    /// Warnings left, such as reference pictures that are yet to be taken.
    pub issues: Vec<Issue>,
    pub calls: Vec<AiToolCall>,
    pub unavailable: Vec<McpProblem>,
}

#[uniffi::export]
impl AiSession {
    /// Drafts a test from `request`, with the tools of `servers`. Nothing is sent before
    /// [`AiSession::step`].
    #[uniffi::constructor]
    pub fn draft_test(
        settings: VisionSettings,
        servers: Vec<McpServer>,
        request: DraftRequest,
    ) -> Result<Arc<Self>, AiError> {
        // Checks by meaning send screenshots to the model, which must see them and be allowed to.
        let meaning = settings.pictures
            && (!request.local_only || settings.client().is_ok_and(|c| c.config().is_local()));
        let messages = conversation(&request);
        let job = DraftJob {
            app: request.app,
            name: request.name.trim().to_owned(),
            folder: PathBuf::from(request.folder),
            meaning,
        };
        Self::new(&settings, servers, Job::Draft(job), messages)
    }
}

pub(crate) struct DraftJob {
    pub(super) app: String,
    pub(super) name: String,
    pub(super) folder: PathBuf,
    /// Checks by meaning are allowed.
    pub(super) meaning: bool,
}

fn conversation(request: &DraftRequest) -> Vec<Message> {
    let mut asked = format!(
        "Write a test for the app {}. What it should do:\n{}",
        request.app,
        request.description.trim()
    );
    if let Some(current) = request.current.as_ref().filter(|f| !f.blocks.is_empty()) {
        asked.push_str(&format!(
            "\n\nThe test as it is now, which your test replaces. Keep the blocks that still \
fit, with their ids:\n{}",
            jevision_core::to_canonical_json(current)
        ));
    }
    let mut messages = vec![Message::asked(asked)];
    if let Some(previous) = &request.previous {
        messages.push(Message::answered(
            json!({ "answer": { "test": test_json(previous), "summary": "" } }).to_string(),
        ));
        messages.push(Message::asked(format!(
            "Change the test: {}",
            request.feedback.trim()
        )));
    }
    messages
}

fn test_json(flow: &Flow) -> Value {
    let mut test: Value =
        serde_json::from_str(&jevision_core::to_canonical_json(flow)).unwrap_or_default();
    if let Some(test) = test.as_object_mut() {
        // The model writes blocks, not where they stand on the canvas.
        test.remove("canvas");
    }
    test
}

pub(crate) fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "test": { "type": "object", "description": "the whole test file" },
            "summary": { "type": "string", "description": "what the test does, in the language of the description" },
        },
        "required": ["test", "summary"],
    })
}

impl DraftJob {
    pub(crate) fn instructions(&self) -> String {
        let checks = if self.meaning {
            "Check the result of the important steps with check blocks. Prefer \"compare\": \
\"meaning\" with an \"expect\" sentence saying what the screen should show; use a reference \
image only for a screen that must look exactly the same each time."
        } else {
            "Check the result of the important steps with check blocks comparing the screen \
with a reference image (refs/<what>.png); never use \"compare\": \"meaning\", which can't be used \
here."
        };
        format!(
            "You write JEVision tests. A JEVision test checks the user interface of a Mac app \
by doing what a person would do in it, step by step. It is a flowchart of blocks saved as JSON.\n\
\n\
The file format and every block type:\n{guide}\n\
\n\
Rules:\n\
- Use only these block types and fields. Start with openApp.\n\
- Click on elements by their picture: {{\"image\": \"refs/<what>.png\"}} with a short name of \
the element, such as refs/save-button.png. The person takes these pictures after accepting the \
test. Use a point only for a place that never moves.\n\
- {checks}\n\
- Wait for the app where it needs time (waitFor) instead of fixed pauses.\n\
- Where the description has alternatives (\"if a dialog appears…\"), use an if block whose \
branches meet again.\n\
- Type secrets as ${{secret.NAME}}, never their values.\n\
- Make the test as short as the description allows.\n\
- \"summary\" says what the test does, in one or two sentences in the language of the description.",
            guide = crate::mcp::blocks::guide(),
        )
    }

    pub(crate) fn finish(
        &mut self,
        answer: &Value,
        calls: &[AiToolCall],
        unavailable: &[McpProblem],
    ) -> Finished {
        let mut test = match &answer["test"] {
            Value::String(s) => serde_json::from_str(s).unwrap_or_default(),
            other => other.clone(),
        };
        let Some(fields) = test.as_object_mut() else {
            return Finished::Fix("Give the whole test as a JSON object in \"test\".".to_owned());
        };
        fields.insert("format".to_owned(), json!(FORMAT_VERSION));
        fields.insert("app".to_owned(), json!(self.app));
        fields.remove("canvas");
        if !self.name.is_empty() {
            fields.insert("name".to_owned(), json!(self.name));
        }
        let mut flow = match parse(&test.to_string()) {
            Ok(flow) => flow,
            Err(e) => return Finished::Fix(listed(&e.issues)),
        };
        let issues = validate_in(&flow, folder(&self.folder));
        if issues.iter().any(|i| i.severity == Severity::Error) {
            return Finished::Fix(listed(&issues));
        }
        flow.arrange_automatically();
        Finished::Done(Box::new(AiStep::Drafted {
            draft: TestDraft {
                flow,
                summary: answer["summary"]
                    .as_str()
                    .unwrap_or_default()
                    .trim()
                    .to_owned(),
                issues,
                calls: calls.to_vec(),
                unavailable: unavailable.to_vec(),
            },
        }))
    }
}

fn folder(folder: &Path) -> &Path {
    if folder.as_os_str().is_empty() {
        Path::new(".")
    } else {
        folder
    }
}

fn listed(issues: &[Issue]) -> String {
    issues
        .iter()
        .filter(|i| i.severity == Severity::Error)
        .map(|i| format!("- {}", describe(i)))
        .collect::<Vec<_>>()
        .join("\n")
}
