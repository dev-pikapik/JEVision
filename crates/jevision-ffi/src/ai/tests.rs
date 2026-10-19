//! The AI jobs against a model and MCP servers that run inside the tests, on this computer.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use jevision_core::{BlockKind, Severity, WaitUntil};
use jevision_vision::image::Image;
use serde_json::{Value, json};

use super::*;
use crate::vision::VisionSettings;

/// A folder that removes itself.
struct Folder(PathBuf);

impl Folder {
    fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "jevision-ai-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn write(&self, rel: &str, bytes: impl AsRef<[u8]>) {
        let path = self.0.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    fn path(&self) -> String {
        self.0.display().to_string()
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A model server that gives `replies` in turn and keeps every request it got.
struct Model {
    address: String,
    requests: Arc<Mutex<Vec<String>>>,
}

impl Model {
    fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

fn http_request(reader: &mut BufReader<TcpStream>) -> Option<String> {
    let mut head = String::new();
    let mut length = 0;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).ok()? == 0 {
            return None;
        }
        if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            length = v.trim().parse().unwrap();
        }
        head.push_str(&line);
        if line == "\r\n" {
            break;
        }
    }
    let mut content = vec![0; length];
    reader.read_exact(&mut content).ok()?;
    head.push_str(&String::from_utf8_lossy(&content));
    Some(head)
}

fn respond(stream: &mut TcpStream, status: u16, headers: &str, body: &str) {
    let _ = write!(
        stream,
        "HTTP/1.1 {status} X\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

fn model(replies: Vec<(u16, String)>) -> Model {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let seen = requests.clone();
    thread::spawn(move || {
        for (status, body) in replies {
            let Ok((stream, _)) = listener.accept() else {
                return;
            };
            let mut reader = BufReader::new(stream);
            let Some(request) = http_request(&mut reader) else {
                return;
            };
            seen.lock().unwrap().push(request);
            respond(
                &mut reader.into_inner(),
                status,
                "Content-Type: application/json\r\n",
                &body,
            );
        }
    });
    Model { address, requests }
}

/// A reply of a local model saying `answer`.
fn says(answer: Value) -> (u16, String) {
    (
        200,
        json!({ "message": { "role": "assistant", "content": answer.to_string() }, "done": true })
            .to_string(),
    )
}

fn settings(model: &Model) -> VisionSettings {
    VisionSettings {
        provider: "ollama".to_owned(),
        model: "test-model".to_owned(),
        address: model.address.clone(),
        key: None,
    }
}

/// The issue tracker of `tests/fixtures/mock-mcp.py`, logging its calls to `log`.
fn tracker(log: &Path) -> McpServer {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mock-mcp.py");
    McpServer {
        name: "tracker".to_owned(),
        transport: McpTransport::Command {
            command: format!("python3 '{}' '{}'", script.display(), log.display()),
        },
    }
}

fn good_test() -> Value {
    json!({
        "format": jevision_core::FORMAT_VERSION,
        "name": "Saves",
        "app": "com.example.wrong",
        "start": "open",
        "blocks": {
            "open": { "type": "openApp", "next": "save" },
            "save": { "type": "click", "target": { "image": "refs/save-button.png" }, "next": "asked" },
            "asked": {
                "type": "if",
                "reference": { "compare": "meaning", "expect": "A dialog asks where to save" },
                "then": "name",
                "else": "saved",
            },
            "name": { "type": "type", "text": "Report", "next": "saved" },
            "saved": {
                "type": "check",
                "reference": { "compare": "meaning", "expect": "The title bar shows Report" },
            },
        },
    })
}

fn request(folder: &Folder) -> DraftRequest {
    DraftRequest {
        description: "Save the document; if a dialog asks, name it Report".to_owned(),
        app: "com.example.Editor".to_owned(),
        name: "Save a document".to_owned(),
        current: None,
        folder: folder.path(),
        previous: None,
        feedback: String::new(),
        local_only: false,
    }
}

#[test]
fn a_draft_is_a_valid_test_and_a_broken_one_goes_back_to_the_model() {
    let folder = Folder::new();
    let broken = json!({ "name": "x", "start": "a", "blocks": { "a": { "type": "fly" } } });
    let model = model(vec![
        says(json!({ "answer": { "test": broken, "summary": "" } })),
        says(json!({ "answer": { "test": good_test(), "summary": "Saves the document." } })),
    ]);
    let session = AiSession::draft_test(settings(&model), Vec::new(), request(&folder)).unwrap();
    let AiStep::Drafted { draft } = session.step().unwrap() else {
        panic!("expected a draft");
    };
    assert_eq!(draft.flow.app, "com.example.Editor");
    assert_eq!(draft.flow.name, "Save a document");
    assert_eq!(draft.summary, "Saves the document.");
    assert_eq!(draft.flow.blocks.len(), 5);
    assert!(
        draft
            .flow
            .blocks
            .iter()
            .any(|b| matches!(b.kind, BlockKind::If { .. }))
    );
    // The picture to click is yet to be taken: a warning, not an error.
    assert!(!draft.issues.is_empty());
    assert!(draft.issues.iter().all(|i| i.severity == Severity::Warning));

    let requests = model.requests();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].contains("name it Report"));
    assert!(requests[0].contains("openApp"), "the format guide is sent");
    assert!(requests[1].contains("can't be used"));
    assert!(requests[1].contains("fly"));
}

#[test]
fn a_draft_that_stays_broken_fails_in_words() {
    let folder = Folder::new();
    let broken = says(json!({ "answer": { "test": "not a test", "summary": "" } }));
    let model = model(vec![broken.clone(), broken.clone(), broken]);
    let session = AiSession::draft_test(settings(&model), Vec::new(), request(&folder)).unwrap();
    assert!(matches!(session.step(), Err(AiError::Unusable { .. })));
    assert_eq!(model.requests().len(), 3);
}

#[test]
fn a_rework_shows_the_model_its_draft_and_what_to_change() {
    let folder = Folder::new();
    let model = model(vec![says(
        json!({ "answer": { "test": good_test(), "summary": "Saves." } }),
    )]);
    let mut previous = jevision_core::parse(&good_test().to_string()).unwrap();
    previous.name = "The first draft".to_owned();
    let request = DraftRequest {
        previous: Some(previous),
        feedback: "Close the window at the end".to_owned(),
        ..request(&folder)
    };
    let session = AiSession::draft_test(settings(&model), Vec::new(), request).unwrap();
    assert!(matches!(session.step(), Ok(AiStep::Drafted { .. })));
    let sent = &model.requests()[0];
    assert!(sent.contains("The first draft"));
    assert!(sent.contains("Close the window at the end"));
}

#[test]
fn local_only_projects_get_no_checks_by_meaning_from_a_cloud_model() {
    let folder = Folder::new();
    let request = DraftRequest {
        local_only: true,
        ..request(&folder)
    };
    // A model on this computer may see the screen.
    let local = model(vec![says(
        json!({ "answer": { "test": good_test(), "summary": "" } }),
    )]);
    let session = AiSession::draft_test(settings(&local), Vec::new(), request).unwrap();
    session.step().unwrap();
    assert!(local.requests()[0].contains("Prefer"));

    let job = DraftJob {
        app: String::new(),
        name: String::new(),
        folder: PathBuf::new(),
        meaning: false,
    };
    assert!(job.instructions().contains("never use"));
}

#[test]
fn tools_that_read_run_at_once_and_tools_that_change_wait_for_the_person() {
    let folder = Folder::new();
    let log = folder.0.join("calls.log");
    let model = model(vec![
        says(
            json!({ "tool": { "server": "tracker", "name": "find_issue", "arguments": { "test": "Saves" } } }),
        ),
        says(
            json!({ "tool": { "server": "tracker", "name": "create_issue", "arguments": { "title": "Save fails" } } }),
        ),
        says(json!({ "answer": { "test": good_test(), "summary": "Saves." } })),
    ]);
    let session =
        AiSession::draft_test(settings(&model), vec![tracker(&log)], request(&folder)).unwrap();
    let AiStep::Approve { call } = session.step().unwrap() else {
        panic!("expected a call to allow");
    };
    assert_eq!(call.server, "tracker");
    assert_eq!(call.tool, "create_issue");
    assert!(call.arguments.contains("Save fails"));
    assert!(!call.allowed);
    // Asking again doesn't skip the question.
    assert!(matches!(session.step(), Ok(AiStep::Approve { .. })));

    let AiStep::Drafted { draft } = session.decide(false).unwrap() else {
        panic!("expected a draft");
    };
    let calls: Vec<_> = draft
        .calls
        .iter()
        .map(|c| (c.tool.as_str(), c.allowed))
        .collect();
    assert_eq!(calls, [("find_issue", true), ("create_issue", false)]);
    assert_eq!(draft.calls[0].title, "Find Issue");
    assert!(draft.unavailable.is_empty());

    let logged = std::fs::read_to_string(&log).unwrap();
    assert_eq!(logged.lines().count(), 1, "{logged}");
    assert!(logged.starts_with("find_issue"));

    let requests = model.requests();
    assert!(requests[0].contains("create_issue"), "the tools are listed");
    assert!(requests[1].contains("ISSUE-7"));
    assert!(requests[2].contains("did not allow"));
}

#[test]
fn an_allowed_call_runs() {
    let folder = Folder::new();
    let log = folder.0.join("calls.log");
    let model = model(vec![
        says(json!({ "tool": { "server": "tracker", "name": "create_issue", "arguments": {} } })),
        says(json!({ "answer": { "test": good_test(), "summary": "" } })),
    ]);
    let session =
        AiSession::draft_test(settings(&model), vec![tracker(&log)], request(&folder)).unwrap();
    assert!(matches!(session.step(), Ok(AiStep::Approve { .. })));
    assert!(matches!(session.decide(true), Ok(AiStep::Drafted { .. })));
    assert!(
        std::fs::read_to_string(&log)
            .unwrap()
            .starts_with("create_issue")
    );
    assert!(model.requests()[1].contains("ISSUE-8"));
}

#[test]
fn a_server_that_fails_is_left_out() {
    let folder = Folder::new();
    let model = model(vec![says(
        json!({ "answer": { "test": good_test(), "summary": "" } }),
    )]);
    let broken = McpServer {
        name: "broken".to_owned(),
        transport: McpTransport::Command {
            command: "echo no such server >&2; exit 3".to_owned(),
        },
    };
    let session = AiSession::draft_test(settings(&model), vec![broken], request(&folder)).unwrap();
    let AiStep::Drafted { draft } = session.step().unwrap() else {
        panic!("expected a draft");
    };
    assert_eq!(draft.unavailable.len(), 1);
    assert_eq!(draft.unavailable[0].server, "broken");
    assert!(
        matches!(&draft.unavailable[0].error, McpError::Stopped { output } if output.contains("no such server")),
        "{:?}",
        draft.unavailable[0].error
    );
}

#[test]
fn provider_errors_keep_their_code() {
    let folder = Folder::new();
    let model = model(vec![(404, r#"{"error":"model not found"}"#.to_owned())]);
    let session = AiSession::draft_test(settings(&model), Vec::new(), request(&folder)).unwrap();
    let Err(AiError::Provider { failure, message }) = session.step() else {
        panic!("expected a provider error");
    };
    assert_eq!(failure.code, "run.vision.unknownModel");
    assert!(message.contains("test-model"), "{message}");
}

#[test]
fn a_cancelled_session_stops() {
    let folder = Folder::new();
    let model = model(Vec::new());
    let session = AiSession::draft_test(settings(&model), Vec::new(), request(&folder)).unwrap();
    session.cancel();
    assert_eq!(session.step(), Err(AiError::Cancelled));
    assert!(model.requests().is_empty());
}

fn png(shade: u8) -> Vec<u8> {
    Image {
        width: 4,
        height: 4,
        rgba: [shade, shade, shade, 255].repeat(16),
    }
    .encode()
    .unwrap()
}

/// A project with a failed run of a test whose check did not match its reference.
fn failed_run() -> (Folder, String) {
    let project = Folder::new();
    let flow = jevision_core::parse(&good_test().to_string()).unwrap();
    project.write("save.flow.json", jevision_core::to_canonical_json(&flow));
    let run = ".jevision/runs/1";
    for (file, shade) in [("after.png", 10), ("expected.png", 200), ("diff.png", 90)] {
        project.write(&format!("{run}/{file}"), png(shade));
    }
    let step = |seq: u32, block: &str, kind: &str, status: &str| {
        json!({
            "seq": seq, "test": "save.flow.json", "block": block, "type": kind,
            "status": status, "durationMs": 5,
        })
    };
    let failure = json!({ "code": "run.check.mismatch", "params": { "similarity": "71", "threshold": "95" } });
    let mut check = step(3, "saved", "check", "failed");
    let fields = check.as_object_mut().unwrap();
    fields.insert("after".to_owned(), json!("after.png"));
    fields.insert("expected".to_owned(), json!("expected.png"));
    fields.insert("diff".to_owned(), json!("diff.png"));
    fields.insert("reference".to_owned(), json!("refs/saved.png"));
    fields.insert("similarity".to_owned(), json!(0.71));
    fields.insert("failure".to_owned(), failure.clone());
    let report = json!({
        "format": 1, "test": "Saves", "file": "save.flow.json", "app": "com.example.Editor",
        "startedAt": "2026-10-19T10:00:00Z", "durationMs": 20, "verdict": "failed", "flaky": false,
        "attempts": [{
            "number": 1, "verdict": "failed", "durationMs": 20, "failure": failure,
            "steps": [step(1, "open", "openApp", "passed"), step(2, "save", "click", "passed"), check],
        }],
    });
    project.write(&format!("{run}/report.json"), report.to_string());
    let dir = project.0.join(run).display().to_string();
    (project, dir)
}

fn failure_request(project: &Folder, run: String, local_only: bool) -> FailureRequest {
    FailureRequest {
        run,
        project: project.path(),
        local_only,
        flaky: None,
    }
}

#[test]
fn a_failed_run_is_explained_with_its_screenshots() {
    let (project, run) = failed_run();
    let model = model(vec![says(json!({ "answer": {
        "cause": "outdatedReference",
        "explanation": "The Save button moved; the reference shows the old toolbar.",
        "fix": "updateReference",
    } }))]);
    let request = failure_request(&project, run, false);
    let session =
        AiSession::explain_failure(settings(&model), Vec::new(), request, "English".to_owned())
            .unwrap();
    let AiStep::Explained { explanation } = session.step().unwrap() else {
        panic!("expected an explanation");
    };
    assert_eq!(explanation.cause, FailureCause::OutdatedReference);
    assert_eq!(explanation.fix, FailureFix::UpdateReference);
    assert_eq!(explanation.test, "save.flow.json");
    assert_eq!(explanation.block.as_deref(), Some("saved"));
    assert_eq!(explanation.pictures_sent, 3);
    assert!(!explanation.pictures_withheld);

    let sent = &model.requests()[0];
    assert!(sent.contains("\"images\""));
    assert!(sent.contains("similarity 71.0%"));
    assert!(sent.contains("updateReference"));
    assert!(sent.contains("addWait"));
}

#[test]
fn screenshots_of_a_local_only_project_stay_away_from_cloud_models() {
    let (project, run) = failed_run();
    let request = failure_request(&project, run, true);
    let (mut job, message) = TriageJob::read(&request, false, "English").unwrap();
    assert!(message.pictures.is_empty());
    assert!(message.text.contains("not shown"));
    let Finished::Done(step) = job.finish(
        &json!({ "cause": "flakyTest", "explanation": "Timing.", "fix": "nothing" }),
        &[],
        &[],
    ) else {
        panic!("expected an explanation");
    };
    let AiStep::Explained { explanation } = *step else {
        panic!("expected an explanation");
    };
    assert_eq!(explanation.pictures_sent, 0);
    assert!(explanation.pictures_withheld);
    // A fix that wasn't offered becomes the one for the cause.
    assert_eq!(explanation.fix, FailureFix::AddWait);
}

#[test]
fn a_missing_run_cannot_be_explained() {
    let model = model(Vec::new());
    let request = FailureRequest {
        run: "/nonexistent/run".to_owned(),
        project: "/nonexistent".to_owned(),
        local_only: false,
        flaky: None,
    };
    let result =
        AiSession::explain_failure(settings(&model), Vec::new(), request, "English".into());
    assert!(matches!(result, Err(AiError::NoReport { .. })));
}

#[test]
fn a_wait_goes_before_the_block_or_waits_longer() {
    let flow = jevision_core::parse(&good_test().to_string()).unwrap();

    let edited = add_wait_before(flow.clone(), "open".to_owned()).unwrap();
    let id = edited.selection.unwrap();
    assert_eq!(edited.flow.start, id);
    let wait = edited.flow.blocks.iter().find(|b| b.id == id).unwrap();
    assert!(matches!(
        &wait.kind,
        BlockKind::WaitFor { until: WaitUntil::ScreenStable, next: Some(n), .. } if n == "open"
    ));

    let edited = add_wait_before(edited.flow, "name".to_owned()).unwrap();
    let id = edited.selection.unwrap();
    let asked = edited.flow.blocks.iter().find(|b| b.id == "asked").unwrap();
    assert!(asked.kind.edges().iter().any(|(_, to)| *to == id));

    let longer = add_wait_before(edited.flow, id.clone()).unwrap();
    let BlockKind::WaitFor { timeout_ms, .. } =
        longer.flow.blocks.iter().find(|b| b.id == id).unwrap().kind
    else {
        panic!("expected a wait");
    };
    assert_eq!(timeout_ms, 30_000);
    assert_eq!(longer.selection, Some(id));

    assert!(add_wait_before(flow, "nothing".to_owned()).is_none());
}

/// An MCP server over HTTP that answers with server-sent events and wants the token `token`.
fn http_tracker(token: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}/mcp", listener.local_addr().unwrap());
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { return };
            let mut reader = BufReader::new(stream);
            let Some(request) = http_request(&mut reader) else {
                continue;
            };
            let mut stream = reader.into_inner();
            let lower = request.to_ascii_lowercase();
            if !lower.contains(&format!("authorization: bearer {token}")) {
                respond(&mut stream, 401, "", "");
                continue;
            }
            let body: Value = request
                .split("\r\n\r\n")
                .nth(1)
                .and_then(|b| serde_json::from_str(b).ok())
                .unwrap_or_default();
            let result = match body["method"].as_str() {
                Some("initialize") => json!({
                    "protocolVersion": "2025-11-25", "capabilities": { "tools": {} },
                    "serverInfo": { "name": "web", "version": "1" },
                }),
                Some("tools/list") if lower.contains("mcp-session-id: s-1") => json!({
                    "tools": [{ "name": "search", "inputSchema": { "type": "object" },
                        "annotations": { "readOnlyHint": true } }],
                }),
                _ => {
                    respond(&mut stream, 202, "", "");
                    continue;
                }
            };
            let event = json!({ "jsonrpc": "2.0", "id": body["id"], "result": result });
            respond(
                &mut stream,
                200,
                "Content-Type: text/event-stream\r\nMcp-Session-Id: s-1\r\n",
                &format!("event: message\ndata: {event}\n\n"),
            );
        }
    });
    address
}

#[test]
fn tools_are_listed_over_http() {
    let url = http_tracker("t0ken");
    let server = |token: &str| McpServer {
        name: "web".to_owned(),
        transport: McpTransport::Http {
            url: url.clone(),
            token: Some(token.to_owned()),
        },
    };
    let tools = mcp_tools(server("t0ken")).unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "search");
    assert_eq!(tools[0].title, "search");
    assert!(tools[0].read_only);
    assert_eq!(mcp_tools(server("wrong")), Err(McpError::Unauthorized));
}

#[test]
fn tools_are_listed_over_stdio_across_pages() {
    let folder = Folder::new();
    let tools = mcp_tools(tracker(&folder.0.join("calls.log"))).unwrap();
    let names: Vec<_> = tools
        .iter()
        .map(|t| (t.name.as_str(), t.read_only))
        .collect();
    assert_eq!(names, [("find_issue", true), ("create_issue", false)]);
}
