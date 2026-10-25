//! A saved report must remain useful to history, full reports and offline exports.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use jevision_runner::{Run, RunReport, StepStatus, Verdict, html_in, junit_xml_in};

struct Project(PathBuf);

impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn format_1_report_opens_in_history_and_exports_without_rewriting_its_file() {
    let project = Project(std::env::temp_dir().join(format!(
        "jevision-report-fixture-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
    )));
    let dir = project.0.join(".jevision/runs/20260102-093000-sign-in");
    fs::create_dir_all(&dir).unwrap();
    let text = include_str!("fixtures/run-report/format-1.json");
    let file = dir.join("report.json");
    fs::write(&file, text).unwrap();
    let run = Run::load(&dir).unwrap();
    assert_eq!(run.report.format, 1);
    assert_eq!(run.report.test, "Sign in");
    assert_eq!(run.report.file, "login/sign-in.flow.json");
    assert_eq!(run.report.verdict, Verdict::Failed);
    assert_eq!(run.report.duration_ms, 1250);
    assert_eq!(run.report.attempts.len(), 1);
    let attempt = &run.report.attempts[0];
    assert_eq!(attempt.number, 1);
    assert_eq!(attempt.steps.len(), 2);
    assert_eq!(attempt.steps[0].status, StepStatus::Passed);
    assert_eq!(attempt.steps[1].status, StepStatus::Failed);
    let failure = attempt.failure.as_ref().unwrap();
    assert_eq!(failure.code, "run.waitFor.timeout");
    assert_eq!(failure.params["target"], "Inbox");
    assert_eq!(
        failure.message, "",
        "older reports have no stored failure words"
    );
    assert_eq!(attempt.steps[1].failure.as_ref(), Some(failure));
    assert_eq!(
        attempt.steps[1].before.as_deref(),
        Some("shots/2-before.png")
    );
    assert!(run.report.video.is_none());
    assert!(!run.report.nothing_checked);
    assert!(attempt.steps[0].outputs.is_empty());
    assert!(attempt.steps[0].video_at_ms.is_none());

    let summaries = Run::summaries(&project.0).unwrap();
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].report, run.report);
    let encoded = serde_json::to_string(&run.report).unwrap();
    assert_eq!(
        serde_json::from_str::<RunReport>(&encoded).unwrap(),
        run.report
    );

    let html = html_in("en", &run.report, &dir, None);
    assert!(html.contains("Sign in"));
    assert!(html.contains("Inbox"));
    let xml = junit_xml_in("en", std::slice::from_ref(&run.report));
    assert!(xml.contains("failures=\"1\""));
    assert!(xml.contains("<failure"));
    assert_eq!(fs::read_to_string(&file).unwrap(), text);
}
