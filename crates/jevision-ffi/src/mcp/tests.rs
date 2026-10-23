use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use jevision_core::{
    FORMAT_VERSION, Modifiers, MouseButton, Point, Rect, ScrollDirection, ScrollUnit,
};
use serde_json::{Value, json};

use super::{
    AssistantHost, AssistantListener, AssistantPermissions, AssistantRequest, ConnectedAssistant,
    Server, answer_assistant_request, connected_assistants, forget_assistants,
    listen_for_assistants, pending_assistant_requests,
};
use crate::{Hands, HandsError, Openable, RunOptions, SecretSource, TestRun};
use jevision_core::{WindowPosition, WindowSize};
use jevision_runner::{AppStart, Screenshot};

const SECRET: &str = "hunter2-secret";

/// A project folder that removes itself.
pub(crate) struct Project(pub(crate) PathBuf);

impl Project {
    pub(crate) fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "jevision-mcp-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    pub(crate) fn write(&self, rel: &str, text: &str) {
        let path = self.0.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn read(&self, rel: &str) -> Option<String> {
        std::fs::read_to_string(self.0.join(rel)).ok()
    }
}

impl Default for Project {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A Mac where every action works and nothing is really clicked or typed.
#[derive(Default)]
struct FakeHands {
    typed: Mutex<Vec<String>>,
}

impl Hands for FakeHands {
    fn open_app(&self, _: String, _: AppStart) -> Result<(), HandsError> {
        Ok(())
    }
    fn sandboxed(&self, _: String) -> Result<bool, HandsError> {
        Ok(false)
    }
    fn arrange_window(
        &self,
        _: String,
        _: Option<WindowSize>,
        _: Option<WindowPosition>,
    ) -> Result<(), HandsError> {
        Ok(())
    }
    fn open_items(&self, _: Vec<Openable>, _: Option<String>, _: bool) -> Result<(), HandsError> {
        Ok(())
    }
    fn close_items(&self, _: Vec<Openable>, _: Option<String>) -> Result<(), HandsError> {
        Ok(())
    }
    fn activate(&self, _: String) -> Result<(), HandsError> {
        Ok(())
    }
    fn quit(&self, _: String) -> Result<(), HandsError> {
        Ok(())
    }
    fn capture(&self, _: String) -> Result<Screenshot, HandsError> {
        Ok(screen())
    }
    fn locate(&self, _: String, _: Vec<u8>, _: f64) -> Result<Option<Point>, HandsError> {
        Ok(None)
    }
    fn click(
        &self,
        _: String,
        _: Point,
        _: MouseButton,
        _: bool,
        _: Modifiers,
    ) -> Result<(), HandsError> {
        Ok(())
    }
    fn hover(&self, _: String, _: Point, _: Modifiers) -> Result<(), HandsError> {
        Ok(())
    }
    fn drag(
        &self,
        _: String,
        _: Vec<Point>,
        _: MouseButton,
        _: Modifiers,
    ) -> Result<(), HandsError> {
        Ok(())
    }
    fn scroll(
        &self,
        _: String,
        _: Point,
        _: ScrollDirection,
        _: u32,
        _: ScrollUnit,
        _: Modifiers,
    ) -> Result<(), HandsError> {
        Ok(())
    }
    fn type_text(&self, text: String) -> Result<(), HandsError> {
        self.typed.lock().unwrap().push(text);
        Ok(())
    }
    fn press_keys(&self, _: String) -> Result<(), HandsError> {
        Ok(())
    }
    fn clipboard(&self) -> Result<String, HandsError> {
        Ok(String::new())
    }
    fn set_clipboard(&self, _: String) -> Result<(), HandsError> {
        Ok(())
    }
    fn clipboard_image(&self) -> Result<Option<Vec<u8>>, HandsError> {
        Ok(None)
    }
    fn clipboard_files(&self) -> Result<Vec<String>, HandsError> {
        Ok(Vec::new())
    }
    fn set_clipboard_image(&self, _: Vec<u8>) -> Result<(), HandsError> {
        Ok(())
    }
    fn user_input_detected(&self) -> bool {
        false
    }

    fn start_video(&self, _: String, _: String, _: Vec<Rect>) -> Result<(), HandsError> {
        Ok(())
    }

    fn stop_video(&self) -> Result<(), HandsError> {
        Ok(())
    }
}

struct Secrets;

impl SecretSource for Secrets {
    fn secret(&self, _: String, _: Option<String>) -> Option<String> {
        Some(SECRET.to_owned())
    }
}

/// What opening the app does.
#[derive(Default)]
pub(crate) enum Launch {
    /// macOS can't open it.
    #[default]
    Fails,
    /// It starts, but never gets ready to ask.
    Hangs,
    /// It starts, and the person allows what it asks.
    Allows,
}

#[derive(Default)]
pub(crate) struct Host {
    /// Where the server and the app meet.
    pub(crate) shared: Project,
    pub(crate) permissions: Mutex<AssistantPermissions>,
    /// Why this Mac can't run tests, such as a locked screen.
    pub(crate) problem: Mutex<Option<String>>,
    hands: Arc<FakeHands>,
    /// How many runs started.
    pub(crate) runs: AtomicU32,
    /// How many runs ended.
    pub(crate) ended: AtomicU32,
    /// Runs wait for the person to step through them, so they stay going.
    pub(crate) stepping: AtomicBool,
    pub(crate) launch: Mutex<Launch>,
    /// How many times the app was opened.
    pub(crate) opened: AtomicU32,
    /// The app, once opened, listens until the test ends.
    app: Mutex<Option<Arc<AssistantListener>>>,
}

impl AssistantHost for Host {
    fn permissions(&self) -> AssistantPermissions {
        *self.permissions.lock().unwrap()
    }
    fn readiness_problem(&self) -> Option<String> {
        self.problem.lock().unwrap().clone()
    }
    fn start_run(&self, run: Arc<TestRun>) -> Result<Arc<dyn Hands>, HandsError> {
        run.set_step_mode(self.stepping.load(Ordering::Relaxed));
        self.runs.fetch_add(1, Ordering::Relaxed);
        Ok(self.hands.clone())
    }
    fn run_paused(&self, run: Arc<TestRun>) {
        run.resume();
    }
    fn run_ended(&self) {
        self.ended.fetch_add(1, Ordering::Relaxed);
    }
    fn secrets(&self) -> Arc<dyn SecretSource> {
        Arc::new(Secrets)
    }
    fn run_options(&self) -> RunOptions {
        RunOptions::default()
    }
    fn capture(&self, _: String) -> Result<Screenshot, HandsError> {
        Ok(screen())
    }
    fn shared_folder(&self) -> String {
        self.shared.0.display().to_string()
    }
    fn open_app(&self) -> Result<(), HandsError> {
        self.opened.fetch_add(1, Ordering::Relaxed);
        match *self.launch.lock().unwrap() {
            Launch::Fails => Err(HandsError::AppNotFound {
                app: "JEVision".to_owned(),
            }),
            Launch::Hangs => Ok(()),
            Launch::Allows => {
                let folder = self.shared_folder();
                *self.app.lock().unwrap() = listen_for_assistants(folder.clone());
                std::thread::spawn(move || app_answers(&folder, true));
                Ok(())
            }
        }
    }
}

fn screen() -> Screenshot {
    let mut png = Vec::new();
    let mut encoder = png::Encoder::new(&mut png, 4, 2);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&[200; 4 * 2 * 3]).unwrap();
    writer.finish().unwrap();
    Screenshot {
        png,
        scale: 1.0,
        dark: false,
        secure: Vec::new(),
    }
}

pub(crate) fn sign_in() -> String {
    json!({
        "format": FORMAT_VERSION,
        "name": "Sign in",
        "app": "com.example.mail",
        "tags": ["login"],
        "author": "Anna",
        "start": "open",
        "blocks": {
            "open": { "type": "openApp", "next": "password" },
            "password": { "type": "type", "text": "${secret.PASSWORD}" },
        },
    })
    .to_string()
}

fn setup() -> (Project, Arc<Host>, Server) {
    let project = Project::new();
    project.write("login/sign-in.flow.json", &sign_in());
    let host = Arc::new(Host::default());
    let server = Server::new(&project.0, host.clone()).unwrap();
    (project, host, server)
}

/// Calls tool `name`: whether it failed, and its text content.
fn call(server: &Server, name: &str, arguments: Value) -> (bool, String) {
    let request = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": { "name": name, "arguments": arguments },
    });
    let reply: Value = serde_json::from_str(&server.handle(&request.to_string()).unwrap()).unwrap();
    let result = &reply["result"];
    let texts: Vec<&str> = result["content"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["text"].as_str())
        .collect();
    (result["isError"] == true, texts.join("\n"))
}

#[test]
fn lists_only_the_tests_a_card_filter_chooses() {
    let (project, _, server) = setup();
    let other = sign_in().replace(
        r#""author":"Anna""#,
        r#""author":"Bo","priority":"high","flaky":true"#,
    );
    project.write("checkout.flow.json", &other);
    let listed = |arguments: Value| {
        let (failed, text) = call(&server, "list_tests", arguments);
        assert!(!failed, "{text}");
        text
    };
    assert!(listed(json!({})).starts_with("2 tests."));
    let text = listed(json!({ "author": " anna " }));
    assert!(
        text.starts_with("1 test.") && text.contains("login/sign-in"),
        "{text}"
    );
    let text = listed(json!({ "priority": "HIGH", "flaky": true }));
    assert!(
        text.starts_with("1 test.") && text.contains("checkout.flow.json"),
        "{text}"
    );
    assert!(listed(json!({ "testType": "smoke" })).starts_with("No test matches."));
    for (arguments, said) in [
        (json!({ "priority": "urgent" }), "isn’t a test priority"),
        (json!({ "flaky": "yes" }), "must be true or false"),
    ] {
        let (failed, text) = call(&server, "list_tests", arguments);
        assert!(failed && text.contains(said), "{text}");
    }
}

#[test]
fn speaks_the_protocol_over_lines() {
    let (_project, _, server) = setup();
    let input = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"list_tests","arguments":{"query":"LOGIN"}}}"#,
        r#"{"jsonrpc":"2.0","id":"four","method":"ping"}"#,
        r#"{"jsonrpc":"2.0","id":5,"method":"resources/list"}"#,
        r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"no_such_tool"}}"#,
        "{not json",
    ]
    .join("\n");
    let mut output = Vec::new();
    server.serve(input.as_bytes(), &mut output).unwrap();
    let replies: Vec<Value> = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(replies.len(), 7, "the notification gets no answer");
    let by_id = |id: Value| replies.iter().find(|r| r["id"] == id).unwrap();

    let init = &by_id(json!(1))["result"];
    assert_eq!(init["protocolVersion"], "2025-06-18");
    assert!(init["capabilities"]["tools"].is_object());
    assert_eq!(init["serverInfo"]["name"], "jevision");

    let tools = by_id(json!(2))["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        [
            "list_tests",
            "read_test",
            "write_test",
            "add_blocks",
            "remove_block",
            "validate_test",
            "check_project",
            "list_block_types",
            "run_test",
            "get_run",
            "stop_run",
            "get_report",
            "screenshot",
            "draft_bug_report",
            "send_bug_report"
        ]
    );
    assert!(
        tools.iter().all(|t| t["inputSchema"]["type"] == "object"
            && !t["description"].as_str().unwrap().is_empty())
    );

    let listed = &by_id(json!(3))["result"];
    assert_eq!(listed["isError"], false);
    let text = listed["content"][1]["text"].as_str().unwrap();
    assert!(text.contains("login/sign-in.flow.json") && text.contains("\"Sign in\""));
    assert!(text.contains("\"Anna\""), "the details are listed: {text}");

    assert_eq!(by_id(json!("four"))["result"], json!({}));
    assert_eq!(by_id(json!(5))["error"]["code"], -32601);
    assert_eq!(by_id(json!(6))["error"]["code"], -32602);
    assert_eq!(by_id(Value::Null)["error"]["code"], -32700);
}

#[test]
fn newer_protocol_versions_get_the_newest_known() {
    let (_project, _, server) = setup();
    let reply = server
        .handle(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2099-01-01"}}"#)
        .unwrap();
    let reply: Value = serde_json::from_str(&reply).unwrap();
    assert_eq!(
        reply["result"]["protocolVersion"],
        super::PROTOCOL_VERSIONS[0]
    );
}

#[test]
fn write_test_explains_what_is_wrong_and_saves_nothing() {
    let (project, _, server) = setup();
    let (failed, text) = call(
        &server,
        "write_test",
        json!({ "path": "new.flow.json", "test": "{ \"format\": 1,\n  \"name\": " }),
    );
    assert!(failed);
    assert!(text.contains("isn’t valid JSON. Check line 2"), "{text}");
    assert!(project.read("new.flow.json").is_none());

    let mut test: Value = serde_json::from_str(&sign_in()).unwrap();
    test["blocks"]["password"]["type"] = json!("pressKeys");
    test["blocks"]["password"]["keys"] = json!("cmd+");
    let (failed, text) = call(
        &server,
        "write_test",
        json!({ "path": "new.flow.json", "test": test }),
    );
    assert!(failed);
    assert!(text.contains("Block “password”"), "{text}");
    assert!(project.read("new.flow.json").is_none());

    let (failed, text) = call(
        &server,
        "write_test",
        json!({ "path": "new/one.flow.json", "test": sign_in() }),
    );
    assert!(!failed, "{text}");
    assert_eq!(
        project.read("new/one.flow.json").unwrap(),
        jevision_core::canonicalize(&sign_in()).unwrap()
    );
}

#[test]
fn missing_reference_images_are_reported_next_to_the_test() {
    let (project, _, server) = setup();
    let mut test: Value = serde_json::from_str(&sign_in()).unwrap();
    test["blocks"]["password"]["next"] = json!("done");
    test["blocks"]["done"] = json!({ "type": "check", "reference": { "image": "refs/none.png" } });
    let (failed, text) = call(
        &server,
        "write_test",
        json!({ "path": "login/new.flow.json", "test": test }),
    );
    assert!(!failed, "{text}");
    assert!(
        text.contains("- warning: Block “done”, field “reference.image”: refs/none.png doesn’t exist yet. The first run will save the screen there as the reference."),
        "{text}"
    );

    project.write("login/refs/none.png", "png");
    let (_, text) = call(
        &server,
        "validate_test",
        json!({ "path": "login/new.flow.json" }),
    );
    assert!(text.ends_with("No problems found."), "{text}");
}

#[test]
fn the_project_check_names_the_files_it_lacks() {
    let (project, _, server) = setup();
    let (failed, text) = call(&server, "check_project", json!({}));
    assert!(!failed, "{text}");
    assert_eq!(
        text,
        "Every test reads only files that are in the project, under the same names."
    );

    let mut test: Value = serde_json::from_str(&sign_in()).unwrap();
    test["blocks"]["password"]["next"] = json!("go");
    test["blocks"]["go"] = json!({ "type": "click", "target": { "image": "refs/go.png" } });
    project.write("login/go.flow.json", &test.to_string());
    let (failed, text) = call(&server, "check_project", json!({}));
    assert!(!failed, "{text}");
    assert_eq!(
        text,
        "login/go.flow.json: Problems:\n- error: Block “go”, field “target.image”: Missing file “login/refs/go.png” in the project."
    );
}

#[test]
fn edits_blocks_in_place() {
    let (project, _, server) = setup();
    let path = "login/sign-in.flow.json";
    let (failed, text) = call(
        &server,
        "add_blocks",
        json!({ "path": path, "after": "open", "blocks": [{ "type": "pressKeys", "keys": "cmd+n" }, { "type": "waitFor", "until": "screenStable" }] }),
    );
    assert!(!failed, "{text}");
    let flow = jevision_core::parse(&project.read(path).unwrap()).unwrap();
    let next = |id: &str| {
        flow.blocks
            .iter()
            .find(|b| b.id == id)
            .unwrap()
            .kind
            .edge("next")
            .map(str::to_owned)
    };
    assert_eq!(next("open").as_deref(), Some("pressKeys"));
    assert_eq!(next("pressKeys").as_deref(), Some("waitFor"));
    assert_eq!(next("waitFor").as_deref(), Some("password"));

    let (failed, text) = call(
        &server,
        "add_blocks",
        json!({ "path": path, "after": "nope", "blocks": [{ "type": "pressKeys", "keys": "a" }] }),
    );
    assert!(
        failed && text.contains("There is no block “nope”"),
        "{text}"
    );
    let (failed, text) = call(
        &server,
        "add_blocks",
        json!({ "path": path, "blocks": [{ "type": "pressKeys" }] }),
    );
    assert!(
        failed && text.contains("blocks[0]") && text.contains("“keys”"),
        "{text}"
    );

    let (failed, text) = call(
        &server,
        "remove_block",
        json!({ "path": path, "block": "pressKeys" }),
    );
    assert!(!failed, "{text}");
    let flow = jevision_core::parse(&project.read(path).unwrap()).unwrap();
    assert!(flow.blocks.iter().all(|b| b.id != "pressKeys"));
}

#[test]
fn stays_inside_the_project() {
    let (project, _, server) = setup();
    let outside = Project::new();
    outside.write("secret.flow.json", &sign_in());
    std::os::unix::fs::symlink(&outside.0, project.0.join("link")).unwrap();
    let escape = format!("{}/secret.flow.json", outside.0.display());
    for path in [
        "../secret.flow.json",
        escape.as_str(),
        "link/secret.flow.json",
        "link/new.flow.json",
    ] {
        let (failed, text) = call(&server, "read_test", json!({ "path": path }));
        assert!(
            failed && text.contains("isn’t inside the project folder"),
            "{path}: {text}"
        );
        let (failed, _) = call(
            &server,
            "write_test",
            json!({ "path": path, "test": sign_in() }),
        );
        assert!(failed, "{path}");
    }
    assert!(outside.read("new.flow.json").is_none());
    let (_, listed) = call(&server, "list_tests", json!({}));
    assert!(!listed.contains("secret.flow.json"), "{listed}");
    let (failed, _) = call(
        &server,
        "write_test",
        json!({ "path": ".jevision/x.flow.json", "test": sign_in() }),
    );
    assert!(failed);
}

#[test]
fn runs_and_screenshots_need_permission() {
    let (_project, host, server) = setup();
    let (failed, text) = call(&server, "run_test", json!({ "path": "login" }));
    assert!(
        failed && text.contains("Allow assistants to run tests"),
        "{text}"
    );
    assert_eq!(host.runs.load(Ordering::Relaxed), 0);
    let (failed, text) = call(&server, "screenshot", json!({ "app": "com.example.mail" }));
    assert!(
        failed && text.contains("Allow assistants to take screenshots"),
        "{text}"
    );

    host.permissions.lock().unwrap().screenshots = true;
    let reply = server
        .handle(&json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": "screenshot", "arguments": { "path": "login/sign-in.flow.json" } } }).to_string())
        .unwrap();
    let reply: Value = serde_json::from_str(&reply).unwrap();
    assert_eq!(reply["result"]["content"][1]["type"], "image");
    assert_eq!(reply["result"]["content"][1]["mimeType"], "image/png");
}

/// The picture of an image content block, as RGB rows.
fn picture(content: &Value) -> Vec<[u8; 3]> {
    use base64::Engine;
    let png = base64::engine::general_purpose::STANDARD
        .decode(content["data"].as_str().unwrap())
        .unwrap();
    let mut reader = png::Decoder::new(std::io::Cursor::new(png))
        .read_info()
        .unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    let channels = info.line_size / info.width as usize;
    buf.chunks_exact(channels)
        .take((info.width * info.height) as usize)
        .map(|p| [p[0], p[1], p[2]])
        .collect()
}

#[test]
fn screenshots_for_assistants_are_masked_or_kept_on_this_mac() {
    let (project, host, server) = setup();
    host.permissions.lock().unwrap().screenshots = true;
    // The project hides the left half of every window.
    project.write(
        "jevision.project.json",
        r#"{"format":1,"hide":[{"x":0,"y":0,"width":0.5,"height":1}]}"#,
    );
    let ask = || {
        let reply = server
            .handle(&json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": "screenshot", "arguments": { "app": "com.example.mail" } } }).to_string())
            .unwrap();
        serde_json::from_str::<Value>(&reply).unwrap()["result"].clone()
    };
    let result = ask();
    assert!(
        result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Grey boxes")
    );
    // 4 × 2 pixels: the left two of each row covered, the rest as captured.
    let grey = [128, 128, 128];
    let shown = [200, 200, 200];
    assert_eq!(
        picture(&result["content"][1]),
        [grey, grey, shown, shown, grey, grey, shown, shown]
    );

    project.write("jevision.project.json", r#"{"format":1,"localOnly":true}"#);
    let result = ask();
    assert_eq!(result["isError"], true);
    assert!(
        result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("stay on this Mac")
    );
    assert!(result["content"].as_array().unwrap().len() == 1);
}

#[test]
fn runs_in_an_environment_of_the_project() {
    let (project, host, server) = setup();
    host.permissions.lock().unwrap().run_tests = true;
    let (failed, text) = call(
        &server,
        "run_test",
        json!({ "path": "login", "env": "prod" }),
    );
    assert!(failed && text.contains("no environments"), "{text}");

    project.write(
        "jevision.environments.json",
        r#"{"format":1,"environments":{"staging":{}}}"#,
    );
    let (failed, text) = call(
        &server,
        "run_test",
        json!({ "path": "login", "env": "prod" }),
    );
    assert!(
        failed && text.contains("The project has: staging"),
        "{text}"
    );
    let (failed, text) = call(
        &server,
        "run_test",
        json!({ "path": "login", "env": "staging" }),
    );
    assert!(!failed, "{text}");
    let (_, status) = call(&server, "get_run", json!({ "run": "1", "wait": 60 }));
    let status: Value = serde_json::from_str(&status).unwrap();
    assert_eq!(status["passed"], 1, "{status}");
    let runs = jevision_runner::Run::list(&project.0).unwrap();
    assert_eq!(runs[0].report.environment.as_deref(), Some("staging"));

    // Without "env" the run uses the file's default environment.
    project.write(
        "jevision.environments.json",
        r#"{"format":1,"default":"dev","environments":{"dev":{},"staging":{}}}"#,
    );
    let (failed, text) = call(&server, "run_test", json!({ "path": "login" }));
    assert!(!failed, "{text}");
    let (_, status) = call(&server, "get_run", json!({ "run": "2", "wait": 60 }));
    let status: Value = serde_json::from_str(&status).unwrap();
    assert_eq!(status["passed"], 1, "{status}");
    let runs = jevision_runner::Run::list(&project.0).unwrap();
    assert!(
        runs.iter()
            .any(|r| r.report.environment.as_deref() == Some("dev")),
        "{runs:?}"
    );
}

#[test]
fn runs_on_fake_hands_without_giving_away_secrets() {
    let (project, host, server) = setup();
    host.permissions.lock().unwrap().run_tests = true;
    let (failed, text) = call(&server, "run_test", json!({ "path": "login" }));
    assert!(!failed, "{text}");
    let (_, status) = call(&server, "get_run", json!({ "run": "1", "wait": 60 }));
    let status: Value = serde_json::from_str(&status).unwrap();
    assert_eq!(status["status"], "finished", "{status}");
    assert_eq!(status["passed"], 1, "{status}");
    assert_eq!(*host.hands.typed.lock().unwrap(), [SECRET]);

    let report = status["tests"][0]["report"].as_str().unwrap();
    let (failed, text) = call(
        &server,
        "get_report",
        json!({ "report": report, "images": "all" }),
    );
    assert!(!failed, "{text}");
    assert!(text.contains("\"verdict\": \"passed\""), "{text}");

    let (_, read) = call(
        &server,
        "read_test",
        json!({ "path": "login/sign-in.flow.json" }),
    );
    assert!(read.contains("${secret.PASSWORD}"));
    for answer in [
        text,
        read,
        status.to_string(),
        call(&server, "get_report", json!({})).1,
    ] {
        assert!(!answer.contains(SECRET), "{answer}");
    }
    let saved = std::fs::read_to_string(
        project
            .0
            .join(".jevision/runs")
            .join(report)
            .join("report.json"),
    )
    .unwrap();
    assert!(!saved.contains(SECRET));
}

/// The values the core allows for `field` (`button`, or `reference.compare` inside an object)
/// of `block`, from what it says about a wrong one.
fn allowed_values(block: &Value, field: &str) -> Vec<String> {
    let mut values = Vec::new();
    for wrong in [json!("\u{1}"), json!(["\u{1}"])] {
        let mut probe = block.clone();
        match field.split_once('.') {
            Some((object, key)) => probe[object][key] = wrong,
            None => probe[field] = wrong,
        }
        let Err(e) = jevision_core::parse_block(&probe) else {
            continue;
        };
        for issue in &e.issues {
            if issue.code == "flow.field.value" && issue.param("field") == Some(field) {
                let allowed = issue.param("allowed").unwrap_or_default();
                values.extend(
                    allowed
                        .split(", ")
                        .map(|v| v.trim_end_matches(" …"))
                        .filter(|v| *v != "…")
                        // A shape like {"contains": …} is named by its key.
                        .map(|v| v.split('"').nth(1).unwrap_or(v))
                        .map(str::to_owned),
                );
            }
        }
    }
    values
}

#[test]
fn the_format_guide_describes_every_field_and_value_the_core_reads() {
    let guide = super::blocks::guide();
    let types: Vec<&str> = guide["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["type"].as_str().unwrap())
        .collect();
    let catalog = jevision_core::catalog();
    assert_eq!(
        types,
        catalog
            .iter()
            .map(|s| s.type_name.as_str())
            .collect::<Vec<_>>()
    );
    let base = jevision_core::parse(
        &json!({
            "format": FORMAT_VERSION,
            "name": "Guide",
            "app": "com.example.mail",
            "start": "wait",
            "blocks": { "wait": { "type": "pause", "duration": "1s" } },
        })
        .to_string(),
    )
    .unwrap();
    let mut gaps = Vec::new();
    for block in guide["blocks"].as_array().unwrap() {
        let type_name = block["type"].as_str().unwrap();
        let fields = block["fields"].as_object().unwrap();
        let mut described: Vec<&str> = fields.keys().map(String::as_str).collect();
        described.extend(
            block["edges"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| e.as_str().unwrap()),
        );
        described.push("type");
        described.sort_unstable();
        let mut read = jevision_core::block_fields(type_name);
        read.sort_unstable();
        if described != read {
            gaps.push(format!(
                "{type_name}: the guide has {described:?}, the core reads {read:?}"
            ));
        }
        let bare = json!({ "type": type_name });
        let choices: Vec<(&str, Vec<String>)> = read
            .iter()
            .map(|f| (*f, allowed_values(&bare, f)))
            .collect();
        for field in &read {
            let mut values = allowed_values(&bare, field);
            for (choice, options) in &choices {
                for option in options {
                    let mut chosen = bare.clone();
                    chosen[*choice] = json!(option);
                    values.extend(allowed_values(&chosen, field));
                }
            }
            let text = fields
                .get(*field)
                .and_then(Value::as_str)
                .unwrap_or_default();
            values.sort_unstable();
            values.dedup();
            for value in values
                .iter()
                .filter(|v| !text.contains(&format!("\"{v}\"")))
            {
                gaps.push(format!("{type_name}.{field}: \"{value}\" isn't named"));
            }
        }
        let example = jevision_core::parse_block(&super::tools::without_edges(&block["example"]))
            .unwrap_or_else(|e| panic!("the example of {type_name} doesn't parse: {:?}", e.issues));
        let mut flow = base.clone();
        flow.insert_blocks(vec![example], None).unwrap();
        // A loop added on its own starts with an empty body, which the assistant fills next.
        for issue in jevision_core::validate(&flow).iter().filter(|i| {
            i.severity == jevision_core::Severity::Error && i.code != "flow.loop.emptyBody"
        }) {
            gaps.push(format!("the example of {type_name}: {}", issue.code));
        }
    }
    let shapes = &guide["shapes"];
    let target = shapes["target"].as_str().unwrap();
    for field in jevision_core::target_fields() {
        if !target.contains(&format!("\"{field}\"")) {
            gaps.push(format!("target: \"{field}\" isn't named"));
        }
    }
    let reference = shapes["reference"].as_str().unwrap();
    let check = json!({ "type": "check", "reference": { "image": "refs/inbox.png" } });
    for field in jevision_core::reference_fields() {
        let mut named = vec![field.to_owned()];
        named.extend(allowed_values(&check, &format!("reference.{field}")));
        if field == "provider" {
            named.extend(jevision_core::AI_PROVIDERS.iter().map(|p| (*p).to_owned()));
        }
        for word in named
            .iter()
            .filter(|w| !reference.contains(&format!("\"{w}\"")))
        {
            gaps.push(format!("reference.{field}: \"{word}\" isn't named"));
        }
    }
    assert!(gaps.is_empty(), "the guide misses:\n{}", gaps.join("\n"));
}

#[test]
fn runs_chosen_rows_of_a_data_table() {
    let (project, host, server) = setup();
    host.permissions.lock().unwrap().run_tests = true;
    project.write("data/users.csv", "name;user\nAnn;ann\nBo;bo\nCy;cy\n");
    project.write(
        "data/each.flow.json",
        &json!({
            "format": FORMAT_VERSION,
            "name": "Each user",
            "app": "com.example.mail",
            "start": "rows",
            "blocks": {
                "rows": { "type": "useData", "file": "users.csv", "body": "type" },
                "type": { "type": "type", "text": "${data.user}" },
            },
        })
        .to_string(),
    );
    let (failed, text) = call(
        &server,
        "run_test",
        json!({ "path": "data", "rows": "4-2" }),
    );
    assert!(failed, "{text}");

    let (failed, text) = call(
        &server,
        "run_test",
        json!({ "path": "data", "rows": "2-3" }),
    );
    assert!(!failed, "{text}");
    let (_, status) = call(&server, "get_run", json!({ "run": "1", "wait": 60 }));
    let status: Value = serde_json::from_str(&status).unwrap();
    assert_eq!(status["passed"], 1, "{status}");
    let rows = &status["tests"][0]["rows"];
    assert_eq!(rows[0]["row"], 2, "{status}");
    assert_eq!(rows[1]["label"], "Cy", "{status}");
    assert_eq!(rows[1]["verdict"], "passed", "{status}");
    assert_eq!(*host.hands.typed.lock().unwrap(), ["bo", "cy"]);
}

/// A failed run of the sign-in test saved in `project`, with its screenshots; returns its name.
fn failed_run(project: &Project) -> &'static str {
    project.write(
        "login/sign-in.flow.json",
        &json!({
            "format": FORMAT_VERSION, "name": "Sign in", "app": "com.example.mail", "start": "open",
            "blocks": {
                "open": { "type": "openApp", "next": "password" },
                "password": { "type": "type", "text": "${secret.PASSWORD}", "next": "inbox" },
                "inbox": { "type": "check", "reference": { "image": "refs/inbox.png", "compare": "similar", "threshold": 0.95 } },
            },
        })
        .to_string(),
    );
    let name = "2026-10-18-120000-sign-in";
    let run = format!(".jevision/runs/{name}");
    project.write(
        &format!("{run}/report.json"),
        r#"{"format":1,"test":"Sign in","file":"login/sign-in.flow.json","app":"com.example.mail",
        "startedAt":"2026-10-18T12:00:00.000Z","durationMs":4200,"verdict":"failed","flaky":false,
        "attempts":[{"number":1,"verdict":"failed","durationMs":2200,"steps":[
          {"seq":1,"test":"login/sign-in.flow.json","block":"open","type":"openApp","status":"passed","durationMs":900},
          {"seq":2,"test":"login/sign-in.flow.json","block":"password","type":"type","status":"passed","durationMs":300},
          {"seq":3,"test":"login/sign-in.flow.json","block":"inbox","type":"check","status":"failed","durationMs":200,
           "after":"shots/1-003-inbox-after.png","expected":"shots/1-003-inbox-expected.png","similarity":0.81,
           "failure":{"code":"run.check.mismatch","params":{"block":"inbox","image":"refs/inbox.png",
             "similarity":"81.0%","threshold":"95.0%"}}}]}]}"#,
    );
    for shot in ["1-003-inbox-after", "1-003-inbox-expected"] {
        let path = project.0.join(&run).join(format!("shots/{shot}.png"));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, screen().png).unwrap();
    }
    name
}

/// A GitHub API on this Mac that gives `replies` in turn; the requests it got.
fn github(replies: Vec<(u16, &str)>) -> (String, Arc<Mutex<Vec<String>>>) {
    use std::io::{BufRead, BufReader, Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let seen = requests.clone();
    let replies: Vec<(u16, String)> = replies.into_iter().map(|(s, b)| (s, b.into())).collect();
    std::thread::spawn(move || {
        for (status, body) in replies {
            let Ok((stream, _)) = listener.accept() else {
                return;
            };
            let mut reader = BufReader::new(stream);
            let (mut request, mut length) = (String::new(), 0);
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = v.trim().parse().unwrap();
                }
                request.push_str(&line);
                if line == "\r\n" || line.is_empty() {
                    break;
                }
            }
            let mut content = vec![0; length];
            reader.read_exact(&mut content).unwrap();
            request.push_str(&String::from_utf8_lossy(&content));
            seen.lock().unwrap().push(request);
            let _ = write!(
                reader.into_inner(),
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    (address, requests)
}

#[test]
fn a_failed_run_is_drafted_as_a_bug_report_and_sent_with_secrets_masked() {
    let (project, host, server) = setup();
    let name = failed_run(&project);

    let (failed, draft) = call(&server, "draft_bug_report", json!({}));
    assert!(!failed, "{draft}");
    let draft: Value = serde_json::from_str(&draft).unwrap();
    assert_eq!(draft["report"], name);
    assert_eq!(draft["title"], "Sign in fails at Check “inbox”");
    assert!(
        draft["body"]
            .as_str()
            .unwrap()
            .contains("### Steps to reproduce")
    );
    assert_eq!(draft["screenshots"], json!(["Expected", "Actual"]));

    // Not on GitHub: nothing is sent, and the person is told why.
    let (failed, text) = call(&server, "send_bug_report", json!({ "report": name }));
    assert!(
        failed && text.contains("isn’t in a git repository"),
        "{text}"
    );

    // What reaches GitHub has the secret masked, even when typed into the text.
    let dir = project.0.join(".jevision/runs").join(name);
    let report = crate::draft_bug_report(
        project.0.to_string_lossy().into(),
        dir.to_string_lossy().into(),
        host.secrets(),
    )
    .unwrap()
    .unwrap();
    let mut edited = report.clone();
    edited.body = format!("The password {SECRET} is typed.\n");
    edited.screenshots.retain(|s| s.label == "Actual");
    let (address, requests) = github(vec![
        (404, "{}"),
        (201, r#"{"sha": "b1"}"#),
        (201, r#"{"sha": "t1"}"#),
        (201, r#"{"sha": "c1"}"#),
        (201, "{}"),
        (
            201,
            r#"{"number": 4, "title": "t", "html_url": "https://github.com/acme/qa/issues/4"}"#,
        ),
    ]);
    let hub = jevision_git::GitHub::new(&address, "token".into());
    let repo = jevision_git::GitHubRepo::from_url("https://github.com/acme/qa.git").unwrap();
    let url = crate::bug::send(&hub, &repo, &project.0, &dir, &edited, None, &Secrets).unwrap();
    assert_eq!(url, "https://github.com/acme/qa/issues/4");
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 6);
    let issue: Value = serde_json::from_str(requests[5].split("\r\n\r\n").nth(1).unwrap()).unwrap();
    let body = issue["body"].as_str().unwrap();
    assert!(body.starts_with("The password •••• is typed."), "{body}");
    assert!(
        body.contains(&format!("/blob/c1/{name}/Actual.png?raw=true")),
        "{body}"
    );
    assert!(body.contains(&format!("<!-- jevision-bug: {} -->", report.fingerprint)));
    assert!(requests.iter().all(|r| !r.contains(SECRET)));

    // Only pictures of the run itself are sent.
    let mut outside = report;
    outside.screenshots[0].path = project
        .0
        .join("login/sign-in.flow.json")
        .to_string_lossy()
        .into();
    assert!(crate::bug::send(&hub, &repo, &project.0, &dir, &outside, None, &Secrets).is_err());
}

fn hello(server: &Server, name: &str, title: Option<&str>) {
    let mut info = json!({ "name": name, "version": "2.1" });
    if let Some(title) = title {
        info["title"] = json!(title);
    }
    let request = json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": { "protocolVersion": "2025-06-18", "clientInfo": info },
    });
    server.handle(&request.to_string()).unwrap();
}

/// One assistant by name, whatever place it holds in the list.
fn assistant<'a>(listed: &'a [ConnectedAssistant], name: &str) -> &'a ConnectedAssistant {
    listed.iter().find(|a| a.name == name).unwrap()
}

#[test]
fn connected_assistants_are_listed_with_their_last_call() {
    let (_project, host, server) = setup();
    let folder = host.shared_folder();
    assert!(connected_assistants(folder.clone()).is_empty());

    // Listed newest first, then by name: Claude Code leads whether or not a second passed.
    let other = Server::new(&_project.0, host.clone()).unwrap();
    hello(&other, "cursor", None);
    hello(&server, "claude-code", Some("Claude Code"));
    call(&server, "list_tests", json!({}));
    call(
        &server,
        "validate_test",
        json!({ "path": "login/sign-in.flow.json" }),
    );

    // Two connections that fall in different seconds list in the other order: the order has its
    // own test, with the seconds written down.
    let listed = connected_assistants(folder.clone());
    assert_eq!(listed.len(), 2);
    let claude = assistant(&listed, "claude-code");
    assert_eq!(
        (claude.title.as_str(), claude.version.as_str(), claude.calls),
        ("Claude Code", "2.1", 2)
    );
    assert!(claude.connected);
    assert_eq!(claude.last_call.as_ref().unwrap().tool, "validate_test");
    assert!(claude.first_connected_at > 1_700_000_000);
    // Without a title the name is shown; without a call, none is shown.
    let cursor = assistant(&listed, "cursor");
    assert_eq!(cursor.title, "cursor");
    assert_eq!(cursor.last_call, None);

    // The same assistant connecting again is one entry with both connections' calls.
    let again = Server::new(&_project.0, host.clone()).unwrap();
    hello(&again, "claude-code", Some("Claude Code"));
    call(&again, "list_tests", json!({}));
    let listed = connected_assistants(folder.clone());
    assert_eq!(listed.len(), 2);
    let claude = assistant(&listed, "claude-code");
    assert_eq!(claude.calls, 3);
    assert_eq!(claude.last_call.as_ref().unwrap().tool, "list_tests");

    // One that hung up, or went quiet, isn't connected any more, and can be forgotten.
    server.agents.end();
    let quiet = std::fs::read_dir(host.shared.0.join("sessions"))
        .unwrap()
        .flatten()
        .find(|e| e.file_name().to_string_lossy().starts_with("cursor"))
        .unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .open(quiet.path())
        .unwrap()
        .set_modified(std::time::SystemTime::now() - Duration::from_secs(120))
        .unwrap();
    let listed = connected_assistants(folder.clone());
    assert!(assistant(&listed, "claude-code").connected);
    assert!(!assistant(&listed, "cursor").connected);
    server.agents.end();
    again.agents.end();
    assert!(
        connected_assistants(folder.clone())
            .iter()
            .all(|a| !a.connected)
    );
    forget_assistants(folder.clone());
    assert!(connected_assistants(folder).is_empty());
}

/// Connected ones first, then the one that connected last, then by name: a tie in the second
/// goes to the name, and not to which of two threads wrote its file first.
#[test]
fn assistants_are_listed_connected_first_then_latest_then_by_name() {
    let (_project, host, _server) = setup();
    let sessions = host.shared.0.join("sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    for (file, name, started, ended) in [
        ("1.json", "zed", 1_700_000_300_i64, json!(null)),
        ("2.json", "claude-code", 1_700_000_300, json!(null)),
        ("3.json", "cursor", 1_700_000_100, json!(null)),
        ("4.json", "gone", 1_700_000_500, json!(1_700_000_600)),
    ] {
        let record = json!({
            "name": name, "title": name, "version": "1", "remote": null,
            "started": started, "calls": 0, "lastCall": null, "ended": ended,
        });
        std::fs::write(sessions.join(file), record.to_string()).unwrap();
    }
    let listed = connected_assistants(host.shared_folder());
    assert_eq!(
        listed
            .iter()
            .map(|a| (a.name.as_str(), a.connected))
            .collect::<Vec<_>>(),
        [
            ("claude-code", true),
            ("zed", true),
            ("cursor", true),
            ("gone", false)
        ]
    );
}

/// What the app does: polls for questions, keeps what it saw, and answers.
pub(crate) fn app_answers(folder: &str, allow: bool) -> AssistantRequest {
    for _ in 0..100 {
        if let Some(request) = pending_assistant_requests(folder.to_owned())
            .into_iter()
            .next()
        {
            assert!(answer_assistant_request(
                folder.to_owned(),
                request.id.clone(),
                allow
            ));
            return request;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("the assistant never asked");
}

/// An assistant on this Mac whose runs the person must allow first.
fn asking() -> (Project, Arc<Host>, Server) {
    let (project, host, server) = setup();
    *host.permissions.lock().unwrap() = AssistantPermissions {
        run_tests: true,
        screenshots: false,
        ask_first: true,
    };
    hello(&server, "claude-code", Some("Claude Code"));
    (project, host, server)
}

#[test]
fn a_closed_app_is_opened_in_the_background_to_ask() {
    let (_project, host, server) = asking();
    *host.launch.lock().unwrap() = Launch::Allows;
    let folder = host.shared_folder();
    let opened = || host.opened.load(Ordering::Relaxed);

    let (failed, text) = call(&server, "run_test", json!({ "path": "login" }));
    assert!(!failed, "{text}");
    call(&server, "get_run", json!({ "run": "1", "wait": 60 }));
    assert_eq!((opened(), host.runs.load(Ordering::Relaxed)), (1, 1));

    // Now that it is open, it is asked without opening it again.
    let (allowed, _) = std::thread::scope(|scope| {
        let app = scope.spawn(|| app_answers(&folder, true));
        let outcome = call(&server, "run_test", json!({ "path": "login" }));
        (outcome, app.join().unwrap())
    });
    assert!(!allowed.0, "{}", allowed.1);
    assert_eq!(opened(), 1);
}

#[test]
fn an_open_app_counts_as_open_however_long_it_was_quiet_and_not_once_it_quits() {
    let (_project, host, server) = asking();
    let folder = host.shared_folder();
    let app = listen_for_assistants(folder.clone()).unwrap();
    // macOS lets a hidden app nap, so it may touch nothing for minutes.
    std::fs::File::options()
        .write(true)
        .open(host.shared.0.join("listening"))
        .unwrap()
        .set_modified(std::time::SystemTime::now() - Duration::from_secs(3600))
        .unwrap();
    let (allowed, _) = std::thread::scope(|scope| {
        let answers = scope.spawn(|| app_answers(&folder, true));
        let outcome = call(&server, "run_test", json!({ "path": "login" }));
        (outcome, answers.join().unwrap())
    });
    assert!(!allowed.0, "{}", allowed.1);
    assert_eq!(host.opened.load(Ordering::Relaxed), 0);
    call(&server, "get_run", json!({ "run": "1", "wait": 60 }));

    // Once it quits, it is opened again to ask.
    drop(app);
    let (failed, text) = call(&server, "run_test", json!({ "path": "login" }));
    assert!(
        failed && text.contains("JEVision couldn’t open to ask"),
        "{text}"
    );
    assert_eq!(host.opened.load(Ordering::Relaxed), 1);
}

#[test]
fn an_app_that_cant_open_to_ask_says_why_and_nothing_runs() {
    let (_project, host, mut server) = asking();
    let tried = || {
        (
            host.opened.load(Ordering::Relaxed),
            host.runs.load(Ordering::Relaxed),
        )
    };

    // macOS can't open it: the assistant learns why and what the person can do.
    let (failed, text) = call(&server, "run_test", json!({ "path": "login" }));
    assert!(
        failed
            && text.contains("JEVision couldn’t open to ask")
            && text.contains("The app JEVision isn’t installed on this Mac.")
            && text.contains("“Ask before an assistant runs a test”"),
        "{text}"
    );
    assert_eq!(tried(), (1, 0));

    // It opens but never gets ready to ask.
    *host.launch.lock().unwrap() = Launch::Hangs;
    Arc::get_mut(&mut server.agents).unwrap().open_timeout = Duration::from_secs(1);
    let (failed, text) = call(&server, "run_test", json!({ "path": "login" }));
    assert!(
        failed && text.contains("didn’t finish starting within 1 second."),
        "{text}"
    );
    assert_eq!(tried(), (2, 0));
}

#[test]
fn a_run_an_assistant_starts_waits_for_the_person() {
    let (project, host, mut server) = setup();
    *host.permissions.lock().unwrap() = AssistantPermissions {
        run_tests: true,
        screenshots: false,
        ask_first: true,
    };
    let folder = host.shared_folder();
    hello(&server, "claude-code", Some("Claude Code"));

    // The person allows it and sees what the assistant wants to do.
    let _app = listen_for_assistants(folder.clone()).unwrap();
    let (allowed, seen) = std::thread::scope(|scope| {
        let app = scope.spawn(|| app_answers(&folder, true));
        let outcome = call(&server, "run_test", json!({ "path": "login" }));
        (outcome, app.join().unwrap())
    });
    assert!(!allowed.0, "{}", allowed.1);
    assert_eq!(seen.assistant, "Claude Code");
    assert_eq!(
        seen.project,
        std::fs::canonicalize(&project.0)
            .unwrap()
            .display()
            .to_string()
    );
    assert_eq!(seen.tests, ["login/sign-in.flow.json"]);
    assert_eq!(seen.apps, ["com.example.mail"]);
    assert_eq!(seen.environment, None);
    call(&server, "get_run", json!({ "run": "1", "wait": 60 }));
    assert_eq!(host.runs.load(Ordering::Relaxed), 1);
    assert!(pending_assistant_requests(folder.clone()).is_empty());

    // The person says no: the assistant is told, and nothing runs.
    let (denied, _) = std::thread::scope(|scope| {
        let app = scope.spawn(|| app_answers(&folder, false));
        let outcome = call(&server, "run_test", json!({ "path": "login" }));
        (outcome, app.join().unwrap())
    });
    assert!(
        denied.0 && denied.1.contains("didn’t allow"),
        "{}",
        denied.1
    );
    assert_eq!(host.runs.load(Ordering::Relaxed), 1);

    // Nobody answers: that is a no too, and the question is withdrawn.
    Arc::get_mut(&mut server.agents).unwrap().timeout = Duration::from_secs(1);
    pending_assistant_requests(folder.clone());
    let (failed, text) = call(&server, "run_test", json!({ "path": "login" }));
    assert!(
        failed && text.contains("didn’t answer within 1 second,"),
        "{text}"
    );
    assert_eq!(host.runs.load(Ordering::Relaxed), 1);
    assert!(pending_assistant_requests(folder.clone()).is_empty());

    // An answer to a question that is gone, or isn't one, changes nothing.
    assert!(!answer_assistant_request(
        folder.clone(),
        "1-2-3".into(),
        true
    ));
    assert!(!answer_assistant_request(folder, "../../x".into(), true));

    // Turned off, assistants run tests without asking.
    host.permissions.lock().unwrap().ask_first = false;
    let (failed, text) = call(&server, "run_test", json!({ "path": "login" }));
    assert!(!failed, "{text}");
    // The app was open whenever it was asked, so it was never opened.
    assert_eq!(host.opened.load(Ordering::Relaxed), 0);
}

#[test]
fn pairing_and_tool_errors_read_in_russian_and_english() {
    use jevision_l10n::tr_in;
    assert_eq!(
        tr_in!("en", "mcp.pairing.codeExpired"),
        "The pairing code has expired. Show a new one in JEVision → Settings → iPhone & Remote."
    );
    assert_eq!(
        tr_in!("ru", "mcp.pairing.codeExpired"),
        "Срок действия кода подключения истёк. Покажите новый в JEVision → «Настройки» → «iPhone и удалённо»."
    );
    assert_eq!(
        tr_in!("en", "mcp.block.notFound", "nope", "open, waitFor"),
        "There is no block “nope”. The blocks are: open, waitFor."
    );
    assert_eq!(
        tr_in!("ru", "mcp.block.notFound", "nope", "open, waitFor"),
        "Блока «nope» нет. Есть блоки: open, waitFor."
    );
    for (n, en, ru) in [
        (1, "within 1 second,", "за 1 секунду,"),
        (2, "within 2 seconds,", "за 2 секунды,"),
        (5, "within 5 seconds,", "за 5 секунд,"),
    ] {
        assert!(tr_in!("en", "mcp.ask.timeout", n).contains(en));
        assert!(tr_in!("ru", "mcp.ask.timeout", n).contains(ru));
    }
    assert_eq!(
        tr_in!("en", "mcp.ask.notOpen", "It broke."),
        "JEVision couldn’t open to ask the person at this Mac about the run. It broke. Ask them to open JEVision and try again, or to turn off “Ask before an assistant runs a test” in JEVision → Settings → AI → Assistants."
    );
    assert!(
        tr_in!("ru", "mcp.ask.notOpen", "Сломалось.").contains("о запуске. Сломалось. Попросите")
    );
    for (n, en, ru) in [
        (1, "within 1 second.", "за 1 секунду."),
        (2, "within 2 seconds.", "за 2 секунды."),
        (5, "within 5 seconds.", "за 5 секунд."),
    ] {
        assert!(tr_in!("en", "mcp.ask.notReady", n).contains(en));
        assert!(tr_in!("ru", "mcp.ask.notReady", n).contains(ru));
    }
    for (n, en, ru) in [
        (1, "the first 1 screenshot is", "только первый 1 снимок"),
        (2, "the first 2 screenshots are", "только первые 2 снимка"),
        (5, "the first 5 screenshots are", "только первые 5 снимков"),
    ] {
        assert!(tr_in!("en", "mcp.report.tooManyImages", n).contains(en));
        assert!(tr_in!("ru", "mcp.report.tooManyImages", n).contains(ru));
    }
}
