//! A local MCP server: AI assistants on this Mac list, read, edit, check and run the project's
//! tests through it. Messages are JSON-RPC 2.0, one per line on standard input and output.
//!
//! Assistants see the same JSON people edit, never a secret value or an API key: test files
//! hold only `${secret.NAME}`, and reports are saved with secrets masked. Runs and screenshots
//! work only while the person allows them in the app.

pub(crate) mod blocks;
mod jobs;
mod tools;

#[cfg(test)]
mod tests;

use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use crate::{Hands, HandsError, RunOptions, SecretSource, TestRun};
use jevision_runner::Screenshot;

/// Protocol versions this server speaks, newest first.
const PROTOCOL_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

/// What the person allowed assistants to do, in the app's settings.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, uniffi::Record)]
pub struct AssistantPermissions {
    /// Run tests: open apps, click and type on this Mac.
    pub run_tests: bool,
    /// Take screenshots of app windows.
    pub screenshots: bool,
}

/// The platform side of the server, implemented by the app. Calls come from the server's
/// threads, never the main one.
#[uniffi::export(with_foreign)]
pub trait AssistantHost: Send + Sync {
    /// Read before every run and screenshot, so a change in the settings applies at once.
    fn permissions(&self) -> AssistantPermissions;
    /// Why this Mac can't run tests right now (locked screen, missing permission), in words for
    /// the person; `None` when it can.
    fn readiness_problem(&self) -> Option<String>;
    /// Hands for `run`, which starts right after. Pressing Esc twice should cancel `run`.
    fn start_run(&self, run: Arc<TestRun>) -> Result<Arc<dyn Hands>, HandsError>;
    /// A person used the mouse or keyboard and `run` paused: resume it once they let go.
    /// Called on the run's thread; return at once.
    fn run_paused(&self, run: Arc<TestRun>);
    /// The run started last has ended.
    fn run_ended(&self);
    fn secrets(&self) -> Arc<dyn SecretSource>;
    /// How runs start: the model for checks by meaning, chosen in the app.
    fn run_options(&self) -> RunOptions;
    /// The front window of the application with bundle id `app`.
    fn capture(&self, app: String) -> Result<Screenshot, HandsError>;
}

/// The server could not start or lost its input or output.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum AssistantServerError {
    #[error("there is no folder {path}")]
    NoProject { path: String },
    #[error("{reason}")]
    Io { reason: String },
}

/// Serves the tests in the folder `project` over standard input and output until the input
/// ends; a run that is going then stops.
#[uniffi::export]
pub fn serve_assistants(
    project: String,
    host: Arc<dyn AssistantHost>,
) -> Result<(), AssistantServerError> {
    let server = Server::new(Path::new(&project), host)?;
    server
        .serve(io::stdin().lock(), io::stdout())
        .map_err(|e| AssistantServerError::Io {
            reason: e.to_string(),
        })
}

pub(crate) struct Server {
    /// Real path of the project folder.
    project: PathBuf,
    host: Arc<dyn AssistantHost>,
    jobs: jobs::Jobs,
}

impl Server {
    pub(crate) fn new(
        project: &Path,
        host: Arc<dyn AssistantHost>,
    ) -> Result<Self, AssistantServerError> {
        let project = project
            .canonicalize()
            .ok()
            .filter(|p| p.is_dir())
            .ok_or_else(|| AssistantServerError::NoProject {
                path: project.display().to_string(),
            })?;
        Ok(Self {
            project,
            host,
            jobs: jobs::Jobs::default(),
        })
    }

    /// Answers each message on its own thread, so waiting for a run doesn't hold up the rest.
    pub(crate) fn serve(&self, input: impl BufRead, output: impl Write + Send) -> io::Result<()> {
        let output = Mutex::new(output);
        let result = std::thread::scope(|scope| {
            let mut lines = input.lines();
            let ended = loop {
                let line = match lines.next() {
                    None => break Ok(()),
                    Some(Err(e)) => break Err(e),
                    Some(Ok(line)) => line,
                };
                if line.trim().is_empty() {
                    continue;
                }
                let output = &output;
                scope.spawn(move || {
                    if let Some(reply) = self.handle(&line) {
                        let mut out = output.lock().unwrap_or_else(|e| e.into_inner());
                        // A client that is gone can't be told anything.
                        let _ = writeln!(out, "{reply}").and_then(|()| out.flush());
                    }
                });
            };
            // The assistant is gone: nobody watches a run any more.
            self.jobs.stop_all();
            ended
        });
        self.jobs.wait_all();
        result
    }

    /// The reply to one message, or `None` for a notification.
    pub(crate) fn handle(&self, message: &str) -> Option<String> {
        let reply = match serde_json::from_str::<Value>(message) {
            Err(e) => Some(error_reply(
                Value::Null,
                PARSE_ERROR,
                &format!("Parse error: {e}"),
            )),
            Ok(Value::Object(m)) => self.handle_object(&m),
            Ok(_) => Some(error_reply(
                Value::Null,
                INVALID_REQUEST,
                "Invalid request: send one JSON-RPC object per line",
            )),
        };
        reply.map(|r| r.to_string())
    }

    fn handle_object(&self, m: &serde_json::Map<String, Value>) -> Option<Value> {
        let id = m.get("id").cloned();
        let Some(method) = m.get("method").and_then(Value::as_str) else {
            // A response to a request this server never sends, or garbage.
            return match id {
                Some(id) if !m.contains_key("result") && !m.contains_key("error") => Some(
                    error_reply(id, INVALID_REQUEST, "Invalid request: no method"),
                ),
                _ => None,
            };
        };
        // Notifications (initialized, cancelled) need no answer.
        let id = id?;
        if m.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return Some(error_reply(
                id,
                INVALID_REQUEST,
                "Invalid request: jsonrpc must be \"2.0\"",
            ));
        }
        let params = m.get("params").cloned().unwrap_or_else(|| json!({}));
        let result = match method {
            "initialize" => Ok(initialize(&params)),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tools::definitions() })),
            "tools/call" => self.call(&params),
            other => Err((METHOD_NOT_FOUND, format!("Method not found: {other}"))),
        };
        Some(match result {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err((code, message)) => error_reply(id, code, &message),
        })
    }

    fn call(&self, params: &Value) -> Result<Value, (i64, String)> {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or((INVALID_PARAMS, "Invalid params: name the tool".to_owned()))?;
        let empty = json!({});
        let args = params.get("arguments").unwrap_or(&empty);
        if !args.is_object() {
            return Err((
                INVALID_PARAMS,
                "Invalid params: arguments must be an object".to_owned(),
            ));
        }
        let Some(result) = self.call_tool(name, args) else {
            return Err((INVALID_PARAMS, format!("Unknown tool: {name}")));
        };
        Ok(match result {
            Ok(content) => json!({ "content": content, "isError": false }),
            Err(message) => json!({ "content": [text(message)], "isError": true }),
        })
    }
}

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

fn error_reply(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn initialize(params: &Value) -> Value {
    let asked = params.get("protocolVersion").and_then(Value::as_str);
    let version = asked
        .filter(|v| PROTOCOL_VERSIONS.contains(v))
        .unwrap_or(PROTOCOL_VERSIONS[0]);
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": {
            "name": "jevision",
            "title": "JEVision",
            "version": env!("CARGO_PKG_VERSION"),
        },
        "instructions": INSTRUCTIONS,
    })
}

const INSTRUCTIONS: &str = "JEVision tests check the user interface of Mac apps. Each test is a \
flowchart of blocks saved as a JSON file (*.flow.json) in the project folder; paths are relative \
to that folder. Call list_block_types before writing a test to learn the file format. A test runs \
for real on this Mac: JEVision opens the app, clicks and types in it. Running tests and taking \
screenshots work only if the person at the Mac allowed them in JEVision → Settings → AI → \
Assistants. Secret values and API keys are never available: tests refer to secrets as \
${secret.NAME}.";

/// MCP text content.
fn text(text: impl Into<String>) -> Value {
    json!({ "type": "text", "text": text.into() })
}

/// What went wrong on the Mac's side, in words for the assistant to pass on.
fn hands_message(e: &HandsError) -> String {
    match e {
        HandsError::PermissionDenied { permission } => {
            let (can, list) = if permission == "screenRecording" {
                ("record the screen", "Screen & System Audio Recording")
            } else {
                ("control the computer", "Accessibility")
            };
            format!(
                "macOS hasn't allowed the app that started jevision to {can}. The person at this Mac can allow it in System Settings → Privacy & Security → {list}."
            )
        }
        HandsError::AppNotFound { app } => format!("The app {app} isn't installed on this Mac."),
        HandsError::NoWindow { app } => {
            format!("{app} has no open window. Open it, or run a test that opens it, first.")
        }
        HandsError::CannotOpen { item } => format!("{item} can't be opened."),
        HandsError::ScreenLocked => {
            "The screen of this Mac is locked. The person at it needs to unlock it first."
                .to_owned()
        }
        HandsError::StillRunning { app } => format!(
            "{app} didn't quit, so it can't start over. Close it, or answer what it asks, and try again."
        ),
        HandsError::WindowFixed { app } => {
            format!("The window of {app} doesn't allow its size or position to be changed.")
        }
        HandsError::WindowDiffers {
            app,
            what,
            wanted,
            actual,
        } => format!("The {what} of the window of {app} should be {wanted} but is {actual}."),
        HandsError::Failed { reason } => reason.clone(),
    }
}
