//! A bug report from a failed run: what to file in the project's issue tracker so that a
//! developer can see what broke without opening JEVision.

use std::collections::HashMap;
use std::fmt::Write as _;

use jevision_core::{
    BlockKind, ClipboardAction, Compare, Condition, FileState, Flow, Modifiers, Reference,
    ScriptLanguage, Target, TimeUnit, ValueTest, WaitUntil, format_duration, is_block_path,
};
use sha2::{Digest, Sha256};

use crate::report::{duration, percent, wait_target};
use crate::system::sign;
use crate::{Attempt, Failure, RetryVerdict, RunReport, SECRET_MASK, Step, StepStatus, Verdict};

/// What the report says about the computer and the app, besides what the run saved.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BugContext {
    /// Version of macOS, such as `26.1`.
    pub macos: String,
    /// Version of JEVision.
    pub jevision: String,
    /// Version of the application under test, as installed now.
    pub app_version: Option<String>,
    /// Web page of the test file, such as its page on GitHub.
    pub test_url: Option<String>,
}

/// A bug report ready to be shown, edited and filed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BugDraft {
    pub title: String,
    /// Markdown, without the screenshots: they are added when the report is filed.
    pub body: String,
    /// The same for every run of the test that fails at the same block for the same reason,
    /// so a repeated failure is added to the report already filed.
    pub fingerprint: String,
    pub pictures: Vec<BugPicture>,
}

/// A screenshot of the failed step, relative to the run folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BugPicture {
    /// `Expected`, `Actual` or `Difference`.
    pub label: String,
    pub path: String,
}

/// Most steps a report lists: the last ones, those that led to the failure.
const MAX_STEPS: usize = 30;

impl RunReport {
    /// The attempt that decided the verdict: the last one, or with a data table the last
    /// attempt of the first row that didn't pass.
    pub fn deciding_attempt(&self) -> Option<&Attempt> {
        self.attempts
            .chunk_by(|a, b| a.row == b.row)
            .filter_map(<[Attempt]>::last)
            .find(|a| a.verdict != Verdict::Passed)
            .or(self.attempts.last())
    }
}

/// The bug report of a failed run; `None` for a run that didn't fail. `flows` are the tests the
/// run's steps belong to, by file relative to the project folder, for describing the blocks.
pub fn bug_report(
    report: &RunReport,
    flows: &HashMap<String, Flow>,
    context: &BugContext,
) -> Option<BugDraft> {
    if report.verdict != Verdict::Failed {
        return None;
    }
    let attempt = report.deciding_attempt()?;
    let failed = attempt
        .steps
        .iter()
        .rev()
        .find(|s| s.status == StepStatus::Failed && s.failure.is_some());
    let failure = failed
        .and_then(|s| s.failure.as_ref())
        .or(attempt.failure.as_ref());
    let block = failed.and_then(|s| block_of(s, flows));
    let name = if report.test.is_empty() {
        &report.file
    } else {
        &report.test
    };
    let title = match failed {
        Some(step) => format!(
            "{name} fails at {} “{}”",
            type_title(&step.block_type),
            step.block
        ),
        None => format!("{name} fails"),
    };

    let mut body = String::new();
    if let Some(failure) = failure {
        let _ = writeln!(body, "**Cause:** {}\n", crate::describe(failure));
    }
    if let Some(verdict) = RetryVerdict::of(report.verdict, report.flaky, report.attempts.len()) {
        let _ = writeln!(body, "{}\n", verdict.describe());
    }

    body.push_str("### Steps to reproduce\n\n");
    if let Some(row) = &attempt.row {
        let _ = writeln!(body, "With data {}.\n", row.title());
    }
    let skipped = attempt.steps.len().saturating_sub(MAX_STEPS);
    if skipped > 0 {
        let _ = writeln!(body, "{skipped} earlier steps passed and are left out.\n");
    }
    for (n, step) in attempt.steps.iter().enumerate().skip(skipped) {
        let mut text = step_text(step, flows, &report.app);
        if step.test != report.file {
            let _ = write!(text, " (in `{}`)", step.test);
        }
        if let Some(branch) = step.branch_text() {
            let _ = write!(text, " — {branch}");
        }
        if failed.is_some_and(|f| f.seq == step.seq) {
            let _ = writeln!(body, "{}. **{text} — fails here**", n + 1);
        } else {
            let _ = writeln!(body, "{}. {text}", n + 1);
        }
    }

    if let Some(failure) = failure {
        let (expected, actual) = expected_and_actual(failure, block);
        let _ = write!(
            body,
            "\n### Expected\n\n{expected}\n\n### Actual\n\n{actual}\n"
        );
    }

    body.push_str("\n### Environment\n\n| | |\n|---|---|\n");
    let mut fact = |name: &str, value: &str| {
        let _ = writeln!(body, "| {name} | {} |", value.replace('|', "\\|"));
    };
    if !context.macos.is_empty() {
        fact("macOS", &context.macos);
    }
    if let Some(machine) = report.machine() {
        fact("Computer", &machine);
    }
    if !context.jevision.is_empty() {
        fact("JEVision", &context.jevision);
    }
    let app = match &context.app_version {
        Some(version) => format!("`{}` {version}", report.app),
        None => format!("`{}`", report.app),
    };
    fact("App under test", &app);
    if let Some(environment) = &report.environment {
        fact("Environment", environment);
    }
    let tries = match report.attempts.len() {
        1 => String::new(),
        n => format!(", {n} tries"),
    };
    fact(
        "Run",
        &format!(
            "{} UTC, took {}{tries}",
            report.started_at.replace('T', " ").trim_end_matches('Z'),
            duration(report.duration_ms)
        ),
    );
    let test = match &context.test_url {
        Some(url) => format!("[{}]({url})", report.file),
        None => format!("`{}`", report.file),
    };
    fact("Test", &test);

    let pictures = failed.map_or_else(Vec::new, |step| {
        [
            ("Expected", &step.expected),
            (
                "Actual",
                if step.after.is_some() {
                    &step.after
                } else {
                    &step.before
                },
            ),
            ("Difference", &step.diff),
        ]
        .into_iter()
        .filter_map(|(label, path)| {
            Some(BugPicture {
                label: label.to_owned(),
                path: path.clone()?,
            })
        })
        .collect()
    });

    Some(BugDraft {
        title,
        body,
        fingerprint: fingerprint(report, failed, failure),
        pictures,
    })
}

/// Hex of the first 8 bytes of SHA-256 over the test, the block it failed at and the reason.
fn fingerprint(report: &RunReport, step: Option<&Step>, failure: Option<&Failure>) -> String {
    let mut hash = Sha256::new();
    for part in [
        report.file.as_str(),
        step.map_or("", |s| s.test.as_str()),
        step.map_or("", |s| s.block.as_str()),
        failure.map_or("", |f| f.code.as_str()),
    ] {
        hash.update(part.as_bytes());
        hash.update([0]);
    }
    hash.finalize()[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// `text` with every one of `secrets` replaced by the mask, the longest first so that a secret
/// inside another one doesn't leave part of the longer one.
pub fn mask_secrets(text: &str, secrets: &[String]) -> String {
    let mut values: Vec<&String> = secrets.iter().filter(|v| !v.is_empty()).collect();
    values.sort_by_key(|v| std::cmp::Reverse(v.len()));
    values.into_iter().fold(text.to_owned(), |text, v| {
        text.replace(v.as_str(), SECRET_MASK)
    })
}

fn block_of<'a>(step: &Step, flows: &'a HashMap<String, Flow>) -> Option<&'a BlockKind> {
    flows
        .get(&step.test)?
        .blocks
        .iter()
        .find(|b| b.id == step.block)
        .map(|b| &b.kind)
}

/// The type of a block in words, such as `Wait for`.
fn type_title(block_type: &str) -> String {
    match block_type {
        "openApp" => "Open App".to_owned(),
        "openFile" => "Open File".to_owned(),
        "click" => "Click".to_owned(),
        "hover" => "Hover".to_owned(),
        "drag" => "Drag".to_owned(),
        "scroll" => "Scroll".to_owned(),
        "type" => "Type".to_owned(),
        "pressKeys" => "Press Keys".to_owned(),
        "waitFor" => "Wait For".to_owned(),
        "pause" => "Pause".to_owned(),
        "check" => "Check".to_owned(),
        "if" => "If".to_owned(),
        "repeat" => "Repeat".to_owned(),
        "useData" => "Use Data".to_owned(),
        "runTest" => "Run Test".to_owned(),
        "script" => "Script".to_owned(),
        "command" => "Command".to_owned(),
        "http" => "HTTP Request".to_owned(),
        "checkFile" => "Check File".to_owned(),
        "database" => "Database".to_owned(),
        "clipboard" => "Clipboard".to_owned(),
        "dateTime" => "Date And Time".to_owned(),
        other => other.to_owned(),
    }
}

/// What a step did, in one line.
fn step_text(step: &Step, flows: &HashMap<String, Flow>, app: &str) -> String {
    match block_of(step, flows) {
        Some(kind) => describe_block(kind, app),
        None => format!("{} “{}”", type_title(&step.block_type), step.block),
    }
}

/// What the block `kind` does, in one English line, for a person who hasn't seen the test:
/// `app` is the application under test, which an open-app block without one opens.
pub fn describe_block(kind: &BlockKind, app: &str) -> String {
    let target = |t: &Target| match t {
        Target::Image(image) => format!("“{image}”"),
        Target::Point(p) => format!(
            "the point at {} across, {} down the window",
            percent(p.x),
            percent(p.y)
        ),
        Target::Description(what) => format!("{} (found by description)", what.trim()),
    };
    match kind {
        BlockKind::OpenApp { app: other, .. } => {
            format!("Open `{}`", other.as_deref().unwrap_or(app))
        }
        BlockKind::OpenFile { files, .. } => format!("Open {}", files.join(", ")),
        BlockKind::Click {
            target: t,
            button,
            double,
            modifiers,
            ..
        } => {
            let verb = match (button, double) {
                (jevision_core::MouseButton::Right, _) => "Right-click",
                (_, true) => "Double-click",
                _ => "Click",
            };
            format!("{verb} {}{}", target(t), holding(modifiers))
        }
        BlockKind::Hover { target: t, .. } => format!("Hover over {}", target(t)),
        BlockKind::Drag {
            from,
            to,
            button,
            modifiers,
            ..
        } => {
            let with = if button.is_left() {
                ""
            } else {
                " with the right button"
            };
            format!(
                "Drag from {} to {}{with}{}",
                target(from),
                target(to),
                holding(modifiers)
            )
        }
        BlockKind::Scroll {
            direction,
            amount,
            unit,
            at,
            ..
        } => {
            let direction = match direction {
                jevision_core::ScrollDirection::Up => "up",
                jevision_core::ScrollDirection::Down => "down",
                jevision_core::ScrollDirection::Left => "left",
                jevision_core::ScrollDirection::Right => "right",
            };
            let unit = match (unit, amount) {
                (jevision_core::ScrollUnit::Lines, 1) => "line",
                (jevision_core::ScrollUnit::Lines, _) => "lines",
                (jevision_core::ScrollUnit::Pixels, 1) => "point",
                (jevision_core::ScrollUnit::Pixels, _) => "points",
            };
            let place = at.as_ref().map_or_else(|| "the window".to_owned(), target);
            format!("Scroll {direction} {amount} {unit} over {place}")
        }
        BlockKind::Type { text, into, .. } => match into {
            Some(t) => format!("Type “{text}” into {}", target(t)),
            None => format!("Type “{text}”"),
        },
        BlockKind::PressKeys { keys, .. } => format!("Press {keys}"),
        BlockKind::WaitFor {
            until, timeout_ms, ..
        } => {
            let what = match until {
                WaitUntil::Appears(r) => format!("“{}” appears", r.image),
                WaitUntil::Disappears(r) => format!("“{}” disappears", r.image),
                WaitUntil::ScreenStable => "the screen settles".to_owned(),
            };
            format!("Wait up to {} until {what}", format_duration(*timeout_ms))
        }
        BlockKind::Pause { duration_ms, .. } => {
            format!("Pause for {}", format_duration(*duration_ms))
        }
        BlockKind::Check { reference, .. } => format!("Check that {}", looks_like(reference)),
        BlockKind::If { condition, .. } => match condition {
            Condition::Screen(reference) => format!("If {}", looks_like(reference)),
            Condition::Value(v) => format!("If `{}` {} `{}`", v.of, sign(v.is), v.to),
        },
        BlockKind::Repeat { times, .. } => format!("Repeat {times} times"),
        BlockKind::UseData { file, .. } => format!("For each row of {file}"),
        BlockKind::RunTest { test, .. } if is_block_path(test) => {
            format!("Run the custom block `{test}`")
        }
        BlockKind::RunTest { test, .. } => format!("Run test `{test}`"),
        BlockKind::Script { language, .. } => {
            let language = match language {
                ScriptLanguage::Sh => "sh",
                ScriptLanguage::Zsh => "zsh",
                ScriptLanguage::Python3 => "Python",
                ScriptLanguage::Node => "Node.js",
                ScriptLanguage::AppleScript => "AppleScript",
            };
            format!("Run a {language} script")
        }
        BlockKind::Command { command, .. } => format!("Run `{command}`"),
        BlockKind::Http { method, url, .. } => {
            format!("Send {} {url}", format!("{method:?}").to_uppercase())
        }
        BlockKind::CheckFile { path, until, .. } => {
            let state = match until {
                FileState::Exists => "exists".to_owned(),
                FileState::Missing => "is gone".to_owned(),
                FileState::Changed => "changes".to_owned(),
                FileState::Contains(text) => format!("contains “{text}”"),
            };
            format!("Wait until `{path}` {state}")
        }
        BlockKind::Database { file, query, .. } => format!("Query `{file}`: `{query}`"),
        BlockKind::Clipboard { action, .. } => match action {
            ClipboardAction::Put { text } => format!("Put “{text}” on the clipboard"),
            ClipboardAction::Read { save_as } => {
                format!("Read the clipboard into `{save_as}`")
            }
            ClipboardAction::Check { is, to, .. } => {
                format!("Check that the clipboard {} `{to}`", test_words(*is))
            }
        },
        BlockKind::DateTime {
            save_as,
            format,
            shift,
            ..
        } => {
            let moved = shift.as_ref().map_or_else(String::new, |s| {
                let unit = match s.unit {
                    TimeUnit::Seconds => "second",
                    TimeUnit::Minutes => "minute",
                    TimeUnit::Hours => "hour",
                    TimeUnit::Days => "day",
                    TimeUnit::Weeks => "week",
                    TimeUnit::Months => "month",
                    TimeUnit::Years => "year",
                };
                let plural = if s.amount.abs() == 1 { "" } else { "s" };
                format!(", moved by {} {unit}{plural}", s.amount)
            });
            format!("Save the date and time as `{save_as}` in the form `{format}`{moved}")
        }
    }
}

/// How a clipboard check compares, in words.
fn test_words(test: ValueTest) -> &'static str {
    match test {
        ValueTest::Equals => "is",
        ValueTest::NotEquals => "is not",
        ValueTest::Contains => "contains",
        ValueTest::GreaterThan => "is greater than",
        ValueTest::LessThan => "is less than",
    }
}

/// The keys held down during a click or a drag, as a phrase that ends a sentence, or nothing.
fn holding(modifiers: &Modifiers) -> String {
    if modifiers.is_empty() {
        String::new()
    } else {
        format!(" holding {}", modifiers.names().join("+"))
    }
}

/// What the screen should look like for `reference` to match.
fn looks_like(reference: &Reference) -> String {
    match &reference.compare {
        Compare::Meaning { expect, .. } if !expect.is_empty() => {
            format!("the screen shows {expect}")
        }
        Compare::Meaning { .. } => format!("the screen means the same as “{}”", reference.image),
        Compare::Exact => format!("the screen is exactly “{}”", reference.image),
        Compare::Similar { threshold } => format!(
            "the screen matches “{}” by at least {}",
            reference.image,
            percent(*threshold)
        ),
    }
}

/// What should have happened and what did, for the failure of a block.
fn expected_and_actual(failure: &Failure, block: Option<&BlockKind>) -> (String, String) {
    let p = |name| failure.param(name);
    match failure.code.as_str() {
        "run.check.mismatch" | "run.check.mismatchDensity" => (
            format!(
                "The screen matches “{}” by at least {}.",
                p("image"),
                p("threshold")
            ),
            format!("It matches by {}.", p("similarity")),
        ),
        "run.check.meaningMismatch" => {
            let expected = match block {
                Some(
                    BlockKind::Check { reference, .. }
                    | BlockKind::If {
                        condition: Condition::Screen(reference),
                        ..
                    },
                ) => {
                    format!("{}.", capitalized(&looks_like(reference)))
                }
                _ => "The screen shows what the check expects.".to_owned(),
            };
            (expected, format!("{}: {}", p("provider"), p("reason")))
        }
        "run.waitFor.timeout" => (
            format!(
                "{} within {} s.",
                capitalized(&wait_target(p("target"))),
                p("seconds")
            ),
            "It did not happen in time.".to_owned(),
        ),
        "run.target.notFound" => (
            format!("“{}” is on the screen.", p("image")),
            "It was not found.".to_owned(),
        ),
        "run.click.noChange" => (
            "The screen changes after the click.".to_owned(),
            format!("Nothing changed within {} s.", p("seconds")),
        ),
        _ => (
            format!("Step “{}” passes.", p("block")),
            crate::describe(failure),
        ),
    }
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flow() -> Flow {
        jevision_core::parse(
            r#"{"format": 1, "name": "Sign in", "app": "com.example.Mail", "start": "open",
            "blocks": {
              "open": {"type": "openApp", "next": "type"},
              "type": {"type": "type", "text": "${secret.password}", "into": {"image": "refs/field.png"}, "next": "inbox"},
              "inbox": {"type": "check", "reference": {"image": "refs/inbox.png", "compare": "similar", "threshold": 0.95}}
            }}"#,
        )
        .unwrap()
    }

    fn report() -> RunReport {
        serde_json::from_str(
            r#"{"format":1,"test":"Sign in","file":"mail/sign-in.flow.json","app":"com.example.Mail",
            "startedAt":"2026-10-18T12:00:00.000Z","durationMs":4200,"verdict":"failed","flaky":false,
            "environment":"staging","virtualMachine":false,"displayScale":2.0,
            "attempts":[
              {"number":1,"verdict":"failed","durationMs":2000,"steps":[]},
              {"number":2,"verdict":"failed","durationMs":2200,"steps":[
                {"seq":1,"test":"mail/sign-in.flow.json","block":"open","type":"openApp","status":"passed","durationMs":900},
                {"seq":2,"test":"mail/sign-in.flow.json","block":"type","type":"type","status":"passed","durationMs":300},
                {"seq":3,"test":"mail/sign-in.flow.json","block":"inbox","type":"check","status":"failed","durationMs":200,
                 "after":"shots/2-003-inbox-after.png","expected":"shots/2-003-inbox-expected.png",
                 "diff":"shots/2-003-inbox-diff.png","similarity":0.81,
                 "failure":{"code":"run.check.mismatch","params":{"block":"inbox","image":"refs/inbox.png",
                   "similarity":"81.0%","threshold":"95.0%"}}}]}]}"#,
        )
        .unwrap()
    }

    fn context() -> BugContext {
        BugContext {
            macos: "26.1".into(),
            jevision: "0.9.0".into(),
            app_version: Some("16.0".into()),
            test_url: Some("https://github.com/acme/qa/blob/main/mail/sign-in.flow.json".into()),
        }
    }

    #[test]
    fn a_bug_report_says_what_was_done_expected_and_seen() {
        let flows = HashMap::from([("mail/sign-in.flow.json".to_owned(), flow())]);
        let draft = bug_report(&report(), &flows, &context()).unwrap();
        assert_eq!(draft.title, "Sign in fails at Check “inbox”");
        let body = &draft.body;
        for line in [
            "**Cause:** Block inbox: the screen matches refs/inbox.png by 81.0%, at least 95.0% is required.",
            "Failed 2 times in a row — a real failure, not flakiness.",
            "1. Open `com.example.Mail`\n",
            "2. Type “${secret.password}” into “refs/field.png”\n",
            "3. **Check that the screen matches “refs/inbox.png” by at least 95.0% — fails here**",
            "### Expected\n\nThe screen matches “refs/inbox.png” by at least 95.0%.",
            "### Actual\n\nIt matches by 81.0%.",
            "| macOS | 26.1 |",
            "| Computer | Physical Mac, Retina display (2×) |",
            "| JEVision | 0.9.0 |",
            "| App under test | `com.example.Mail` 16.0 |",
            "| Environment | staging |",
            "| Run | 2026-10-18 12:00:00.000 UTC, took 4.2 s, 2 tries |",
            "| Test | [mail/sign-in.flow.json](https://github.com/acme/qa/blob/main/mail/sign-in.flow.json) |",
        ] {
            assert!(body.contains(line), "{line}\n---\n{body}");
        }
        let pictures: Vec<(&str, &str)> = draft
            .pictures
            .iter()
            .map(|p| (p.label.as_str(), p.path.as_str()))
            .collect();
        assert_eq!(
            pictures,
            [
                ("Expected", "shots/2-003-inbox-expected.png"),
                ("Actual", "shots/2-003-inbox-after.png"),
                ("Difference", "shots/2-003-inbox-diff.png"),
            ]
        );
    }

    #[test]
    fn keys_the_clipboard_and_dates_are_told_in_a_step() {
        let flow = jevision_core::parse(
            r#"{"format": 1, "name": "T", "app": "com.example.Mail", "start": "a",
            "blocks": {
              "a": {"type": "click", "target": {"description": "the file"}, "modifiers": ["cmd", "shift"], "next": "b"},
              "b": {"type": "drag", "from": {"point": {"x": 0.1, "y": 0.1}}, "to": {"point": {"x": 0.5, "y": 0.5}}, "button": "right", "modifiers": ["option"], "next": "c"},
              "c": {"type": "clipboard", "action": "put", "text": "hello", "next": "d"},
              "d": {"type": "clipboard", "action": "check", "is": "contains", "to": "hel", "next": "e"},
              "e": {"type": "dateTime", "saveAs": "day", "format": "%F", "shift": {"amount": -1, "unit": "weeks"}}
            }}"#,
        )
        .unwrap();
        let flows = HashMap::from([("t.flow.json".to_owned(), flow)]);
        let told = |block: &str| {
            let step: Step = serde_json::from_str(&format!(
                r#"{{"seq":1,"test":"t.flow.json","block":"{block}","type":"x","status":"passed","durationMs":1}}"#
            ))
            .unwrap();
            step_text(&step, &flows, "app")
        };
        assert_eq!(
            told("a"),
            "Click the file (found by description) holding cmd+shift"
        );
        assert_eq!(
            told("b"),
            "Drag from the point at 10.0% across, 10.0% down the window to the point at 50.0% across, 50.0% down the window with the right button holding option"
        );
        assert_eq!(told("c"), "Put “hello” on the clipboard");
        assert_eq!(told("d"), "Check that the clipboard contains `hel`");
        assert_eq!(
            told("e"),
            "Save the date and time as `day` in the form `%F`, moved by -1 week"
        );
    }

    #[test]
    fn the_same_failure_has_the_same_fingerprint() {
        let flows = HashMap::new();
        let draft =
            |report: &RunReport| bug_report(report, &flows, &BugContext::default()).unwrap();
        let first = draft(&report());
        assert_eq!(first.fingerprint.len(), 16);
        // Another run, another day, another similarity: the same bug.
        let mut again = report();
        again.started_at = "2026-10-19T08:00:00.000Z".into();
        again.attempts[1].steps[2].similarity = Some(0.5);
        assert_eq!(draft(&again).fingerprint, first.fingerprint);
        // Failing for another reason is another bug.
        let mut other = report();
        other.attempts[1].steps[2].failure.as_mut().unwrap().code = "run.target.notFound".into();
        assert_ne!(draft(&other).fingerprint, first.fingerprint);
        // Without its test file the steps are still named.
        assert!(first.body.contains("3. **Check “inbox” — fails here**"));
        assert!(!first.body.contains("| macOS"));

        let mut passed = report();
        passed.verdict = Verdict::Passed;
        assert_eq!(bug_report(&passed, &flows, &BugContext::default()), None);
    }

    #[test]
    fn long_runs_list_the_steps_that_led_to_the_failure() {
        let mut long = report();
        let steps = &mut long.attempts[1].steps;
        let first = steps[0].clone();
        steps.splice(0..0, (0..40).map(|_| first.clone()));
        let draft = bug_report(&long, &HashMap::new(), &BugContext::default()).unwrap();
        assert!(
            draft
                .body
                .contains("13 earlier steps passed and are left out.")
        );
        assert!(draft.body.contains("43. **Check “inbox” — fails here**"));
        assert!(!draft.body.contains("\n13. "));
    }

    #[test]
    fn secrets_are_masked_longest_first() {
        let secrets = ["hunter2".to_owned(), "hunter".to_owned(), String::new()];
        assert_eq!(
            mask_secrets("typed hunter2, then hunter", &secrets),
            format!("typed {SECRET_MASK}, then {SECRET_MASK}")
        );
    }
}
