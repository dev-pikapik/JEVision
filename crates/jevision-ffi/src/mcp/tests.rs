use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use jevision_core::{FORMAT_VERSION, MouseButton, Point};
use serde_json::{Value, json};

use super::{AssistantHost, AssistantPermissions, Server};
use crate::{Hands, HandsError, Openable, RunOptions, SecretSource, TestRun};
use jevision_runner::Screenshot;

const SECRET: &str = "hunter2-secret";

/// A project folder that removes itself.
struct Project(PathBuf);

impl Project {
    fn new() -> Self {
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

    fn write(&self, rel: &str, text: &str) {
        let path = self.0.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn read(&self, rel: &str) -> Option<String> {
        std::fs::read_to_string(self.0.join(rel)).ok()
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
    fn open_app(&self, _: String) -> Result<(), HandsError> {
        Ok(())
    }
    fn open_items(&self, _: Vec<Openable>, _: Option<String>, _: bool) -> Result<(), HandsError> {
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
    fn click(&self, _: String, _: Point, _: MouseButton, _: bool) -> Result<(), HandsError> {
        Ok(())
    }
    fn type_text(&self, text: String) -> Result<(), HandsError> {
        self.typed.lock().unwrap().push(text);
        Ok(())
    }
    fn press_keys(&self, _: String) -> Result<(), HandsError> {
        Ok(())
    }
    fn user_input_detected(&self) -> bool {
        false
    }

    fn start_video(&self, _: String, _: String) -> Result<(), HandsError> {
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

#[derive(Default)]
struct Host {
    permissions: Mutex<AssistantPermissions>,
    hands: Arc<FakeHands>,
    runs: AtomicU32,
}

impl AssistantHost for Host {
    fn permissions(&self) -> AssistantPermissions {
        *self.permissions.lock().unwrap()
    }
    fn readiness_problem(&self) -> Option<String> {
        None
    }
    fn start_run(&self, _: Arc<TestRun>) -> Result<Arc<dyn Hands>, HandsError> {
        self.runs.fetch_add(1, Ordering::Relaxed);
        Ok(self.hands.clone())
    }
    fn run_paused(&self, run: Arc<TestRun>) {
        run.resume();
    }
    fn run_ended(&self) {}
    fn secrets(&self) -> Arc<dyn SecretSource> {
        Arc::new(Secrets)
    }
    fn run_options(&self) -> RunOptions {
        RunOptions::default()
    }
    fn capture(&self, _: String) -> Result<Screenshot, HandsError> {
        Ok(screen())
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

fn sign_in() -> String {
    json!({
        "format": FORMAT_VERSION,
        "name": "Sign in",
        "app": "com.example.mail",
        "tags": ["login"],
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
            "list_block_types",
            "run_test",
            "get_run",
            "stop_run",
            "get_report",
            "screenshot"
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
    assert!(text.contains("not valid JSON at line 2"), "{text}");
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
    assert!(text.contains("Block \"password\""), "{text}");
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
        text.contains("- warning: Block \"done\", field \"reference.image\": refs/none.png doesn't exist yet; the first run will save the screen there as the reference."),
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
        failed && text.contains("There is no block \"nope\""),
        "{text}"
    );
    let (failed, text) = call(
        &server,
        "add_blocks",
        json!({ "path": path, "blocks": [{ "type": "pressKeys" }] }),
    );
    assert!(
        failed && text.contains("blocks[0]") && text.contains("\"keys\""),
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
            failed && text.contains("isn't inside the project folder"),
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

#[test]
fn the_format_guide_covers_every_block_type() {
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
    for block in guide["blocks"].as_array().unwrap() {
        let example = block["example"].to_string();
        let kind = jevision_core::parse_block(&super::tools::without_edges(
            &serde_json::from_str(&example).unwrap(),
        ));
        assert!(
            kind.is_ok(),
            "the example of {} doesn't parse",
            block["type"]
        );
    }
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
