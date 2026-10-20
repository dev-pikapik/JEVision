//! A local MCP server: AI assistants on this Mac list, read, edit, check and run the project's
//! tests through it. Messages are JSON-RPC 2.0, one per line on standard input and output.
//!
//! Assistants see the same JSON people edit, never a secret value or an API key: test files
//! hold only `${secret.NAME}`, and reports are saved with secrets masked. Runs and screenshots
//! work only while the person allows them in the app, and each run asks the person first unless
//! they turned that off.

mod agents;
pub(crate) mod blocks;
mod jobs;
pub(crate) mod network;
mod tools;

#[cfg(test)]
pub(crate) mod tests;

use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};

use jevision_l10n::tr;
use serde_json::{Value, json};

pub use agents::*;
pub use network::*;

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
    /// Ask the person before each run, which an assistant started: they see what it wants to
    /// do and allow it or not.
    pub ask_first: bool,
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
    /// A folder the app and every server share, for who connected and the questions the
    /// person is asked. The app reads it with `connected_assistants` and
    /// `pending_assistant_requests`.
    fn shared_folder(&self) -> String;
}

/// The server could not start or lost its input or output.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum AssistantServerError {
    #[error("{}", tr!("mcp.error.noProject", .path))]
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

/// What answers an assistant's messages: the tools on this Mac, or the connection to the tools
/// on another one. `Err` is a JSON-RPC error code and its message.
pub(crate) trait Backend: Sync {
    fn initialize(&self, params: &Value) -> Result<Value, (i64, String)>;
    fn tools(&self) -> Result<Value, (i64, String)>;
    fn call(&self, params: &Value) -> Result<Value, (i64, String)>;
    /// Called every [`agents::HEARTBEAT`] while the assistant is connected.
    fn beat(&self);
    /// The assistant is gone and replies are still being waited for: nobody watches a run any
    /// more.
    fn hang_up(&self);
    /// Every reply has been sent.
    fn finish(&self);
}

pub(crate) struct Server {
    /// Real path of the project folder.
    project: PathBuf,
    host: Arc<dyn AssistantHost>,
    jobs: Arc<jobs::Jobs>,
    agents: Arc<agents::Agents>,
}

impl Server {
    pub(crate) fn new(
        project: &Path,
        host: Arc<dyn AssistantHost>,
    ) -> Result<Self, AssistantServerError> {
        let agents = Arc::new(agents::Agents::new(&host.shared_folder()));
        Self::shared(project, host, agents, Arc::default())
    }

    /// A server for the project that shares with others what an assistant connected to the
    /// app over the network does in all the projects it uses: who it is, and its runs.
    fn shared(
        project: &Path,
        host: Arc<dyn AssistantHost>,
        agents: Arc<agents::Agents>,
        jobs: Arc<jobs::Jobs>,
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
            jobs,
            agents,
        })
    }

    pub(crate) fn serve(&self, input: impl BufRead, output: impl Write + Send) -> io::Result<()> {
        serve_lines(self, input, output)
    }

    /// The reply to one message, or `None` for a notification.
    #[cfg(test)]
    pub(crate) fn handle(&self, message: &str) -> Option<String> {
        handle_line(self, message)
    }
}

impl Backend for Server {
    fn initialize(&self, params: &Value) -> Result<Value, (i64, String)> {
        self.agents.hello(params);
        Ok(initialize(params))
    }

    fn tools(&self) -> Result<Value, (i64, String)> {
        Ok(json!({ "tools": tools::definitions() }))
    }

    fn call(&self, params: &Value) -> Result<Value, (i64, String)> {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or((INVALID_PARAMS, "Invalid params: name the tool".to_owned()))?;
        self.agents.called(name);
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

    fn beat(&self) {
        self.agents.beat();
    }

    fn hang_up(&self) {
        self.jobs.stop_all();
    }

    fn finish(&self) {
        self.jobs.wait_all();
        self.agents.end();
    }
}

/// Answers each message on its own thread, so waiting for a run doesn't hold up the rest.
fn serve_lines(
    backend: &impl Backend,
    input: impl BufRead,
    output: impl Write + Send,
) -> io::Result<()> {
    let output = Mutex::new(output);
    let result = std::thread::scope(|scope| {
        // While the assistant is connected, the app can tell it is.
        let (hangup, hung_up) = mpsc::channel::<()>();
        scope.spawn(move || {
            while hung_up.recv_timeout(agents::HEARTBEAT) == Err(RecvTimeoutError::Timeout) {
                backend.beat();
            }
        });
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
                if let Some(reply) = handle_line(backend, &line) {
                    let mut out = output.lock().unwrap_or_else(|e| e.into_inner());
                    // A client that is gone can't be told anything.
                    let _ = writeln!(out, "{reply}").and_then(|()| out.flush());
                }
            });
        };
        drop(hangup);
        backend.hang_up();
        ended
    });
    backend.finish();
    result
}

fn handle_line(backend: &impl Backend, message: &str) -> Option<String> {
    let reply = match serde_json::from_str::<Value>(message) {
        Err(e) => Some(error_reply(
            Value::Null,
            PARSE_ERROR,
            &format!("Parse error: {e}"),
        )),
        Ok(Value::Object(m)) => handle_object(backend, &m),
        Ok(_) => Some(error_reply(
            Value::Null,
            INVALID_REQUEST,
            "Invalid request: send one JSON-RPC object per line",
        )),
    };
    reply.map(|r| r.to_string())
}

fn handle_object(backend: &impl Backend, m: &serde_json::Map<String, Value>) -> Option<Value> {
    let id = m.get("id").cloned();
    let Some(method) = m.get("method").and_then(Value::as_str) else {
        // A response to a request this server never sends, or garbage.
        return match id {
            Some(id) if !m.contains_key("result") && !m.contains_key("error") => Some(error_reply(
                id,
                INVALID_REQUEST,
                "Invalid request: no method",
            )),
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
        "initialize" => backend.initialize(&params),
        "ping" => Ok(json!({})),
        "tools/list" => backend.tools(),
        "tools/call" => backend.call(&params),
        other => Err((METHOD_NOT_FOUND, format!("Method not found: {other}"))),
    };
    Some(match result {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err((code, message)) => error_reply(id, code, &message),
    })
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
        HandsError::PermissionDenied { permission } if permission == "screenRecording" => {
            tr!("mcp.hands.screenRecording")
        }
        HandsError::PermissionDenied { .. } => tr!("mcp.hands.accessibility"),
        HandsError::AppNotFound { app } => tr!("mcp.hands.appNotFound", app),
        HandsError::NoWindow { app } => tr!("mcp.hands.noWindow", app),
        HandsError::CannotOpen { item } => tr!("mcp.hands.cannotOpen", item),
        HandsError::ScreenLocked => tr!("mcp.hands.screenLocked"),
        HandsError::StillRunning { app } => tr!("mcp.hands.stillRunning", app),
        HandsError::WindowFixed { app } => tr!("mcp.hands.windowFixed", app),
        HandsError::WindowDiffers {
            app,
            what,
            wanted,
            actual,
        } if what == "position" => tr!("mcp.hands.windowDiffers.position", app, wanted, actual),
        HandsError::WindowDiffers {
            app,
            wanted,
            actual,
            ..
        } => tr!("mcp.hands.windowDiffers.size", app, wanted, actual),
        HandsError::Failed { reason } => reason.clone(),
    }
}
