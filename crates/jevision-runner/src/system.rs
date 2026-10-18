//! Blocks that do not look at the screen: scripts, terminal commands, HTTP requests, files and
//! SQLite databases. They act only while a test runs, never when a test is opened or checked.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant, SystemTime};
use std::{env, fs};

use jevision_core::{FileState, HttpMethod, JsonStep, OutputKind, ScriptLanguage, ValueTest};

use crate::RunControl;

/// Where `python3` and `node` usually are besides the `PATH` the app started with: an app opened
/// from the Finder gets a short `PATH` without Homebrew.
const EXTRA_PATH: [&str; 3] = ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"];

/// Output kept from each stream; a program that prints more is still read to the end.
const OUTPUT_LIMIT: usize = 4 << 20;

/// What a program printed and how it ended.
pub(crate) struct Finished {
    /// `None` when a signal stopped it.
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

pub(crate) enum ProcessError {
    NotFound,
    CannotStart(String),
    TimedOut,
    Cancelled,
}

/// Runs `command` to its end, stopping it and everything it started on time-out or cancel.
pub(crate) fn run_process(
    mut command: Command,
    timeout: Duration,
    control: &RunControl,
) -> Result<Finished, ProcessError> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("PATH", search_path());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    let mut child = command.spawn().map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => ProcessError::NotFound,
        _ => ProcessError::CannotStart(e.to_string()),
    })?;
    let stdout = reader(child.stdout.take());
    let stderr = reader(child.stderr.take());
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(e) => {
                stop(&mut child);
                return Err(ProcessError::CannotStart(e.to_string()));
            }
        }
        if control.is_cancelled() {
            stop(&mut child);
            return Err(ProcessError::Cancelled);
        }
        if Instant::now() >= deadline {
            stop(&mut child);
            return Err(ProcessError::TimedOut);
        }
        thread::sleep(Duration::from_millis(10));
    };
    Ok(Finished {
        code: status.code(),
        stdout: collect(&stdout),
        stderr: collect(&stderr),
    })
}

fn search_path() -> String {
    let mut dirs: Vec<String> = env::var("PATH")
        .unwrap_or_default()
        .split(':')
        .filter(|d| !d.is_empty())
        .map(str::to_owned)
        .collect();
    for dir in EXTRA_PATH {
        if !dirs.iter().any(|d| d == dir) {
            dirs.push(dir.to_owned());
        }
    }
    dirs.join(":")
}

fn reader(pipe: Option<impl Read + Send + 'static>) -> Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    if let Some(mut pipe) = pipe {
        thread::spawn(move || {
            let mut chunk = [0; 8192];
            while let Ok(n @ 1..) = pipe.read(&mut chunk) {
                if tx.send(chunk[..n].to_vec()).is_err() {
                    break;
                }
            }
        });
    }
    rx
}

/// Everything a finished program printed. Something it left running in the background may keep
/// the stream open, so reading stops once the stream is quiet for a moment.
fn collect(rx: &Receiver<Vec<u8>>) -> String {
    let mut bytes = Vec::new();
    while let Ok(chunk) = rx.recv_timeout(Duration::from_millis(200)) {
        let room = OUTPUT_LIMIT.saturating_sub(bytes.len());
        bytes.extend_from_slice(&chunk[..chunk.len().min(room)]);
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Stops the program and whatever it started.
fn stop(child: &mut Child) {
    #[cfg(unix)]
    if let Ok(pid) = i32::try_from(child.id()) {
        // SAFETY: `kill` only sends a signal; `-pid` is the process group the child leads.
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Program that runs a script of `language`, given the script file as its argument.
pub(crate) fn interpreter(language: ScriptLanguage) -> &'static str {
    match language {
        ScriptLanguage::Sh => "/bin/sh",
        ScriptLanguage::Zsh => "/bin/zsh",
        ScriptLanguage::Python3 => "python3",
        ScriptLanguage::Node => "node",
        ScriptLanguage::AppleScript => "/usr/bin/osascript",
    }
}

/// Values a script wrote to `$JEV_OUTPUT`: `name=value` lines, and `name<<END` … `END` for a
/// value of several lines. A later line wins.
pub(crate) fn parse_outputs(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        if let Some((name, end)) = line.split_once("<<")
            && !name.contains('=')
        {
            let value: Vec<&str> = lines.by_ref().take_while(|l| *l != end).collect();
            out.insert(name.trim().to_owned(), value.join("\n"));
        } else if let Some((name, value)) = line.split_once('=') {
            out.insert(name.trim().to_owned(), value.to_owned());
        }
    }
    out
}

/// `value` as an output of `kind`: numbers as written, yes/no answers as `true` or `false`.
pub(crate) fn typed(kind: OutputKind, value: &str) -> Option<String> {
    let trimmed = value.trim();
    match kind {
        OutputKind::Text => Some(value.to_owned()),
        OutputKind::Number => number(trimmed).map(|_| trimmed.to_owned()),
        OutputKind::Boolean => match trimmed.to_ascii_lowercase().as_str() {
            "true" | "yes" | "1" => Some("true".to_owned()),
            "false" | "no" | "0" => Some("false".to_owned()),
            _ => None,
        },
    }
}

pub(crate) fn kind_name(kind: OutputKind) -> &'static str {
    match kind {
        OutputKind::Text => "text",
        OutputKind::Number => "number",
        OutputKind::Boolean => "boolean",
    }
}

fn number(text: &str) -> Option<f64> {
    text.trim().parse::<f64>().ok().filter(|n| n.is_finite())
}

/// Equal as text, or as numbers when both are numbers: `3` equals `3.0`.
pub(crate) fn same(a: &str, b: &str) -> bool {
    a == b || matches!((number(a), number(b)), (Some(x), Some(y)) if x == y)
}

/// Whether `value` passes `test` against `to`. `Err` holds the side that should be a number and
/// is not.
pub(crate) fn compare(value: &str, test: ValueTest, to: &str) -> Result<bool, String> {
    Ok(match test {
        ValueTest::Equals => same(value, to),
        ValueTest::NotEquals => !same(value, to),
        ValueTest::Contains => value.contains(to),
        ValueTest::GreaterThan | ValueTest::LessThan => {
            let a = number(value).ok_or_else(|| value.to_owned())?;
            let b = number(to).ok_or_else(|| to.to_owned())?;
            if test == ValueTest::GreaterThan {
                a > b
            } else {
                a < b
            }
        }
    })
}

/// What a value condition tests, in symbols that need no translation.
pub(crate) fn sign(test: ValueTest) -> &'static str {
    match test {
        ValueTest::Equals => "=",
        ValueTest::NotEquals => "≠",
        ValueTest::Contains => "∋",
        ValueTest::GreaterThan => ">",
        ValueTest::LessThan => "<",
    }
}

/// Whether the file at `path` is in the state the block waits for; `since` is when the attempt
/// started.
pub(crate) fn file_holds(path: &Path, until: &FileState, since: SystemTime) -> bool {
    match until {
        FileState::Exists => path.exists(),
        FileState::Missing => !path.exists(),
        FileState::Changed => fs::metadata(path)
            .and_then(|m| m.modified())
            .is_ok_and(|t| t > since),
        FileState::Contains(text) => fs::read_to_string(path).is_ok_and(|s| s.contains(text)),
    }
}

/// An HTTP answer.
pub(crate) struct Answer {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

/// Sends one request. Any status is an answer; `Err` says why there was none.
pub(crate) fn http(
    method: HttpMethod,
    url: &str,
    headers: &[(String, String)],
    body: &str,
    timeout: Duration,
) -> Result<Answer, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(timeout))
        .build()
        .into();
    let json = !body.is_empty()
        && !headers
            .iter()
            .any(|(n, _)| n.eq_ignore_ascii_case("content-type"))
        && serde_json::from_str::<serde_json::Value>(body).is_ok();
    let with = |r| with_headers(r, headers, json);
    let result = match method {
        HttpMethod::Post => with_headers(agent.post(url), headers, json).send(body),
        HttpMethod::Put => with_headers(agent.put(url), headers, json).send(body),
        HttpMethod::Patch => with_headers(agent.patch(url), headers, json).send(body),
        HttpMethod::Get | HttpMethod::Head | HttpMethod::Delete => {
            let r = match method {
                HttpMethod::Get => agent.get(url),
                HttpMethod::Head => agent.head(url),
                _ => agent.delete(url),
            };
            let r = with(r);
            if body.is_empty() {
                r.call()
            } else {
                r.force_send_body().send(body)
            }
        }
    };
    let mut response = result.map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    let headers = response
        .headers()
        .iter()
        .map(|(n, v)| {
            let value = String::from_utf8_lossy(v.as_bytes()).into_owned();
            (n.as_str().to_owned(), value)
        })
        .collect();
    let body = response
        .body_mut()
        .read_to_vec()
        .map_err(|e| e.to_string())?;
    Ok(Answer {
        status,
        headers,
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}

fn with_headers<B>(
    mut request: ureq::RequestBuilder<B>,
    headers: &[(String, String)],
    json: bool,
) -> ureq::RequestBuilder<B> {
    for (name, value) in headers {
        request = request.header(name, value);
    }
    if json {
        request = request.header("Content-Type", "application/json");
    }
    request
}

/// The part of an answer `from` names: `status`, `body`, `body.PATH` into a JSON body, or
/// `header.NAME`.
pub(crate) fn answer_part(answer: &Answer, from: &str) -> Option<String> {
    match from {
        "status" => Some(answer.status.to_string()),
        "body" => Some(answer.body.clone()),
        _ => {
            if let Some(name) = from.strip_prefix("header.") {
                return answer
                    .headers
                    .iter()
                    .find(|(n, _)| n.eq_ignore_ascii_case(name))
                    .map(|(_, v)| v.clone());
            }
            json_value(&answer.body, from.strip_prefix("body.")?)
        }
    }
}

/// The value at `path` in a JSON text: strings as they are, anything else as JSON.
pub(crate) fn json_value(text: &str, path: &str) -> Option<String> {
    let root: serde_json::Value = serde_json::from_str(text).ok()?;
    let mut value = &root;
    for step in jevision_core::json_path(path)? {
        value = match step {
            JsonStep::Field(name) => value.get(name)?,
            JsonStep::Index(i) => value.get(i)?,
        };
    }
    Some(match value {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    })
}

/// What a query returned: the first column of the first row, and how many rows.
pub(crate) struct Rows {
    /// Empty when there are no rows, or the value is NULL.
    pub first: String,
    pub count: u32,
}

/// Runs one SQL statement on an SQLite file that exists.
pub(crate) fn query(file: &Path, sql: &str) -> Result<Rows, String> {
    use rusqlite::types::ValueRef;
    let run = || -> rusqlite::Result<Rows> {
        let db = rusqlite::Connection::open_with_flags(
            file,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        db.busy_timeout(Duration::from_secs(1))?;
        let mut statement = db.prepare(sql)?;
        let columns = statement.column_count();
        let mut rows = statement.query([])?;
        let mut out = Rows {
            first: String::new(),
            count: 0,
        };
        while let Some(row) = rows.next()? {
            if out.count == 0 && columns > 0 {
                out.first = match row.get_ref(0)? {
                    ValueRef::Null => String::new(),
                    ValueRef::Integer(n) => n.to_string(),
                    ValueRef::Real(x) => x.to_string(),
                    ValueRef::Text(t) | ValueRef::Blob(t) => {
                        String::from_utf8_lossy(t).into_owned()
                    }
                };
            }
            out.count = out.count.saturating_add(1);
        }
        Ok(out)
    };
    run().map_err(|e| e.to_string())
}

/// The end of what a program printed, enough to say why it failed.
pub(crate) fn excerpt(text: &str) -> String {
    const LIMIT: usize = 600;
    let text = text.trim();
    if text.len() <= LIMIT {
        return text.to_owned();
    }
    let mut start = text.len() - LIMIT;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    format!("…{}", &text[start..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outputs_read_lines_and_blocks() {
        let out = parse_outputs("a=1\nb = x=y\nnote<<END\none\ntwo\nEND\na=2\nnoise\n");
        assert_eq!(out["a"], "2");
        assert_eq!(out["b"], " x=y");
        assert_eq!(out["note"], "one\ntwo");
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn outputs_are_typed() {
        assert_eq!(typed(OutputKind::Number, " 3.5\n").as_deref(), Some("3.5"));
        assert_eq!(typed(OutputKind::Number, "three"), None);
        assert_eq!(typed(OutputKind::Number, "NaN"), None);
        assert_eq!(typed(OutputKind::Boolean, "Yes").as_deref(), Some("true"));
        assert_eq!(typed(OutputKind::Boolean, "0").as_deref(), Some("false"));
        assert_eq!(typed(OutputKind::Boolean, "maybe"), None);
        assert_eq!(typed(OutputKind::Text, " a ").as_deref(), Some(" a "));
    }

    #[test]
    fn values_compare_as_numbers_when_they_are() {
        assert_eq!(compare("3.0", ValueTest::Equals, "3"), Ok(true));
        assert_eq!(compare("abc", ValueTest::NotEquals, "abd"), Ok(true));
        assert_eq!(
            compare("hello world", ValueTest::Contains, "lo w"),
            Ok(true)
        );
        assert_eq!(compare("10", ValueTest::GreaterThan, "9"), Ok(true));
        assert_eq!(compare("10", ValueTest::LessThan, "9"), Ok(false));
        assert_eq!(
            compare("many", ValueTest::LessThan, "9"),
            Err("many".to_owned())
        );
    }

    #[test]
    fn json_values_follow_paths() {
        let body = r#"{"items": [{"id": 7, "name": "a"}, {"tags": ["x"]}], "ok": true}"#;
        assert_eq!(json_value(body, "items[0].id").as_deref(), Some("7"));
        assert_eq!(json_value(body, "items[0].name").as_deref(), Some("a"));
        assert_eq!(
            json_value(body, "items[1].tags").as_deref(),
            Some(r#"["x"]"#)
        );
        assert_eq!(json_value(body, "ok").as_deref(), Some("true"));
        assert_eq!(json_value(body, "items[2]"), None);
        assert_eq!(json_value("not json", "a"), None);
        assert_eq!(json_value("[1, 2]", "[1]").as_deref(), Some("2"));
    }

    #[test]
    fn long_output_keeps_its_end() {
        let text = "é".repeat(1000);
        let short = excerpt(&text);
        assert!(short.starts_with('…') && short.len() <= 604);
        assert_eq!(excerpt("  short\n"), "short");
    }
}
