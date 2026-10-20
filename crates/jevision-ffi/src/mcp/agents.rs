//! Who connected to the server, and the question the person is asked before an assistant acts
//! on the screen. The server lives in the process the assistant started and the app in its
//! own, so they talk through a folder both reach:
//!
//! - `sessions/NAME-PID-START.json`: one file per connection, written only by the server that
//!   holds it, which touches it every [`HEARTBEAT`] while the assistant is connected.
//! - `asks/ID.json`: a question for the person; `asks/ID.answer`: `allow` or `deny`.
//! - `listening`: the app touches it while it is open and can ask.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::{fs, io, thread};

use serde_json::{Value, json};

/// How often a connected server shows it is still there.
pub(super) const HEARTBEAT: Duration = Duration::from_secs(15);
/// A connection not heard from for this long has ended without saying so.
const STALE: Duration = Duration::from_secs(HEARTBEAT.as_secs() * 3);
/// Connections older than this are forgotten.
const KEEP: Duration = Duration::from_secs(90 * 24 * 3600);
/// How long the person has to answer.
pub(super) const ASK_TIMEOUT: Duration = Duration::from_secs(60);
/// The app counts as open when it touched `listening` this recently.
const LISTENING: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_millis(200);

/// What an assistant asks to do on the Mac.
pub(super) struct Ask<'a> {
    /// The project folder.
    pub project: &'a str,
    /// The test files to run, relative to the project.
    pub tests: &'a [String],
    /// The apps those tests drive, by bundle id.
    pub apps: Vec<String>,
    pub environment: Option<&'a str>,
}

/// One connection of an assistant: the file the server keeps about it.
struct Session {
    file: PathBuf,
    record: Value,
}

pub(super) struct Agents {
    folder: PathBuf,
    session: Mutex<Option<Session>>,
    /// How long the person has to answer.
    pub(super) timeout: Duration,
}

impl Agents {
    pub(super) fn new(folder: &str) -> Self {
        Self {
            folder: PathBuf::from(folder),
            session: Mutex::new(None),
            timeout: ASK_TIMEOUT,
        }
    }

    /// The assistant said who it is, in the `initialize` request's `clientInfo`.
    pub(super) fn hello(&self, params: &Value) {
        let info = &params["clientInfo"];
        let text = |key: &str| info[key].as_str().map(str::trim).unwrap_or_default();
        let name = Some(text("name"))
            .filter(|n| !n.is_empty())
            .unwrap_or("unknown");
        let title = Some(text("title"))
            .filter(|t| !t.is_empty())
            .unwrap_or(name);
        let start = unix_ms(SystemTime::now());
        let slug: String = name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .take(40)
            .collect();
        let file = self
            .folder
            .join("sessions")
            .join(format!("{slug}-{}-{start}.json", std::process::id()));
        let session = Session {
            file,
            record: json!({
                "name": name,
                "title": title,
                "version": text("version"),
                "started": start / 1000,
                "calls": 0,
                "lastCall": null,
                "ended": null,
            }),
        };
        session.save();
        *self.lock() = Some(session);
    }

    /// The assistant called the tool `tool`.
    pub(super) fn called(&self, tool: &str) {
        if self.lock().is_none() {
            self.hello(&json!({}));
        }
        if let Some(session) = self.lock().as_mut() {
            let calls = session.record["calls"].as_u64().unwrap_or_default() + 1;
            session.record["calls"] = json!(calls);
            let now = SystemTime::now();
            // Calls a moment apart share a second; the microseconds tell which came last.
            session.record["lastCall"] =
                json!({ "tool": tool, "at": unix(now), "atMicros": unix_micros(now) });
            session.save();
        }
    }

    /// Shows the assistant is still connected.
    pub(super) fn beat(&self) {
        if let Some(session) = self.lock().as_ref() {
            let _ = fs::OpenOptions::new()
                .write(true)
                .open(&session.file)
                .and_then(|f| f.set_modified(SystemTime::now()));
        }
    }

    /// The assistant is gone.
    pub(super) fn end(&self) {
        if let Some(session) = self.lock().as_mut() {
            session.record["ended"] = json!(unix(SystemTime::now()));
            session.save();
        }
    }

    /// Asks the person to allow `ask`, and waits for the answer. `Err` says, for the
    /// assistant, why it may not go on.
    pub(super) fn ask(&self, ask: &Ask<'_>) -> Result<(), String> {
        if !fresh(&self.folder.join("listening"), LISTENING) {
            return Err("JEVision isn't open, so nobody at this Mac could allow the run. Ask the person to open JEVision and try again, or to turn off \"Ask before an assistant runs a test\" in JEVision → Settings → AI → Assistants.".to_owned());
        }
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let id = format!(
            "{}-{}-{}",
            std::process::id(),
            unix_ms(SystemTime::now()),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let assistant = self
            .lock()
            .as_ref()
            .and_then(|s| s.record["title"].as_str().map(str::to_owned))
            .unwrap_or_else(|| "An assistant".to_owned());
        let now = unix(SystemTime::now());
        let asks = self.folder.join("asks");
        let (request, answer) = (
            asks.join(format!("{id}.json")),
            asks.join(format!("{id}.answer")),
        );
        let written = fs::create_dir_all(&asks).and_then(|()| {
            write_atomic(
                &request,
                &json!({
                    "id": id,
                    "assistant": assistant,
                    "project": ask.project,
                    "tests": ask.tests,
                    "apps": ask.apps,
                    "environment": ask.environment,
                    "requestedAt": now,
                    "expiresAt": now + self.timeout.as_secs() as i64,
                })
                .to_string(),
            )
        });
        if let Err(e) = written {
            return Err(format!("The person couldn't be asked: {e}."));
        }
        let deadline = Instant::now() + self.timeout;
        let allowed = loop {
            if let Ok(text) = fs::read_to_string(&answer) {
                break Some(text.trim() == "allow");
            }
            if Instant::now() >= deadline {
                break None;
            }
            thread::sleep(POLL);
        };
        let _ = fs::remove_file(&request);
        let _ = fs::remove_file(&answer);
        match allowed {
            Some(true) => Ok(()),
            Some(false) => Err("The person at this Mac didn't allow this run. Don't ask again unless they tell you to.".to_owned()),
            None => Err(format!(
                "The person at this Mac didn't answer within {} seconds, so the run didn't start. Ask them to look at JEVision, then try again.",
                self.timeout.as_secs()
            )),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Session>> {
        self.session.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Session {
    /// Writes the file; the list of assistants is a courtesy, so a failure is not the
    /// assistant's problem.
    fn save(&self) {
        let _ = self
            .file
            .parent()
            .map_or(Ok(()), fs::create_dir_all)
            .and_then(|()| write_atomic(&self.file, &self.record.to_string()));
    }
}

/// An assistant that connected to the server, with all its connections put together.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ConnectedAssistant {
    /// As the assistant calls itself, such as `claude-code`.
    pub name: String,
    /// What to show: the title the assistant gave, or its name.
    pub title: String,
    pub version: String,
    /// Unix seconds.
    pub first_connected_at: i64,
    pub last_connected_at: i64,
    /// It is connected now.
    pub connected: bool,
    pub calls: u64,
    pub last_call: Option<AssistantCall>,
}

/// The last tool an assistant called.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct AssistantCall {
    pub tool: String,
    /// Unix seconds.
    pub at: i64,
}

/// A question to the person: may the assistant do this on the Mac?
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct AssistantRequest {
    pub id: String,
    /// What the assistant calls itself.
    pub assistant: String,
    pub project: String,
    /// Test files it wants to run, relative to the project.
    pub tests: Vec<String>,
    /// Apps those tests click and type in, by bundle id.
    pub apps: Vec<String>,
    pub environment: Option<String>,
    /// Unix seconds. After this the assistant has been told no.
    pub expires_at: i64,
}

/// The assistants that connected to servers using the shared `folder`, most recent first.
#[uniffi::export]
pub fn connected_assistants(folder: String) -> Vec<ConnectedAssistant> {
    let now = SystemTime::now();
    // Each assistant with when its last call came, in Unix microseconds.
    let mut found: Vec<(ConnectedAssistant, Option<i64>)> = Vec::new();
    for (path, modified, record) in sessions(Path::new(&folder)) {
        let age = now.duration_since(modified).unwrap_or_default();
        if age > KEEP {
            let _ = fs::remove_file(path);
            continue;
        }
        let text = |key: &str| record[key].as_str().unwrap_or_default().to_owned();
        let started = record["started"].as_i64().unwrap_or_default();
        let connected = record["ended"].is_null() && age <= STALE;
        let calls = record["calls"].as_u64().unwrap_or_default();
        let last_call = record["lastCall"].as_object().map(|c| AssistantCall {
            tool: c
                .get("tool")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            at: c.get("at").and_then(Value::as_i64).unwrap_or_default(),
        });
        let call_at = record["lastCall"]["atMicros"]
            .as_i64()
            .or_else(|| record["lastCall"]["at"].as_i64().map(|s| s * 1_000_000));
        match found.iter_mut().find(|(a, _)| a.name == text("name")) {
            Some((a, latest)) => {
                a.first_connected_at = a.first_connected_at.min(started);
                a.connected |= connected;
                a.calls += calls;
                if call_at > *latest {
                    a.last_call = last_call;
                    *latest = call_at;
                }
                if started > a.last_connected_at {
                    a.last_connected_at = started;
                    a.title = text("title");
                    a.version = text("version");
                }
            }
            None => found.push((
                ConnectedAssistant {
                    name: text("name"),
                    title: text("title"),
                    version: text("version"),
                    first_connected_at: started,
                    last_connected_at: started,
                    connected,
                    calls,
                    last_call,
                },
                call_at,
            )),
        }
    }
    let mut found: Vec<ConnectedAssistant> = found.into_iter().map(|(a, _)| a).collect();
    found.sort_by(|a, b| {
        (b.connected, b.last_connected_at, &a.name).cmp(&(
            a.connected,
            a.last_connected_at,
            &b.name,
        ))
    });
    found
}

/// Forgets the assistants that aren't connected now.
#[uniffi::export]
pub fn forget_assistants(folder: String) {
    let now = SystemTime::now();
    for (path, modified, record) in sessions(Path::new(&folder)) {
        let age = now.duration_since(modified).unwrap_or_default();
        if !record["ended"].is_null() || age > STALE {
            let _ = fs::remove_file(path);
        }
    }
}

/// The questions waiting for the person, oldest first. Call it about once a second while the
/// app is open: that also tells the servers that somebody can answer.
#[uniffi::export]
pub fn pending_assistant_requests(folder: String) -> Vec<AssistantRequest> {
    let folder = Path::new(&folder);
    let _ = fs::create_dir_all(folder).and_then(|()| {
        write_atomic(
            &folder.join("listening"),
            &unix(SystemTime::now()).to_string(),
        )
    });
    let now = unix(SystemTime::now());
    let mut found: Vec<(i64, AssistantRequest)> = Vec::new();
    for entry in fs::read_dir(folder.join("asks"))
        .into_iter()
        .flatten()
        .flatten()
    {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "json") || path.with_extension("answer").exists() {
            continue;
        }
        let Some(r) = fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        else {
            continue;
        };
        let expires_at = r["expiresAt"].as_i64().unwrap_or_default();
        if expires_at <= now {
            continue;
        }
        let text = |key: &str| r[key].as_str().unwrap_or_default().to_owned();
        let list = |key: &str| -> Vec<String> {
            r[key]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        };
        found.push((
            r["requestedAt"].as_i64().unwrap_or_default(),
            AssistantRequest {
                id: text("id"),
                assistant: text("assistant"),
                project: text("project"),
                tests: list("tests"),
                apps: list("apps"),
                environment: r["environment"].as_str().map(str::to_owned),
                expires_at,
            },
        ));
    }
    found.sort_by(|a, b| (a.0, &a.1.id).cmp(&(b.0, &b.1.id)));
    found.into_iter().map(|(_, r)| r).collect()
}

/// Answers the question `id`. `false` when it is gone: the assistant stopped waiting.
#[uniffi::export]
pub fn answer_assistant_request(folder: String, id: String, allow: bool) -> bool {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit() || c == '-') {
        return false;
    }
    let asks = Path::new(&folder).join("asks");
    if !asks.join(format!("{id}.json")).exists() {
        return false;
    }
    let word = if allow { "allow" } else { "deny" };
    write_atomic(&asks.join(format!("{id}.answer")), word).is_ok()
}

/// Every session file with when it was last touched and what it says.
fn sessions(folder: &Path) -> Vec<(PathBuf, SystemTime, Value)> {
    fs::read_dir(folder.join("sessions"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let modified = e.metadata().ok()?.modified().ok()?;
            let record = serde_json::from_str(&fs::read_to_string(&path).ok()?).ok()?;
            Some((path, modified, record))
        })
        .collect()
}

/// `path` was touched within `within`.
fn fresh(path: &Path, within: Duration) -> bool {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .is_ok_and(|t| {
            SystemTime::now()
                .duration_since(t)
                .is_ok_and(|age| age <= within)
        })
}

/// Writes `text` so that a reader sees all of it or nothing.
fn write_atomic(path: &Path, text: &str) -> io::Result<()> {
    let temp = path.with_extension("tmp");
    fs::write(&temp, text)?;
    fs::rename(&temp, path)
}

fn unix(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn unix_micros(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as i64
}

fn unix_ms(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
