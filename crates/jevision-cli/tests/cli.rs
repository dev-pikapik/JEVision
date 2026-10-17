use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

const VALID: &str = r#"{"format": 1, "name": "t", "app": "com.example.App", "start": "a",
  "blocks": {"a": {"type": "pressKeys", "keys": "cmd+s"}}}"#;
const BROKEN_EDGE: &str = r#"{"format": 1, "name": "t", "app": "com.example.App", "start": "a",
  "blocks": {"a": {"type": "pressKeys", "keys": "cmd+s", "next": "gone"}}}"#;

/// A fresh scratch folder per test, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("jevision-cli-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn file(&self, name: &str, text: &str) -> String {
        let path = self.0.join(name);
        fs::write(&path, text).unwrap();
        path.to_string_lossy().into_owned()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn jevision(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_jevision"))
        .args(args)
        .output()
        .unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn validate_reports_status_and_issues() {
    let dir = Scratch::new("validate");
    let good = dir.file("good.flow.json", VALID);
    let bad = dir.file("bad.flow.json", BROKEN_EDGE);

    let ok = jevision(&["validate", &good]);
    assert_eq!(ok.status.code(), Some(0));
    assert_eq!(stdout(&ok), format!("{good}: ok\n"));

    let failed = jevision(&["validate", &good, &bad]);
    assert_eq!(failed.status.code(), Some(1));
    let text = stdout(&failed);
    assert!(
        text.contains(&format!("{bad}: 1 error, 0 warnings")),
        "{text}"
    );
    assert!(
        text.contains("error   flow.edge.unknownTarget block=a edge=next target=gone"),
        "{text}"
    );

    let missing = jevision(&["validate", &dir.0.join("none.flow.json").to_string_lossy()]);
    assert_eq!(missing.status.code(), Some(2));
}

#[test]
fn validate_json_is_machine_readable() {
    let dir = Scratch::new("json");
    dir.file("bad.flow.json", BROKEN_EDGE);
    dir.file("notes.txt", "not a test");
    let out = jevision(&["validate", "--json", &dir.0.to_string_lossy()]);
    assert_eq!(out.status.code(), Some(1));
    let reports: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let reports = reports.as_array().unwrap();
    assert_eq!(reports.len(), 1, "folders yield only *.flow.json files");
    assert_eq!(reports[0]["valid"], false);
    let issue = &reports[0]["issues"][0];
    assert_eq!(issue["code"], "flow.edge.unknownTarget");
    assert_eq!(issue["severity"], "error");
    assert_eq!(issue["params"]["target"], "gone");
}

#[test]
fn fmt_check_then_fix() {
    let dir = Scratch::new("fmt");
    let file = dir.file("t.flow.json", VALID);

    let check = jevision(&["fmt", "--check", &file]);
    assert_eq!(check.status.code(), Some(1));
    assert_eq!(stdout(&check), format!("{file}: not formatted\n"));
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        VALID,
        "--check never writes"
    );

    let fix = jevision(&["fmt", &file]);
    assert_eq!(fix.status.code(), Some(0));
    let formatted = fs::read_to_string(&file).unwrap();
    assert!(formatted.starts_with("{\n  \"format\": 1,\n") && formatted.ends_with("}\n"));

    let again = jevision(&["fmt", "--check", &file]);
    assert_eq!(again.status.code(), Some(0));
    assert_eq!(stdout(&again), "");
    assert_eq!(
        fs::read_dir(&dir.0).unwrap().count(),
        1,
        "no temporary files left"
    );
}

#[test]
fn fmt_refuses_unreadable_tests() {
    let dir = Scratch::new("fmt-bad");
    let file = dir.file("t.flow.json", "{ not json");
    let out = jevision(&["fmt", &file]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).contains("flow.json.syntax"));
    assert_eq!(fs::read_to_string(&file).unwrap(), "{ not json");
}

#[test]
fn usage_errors_exit_with_2() {
    assert_eq!(jevision(&["validate"]).status.code(), Some(2));
    assert_eq!(jevision(&["run", "x"]).status.code(), Some(2));
}
