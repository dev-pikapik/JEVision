//! Who connected to the server, and the question the person is asked before an assistant acts
//! on the screen. The server lives in the process the assistant started and the app in its
//! own, so they talk through a folder both reach:
//!
//! - `sessions/NAME-PID-START-N.json`: one file per connection, written only by the server that
//!   holds it, which touches it every [`HEARTBEAT`] while the assistant is connected. An
//!   assistant that comes over the network (`remote`) is held by the app itself, which writes
//!   the same files, so the app lists and asks about it like about any other.
//! - `asks/ID.json`: a question for the person; `asks/ID.answer`: `allow` or `deny`.
//! - `listening`: the app holds it locked while it is open and can ask, see
//!   [`listen_for_assistants`]. A server that has a question while it isn't opens the app in the
//!   background first.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::{fs, io, thread};

use jevision_l10n::tr;
use serde_json::{Value, json};

use super::hands_message;
use crate::HandsError;

/// How often a connected server shows it is still there.
pub(super) const HEARTBEAT: Duration = Duration::from_secs(15);
/// A connection not heard from for this long has ended without saying so.
const STALE: Duration = Duration::from_secs(HEARTBEAT.as_secs() * 3);
/// Connections older than this are forgotten.
const KEEP: Duration = Duration::from_secs(90 * 24 * 3600);
/// How long the person has to answer.
pub(super) const ASK_TIMEOUT: Duration = Duration::from_secs(60);
/// How long the app may take to start and lock `listening`.
const OPEN_TIMEOUT: Duration = Duration::from_secs(30);
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
    /// The computer the assistant works on, when it comes over the network.
    remote: Option<String>,
    session: Mutex<Option<Session>>,
    /// How long the person has to answer.
    pub(super) timeout: Duration,
    /// How long the app may take to start, see [`OPEN_TIMEOUT`].
    pub(super) open_timeout: Duration,
}

impl Agents {
    pub(super) fn new(folder: &str) -> Self {
        Self {
            folder: PathBuf::from(folder),
            remote: None,
            session: Mutex::new(None),
            timeout: ASK_TIMEOUT,
            open_timeout: OPEN_TIMEOUT,
        }
    }

    /// For an assistant that comes over the network from the computer called `computer`.
    pub(super) fn remote(folder: &str, computer: &str) -> Self {
        Self {
            remote: Some(computer.to_owned()),
            ..Self::new(folder)
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
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let file = self.folder.join("sessions").join(format!(
            "{slug}-{}-{start}-{}.json",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let session = Session {
            file,
            record: json!({
                "name": name,
                "title": title,
                "version": text("version"),
                "remote": self.remote,
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

    /// Asks the person to allow `ask`, and waits for the answer. When the app isn't open,
    /// `open` opens it, and the question waits until it is ready to ask. `Err` says, for the
    /// assistant, why it may not go on.
    pub(super) fn ask(
        &self,
        ask: &Ask<'_>,
        open: impl FnOnce() -> Result<(), HandsError>,
    ) -> Result<(), String> {
        if !app_listening(&self.folder) {
            open().map_err(|e| tr!("mcp.ask.notOpen", hands_message(&e)))?;
            let deadline = Instant::now() + self.open_timeout;
            while !app_listening(&self.folder) {
                if Instant::now() >= deadline {
                    let late = tr!("mcp.ask.notReady", self.open_timeout.as_secs());
                    return Err(tr!("mcp.ask.notOpen", late));
                }
                thread::sleep(POLL);
            }
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
            .unwrap_or_else(|| tr!("mcp.ask.unnamed"));
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
                    "remote": self.remote,
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
            return Err(tr!("mcp.ask.failed", e.to_string()));
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
            Some(false) => Err(tr!("mcp.ask.denied")),
            None => Err(tr!("mcp.ask.timeout", self.timeout.as_secs())),
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
    /// The computer it works on, when it is connected over the network; `None` for an
    /// assistant on this Mac.
    pub remote: Option<String>,
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
    /// The computer it works on, when it is connected over the network.
    pub remote: Option<String>,
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
        let remote = record["remote"].as_str().map(str::to_owned);
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
        match found
            .iter_mut()
            .find(|(a, _)| a.name == text("name") && a.remote == remote)
        {
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
                    remote,
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
        (b.connected, b.last_connected_at, &a.name, &a.remote).cmp(&(
            a.connected,
            a.last_connected_at,
            &b.name,
            &b.remote,
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

/// The app is open and can ask the person while it keeps this: the servers find `listening`
/// locked. Nothing has to touch a file on time, so an app that macOS lets nap in the background
/// still counts as open, and the system lets go of the lock when the app quits or crashes.
#[derive(uniffi::Object)]
pub struct AssistantListener {
    _lock: File,
}

/// Tells the servers using the shared `folder` that the app can ask; keep the listener while it
/// can. `None` when the folder can't be written or another copy of JEVision already listens.
#[uniffi::export]
pub fn listen_for_assistants(folder: String) -> Option<Arc<AssistantListener>> {
    let folder = Path::new(&folder);
    fs::create_dir_all(folder.join("asks")).ok()?;
    let file = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(folder.join("listening"))
        .ok()?;
    // A server that looks holds the lock for a moment.
    for _ in 0..50 {
        if try_lock(&file, true).ok()? {
            return Some(Arc::new(AssistantListener { _lock: file }));
        }
        thread::sleep(Duration::from_millis(10));
    }
    None
}

/// The app holds `listening` locked, see [`listen_for_assistants`].
fn app_listening(folder: &Path) -> bool {
    File::open(folder.join("listening"))
        .is_ok_and(|file| try_lock(&file, false).is_ok_and(|got| !got))
}

/// Locks `file` for this open file without waiting, `exclusive` or shared; whether it got the
/// lock. Dropping the file lets go of it.
#[cfg(unix)]
fn try_lock(file: &File, exclusive: bool) -> io::Result<bool> {
    use std::os::fd::AsRawFd;
    let kind = if exclusive {
        libc::LOCK_EX
    } else {
        libc::LOCK_SH
    };
    // SAFETY: `flock` only reads the descriptor, which `file` keeps open.
    if unsafe { libc::flock(file.as_raw_fd(), kind | libc::LOCK_NB) } == 0 {
        return Ok(true);
    }
    let error = io::Error::last_os_error();
    if error.kind() == io::ErrorKind::WouldBlock {
        Ok(false)
    } else {
        Err(error)
    }
}

/// Where this platform will lock `listening`; until then the app never counts as open, and a
/// run that needs the person's answer doesn't start.
#[cfg(not(unix))]
fn try_lock(_: &File, _: bool) -> io::Result<bool> {
    Ok(true)
}

/// The questions waiting for the person, oldest first.
#[uniffi::export]
pub fn pending_assistant_requests(folder: String) -> Vec<AssistantRequest> {
    let folder = Path::new(&folder);
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
                remote: r["remote"].as_str().map(str::to_owned),
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
