//! The AI of the app: it drafts a test from a description and explains why a run failed. On
//! the way it may use tools of the MCP servers the person connected; a tool that changes
//! anything runs only after the person allows that call.
//!
//! An [`AiSession`] is a conversation in steps. [`AiSession::step`] talks to the model, and runs
//! the tools that only read, until there is an answer or a call to allow;
//! [`AiSession::decide`] answers that question and goes on. Every step blocks on the network:
//! call it off the main thread.

mod draft;
mod mcp;
mod triage;

#[cfg(test)]
mod tests;

pub use draft::*;
pub use mcp::*;
pub use triage::*;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use jevision_runner::{VisionClient, VisionJudge};
use jevision_vision::Message;
use serde_json::{Value, json};

use crate::run::Failure;
use crate::vision::VisionSettings;

/// Most tool calls in one session.
const MAX_CALLS: usize = 10;
/// Most times the model is asked to fix an answer that can't be used.
const MAX_FIXES: u32 = 2;

/// A tool call the model wants to make.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct AiToolCall {
    pub server: String,
    pub tool: String,
    /// The tool's human name.
    pub title: String,
    /// The arguments as indented JSON.
    pub arguments: String,
    /// The person allowed it, or it only reads; `false` once declined.
    pub allowed: bool,
}

/// A server that could not be reached; the session went on without its tools.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct McpProblem {
    pub server: String,
    pub error: McpError,
}

/// What a step ended with.
#[allow(clippy::large_enum_variant, reason = "UniFFI enums can't hold a Box")]
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum AiStep {
    /// The model wants to call a tool that may change something: [`AiSession::decide`].
    Approve {
        call: AiToolCall,
    },
    Drafted {
        draft: TestDraft,
    },
    Explained {
        explanation: FailureExplanation,
    },
}

/// Why a session stopped.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum AiError {
    #[error("unknown AI provider `{provider}`")]
    UnknownProvider { provider: String },
    /// A `run.vision.*` code with its values, and the English sentence.
    #[error("{message}")]
    Provider { failure: Failure, message: String },
    /// The run's report can't be read.
    #[error("the report can't be read: {reason}")]
    NoReport { reason: String },
    /// The model's answers could not be used, even after it was asked to fix them.
    #[error("the AI's answer can't be used: {problems}")]
    Unusable { problems: String },
    #[error("the AI asked for more than {limit} tool calls")]
    TooManyCalls { limit: u32 },
    #[error("stopped")]
    Cancelled,
}

/// A conversation with the model about one task.
#[derive(uniffi::Object)]
pub struct AiSession {
    state: Mutex<State>,
    cancelled: AtomicBool,
}

struct State {
    client: VisionClient,
    servers: Vec<McpServer>,
    /// Open once the first step needs them.
    connections: Option<Vec<Connection>>,
    unavailable: Vec<McpProblem>,
    job: Job,
    messages: Vec<Message>,
    calls: Vec<AiToolCall>,
    /// The call waiting for the person: connection, tool and arguments.
    pending: Option<(usize, String, Value)>,
    fixes: u32,
}

enum Job {
    Draft(DraftJob),
    Triage(TriageJob),
}

/// What became of the model's answer.
pub(crate) enum Finished {
    Done(Box<AiStep>),
    /// Tell the model what is wrong and ask again.
    Fix(String),
}

impl Job {
    fn instructions(&self) -> String {
        match self {
            Self::Draft(j) => j.instructions(),
            Self::Triage(j) => j.instructions(),
        }
    }

    fn schema(&self) -> Value {
        match self {
            Self::Draft(_) => draft::schema(),
            Self::Triage(j) => j.schema(),
        }
    }

    fn finish(
        &mut self,
        answer: &Value,
        calls: &[AiToolCall],
        unavailable: &[McpProblem],
    ) -> Finished {
        match self {
            Self::Draft(j) => j.finish(answer, calls, unavailable),
            Self::Triage(j) => j.finish(answer, calls, unavailable),
        }
    }
}

impl AiSession {
    fn new(
        settings: &VisionSettings,
        servers: Vec<McpServer>,
        job: Job,
        messages: Vec<Message>,
    ) -> Result<Arc<Self>, AiError> {
        let client = settings.client().map_err(|_| AiError::UnknownProvider {
            provider: settings.provider.clone(),
        })?;
        Ok(Arc::new(Self {
            state: Mutex::new(State {
                client,
                servers,
                connections: None,
                unavailable: Vec::new(),
                job,
                messages,
                calls: Vec::new(),
                pending: None,
                fixes: 0,
            }),
            cancelled: AtomicBool::new(false),
        }))
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn go_on(&self, state: &mut State) -> Result<AiStep, AiError> {
        let connections = state.connections.get_or_insert_with(Vec::new);
        if connections.is_empty() && !state.servers.is_empty() {
            for server in std::mem::take(&mut state.servers) {
                self.check()?;
                match Connection::open(&server, mcp::TIMEOUT) {
                    Ok(c) => connections.push(c),
                    Err(error) => state.unavailable.push(McpProblem {
                        server: server.name,
                        error,
                    }),
                }
            }
        }
        loop {
            self.check()?;
            let system = system(&state.job, state.connections.as_deref().unwrap_or_default());
            let schema = answer_schema(state.job.schema(), state.has_tools());
            let answer = state
                .client
                .ask(&system, &state.messages, &schema)
                .map_err(|e| provider_error(&state.client, &e))?;
            self.check()?;
            state.messages.push(Message::answered(answer.to_string()));
            if let Some(call) = answer.get("tool").filter(|t| t.is_object()) {
                if state.calls.len() >= MAX_CALLS {
                    return Err(AiError::TooManyCalls {
                        limit: MAX_CALLS as u32,
                    });
                }
                match state.resolve(call) {
                    Err(problem) => state.messages.push(Message::asked(problem)),
                    Ok((index, tool, arguments)) => {
                        let record = state.record(index, &tool, &arguments, None);
                        let read_only = record.allowed;
                        if !read_only {
                            state.pending = Some((index, tool, arguments));
                            return Ok(AiStep::Approve { call: record });
                        }
                        state.calls.push(record);
                        self.check()?;
                        state.run(index, &tool, &arguments);
                    }
                }
                continue;
            }
            let answer = answer.get("answer").cloned().unwrap_or(answer);
            let State {
                job,
                calls,
                unavailable,
                ..
            } = state;
            match job.finish(&answer, calls, unavailable) {
                Finished::Done(step) => return Ok(*step),
                Finished::Fix(problems) if state.fixes < MAX_FIXES => {
                    state.fixes += 1;
                    state.messages.push(Message::asked(format!(
                        "This answer can't be used:\n{problems}\nFix it and answer again."
                    )));
                }
                Finished::Fix(problems) => return Err(AiError::Unusable { problems }),
            }
        }
    }

    fn check(&self) -> Result<(), AiError> {
        if self.cancelled.load(Ordering::Relaxed) {
            Err(AiError::Cancelled)
        } else {
            Ok(())
        }
    }
}

#[uniffi::export]
impl AiSession {
    /// Talks to the model until it answers or wants a call allowed.
    pub fn step(&self) -> Result<AiStep, AiError> {
        let mut state = self.state();
        if let Some((index, tool, arguments)) = state.pending.take() {
            // Still waiting for the person.
            let call = state.record(index, &tool, &arguments, Some(false));
            state.pending = Some((index, tool, arguments));
            return Ok(AiStep::Approve { call });
        }
        self.go_on(&mut state)
    }

    /// Runs the call waiting for the person if `allow`, tells the model it was declined
    /// otherwise, and goes on.
    pub fn decide(&self, allow: bool) -> Result<AiStep, AiError> {
        let mut state = self.state();
        if let Some((index, tool, arguments)) = state.pending.take() {
            let record = state.record(index, &tool, &arguments, Some(allow));
            state.calls.push(record);
            if allow {
                self.check()?;
                state.run(index, &tool, &arguments);
            } else {
                state.messages.push(Message::asked(
                    "The person did not allow this call. Do without it, and don't ask for it again.",
                ));
            }
        }
        self.go_on(&mut state)
    }

    /// Stops the session: the step going on ends with [`AiError::Cancelled`] once the request
    /// it is waiting for returns, and so does every step after.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

impl State {
    /// The call of `tool` as the person sees it; allowed when it only reads, unless `allowed`
    /// says otherwise.
    fn record(
        &self,
        index: usize,
        tool: &str,
        arguments: &Value,
        allowed: Option<bool>,
    ) -> AiToolCall {
        let connection = &self.connections.as_ref().expect("opened")[index];
        let info = connection.tools.iter().find(|t| t.name == tool);
        AiToolCall {
            server: connection.name.clone(),
            tool: tool.to_owned(),
            title: info.map_or_else(|| tool.to_owned(), |t| t.title.clone()),
            arguments: serde_json::to_string_pretty(arguments).unwrap_or_default(),
            allowed: allowed.unwrap_or_else(|| info.is_some_and(|t| t.read_only)),
        }
    }

    fn has_tools(&self) -> bool {
        self.connections
            .as_ref()
            .is_some_and(|c| c.iter().any(|c| !c.tools.is_empty()))
    }

    /// The connection, tool and arguments of a call, or what is wrong with it for the model.
    fn resolve(&self, call: &Value) -> Result<(usize, String, Value), String> {
        let server = call["server"].as_str().unwrap_or_default();
        let name = call["name"].as_str().unwrap_or_default();
        let connections = self.connections.as_deref().unwrap_or_default();
        let Some(index) = connections.iter().position(|c| c.name == server) else {
            return Err(format!(
                "There is no server \"{server}\". Use one from the list."
            ));
        };
        if !connections[index].tools.iter().any(|t| t.name == name) {
            return Err(format!(
                "{server} has no tool \"{name}\". Use one from the list."
            ));
        }
        let arguments = match &call["arguments"] {
            Value::Null => json!({}),
            Value::String(s) => serde_json::from_str(s).unwrap_or_else(|_| json!({})),
            other => other.clone(),
        };
        if !arguments.is_object() {
            return Err("The arguments must be a JSON object.".to_owned());
        }
        Ok((index, name.to_owned(), arguments))
    }

    /// Calls the tool and gives the model what came of it.
    fn run(&mut self, index: usize, tool: &str, arguments: &Value) {
        let connection = &mut self.connections.as_mut().expect("opened")[index];
        let said = match connection.call(tool, arguments) {
            Ok(outcome) if outcome.is_error => {
                format!("{}/{tool} failed:\n{}", connection.name, outcome.text)
            }
            Ok(outcome) => format!("{}/{tool} returned:\n{}", connection.name, outcome.text),
            Err(e) => format!("{}/{tool} could not be called: {e}", connection.name),
        };
        self.messages.push(Message::asked(said));
    }
}

/// The job's instructions and the tools there are.
fn system(job: &Job, connections: &[Connection]) -> String {
    let mut system = job.instructions();
    let tools: Vec<Value> = connections
        .iter()
        .flat_map(|c| {
            c.tools.iter().map(|t| {
                json!({
                    "server": c.name,
                    "name": t.name,
                    "description": t.description,
                    "inputSchema": serde_json::from_str::<Value>(&t.input_schema).unwrap_or_default(),
                    "changesThings": !t.read_only,
                })
            })
        })
        .collect();
    if !tools.is_empty() {
        system.push_str(&format!(
            "\n\nYou can use tools of MCP servers the person connected, such as their issue \
tracker, when they help with the task. To call one, answer {{\"tool\": {{\"server\": …, \"name\": …, \
\"arguments\": {{…}}}}}} and nothing else; the next message gives you its result. Tools that change \
things run only if the person allows the call. At most {MAX_CALLS} calls. When you're done, answer \
{{\"answer\": …}}. The tools:\n{}",
            Value::Array(tools)
        ));
    } else {
        system.push_str("\n\nAnswer {\"answer\": …}.");
    }
    system
}

/// The shape of every answer: a tool call or the answer itself.
fn answer_schema(answer: Value, tools: bool) -> Value {
    if !tools {
        return json!({
            "type": "object",
            "properties": { "answer": answer },
            "required": ["answer"],
        });
    }
    json!({
        "type": "object",
        "properties": {
            "tool": {
                "type": "object",
                "description": "A tool to call instead of answering now.",
                "properties": {
                    "server": { "type": "string" },
                    "name": { "type": "string" },
                    "arguments": { "type": "object" },
                },
                "required": ["server", "name", "arguments"],
            },
            "answer": answer,
        },
    })
}

fn provider_error(client: &VisionClient, e: &jevision_runner::VisionError) -> AiError {
    let failure = jevision_runner::vision_failure(client as &dyn VisionJudge, e);
    AiError::Provider {
        message: jevision_runner::describe_vision(&failure),
        failure: failure.into(),
    }
}
