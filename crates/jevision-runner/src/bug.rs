//! A bug report from a failed run: what to file in the project's issue tracker so that a
//! developer can see what broke without opening JEVision.

use std::collections::HashMap;
use std::fmt::Write as _;

use jevision_core::{
    BlockKind, ClipboardAction, ClipboardContent, Compare, Condition, FileState, Flow, Modifiers,
    Reference, ScriptLanguage, Target, TimeUnit, ValueTest, WaitUntil, format_duration,
    is_block_path,
};
use jevision_l10n::{text_in, tr_in};
use sha2::{Digest, Sha256};

use crate::report::{duration, percent};
use crate::system::sign;
use crate::words::{share, wait_target};
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
    /// `Expected`, `Actual` or `Difference`: names of files and of the pictures a client asks
    /// for, so they are the same in every language. Never shown as it is: people read
    /// [`picture_caption_in`].
    pub label: String,
    pub path: String,
}

/// The caption of the picture with `label` (see [`BugPicture::label`]) in `lang`.
pub fn picture_caption_in(lang: &str, label: &str) -> String {
    let key = match label {
        "Expected" => "bug.picture.expected",
        "Actual" => "bug.picture.actual",
        "Difference" => "bug.picture.difference",
        other => return other.to_owned(),
    };
    text_in(lang, key, &[])
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

/// [`bug_report_in`] in the language set with `jevision_l10n::set_language`.
pub fn bug_report(
    report: &RunReport,
    flows: &HashMap<String, Flow>,
    context: &BugContext,
) -> Option<BugDraft> {
    bug_report_in(&jevision_l10n::language(), report, flows, context)
}

/// The bug report of a failed run in `lang`; `None` for a run that didn't fail. `flows` are the
/// tests the run's steps belong to, by file relative to the project folder, for describing the
/// blocks.
pub fn bug_report_in(
    lang: &str,
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
        Some(step) => tr_in!(
            lang,
            "bug.title.at",
            name,
            type_title(lang, &step.block_type),
            &step.block
        ),
        None => tr_in!(lang, "bug.title.fails", name),
    };

    let mut body = String::new();
    if let Some(failure) = failure {
        let cause = tr_in!(lang, "bug.cause", crate::describe_in(lang, failure));
        let _ = writeln!(body, "{cause}\n");
    }
    if let Some(verdict) = RetryVerdict::of(report.verdict, report.flaky, report.attempts.len()) {
        let _ = writeln!(body, "{}\n", verdict.describe_in(lang));
    }

    let _ = write!(body, "{}\n\n", tr_in!(lang, "bug.heading.steps"));
    if let Some(row) = &attempt.row {
        let _ = writeln!(
            body,
            "{}\n",
            tr_in!(lang, "bug.steps.withData", row.title_in(lang))
        );
    }
    let skipped = attempt.steps.len().saturating_sub(MAX_STEPS);
    if skipped > 0 {
        let _ = writeln!(body, "{}\n", tr_in!(lang, "bug.steps.skipped", skipped));
    }
    for (n, step) in attempt.steps.iter().enumerate().skip(skipped) {
        let mut text = step_text(lang, step, flows, &report.app);
        if step.test != report.file {
            let _ = write!(text, " {}", tr_in!(lang, "bug.steps.inTest", &step.test));
        }
        if let Some(branch) = step.branch_text_in(lang) {
            let _ = write!(text, " — {branch}");
        }
        if failed.is_some_and(|f| f.seq == step.seq) {
            let _ = writeln!(
                body,
                "{}. **{}**",
                n + 1,
                tr_in!(lang, "bug.steps.failsHere", text)
            );
        } else {
            let _ = writeln!(body, "{}. {text}", n + 1);
        }
    }

    if let Some(failure) = failure {
        let (expected, actual) = expected_and_actual(lang, failure, block);
        let _ = write!(
            body,
            "\n{}\n\n{expected}\n\n{}\n\n{actual}\n",
            tr_in!(lang, "bug.heading.expected"),
            tr_in!(lang, "bug.heading.actual"),
        );
    }

    let _ = write!(
        body,
        "\n{}\n\n| | |\n|---|---|\n",
        tr_in!(lang, "bug.heading.environment")
    );
    let mut fact = |name: &str, value: &str| {
        let _ = writeln!(body, "| {name} | {} |", value.replace('|', "\\|"));
    };
    if !context.macos.is_empty() {
        fact("macOS", &context.macos);
    }
    if let Some(machine) = report.machine_in(lang) {
        fact(&tr_in!(lang, "report.fact.computer"), &machine);
    }
    if !context.jevision.is_empty() {
        fact("JEVision", &context.jevision);
    }
    let app = match &context.app_version {
        Some(version) => format!("`{}` {version}", report.app),
        None => format!("`{}`", report.app),
    };
    fact(&tr_in!(lang, "bug.fact.app"), &app);
    if let Some(environment) = &report.environment {
        fact(&tr_in!(lang, "report.fact.environment"), environment);
    }
    let started = report.started_at.replace('T', " ");
    let started = started.trim_end_matches('Z');
    let took = duration(lang, report.duration_ms);
    let run = match report.attempts.len() {
        1 => tr_in!(lang, "bug.run.took", started, took),
        n => tr_in!(lang, "bug.run.tookTries", started, took, n),
    };
    fact(&tr_in!(lang, "bug.fact.run"), &run);
    let test = match &context.test_url {
        Some(url) => format!("[{}]({url})", report.file),
        None => format!("`{}`", report.file),
    };
    fact(&tr_in!(lang, "bug.fact.test"), &test);

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
/// Made of identifiers only, so it is the same in every language.
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

/// The type of a block in words, such as `Wait For`.
fn type_title(lang: &str, block_type: &str) -> String {
    let key = match block_type {
        "openApp" => "bug.type.openApp",
        "openFile" => "bug.type.openFile",
        "click" => "bug.type.click",
        "hover" => "bug.type.hover",
        "drag" => "bug.type.drag",
        "scroll" => "bug.type.scroll",
        "type" => "bug.type.type",
        "pressKeys" => "bug.type.pressKeys",
        "waitFor" => "bug.type.waitFor",
        "pause" => "bug.type.pause",
        "check" => "bug.type.check",
        "if" => "bug.type.if",
        "repeat" => "bug.type.repeat",
        "useData" => "bug.type.useData",
        "runTest" => "bug.type.runTest",
        "script" => "bug.type.script",
        "command" => "bug.type.command",
        "http" => "bug.type.http",
        "checkFile" => "bug.type.checkFile",
        "database" => "bug.type.database",
        "clipboard" => "bug.type.clipboard",
        "dateTime" => "bug.type.dateTime",
        other => return other.to_owned(),
    };
    text_in(lang, key, &[])
}

/// What a step did, in one line.
fn step_text(lang: &str, step: &Step, flows: &HashMap<String, Flow>, app: &str) -> String {
    match block_of(step, flows) {
        Some(kind) => describe_block_in(lang, kind, app),
        None => tr_in!(
            lang,
            "bug.step.unknown",
            type_title(lang, &step.block_type),
            &step.block
        ),
    }
}

/// [`describe_block_in`] in the language set with `jevision_l10n::set_language`.
pub fn describe_block(kind: &BlockKind, app: &str) -> String {
    describe_block_in(&jevision_l10n::language(), kind, app)
}

/// What the block `kind` does, in one line in `lang`, for a person who hasn't seen the test:
/// `app` is the application under test, which an open-app block without one opens.
pub fn describe_block_in(lang: &str, kind: &BlockKind, app: &str) -> String {
    let target = |t: &Target| match t {
        Target::Image(image) => tr_in!(lang, "bug.target.image", image),
        Target::Point(p) => tr_in!(
            lang,
            "bug.target.point",
            percent(lang, p.x),
            percent(lang, p.y)
        ),
        Target::Description(what) => tr_in!(lang, "bug.target.description", what.trim()),
        Target::Area(area) => tr_in!(lang, "bug.target.area", area.to_string()),
    };
    match kind {
        BlockKind::OpenApp { app: other, .. } => {
            tr_in!(lang, "bug.block.openApp", other.as_deref().unwrap_or(app))
        }
        BlockKind::OpenFile { files, .. } => tr_in!(lang, "bug.block.openFile", files.join(", ")),
        BlockKind::Click {
            target: t,
            button,
            double,
            modifiers,
            ..
        } => {
            let at = target(t);
            let text = match (button, double) {
                (jevision_core::MouseButton::Right, _) => tr_in!(lang, "bug.block.rightClick", at),
                (jevision_core::MouseButton::Middle, _) => {
                    tr_in!(lang, "bug.block.middleClick", at)
                }
                (_, true) => tr_in!(lang, "bug.block.doubleClick", at),
                _ => tr_in!(lang, "bug.block.click", at),
            };
            text + &holding(lang, modifiers)
        }
        BlockKind::Hover {
            target: t,
            modifiers,
            ..
        } => tr_in!(lang, "bug.block.hover", target(t)) + &holding(lang, modifiers),
        BlockKind::Drag {
            from,
            to,
            button,
            modifiers,
            ..
        } => {
            let (from, to) = (target(from), target(to));
            let text = match button {
                jevision_core::MouseButton::Left => tr_in!(lang, "bug.block.drag", from, to),
                jevision_core::MouseButton::Right => tr_in!(lang, "bug.block.dragRight", from, to),
                jevision_core::MouseButton::Middle => {
                    tr_in!(lang, "bug.block.dragMiddle", from, to)
                }
            };
            text + &holding(lang, modifiers)
        }
        BlockKind::Scroll {
            direction,
            amount,
            unit,
            at,
            modifiers,
            ..
        } => {
            let direction = match direction {
                jevision_core::ScrollDirection::Up => text_in(lang, "bug.direction.up", &[]),
                jevision_core::ScrollDirection::Down => text_in(lang, "bug.direction.down", &[]),
                jevision_core::ScrollDirection::Left => text_in(lang, "bug.direction.left", &[]),
                jevision_core::ScrollDirection::Right => text_in(lang, "bug.direction.right", &[]),
            };
            let place = at
                .as_ref()
                .map_or_else(|| text_in(lang, "bug.place.window", &[]), target);
            let text = match unit {
                jevision_core::ScrollUnit::Lines => {
                    tr_in!(lang, "bug.block.scrollLines", *amount, direction, place)
                }
                jevision_core::ScrollUnit::Pixels => {
                    tr_in!(lang, "bug.block.scrollPoints", *amount, direction, place)
                }
            };
            text + &holding(lang, modifiers)
        }
        BlockKind::Type { text, into, .. } => match into {
            Some(t) => tr_in!(lang, "bug.block.typeInto", text, target(t)),
            None => tr_in!(lang, "bug.block.type", text),
        },
        BlockKind::PressKeys { keys, .. } => tr_in!(lang, "bug.block.pressKeys", keys),
        BlockKind::WaitFor {
            until, timeout_ms, ..
        } => {
            let what = match until {
                WaitUntil::Appears(r) => tr_in!(lang, "bug.wait.appears", &r.image),
                WaitUntil::Disappears(r) => tr_in!(lang, "bug.wait.disappears", &r.image),
                WaitUntil::ScreenStable => text_in(lang, "bug.wait.screenStable", &[]),
            };
            tr_in!(
                lang,
                "bug.block.waitFor",
                format_duration(*timeout_ms),
                what
            )
        }
        BlockKind::Pause { duration_ms, .. } => {
            tr_in!(lang, "bug.block.pause", format_duration(*duration_ms))
        }
        BlockKind::Check { reference, .. } => {
            tr_in!(lang, "bug.block.check", looks_like(lang, reference))
        }
        BlockKind::If { condition, .. } => match condition {
            Condition::Screen(reference) => {
                tr_in!(lang, "bug.block.ifScreen", looks_like(lang, reference))
            }
            Condition::Value(v) => tr_in!(lang, "bug.block.ifValue", &v.of, sign(v.is), &v.to),
        },
        BlockKind::Repeat { times, .. } => tr_in!(lang, "bug.block.repeat", *times),
        BlockKind::UseData { file, .. } => tr_in!(lang, "bug.block.useData", file),
        BlockKind::RunTest { test, .. } if is_block_path(test) => {
            tr_in!(lang, "bug.block.runBlock", test)
        }
        BlockKind::RunTest { test, .. } => tr_in!(lang, "bug.block.runTest", test),
        BlockKind::Script { language, .. } => {
            // Names of languages and tools are the same in every language.
            let language = match language {
                ScriptLanguage::Sh => "sh",
                ScriptLanguage::Zsh => "zsh",
                ScriptLanguage::Python3 => "Python",
                ScriptLanguage::Node => "Node.js",
                ScriptLanguage::AppleScript => "AppleScript",
            };
            tr_in!(lang, "bug.block.script", language)
        }
        BlockKind::Command { command, .. } => tr_in!(lang, "bug.block.command", command),
        BlockKind::Http { method, url, .. } => tr_in!(
            lang,
            "bug.block.http",
            format!("{method:?}").to_uppercase(),
            url
        ),
        BlockKind::CheckFile { path, until, .. } => match until {
            FileState::Exists => tr_in!(lang, "bug.block.fileExists", path),
            FileState::Missing => tr_in!(lang, "bug.block.fileMissing", path),
            FileState::Changed => tr_in!(lang, "bug.block.fileChanged", path),
            FileState::Contains(text) => tr_in!(lang, "bug.block.fileContains", path, text),
        },
        BlockKind::Database { file, query, .. } => tr_in!(lang, "bug.block.database", file, query),
        BlockKind::Clipboard { action, .. } => match action {
            ClipboardAction::Put { text } => tr_in!(lang, "bug.block.clipboardPut", text),
            ClipboardAction::PutImage { image } => {
                tr_in!(lang, "bug.block.clipboardPutImage", image)
            }
            ClipboardAction::Read { save_as, content } => match content {
                ClipboardContent::Text => tr_in!(lang, "bug.block.clipboardRead", save_as),
                ClipboardContent::Image => tr_in!(lang, "bug.block.clipboardReadImage", save_as),
                ClipboardContent::Files => tr_in!(lang, "bug.block.clipboardReadFiles", save_as),
            },
            ClipboardAction::Check { is, to, .. } => match is {
                ValueTest::Equals => tr_in!(lang, "bug.block.clipboardIs", to),
                ValueTest::NotEquals => tr_in!(lang, "bug.block.clipboardIsNot", to),
                ValueTest::Contains => tr_in!(lang, "bug.block.clipboardContains", to),
                ValueTest::GreaterThan => tr_in!(lang, "bug.block.clipboardGreater", to),
                ValueTest::LessThan => tr_in!(lang, "bug.block.clipboardLess", to),
            },
        },
        BlockKind::DateTime {
            save_as,
            format,
            language,
            time_zone,
            shift,
            ..
        } => {
            let moved = match shift {
                Some(s) => {
                    let (count, minus) =
                        (s.amount.unsigned_abs(), if s.amount < 0 { "-" } else { "" });
                    let moved = match s.unit {
                        TimeUnit::Seconds => tr_in!(lang, "bug.shift.seconds", count, minus),
                        TimeUnit::Minutes => tr_in!(lang, "bug.shift.minutes", count, minus),
                        TimeUnit::Hours => tr_in!(lang, "bug.shift.hours", count, minus),
                        TimeUnit::Days => tr_in!(lang, "bug.shift.days", count, minus),
                        TimeUnit::Weeks => tr_in!(lang, "bug.shift.weeks", count, minus),
                        TimeUnit::Months => tr_in!(lang, "bug.shift.months", count, minus),
                        TimeUnit::Years => tr_in!(lang, "bug.shift.years", count, minus),
                    };
                    tr_in!(lang, "bug.dateTime.moved", moved)
                }
                None => String::new(),
            };
            tr_in!(lang, "bug.block.dateTime", save_as, format)
                + &language
                    .as_ref()
                    .map_or_else(String::new, |l| tr_in!(lang, "bug.dateTime.language", l))
                + &time_zone
                    .as_ref()
                    .map_or_else(String::new, |z| tr_in!(lang, "bug.dateTime.zone", z))
                + &moved
        }
    }
}

/// The keys held down during a click, drag, hover or scroll, as a phrase that ends the sentence,
/// or nothing.
fn holding(lang: &str, modifiers: &Modifiers) -> String {
    if modifiers.is_empty() {
        String::new()
    } else {
        tr_in!(lang, "bug.holding", modifiers.names().join("+"))
    }
}

/// What the screen should look like for `reference` to match.
fn looks_like(lang: &str, reference: &Reference) -> String {
    match &reference.compare {
        Compare::Meaning { expect, .. } if !expect.is_empty() => {
            tr_in!(lang, "bug.screen.meaning", expect)
        }
        Compare::Meaning { .. } => tr_in!(lang, "bug.screen.sameMeaning", &reference.image),
        Compare::Exact => tr_in!(lang, "bug.screen.exact", &reference.image),
        Compare::Similar { threshold } => tr_in!(
            lang,
            "bug.screen.similar",
            &reference.image,
            percent(lang, *threshold)
        ),
    }
}

/// What should have happened and what did, for the failure of a block.
fn expected_and_actual(
    lang: &str,
    failure: &Failure,
    block: Option<&BlockKind>,
) -> (String, String) {
    let p = |name| failure.param(name);
    match failure.code.as_str() {
        "run.check.mismatch" | "run.check.mismatchDensity" => (
            tr_in!(
                lang,
                "bug.expected.mismatch",
                p("image"),
                share(lang, p("threshold"))
            ),
            tr_in!(lang, "bug.actual.mismatch", share(lang, p("similarity"))),
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
                    format!("{}.", capitalized(&looks_like(lang, reference)))
                }
                _ => text_in(lang, "bug.expected.meaningDefault", &[]),
            };
            (expected, format!("{}: {}", p("provider"), p("reason")))
        }
        "run.waitFor.timeout" => (
            tr_in!(
                lang,
                "bug.expected.waitFor",
                capitalized(&wait_target(lang, p("target"))),
                p("seconds")
            ),
            text_in(lang, "bug.actual.waitFor", &[]),
        ),
        "run.target.notFound" => (
            tr_in!(lang, "bug.expected.targetFound", p("image")),
            text_in(lang, "bug.actual.targetNotFound", &[]),
        ),
        "run.click.noChange" => (
            text_in(lang, "bug.expected.clickChanges", &[]),
            tr_in!(lang, "bug.actual.clickNoChange", p("seconds")),
        ),
        _ => (
            tr_in!(lang, "bug.expected.stepPasses", p("block")),
            crate::describe_in(lang, failure),
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
        let draft = bug_report_in("en", &report(), &flows, &context()).unwrap();
        assert_eq!(draft.title, "Sign in fails at Check “inbox”");
        let body = &draft.body;
        for line in [
            "**Cause:** Block “inbox”: the screen matches refs/inbox.png by 81.0%, and at least 95.0% is needed.",
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
            step_text("en", &step, &flows, "app")
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
        let draft = |report: &RunReport| {
            bug_report_in("en", report, &flows, &BugContext::default()).unwrap()
        };
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
        assert_eq!(
            bug_report_in("en", &passed, &flows, &BugContext::default()),
            None
        );
    }

    #[test]
    fn long_runs_list_the_steps_that_led_to_the_failure() {
        let mut long = report();
        let steps = &mut long.attempts[1].steps;
        let first = steps[0].clone();
        steps.splice(0..0, (0..40).map(|_| first.clone()));
        let draft = bug_report_in("en", &long, &HashMap::new(), &BugContext::default()).unwrap();
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

    #[test]
    fn a_bug_report_is_written_in_the_language_asked_for() {
        let flows = HashMap::from([("mail/sign-in.flow.json".to_owned(), flow())]);
        let (en, ru) = (
            bug_report_in("en", &report(), &flows, &context()).unwrap(),
            bug_report_in("ru", &report(), &flows, &context()).unwrap(),
        );
        assert_eq!(ru.title, "Sign in падает на шаге «inbox» (Проверка)");
        for line in [
            "### Шаги воспроизведения",
            "1. Открыть `com.example.Mail`\n",
            "**Причина:** Блок «inbox»: экран совпадает с refs/inbox.png на 81,0\u{a0}%, а нужно не меньше 95,0\u{a0}%.",
            "### Ожидалось",
            "### Получилось",
            "| Тестируемое приложение | `com.example.Mail` 16.0 |",
            "| Прогон | 2026-10-18 12:00:00.000 UTC, длительность 4,2 с, попыток: 2 |",
        ] {
            assert!(ru.body.contains(line), "{line}\n---\n{}", ru.body);
        }
        // What identifies the report doesn't depend on how it is told.
        assert_eq!(ru.fingerprint, en.fingerprint);
        assert_eq!(ru.pictures, en.pictures);
        assert_ne!(ru.body, en.body);
    }

    #[test]
    fn counts_in_a_report_follow_the_plural_rules_of_the_language() {
        let flow = jevision_core::parse(
            r#"{"format": 1, "name": "T", "app": "com.example.Mail", "start": "a",
            "blocks": {
              "a": {"type": "scroll", "direction": "down", "amount": 1, "unit": "lines", "next": "b"},
              "b": {"type": "scroll", "direction": "up", "amount": 2, "unit": "lines", "next": "c"},
              "c": {"type": "repeat", "times": 5, "body": "a", "next": "d"},
              "d": {"type": "dateTime", "saveAs": "day", "format": "%F", "shift": {"amount": 21, "unit": "days"}}
            }}"#,
        )
        .unwrap();
        let flows = HashMap::from([("t.flow.json".to_owned(), flow)]);
        let told = |lang: &str, block: &str| {
            let step: Step = serde_json::from_str(&format!(
                r#"{{"seq":1,"test":"t.flow.json","block":"{block}","type":"x","status":"passed","durationMs":1}}"#
            ))
            .unwrap();
            step_text(lang, &step, &flows, "app")
        };
        assert_eq!(told("en", "a"), "Scroll down 1 line over the window");
        assert_eq!(told("en", "b"), "Scroll up 2 lines over the window");
        assert_eq!(told("ru", "a"), "Прокрутить вниз на 1 строку: окно");
        assert_eq!(told("ru", "b"), "Прокрутить вверх на 2 строки: окно");
        assert_eq!(told("en", "c"), "Repeat 5 times");
        assert_eq!(told("ru", "c"), "Повторить 5 раз");
        assert_eq!(
            told("ru", "d"),
            "Сохранить дату и время как `day` в формате `%F`, сдвиг: 21 день"
        );
    }

    #[test]
    fn pictures_keep_their_label_and_are_captioned_in_the_language() {
        assert_eq!(picture_caption_in("en", "Difference"), "Difference");
        assert_eq!(picture_caption_in("ru", "Difference"), "Разница");
        assert_eq!(picture_caption_in("ru", "Expected"), "Ожидается");
        assert_eq!(picture_caption_in("ru", "Other"), "Other");
    }
}
