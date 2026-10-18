//! What a run leaves behind: the report model and its HTML and JUnit XML exports.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Version of the `report.json` layout.
pub const REPORT_FORMAT: u32 = 1;

/// Outcome of a whole run, saved as `report.json` in the run folder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunReport {
    pub format: u32,
    /// Name of the test.
    pub test: String,
    /// Test file, relative to the project folder.
    pub file: String,
    /// Bundle id of the application under test.
    pub app: String,
    /// UTC time, RFC 3339.
    pub started_at: String,
    pub duration_ms: u64,
    /// Result of the last attempt.
    pub verdict: Verdict,
    /// Attempts disagreed: a failure was followed by a pass, or attempts failed differently.
    /// A failed run that is not flaky failed the same way every time.
    pub flaky: bool,
    /// References saved by this run because they did not exist, relative to the project folder.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub baselines_created: Vec<String>,
    pub attempts: Vec<Attempt>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Verdict {
    Passed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attempt {
    /// 1 for the first run, 2 for the first retry and so on.
    pub number: u32,
    pub verdict: Verdict,
    pub duration_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<Failure>,
    /// In the order the blocks started.
    pub steps: Vec<Step>,
}

/// One block as it ran. Image paths are relative to the run folder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    /// Order of the step within its attempt, from 1.
    pub seq: u32,
    /// Test file the block belongs to, relative to the project folder.
    pub test: String,
    pub block: String,
    #[serde(rename = "type")]
    pub block_type: String,
    pub status: StepStatus,
    pub duration_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
    /// Copy of the reference the screen was compared with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff: Option<String>,
    /// Reference file of a `check`, relative to the project folder: the one compared with, or
    /// the one this step saved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    /// Share of compared pixels that matched the reference, 0–1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub similarity: Option<f64>,
    /// Edge an `if` block followed: `then` or `else`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// Why the step failed. Absent on a loop or `runTest` step whose inner step failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<Failure>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StepStatus {
    Passed,
    Failed,
    /// There was no reference yet; the screenshot became the reference.
    BaselineCreated,
    Cancelled,
}

/// Why a run failed: a localization key and the values its text refers to, e.g.
/// `run.waitFor.timeout` with `block`, `target` and `seconds`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Failure {
    pub code: String,
    pub params: BTreeMap<String, String>,
}

impl Failure {
    pub fn new(code: &str) -> Self {
        Self {
            code: code.to_owned(),
            params: BTreeMap::new(),
        }
    }

    pub fn with(mut self, name: &str, value: impl ToString) -> Self {
        self.params.insert(name.to_owned(), value.to_string());
        self
    }

    pub fn param(&self, name: &str) -> &str {
        self.params.get(name).map_or("", String::as_str)
    }
}

/// English sentence for a failure, for exports that have no localization at hand.
pub fn describe(f: &Failure) -> String {
    let p = |name| f.param(name);
    match f.code.as_str() {
        "run.test.unreadable" => {
            format!("Test file {} cannot be read: {}.", p("test"), p("reason"))
        }
        "run.test.invalid" => format!(
            "Test file {} has {} errors; fix them in the editor first.",
            p("test"),
            p("errors")
        ),
        "run.start.unknownBlock" => format!("There is no block {} to start from.", p("block")),
        "run.secret.missing" => format!("Secret {} is not set.", p("name")),
        "run.path.outsideProject" => format!(
            "Block {}: {} is outside the project folder.",
            p("block"),
            p("path")
        ),
        "run.file.missing" => format!("Block {}: file {} does not exist.", p("block"), p("file")),
        "run.file.copyFailed" => format!(
            "Block {}: file {} cannot be copied: {}.",
            p("block"),
            p("file"),
            p("reason")
        ),
        "run.target.notFound" => format!(
            "Block {}: {} was not found on screen.",
            p("block"),
            p("image")
        ),
        "run.reference.missing" => format!(
            "Block {}: reference image {} does not exist.",
            p("block"),
            p("image")
        ),
        "run.reference.unreadable" => format!(
            "Block {}: image {} cannot be read: {}.",
            p("block"),
            p("image"),
            p("reason")
        ),
        "run.screenshot.unreadable" => format!(
            "Block {}: the screenshot cannot be read: {}.",
            p("block"),
            p("reason")
        ),
        "run.compare.shapeMismatch" => format!(
            "Block {}: the window is {} but reference {} is {}.",
            p("block"),
            p("actual"),
            p("image"),
            p("expected")
        ),
        "run.compare.meaningUnavailable" => format!(
            "Block {}: comparing by meaning needs a vision model, and none is set up.",
            p("block")
        ),
        "run.click.noChange" => format!(
            "Block {}: the screen did not change within {} s after the click.",
            p("block"),
            p("seconds")
        ),
        "run.waitFor.timeout" => format!(
            "Block {}: {} did not happen within {} s.",
            p("block"),
            wait_target(p("target")),
            p("seconds")
        ),
        "run.check.mismatch" => format!(
            "Block {}: the screen matches {} by {}, at least {} is required.",
            p("block"),
            p("image"),
            p("similarity"),
            p("threshold")
        ),
        "run.data.unreadable" => format!(
            "Block {}: table {} cannot be read: {}.",
            p("block"),
            p("file"),
            p("reason")
        ),
        "run.data.unknownColumn" => format!(
            "Block {}: no table column {} is available here.",
            p("block"),
            p("column")
        ),
        "run.runTest.recursion" => format!(
            "Block {}: test {} is already running; a test cannot run itself.",
            p("block"),
            p("test")
        ),
        "run.io.writeFailed" => format!("{} cannot be saved: {}.", p("path"), p("reason")),
        "run.hands.permissionDenied" => format!(
            "Block {}: permission {} is not granted.",
            p("block"),
            p("permission")
        ),
        "run.hands.appNotFound" => format!(
            "Block {}: application {} is not installed.",
            p("block"),
            p("app")
        ),
        "run.hands.noWindow" => format!(
            "Block {}: application {} has no open window.",
            p("block"),
            p("app")
        ),
        "run.hands.cannotOpen" => format!("Block {}: {} cannot be opened.", p("block"), p("file")),
        "run.hands.screenLocked" => format!(
            "Block {}: the screen is locked. Unlock it and run the test again.",
            p("block")
        ),
        "run.hands.failed" => format!("Block {}: {}.", p("block"), p("reason")),
        _ => {
            let params: Vec<String> = f.params.iter().map(|(k, v)| format!("{k}={v}")).collect();
            format!("{} ({})", f.code, params.join(", "))
        }
    }
}

fn wait_target(target: &str) -> String {
    match target {
        "screenStable" => "the screen settling".to_owned(),
        image => format!("the wait for {image}"),
    }
}

/// JUnit XML for one or more runs, one test case each. Retries use the Surefire elements
/// `flakyFailure` (failed, then passed) and `rerunFailure` (failed again).
pub fn junit_xml(reports: &[RunReport]) -> String {
    let count = |v: Verdict| reports.iter().filter(|r| r.verdict == v).count();
    let seconds = |ms: u64| format!("{:.3}", ms as f64 / 1000.0);
    let total: u64 = reports.iter().map(|r| r.duration_ms).sum();
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let head = format!(
        "name=\"JEVision\" tests=\"{}\" failures=\"{}\" errors=\"0\" skipped=\"{}\" time=\"{}\"",
        reports.len(),
        count(Verdict::Failed),
        count(Verdict::Cancelled),
        seconds(total)
    );
    let _ = writeln!(out, "<testsuites {head}>");
    let timestamp = reports
        .first()
        .map_or("", |r| r.started_at.trim_end_matches('Z'));
    let _ = writeln!(out, "  <testsuite {head} timestamp=\"{}\">", xml(timestamp));
    for r in reports {
        let _ = write!(
            out,
            "    <testcase name=\"{}\" classname=\"{}\" time=\"{}\"",
            xml(&r.test),
            xml(&r.file),
            seconds(r.duration_ms)
        );
        let failures: Vec<&Failure> = r
            .attempts
            .iter()
            .filter_map(|a| a.failure.as_ref())
            .collect();
        if r.verdict == Verdict::Passed && failures.is_empty() {
            out.push_str("/>\n");
            continue;
        }
        out.push_str(">\n");
        let element = |out: &mut String, tag: &str, f: &Failure| {
            let _ = writeln!(
                out,
                "      <{tag} message=\"{}\" type=\"{}\"/>",
                xml(&describe(f)),
                xml(&f.code)
            );
        };
        match r.verdict {
            Verdict::Passed => failures
                .iter()
                .for_each(|f| element(&mut out, "flakyFailure", f)),
            Verdict::Failed => match failures.split_last() {
                Some((last, earlier)) => {
                    element(&mut out, "failure", last);
                    earlier
                        .iter()
                        .for_each(|f| element(&mut out, "rerunFailure", f));
                }
                None => out.push_str("      <failure message=\"The test failed.\"/>\n"),
            },
            Verdict::Cancelled => {
                out.push_str("      <skipped message=\"The run was cancelled.\"/>\n")
            }
        }
        out.push_str("    </testcase>\n");
    }
    out.push_str("  </testsuite>\n</testsuites>\n");
    out
}

/// A self-contained HTML page for the report, with every image of `run_dir` it refers to
/// embedded. Images that are gone are left out.
pub fn html(report: &RunReport, run_dir: &Path) -> String {
    let mut o = String::new();
    let (badge, label) = match (report.verdict, report.flaky) {
        (Verdict::Passed, false) => ("pass", "Passed"),
        (Verdict::Passed, true) => ("flaky", "Passed, flaky"),
        (Verdict::Failed, false) => ("fail", "Failed"),
        (Verdict::Failed, true) => ("fail", "Failed, flaky"),
        (Verdict::Cancelled, _) => ("skip", "Cancelled"),
    };
    let _ = write!(
        o,
        "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>{title} — {label}</title>\n<style>{CSS}</style>\n</head>\n<body>\n<main>\n\
         <header><h1>{title}</h1><span class=\"badge {badge}\">{label}</span></header>\n\
         <dl class=\"facts\"><dt>Test file</dt><dd>{file}</dd><dt>Application</dt><dd>{app}</dd>\
         <dt>Started</dt><dd>{started}</dd><dt>Duration</dt><dd>{duration}</dd>\
         <dt>Attempts</dt><dd>{attempts}</dd></dl>\n",
        title = xml(&report.test),
        file = xml(&report.file),
        app = xml(&report.app),
        started = xml(&report.started_at.replace('T', " ")),
        duration = duration(report.duration_ms),
        attempts = report.attempts.len(),
    );
    if !report.baselines_created.is_empty() {
        let _ = writeln!(
            o,
            "<p class=\"note\">New reference images were saved: {}. Review them before relying on this test.</p>",
            xml(&report.baselines_created.join(", "))
        );
    }
    for a in &report.attempts {
        let _ = writeln!(
            o,
            "<section>\n<h2>Attempt {} <small>{}</small></h2>",
            a.number,
            duration(a.duration_ms)
        );
        if let Some(f) = &a.failure {
            let _ = writeln!(o, "<p class=\"reason\">{}</p>", xml(&describe(f)));
        }
        o.push_str("<ol class=\"steps\">\n");
        for s in &a.steps {
            let (class, status) = match s.status {
                StepStatus::Passed => ("pass", "Passed"),
                StepStatus::Failed => ("fail", "Failed"),
                StepStatus::BaselineCreated => ("new", "Reference saved"),
                StepStatus::Cancelled => ("skip", "Cancelled"),
            };
            let _ = write!(
                o,
                "<li class=\"{class}\"><div class=\"row\"><span class=\"seq\">{}</span>\
                 <code>{}</code><span class=\"type\">{}</span>",
                s.seq,
                xml(&s.block),
                xml(&s.block_type)
            );
            if s.test != report.file {
                let _ = write!(o, "<span class=\"type\">in {}</span>", xml(&s.test));
            }
            if let Some(b) = &s.branch {
                let _ = write!(o, "<span class=\"type\">→ {}</span>", xml(b));
            }
            if let Some(sim) = s.similarity {
                let _ = write!(o, "<span class=\"type\">{} alike</span>", percent(sim));
            }
            let _ = write!(
                o,
                "<span class=\"status\">{status}</span><span class=\"time\">{}</span></div>",
                duration(s.duration_ms)
            );
            if let Some(f) = &s.failure {
                let _ = write!(o, "<p class=\"reason\">{}</p>", xml(&describe(f)));
            }
            let images: Vec<(&str, &String)> = [
                ("Before", &s.before),
                ("After", &s.after),
                ("Expected", &s.expected),
                ("Differences", &s.diff),
            ]
            .into_iter()
            .filter_map(|(name, path)| Some((name, path.as_ref()?)))
            .collect();
            let figures: String = images
                .iter()
                .filter_map(|(name, path)| {
                    let bytes = std::fs::read(run_dir.join(path)).ok()?;
                    Some(format!(
                        "<figure><img tabindex=\"0\" alt=\"{name}\" \
                         src=\"data:image/png;base64,{}\"><figcaption>{name}</figcaption></figure>",
                        base64(&bytes)
                    ))
                })
                .collect();
            if !figures.is_empty() {
                let _ = write!(o, "<div class=\"shots\">{figures}</div>");
            }
            o.push_str("</li>\n");
        }
        o.push_str("</ol>\n</section>\n");
    }
    o.push_str("</main>\n</body>\n</html>\n");
    o
}

const CSS: &str = "\
:root{color-scheme:light dark;--bg:#f6f7f9;--card:#fff;--text:#1d1f23;--muted:#6b7079;--line:#e3e5e9;\
--pass:#1f8a4c;--fail:#c62f3d;--flaky:#b26a00;--new:#2563c9;--skip:#6b7079}\
@media(prefers-color-scheme:dark){:root{--bg:#141518;--card:#1e2024;--text:#eceef1;--muted:#9aa0a9;\
--line:#2d3036;--pass:#4cc27e;--fail:#ff6b78;--flaky:#f0a43a;--new:#6ea2ff;--skip:#9aa0a9}}\
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--text);\
font:15px/1.5 -apple-system,BlinkMacSystemFont,\"Segoe UI\",Roboto,sans-serif}\
main{max-width:1100px;margin:0 auto;padding:32px 24px 64px}\
header{display:flex;align-items:center;gap:14px;flex-wrap:wrap}h1{font-size:26px;margin:0}\
h2{font-size:18px;margin:32px 0 12px}h2 small{color:var(--muted);font-weight:400;margin-left:8px}\
.badge{padding:3px 12px;border-radius:999px;color:#fff;font-weight:600;font-size:13px}\
.badge.pass{background:var(--pass)}.badge.fail{background:var(--fail)}.badge.flaky{background:var(--flaky)}\
.badge.skip{background:var(--skip)}\
.facts{display:grid;grid-template-columns:max-content 1fr;gap:4px 18px;margin:18px 0 0}\
.facts dt{color:var(--muted)}.facts dd{margin:0;overflow-wrap:anywhere}\
.note{background:var(--card);border-left:4px solid var(--new);padding:10px 14px;border-radius:6px}\
.reason{color:var(--fail);font-weight:600;margin:8px 0 0}\
.steps{list-style:none;margin:0;padding:0;display:grid;gap:10px}\
.steps li{background:var(--card);border:1px solid var(--line);border-left:4px solid var(--skip);\
border-radius:8px;padding:10px 14px}\
.steps li.pass{border-left-color:var(--pass)}.steps li.fail{border-left-color:var(--fail)}\
.steps li.new{border-left-color:var(--new)}\
.row{display:flex;align-items:baseline;gap:10px;flex-wrap:wrap}.seq{color:var(--muted);min-width:2ch}\
.type,.time{color:var(--muted);font-size:13px}.status{margin-left:auto;font-weight:600}\
.shots{display:flex;gap:12px;flex-wrap:wrap;margin-top:10px}\
figure{margin:0;width:240px}figure img{width:100%;border:1px solid var(--line);border-radius:6px;\
display:block;background:repeating-conic-gradient(#ccc 0 25%,#fff 0 50%) 0 0/16px 16px}\
figure img:focus{position:fixed;inset:0;margin:auto;width:auto;max-width:96vw;max-height:96vh;\
z-index:1;box-shadow:0 0 0 100vmax rgba(0,0,0,.6);cursor:zoom-out;outline:none}figure img{cursor:zoom-in}\
figcaption{color:var(--muted);font-size:12px;margin-top:4px}";

fn duration(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms} ms")
    } else {
        format!("{:.1} s", ms as f64 / 1000.0)
    }
}

pub(crate) fn percent(share: f64) -> String {
    format!("{:.1}%", share * 100.0)
}

/// Escapes text for XML and HTML content and attribute values.
fn xml(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // Characters XML 1.0 does not allow at all.
            '\u{0}'..='\u{8}' | '\u{b}' | '\u{c}' | '\u{e}'..='\u{1f}' => {}
            c => out.push(c),
        }
    }
    out
}

fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, &b)| n | u32::from(b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ABC[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc_4648() {
        for (input, encoded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(input.as_bytes()), encoded);
        }
    }

    #[test]
    fn xml_escapes_markup_and_drops_control_characters() {
        assert_eq!(
            xml("<a href=\"x\">&'\u{1}"),
            "&lt;a href=&quot;x&quot;&gt;&amp;&apos;"
        );
    }
}
