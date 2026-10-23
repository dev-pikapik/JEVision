//! A client of MCP servers, so the AI of the app can use their tools: JSON-RPC 2.0 over a
//! program's standard input and output, or over Streamable HTTP. Speaks `initialize`,
//! `tools/list` and `tools/call`.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use jevision_l10n::tr;
use serde_json::{Value, json};

/// Protocol version asked for; servers may answer with an older one they speak.
const PROTOCOL_VERSION: &str = "2025-11-25";
/// How long a server may take to answer one message.
pub(crate) const TIMEOUT: Duration = Duration::from_secs(60);
/// Most of a server's error output kept to explain why it stopped.
const OUTPUT_KEPT: usize = 2000;
/// Longest tool result passed on to the model, in characters.
const RESULT_KEPT: usize = 20_000;
/// Shown instead of the access token wherever it would otherwise appear.
const MASK: &str = "••••";

/// How to reach an MCP server, as set in Settings → AI.
#[derive(Clone, PartialEq, Eq, uniffi::Record)]
pub struct McpServer {
    /// Shown to the person and to the model.
    pub name: String,
    pub transport: McpTransport,
}

impl std::fmt::Debug for McpServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let transport = match &self.transport {
            McpTransport::Command { command } => format!("command {command}"),
            McpTransport::Http { url, token } => {
                format!(
                    "{url}{}",
                    if token.is_some() { " with a token" } else { "" }
                )
            }
        };
        write!(f, "McpServer({}, {transport})", self.name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum McpTransport {
    /// A program this Mac starts, such as `npx -y @modelcontextprotocol/server-github`, run by
    /// the login shell so it finds what the Terminal finds.
    Command { command: String },
    /// A server at a web address, with the access token it asks for, if any.
    Http { url: String, token: Option<String> },
}

/// A tool a server offers.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct McpTool {
    pub name: String,
    /// Human name; the `name` when the server gives none.
    pub title: String,
    pub description: String,
    /// The server says the tool only reads: calls go ahead without asking.
    pub read_only: bool,
    /// JSON Schema of the arguments, as JSON text.
    pub input_schema: String,
}

/// Why a server could not be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum McpError {
    #[error("{}", tr!("ai.mcp.noCommand"))]
    NoCommand,
    #[error("{}", tr!("ai.mcp.badAddress", .url))]
    BadAddress { url: String },
    #[error("{}", tr!("ai.mcp.cannotStart", .command, .reason))]
    CannotStart { command: String, reason: String },
    /// The program ended; `output` is the end of what it wrote to its error output.
    #[error("{}", tr!("ai.mcp.stopped", .output))]
    Stopped { output: String },
    #[error("{}", tr!("ai.mcp.unreachable", .url))]
    Unreachable { url: String },
    #[error("{}", tr!("ai.mcp.unauthorized"))]
    Unauthorized,
    #[error("{}", tr!("ai.mcp.timeout", *.seconds))]
    Timeout { seconds: u64 },
    #[error("{}", tr!("ai.mcp.http", *.status, .message))]
    Http { status: u16, message: String },
    /// The server answered a request with a JSON-RPC error.
    #[error("{}", tr!("ai.mcp.rpc", *.code, .message))]
    Rpc { code: i64, message: String },
    /// The server's answer does not follow the protocol.
    #[error("{}", tr!("ai.mcp.protocol", .message))]
    Protocol { message: String },
}

/// Connects to `server`, lists its tools and disconnects. Blocks; call it off the main thread.
#[uniffi::export]
pub fn mcp_tools(server: McpServer) -> Result<Vec<McpTool>, McpError> {
    Ok(Connection::open(&server, TIMEOUT)?.tools)
}

/// What a tool call returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Outcome {
    /// The content as text for the model.
    pub text: String,
    /// The tool reported a failure of its own.
    pub is_error: bool,
}

/// An open session with one server.
pub(crate) struct Connection {
    pub name: String,
    pub tools: Vec<McpTool>,
    pipe: Pipe,
    next_id: u64,
}

enum Pipe {
    Program(Program),
    Http(Http),
}

impl Connection {
    /// Starts or reaches the server, agrees on the protocol and reads its tools.
    pub(crate) fn open(server: &McpServer, timeout: Duration) -> Result<Self, McpError> {
        let pipe = match &server.transport {
            McpTransport::Command { command } => Pipe::Program(Program::start(command, timeout)?),
            McpTransport::Http { url, token } => {
                Pipe::Http(Http::new(url, token.clone(), timeout)?)
            }
        };
        let mut connection = Self {
            name: server.name.clone(),
            tools: Vec::new(),
            pipe,
            next_id: 1,
        };
        let init = connection.request(
            "initialize",
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {
                    "name": "jevision",
                    "title": "JEVision",
                    "version": env!("CARGO_PKG_VERSION"),
                },
            }),
        )?;
        let version = init["protocolVersion"]
            .as_str()
            .ok_or_else(|| protocol("initialize gave no protocolVersion"))?;
        if let Pipe::Http(http) = &mut connection.pipe {
            http.version = Some(version.to_owned());
        }
        if init["capabilities"].get("tools").is_none() {
            return Ok(connection);
        }
        connection.notify("notifications/initialized")?;
        let mut cursor: Option<String> = None;
        loop {
            let params = match &cursor {
                Some(c) => json!({ "cursor": c }),
                None => json!({}),
            };
            let page = connection.request("tools/list", params)?;
            let list = page["tools"]
                .as_array()
                .ok_or_else(|| protocol("tools/list gave no tools"))?;
            connection.tools.extend(list.iter().filter_map(tool));
            cursor = page["nextCursor"].as_str().map(str::to_owned);
            // A server that hands out the same page again would never end.
            if cursor.is_none() || connection.tools.len() > 1000 {
                break;
            }
        }
        Ok(connection)
    }

    /// Calls the tool `name` with `arguments`, an object.
    pub(crate) fn call(&mut self, name: &str, arguments: &Value) -> Result<Outcome, McpError> {
        let result = self.request(
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
        )?;
        let mut parts: Vec<String> = result["content"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|c| match c["type"].as_str() {
                Some("text") => c["text"].as_str().unwrap_or_default().to_owned(),
                Some("resource") => {
                    let r = &c["resource"];
                    r["text"]
                        .as_str()
                        .map_or_else(|| format!("[resource {}]", r["uri"]), str::to_owned)
                }
                Some("resource_link") => format!("[link {}]", c["uri"]),
                Some(other) => format!("[{other}]"),
                None => c.to_string(),
            })
            .collect();
        if parts.iter().all(|p| p.trim().is_empty())
            && let Some(structured) = result.get("structuredContent")
        {
            parts = vec![structured.to_string()];
        }
        let mut text = parts.join("\n");
        if let Some((end, _)) = text.char_indices().nth(RESULT_KEPT) {
            text.truncate(end);
            text.push('…');
        }
        Ok(Outcome {
            text,
            is_error: result["isError"] == true,
        })
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, McpError> {
        let id = self.next_id;
        self.next_id += 1;
        let message = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        let reply = match &mut self.pipe {
            Pipe::Program(p) => p.request(&message, id)?,
            Pipe::Http(h) => h.request(&message, id)?,
        };
        if let Some(error) = reply.get("error") {
            return Err(McpError::Rpc {
                code: error["code"].as_i64().unwrap_or_default(),
                message: error["message"].as_str().unwrap_or_default().to_owned(),
            });
        }
        reply
            .get("result")
            .cloned()
            .ok_or_else(|| protocol("a reply has neither result nor error"))
    }

    fn notify(&mut self, method: &str) -> Result<(), McpError> {
        let message = json!({ "jsonrpc": "2.0", "method": method });
        match &mut self.pipe {
            Pipe::Program(p) => p.send(&message),
            Pipe::Http(h) => h.post(&message).map(drop),
        }
    }
}

fn tool(v: &Value) -> Option<McpTool> {
    let name = v["name"].as_str()?.to_owned();
    let title = v["title"]
        .as_str()
        .or(v["annotations"]["title"].as_str())
        .unwrap_or(&name)
        .to_owned();
    Some(McpTool {
        title,
        description: v["description"].as_str().unwrap_or_default().to_owned(),
        read_only: v["annotations"]["readOnlyHint"] == true,
        input_schema: v
            .get("inputSchema")
            .cloned()
            .unwrap_or_else(|| json!({ "type": "object" }))
            .to_string(),
        name,
    })
}

fn protocol(message: &str) -> McpError {
    McpError::Protocol {
        message: message.to_owned(),
    }
}

/// The reply with `id` among messages from the server; requests from it are answered through
/// `answer`, notifications skipped.
fn reply_in(message: Value, id: u64, answer: &mut dyn FnMut(Value)) -> Option<Value> {
    if message.get("method").is_some() {
        if let Some(their) = message.get("id") {
            answer(match message["method"].as_str() {
                Some("ping") => json!({ "jsonrpc": "2.0", "id": their, "result": {} }),
                _ => json!({
                    "jsonrpc": "2.0", "id": their,
                    "error": { "code": -32601, "message": "JEVision doesn't offer this" }
                }),
            });
        }
        return None;
    }
    (message["id"].as_u64() == Some(id)).then_some(message)
}

/// A server program on this Mac, talking one JSON message per line.
struct Program {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<String>,
    errors: Arc<Mutex<String>>,
    /// Disconnects once the whole error output has been read.
    errors_read: Receiver<()>,
    timeout: Duration,
}

impl Program {
    fn start(command: &str, timeout: Duration) -> Result<Self, McpError> {
        let command = command.trim();
        if command.is_empty() {
            return Err(McpError::NoCommand);
        }
        let shell = std::env::var("SHELL")
            .ok()
            .filter(|s| s.starts_with('/'))
            .unwrap_or_else(|| "/bin/sh".to_owned());
        let mut child = Command::new(shell)
            .args(["-l", "-c", command])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| McpError::CannotStart {
                command: command.to_owned(),
                reason: e.to_string(),
            })?;
        let stdout = child.stdout.take().expect("stdout is piped");
        let stderr = child.stderr.take().expect("stderr is piped");
        let (sender, lines) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        let errors = Arc::new(Mutex::new(String::new()));
        let kept = errors.clone();
        let (read_all, errors_read) = mpsc::channel::<()>();
        thread::spawn(move || {
            let mut stderr = stderr;
            let mut chunk = [0; 1024];
            while let Ok(n @ 1..) = stderr.read(&mut chunk) {
                let mut kept = kept.lock().unwrap_or_else(|e| e.into_inner());
                kept.push_str(&String::from_utf8_lossy(&chunk[..n]));
                if kept.len() > OUTPUT_KEPT * 2 {
                    let mut cut = kept.len() - OUTPUT_KEPT;
                    while !kept.is_char_boundary(cut) {
                        cut += 1;
                    }
                    kept.drain(..cut);
                }
            }
            drop(read_all);
        });
        Ok(Self {
            stdin: child.stdin.take(),
            child,
            lines,
            errors,
            errors_read,
            timeout,
        })
    }

    fn send(&mut self, message: &Value) -> Result<(), McpError> {
        let sent = self
            .stdin
            .as_mut()
            .map(|stdin| writeln!(stdin, "{message}").and_then(|()| stdin.flush()));
        match sent {
            Some(Ok(())) => Ok(()),
            _ => Err(self.stopped()),
        }
    }

    fn request(&mut self, message: &Value, id: u64) -> Result<Value, McpError> {
        self.send(message)?;
        let deadline = Instant::now() + self.timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let line = match self.lines.recv_timeout(left) {
                Ok(line) => line,
                Err(RecvTimeoutError::Timeout) => {
                    return Err(McpError::Timeout {
                        seconds: self.timeout.as_secs().max(1),
                    });
                }
                Err(RecvTimeoutError::Disconnected) => return Err(self.stopped()),
            };
            // Some servers log to standard output too: skip what isn't a message.
            let Ok(parsed) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            let mut answers = Vec::new();
            let reply = reply_in(parsed, id, &mut |a| answers.push(a));
            for answer in answers {
                self.send(&answer)?;
            }
            if let Some(reply) = reply {
                return Ok(reply);
            }
        }
    }

    /// The program ended: its last words.
    fn stopped(&mut self) -> McpError {
        // The error output may still be on its way. Something the program started can keep it
        // open after the program is gone: wait for the end of it, at most two seconds.
        let _ = self.child.wait();
        let _ = self.errors_read.recv_timeout(Duration::from_secs(2));
        let output = self
            .errors
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .trim()
            .to_owned();
        let output = match output.char_indices().rev().nth(OUTPUT_KEPT) {
            Some((start, _)) => format!("…{}", &output[start..]),
            None => output,
        };
        McpError::Stopped { output }
    }
}

impl Drop for Program {
    /// Closing its input asks the server to end; one that doesn't is stopped.
    fn drop(&mut self) {
        self.stdin.take();
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if !matches!(self.child.try_wait(), Ok(None)) {
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A server at a web address (Streamable HTTP).
struct Http {
    url: String,
    token: Option<String>,
    agent: ureq::Agent,
    session: Option<String>,
    version: Option<String>,
    timeout: Duration,
}

impl Http {
    fn new(url: &str, token: Option<String>, timeout: Duration) -> Result<Self, McpError> {
        let url = url.trim().to_owned();
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(McpError::BadAddress { url });
        }
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(timeout))
            .user_agent(concat!("JEVision/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        Ok(Self {
            url,
            token: token.map(|t| t.trim().to_owned()).filter(|t| !t.is_empty()),
            agent,
            session: None,
            version: None,
            timeout,
        })
    }

    /// Posts `message`; the status, the content type and the body.
    fn post(&mut self, message: &Value) -> Result<(u16, String, String), McpError> {
        let mut request = self
            .agent
            .post(&self.url)
            .header("accept", "application/json, text/event-stream");
        if let Some(token) = &self.token {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        if let Some(session) = &self.session {
            request = request.header("mcp-session-id", session);
        }
        if let Some(version) = &self.version {
            request = request.header("mcp-protocol-version", version);
        }
        let mut response = request
            .content_type("application/json")
            .send(message.to_string())
            .map_err(|e| self.transport_error(e))?;
        let status = response.status().as_u16();
        if let Some(session) = response
            .headers()
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
        {
            self.session = Some(session.to_owned());
        }
        let kind = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(|e| self.transport_error(e))?;
        match status {
            200..=299 => Ok((status, kind, body)),
            401 | 403 => Err(McpError::Unauthorized),
            _ => Err(McpError::Http {
                status,
                message: self.excerpt(&body),
            }),
        }
    }

    fn request(&mut self, message: &Value, id: u64) -> Result<Value, McpError> {
        let (_, kind, body) = self.post(message)?;
        let messages: Vec<Value> = if kind.starts_with("text/event-stream") {
            events(&body)
        } else {
            match serde_json::from_str::<Value>(&body) {
                Ok(Value::Array(batch)) => batch,
                Ok(one) => vec![one],
                Err(_) => return Err(protocol(&self.excerpt(&body))),
            }
        };
        let mut answers = Vec::new();
        let reply = messages
            .into_iter()
            .find_map(|m| reply_in(m, id, &mut |a| answers.push(a)));
        for answer in answers {
            self.post(&answer)?;
        }
        reply.ok_or_else(|| protocol("the server sent no reply to the request"))
    }

    fn transport_error(&self, e: ureq::Error) -> McpError {
        match e {
            ureq::Error::Timeout(_) => McpError::Timeout {
                seconds: self.timeout.as_secs().max(1),
            },
            ureq::Error::Io(io) if io.kind() == std::io::ErrorKind::TimedOut => McpError::Timeout {
                seconds: self.timeout.as_secs().max(1),
            },
            ureq::Error::BadUri(_) | ureq::Error::Http(_) => McpError::BadAddress {
                url: self.url.clone(),
            },
            _ => McpError::Unreachable {
                url: self.url.clone(),
            },
        }
    }

    /// A short piece of `text` on one line, the token masked.
    fn excerpt(&self, text: &str) -> String {
        let mut text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if let Some(token) = &self.token {
            text = text.replace(token.as_str(), MASK);
        }
        match text.char_indices().nth(300) {
            Some((end, _)) => format!("{}…", &text[..end]),
            None => text,
        }
    }
}

impl Drop for Http {
    /// Ends the session, when the server keeps one.
    fn drop(&mut self) {
        let Some(session) = &self.session else { return };
        let mut request = self
            .agent
            .delete(&self.url)
            .header("mcp-session-id", session);
        if let Some(token) = &self.token {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        let _ = request.call();
    }
}

/// The JSON messages in a stream of server-sent events.
fn events(stream: &str) -> Vec<Value> {
    let mut out = Vec::new();
    let mut data = String::new();
    for line in stream.lines().chain(std::iter::once("")) {
        if line.is_empty() {
            if let Ok(v) = serde_json::from_str(&data) {
                out.push(v);
            }
            data.clear();
        } else if let Some(rest) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(rest.strip_prefix(' ').unwrap_or(rest));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_words_are_read_even_when_they_come_after_the_program_ended() {
        // The shell is gone at once; what it started writes to the error output a moment later.
        let mut program = Program::start(
            "(sleep 0.2; echo late words >&2) >/dev/null & exit 3",
            Duration::from_secs(30),
        )
        .unwrap();
        let ping = json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" });
        match program.request(&ping, 1) {
            Err(McpError::Stopped { output }) => assert_eq!(output, "late words"),
            other => panic!("expected the program to have stopped, got {other:?}"),
        }
    }

    #[test]
    fn events_are_read_from_a_stream() {
        let stream = "event: message\nid: 1\ndata: {\"jsonrpc\":\"2.0\",\n\
                      data: \"id\":1,\"result\":{}}\n\n: comment\n\ndata: {\"a\":2}";
        assert_eq!(
            events(stream),
            [
                json!({ "jsonrpc": "2.0", "id": 1, "result": {} }),
                json!({ "a": 2 })
            ]
        );
    }

    #[test]
    fn server_requests_are_answered_and_notifications_skipped() {
        let mut answers = Vec::new();
        let mut answer = |a| answers.push(a);
        assert_eq!(
            reply_in(
                json!({ "jsonrpc": "2.0", "id": 7, "method": "ping" }),
                1,
                &mut answer
            ),
            None
        );
        assert_eq!(
            reply_in(
                json!({ "jsonrpc": "2.0", "method": "notifications/progress" }),
                1,
                &mut answer
            ),
            None
        );
        assert_eq!(
            reply_in(
                json!({ "jsonrpc": "2.0", "id": 2, "result": {} }),
                1,
                &mut answer
            ),
            None
        );
        let mine = json!({ "jsonrpc": "2.0", "id": 1, "result": { "ok": true } });
        assert_eq!(reply_in(mine.clone(), 1, &mut answer), Some(mine));
        assert_eq!(
            answers,
            [json!({ "jsonrpc": "2.0", "id": 7, "result": {} })]
        );
    }

    #[test]
    fn tokens_stay_out_of_debug_output_and_errors() {
        let server = McpServer {
            name: "Tracker".into(),
            transport: McpTransport::Http {
                url: "https://mcp.example.com".into(),
                token: Some("tok-secret".into()),
            },
        };
        assert!(!format!("{server:?}").contains("tok-secret"));
        let http = Http::new("https://x", Some("tok-secret".into()), TIMEOUT).unwrap();
        assert_eq!(http.excerpt("bad token tok-secret"), "bad token ••••");
        assert_eq!(
            Http::new("ftp://x", None, TIMEOUT).err(),
            Some(McpError::BadAddress {
                url: "ftp://x".into()
            })
        );
    }
}
