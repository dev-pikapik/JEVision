//! A failure in words. The code of a [`Failure`] is a key of the string catalog; this module picks
//! the variant of the key a failure needs and puts its values in the order the text refers to
//! them, so every surface that shows a failure (the app, the CLI, the reports, the phone, MCP)
//! says the same thing in the language the person chose.

use crate::report::{Failure, percent};
use jevision_l10n::{Arg, text_in};

/// A text of the catalog and the values it refers to, before it is put into a language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Phrase {
    pub key: String,
    pub args: Vec<Arg>,
}

impl Phrase {
    fn new(key: &str, args: &[&str]) -> Self {
        Self {
            key: key.to_owned(),
            args: args.iter().map(|a| Arg::from(*a)).collect(),
        }
    }

    /// The phrase in `lang`.
    pub fn text(&self, lang: &str) -> String {
        text_in(lang, &self.key, &self.args)
    }
}

/// The sentence for a failure in `lang`. A code from a newer version is told in the words the
/// failure was recorded with, when it has them.
pub fn describe_in(lang: &str, failure: &Failure) -> String {
    let phrase = phrase(lang, failure);
    let unknown = |p: &Phrase| p.key == "run.unknown";
    let newer = unknown(&phrase)
        || phrase.key == "run.block.prefixed"
            && failure.code.starts_with("run.vision.")
            && unknown(&vision_phrase(failure));
    if newer && !failure.message.is_empty() {
        return failure.message.clone();
    }
    phrase.text(lang)
}

/// [`describe_in`] in the language set with `jevision_l10n::set_language`.
pub fn describe(failure: &Failure) -> String {
    describe_in(&jevision_l10n::language(), failure)
}

/// The sentence for a problem with a vision model, without the block, in `lang`.
pub fn describe_vision_in(lang: &str, failure: &Failure) -> String {
    vision_phrase(failure).text(lang)
}

/// [`describe_vision_in`] in the language set with `jevision_l10n::set_language`.
pub fn describe_vision(failure: &Failure) -> String {
    describe_vision_in(&jevision_l10n::language(), failure)
}

/// The key of the failure's text and the values it refers to. Some texts depend on the language
/// of what they name (a kind of value, a permission), so this needs `lang` for those.
pub fn phrase(lang: &str, f: &Failure) -> Phrase {
    let p = |name: &str| f.param(name);
    let block = &block_name(lang, f);
    let seconds = p("seconds");
    // A reason that is put before a full stop doesn't bring its own.
    let reason = || p("reason").trim_end_matches('.');
    // What a program said, on a line of its own, or nothing.
    let said = || match p("reason") {
        "" => String::new(),
        said => format!("\n{said}"),
    };
    // An exit code, or why there is none.
    let code = |name: &str| match p(name) {
        "signal" => text_in(lang, "run.exitCode.signal", &[]),
        code => code.to_owned(),
    };
    let key = f.code.as_str();
    match key {
        "run.test.unreadable" => Phrase::new(key, &[p("test"), reason()]),
        "run.test.needsNewer" => Phrase::new(key, &[p("test"), p("found"), p("supported")]),
        "run.test.invalid" => Phrase::new(key, &[p("errors")]),
        "run.start.unknownBlock" => Phrase::new(key, &[p("block")]),
        "run.secret.missing" => Phrase::new(key, &[p("name")]),
        "run.input.missing" => Phrase::new(key, &[block, p("name")]),
        "run.block.notInstalled" => Phrase::new(
            "run.block.notInstalled.cli",
            &[
                block,
                &jevision_core::block_with_version(p("package"), p("version")),
                &jevision_core::install_command(p("source")),
            ],
        ),
        "run.block.needsNewer" => Phrase::new(key, &[block, p("package"), p("needs"), p("have")]),
        "run.path.outsideProject" => Phrase::new(key, &[block, p("path")]),
        "run.file.missing" => Phrase::new(key, &[block, p("file")]),
        "run.file.copyFailed" => Phrase::new(key, &[block, p("file"), reason()]),
        "run.target.notFound" => Phrase::new(key, &[block, p("image")]),
        "run.target.descriptionNotFound" => Phrase::new(key, &[block, p("description")]),
        "run.target.noLocator" | "run.compare.meaningUnavailable" => Phrase::new(key, &[block]),
        "run.reference.missing" => Phrase::new(key, &[block, p("image")]),
        "run.reference.unreadable" => Phrase::new(key, &[block, p("image"), reason()]),
        "run.screenshot.unreadable" => Phrase::new(key, &[block, reason()]),
        "run.compare.shapeMismatch" => {
            Phrase::new(key, &[block, p("actual"), p("image"), p("expected")])
        }
        "run.check.meaningMismatch" => Phrase::new(key, &[block, p("provider"), p("reason")]),
        "run.vision.cloudBlocked" if p("setting") == "project" => {
            Phrase::new("run.vision.cloudBlocked.project", &[block, p("provider")])
        }
        "run.vision.cloudBlocked" => Phrase::new(key, &[block, p("provider")]),
        code if code.starts_with("run.vision.") => {
            let inside = vision_phrase(f).text(lang);
            Phrase::new("run.block.prefixed", &[block, &inside])
        }
        "run.click.noChange" | "run.action.noChange" => Phrase::new(key, &[block, seconds]),
        "run.waitFor.timeout" if p("target") == "screenStable" => {
            Phrase::new("run.waitFor.timeout.screenStable", &[block, seconds])
        }
        "run.waitFor.timeout" if !p("dark").is_empty() => Phrase::new(
            "run.waitFor.timeout.dark",
            &[block, seconds, p("target"), p("dark")],
        ),
        "run.waitFor.timeout" => Phrase::new(key, &[block, seconds, p("target")]),
        "run.check.mismatch" => {
            let (similarity, threshold) =
                (share(lang, p("similarity")), share(lang, p("threshold")));
            Phrase::new(key, &[block, p("image"), &similarity, &threshold])
        }
        "run.check.mismatchDensity" => {
            let (similarity, threshold) =
                (share(lang, p("similarity")), share(lang, p("threshold")));
            Phrase::new(
                key,
                &[
                    block,
                    p("image"),
                    &similarity,
                    &threshold,
                    p("expected"),
                    p("actual"),
                ],
            )
        }
        "run.env.notChosen" => Phrase::new(key, &[p("name"), jevision_core::ENVIRONMENTS_FILE]),
        "run.env.missing" => Phrase::new(
            key,
            &[
                p("environment"),
                p("name"),
                jevision_core::ENVIRONMENTS_FILE,
            ],
        ),
        "run.env.unknown" => Phrase::new(key, &[p("environment"), p("file")]),
        "run.environments.unreadable" | "run.project.unreadable" => {
            Phrase::new(key, &[p("file"), reason()])
        }
        "run.data.noRow" => Phrase::new(key, &[block, p("file"), p("rows"), p("row")]),
        "run.data.unreadable" => Phrase::new(key, &[block, p("file"), &data_problem(lang, f)]),
        "run.data.unknownColumn" => Phrase::new(key, &[block, p("column")]),
        "run.runTest.recursion" => Phrase::new(key, &[block, p("test")]),
        "run.io.writeFailed" => Phrase::new(key, &[p("path"), reason()]),
        "run.hands.permissionDenied" => {
            Phrase::new(key, &[&permission_name(lang, p("permission"))])
        }
        "run.hands.appNotFound"
        | "run.hands.noWindow"
        | "run.hands.stillRunning"
        | "run.hands.windowFixed" => Phrase::new(key, &[block, p("app")]),
        "run.hands.cannotOpen" => Phrase::new(key, &[block, p("file")]),
        "run.hands.screenLocked" => Phrase::new(key, &[block]),
        "run.hands.windowDiffers" => {
            let key = if p("what") == "position" {
                "run.hands.windowDiffers.position"
            } else {
                "run.hands.windowDiffers.size"
            };
            Phrase::new(key, &[block, p("app"), p("wanted"), p("actual")])
        }
        "run.hands.failed" => Phrase::new(key, &[block, p("reason")]),
        "run.openApp.cleanHome" | "run.openApp.sandboxed" | "run.openApp.settingsLater" => {
            Phrase::new(key, &[p("app")])
        }
        "run.openApp.settingsNotSaved" => Phrase::new(key, &[p("app"), reason()]),
        "run.var.missing" => Phrase::new(key, &[block, p("name")]),
        "run.value.notNumber" => Phrase::new(key, &[block, p("value")]),
        "run.process.notFound" => Phrase::new(key, &[block, p("program")]),
        "run.process.cannotStart" => Phrase::new(key, &[block, p("program"), reason()]),
        "run.process.timeout" => Phrase::new(key, &[block, seconds]),
        "run.script.failed" => Phrase::new(key, &[block, &code("code"), &said()]),
        "run.script.missingOutput" => Phrase::new(key, &[block, p("name")]),
        "run.script.badOutput" | "run.script.badInput" => Phrase::new(
            key,
            &[block, p("name"), &value_kind(lang, p("type")), p("value")],
        ),
        "run.clipboard.noImage" | "run.clipboard.noFiles" => Phrase::new(key, &[block]),
        "run.clipboard.notPicture" => Phrase::new(key, &[block, p("image")]),
        "run.target.badArea" => Phrase::new(key, &[block, p("value")]),
        "run.clipboard.mismatch" => {
            let expectation = clipboard_expectation(lang, p("test"), p("expected"));
            Phrase::new(key, &[block, seconds, &expectation, p("actual")])
        }
        "run.dateTime.failed" => Phrase::new(key, &[block, reason()]),
        "run.command.exitCode" => {
            Phrase::new(key, &[block, &code("actual"), p("expected"), &said()])
        }
        "run.http.failed" => Phrase::new(key, &[block, p("url"), reason()]),
        "run.http.status" => {
            let body = match p("body") {
                "" => String::new(),
                body => format!("\n{body}"),
            };
            Phrase::new(key, &[block, p("url"), p("status"), p("expected"), &body])
        }
        "run.http.noValue" => Phrase::new(key, &[block, p("from")]),
        "run.file.timeout" => match p("state") {
            "exists" => Phrase::new("run.file.timeout.exists", &[block, p("path"), seconds]),
            "missing" => Phrase::new("run.file.timeout.missing", &[block, p("path"), seconds]),
            "changed" => Phrase::new("run.file.timeout.changed", &[block, p("path"), seconds]),
            _ => Phrase::new(
                "run.file.timeout.contains",
                &[block, p("path"), p("text"), seconds],
            ),
        },
        "run.database.failed" => Phrase::new(key, &[block, reason()]),
        "run.database.value" | "run.database.rows" => {
            Phrase::new(key, &[block, p("actual"), p("expected"), seconds])
        }
        _ => Phrase::new("run.unknown", &[&unknown(f)]),
    }
}

/// A failure this version has no sentence for: its code and values, so it can still be reported.
fn unknown(f: &Failure) -> String {
    if f.params.is_empty() {
        return f.code.clone();
    }
    let values: Vec<String> = f.params.iter().map(|(k, v)| format!("{k}={v}")).collect();
    format!("{} ({})", f.code, values.join(", "))
}

/// The text for a problem with a vision model, without the block it happened in.
pub fn vision_phrase(f: &Failure) -> Phrase {
    let p = |name: &str| f.param(name);
    let provider = p("provider");
    let key = f.code.as_str();
    let month = p("period") == "month";
    match key {
        "run.vision.noPictures"
        | "run.vision.noModel"
        | "run.vision.noKey"
        | "run.vision.badKey"
        | "run.vision.cannotLocate" => Phrase::new(key, &[provider]),
        "run.vision.badAddress" => Phrase::new(key, &[p("address")]),
        "run.vision.unreachable" => Phrase::new(key, &[provider, p("address")]),
        "run.vision.timeout" => Phrase::new(key, &[provider, p("seconds")]),
        "run.vision.unknownModel" => Phrase::new(key, &[provider, p("model")]),
        "run.vision.rateLimited" | "run.vision.refused" => {
            Phrase::new(key, &[provider, p("message")])
        }
        "run.vision.serviceError" => Phrase::new(key, &[provider, p("status"), p("message")]),
        "run.vision.limitReached" => Phrase::new(
            if month {
                "run.vision.limitReached.month"
            } else {
                "run.vision.limitReached"
            },
            &[provider, p("spent"), p("limit")],
        ),
        "run.vision.limitNear" => Phrase::new(
            if month {
                "run.vision.limitNear.month"
            } else {
                "run.vision.limitNear"
            },
            &[provider, p("spent"), p("limit")],
        ),
        "run.vision.journalFailed" => Phrase::new(key, &[p("path"), p("reason")]),
        "run.vision.badAnswer" => Phrase::new(key, &[provider, p("answer")]),
        "run.vision.modelCannotLocate" => Phrase::new(key, &[p("model")]),
        _ => Phrase::new("run.unknown", &[&unknown(f)]),
    }
}

/// What is wrong with a CSV table, from the `data.*` code and the values the failure carries.
fn data_problem(lang: &str, f: &Failure) -> String {
    let p = |name: &str| f.param(name);
    let code = p("problem");
    let args: &[&str] = match code {
        "data.unreadable" | "data.unwritable" => &[p("reason")],
        "data.blankColumn" => &[p("column")],
        "data.duplicateColumn" => &[p("name")],
        "data.unclosedQuote" | "data.strayQuote" => &[p("line")],
        "data.rowLength" => &[p("line"), p("found"), p("expected")],
        "data.notText" | "data.empty" => &[],
        _ => return p("reason").to_owned(),
    };
    Phrase::new(code, args).text(lang)
}

/// The block a failure is about, named as the editor's canvas names it. A failure saved before
/// failures kept the type of their block, or of a type this version doesn't know, names it by id.
pub(crate) fn block_name(lang: &str, failure: &Failure) -> String {
    block_title(lang, failure.param("blockType"))
        .unwrap_or_else(|| failure.param("block").to_owned())
}

/// The name of a block type on the canvas, such as `Wait For`; `None` for a type this version
/// doesn't know. The keys are the ones the app names its blocks with.
pub(crate) fn block_title(lang: &str, block_type: &str) -> Option<String> {
    let key = match block_type {
        "openApp" => "Open App",
        "openFile" => "Open File",
        "click" => "Click",
        "hover" => "Hover",
        "drag" => "Drag",
        "scroll" => "Scroll",
        "type" => "Type Text",
        "pressKeys" => "Press Keys",
        "waitFor" => "Wait For",
        "pause" => "Pause",
        "check" => "Check",
        "if" => "If",
        "repeat" => "Repeat",
        "useData" => "Use Data",
        "runTest" => "Run Test",
        "http" => "HTTP Request",
        "database" => "Database",
        "checkFile" => "Wait For File",
        "command" => "Run Command",
        "script" => "Script",
        "clipboard" => "Clipboard",
        "dateTime" => "Date and Time",
        _ => return None,
    };
    Some(text_in(lang, key, &[]))
}

/// A share kept in a report as `81.0%`, written the way `lang` writes it. Anything that isn't
/// such a share is shown as it is.
pub(crate) fn share(lang: &str, kept: &str) -> String {
    kept.strip_suffix('%')
        .and_then(|n| n.trim().parse::<f64>().ok())
        .map_or_else(|| kept.to_owned(), |n| percent(lang, n / 100.0))
}

/// The name of a system permission (`screenRecording`, `accessibility`) as System Settings shows it.
pub(crate) fn permission_name(lang: &str, permission: &str) -> String {
    match permission {
        "screenRecording" => text_in(lang, "run.permission.screenRecording", &[]),
        "accessibility" => text_in(lang, "run.permission.accessibility", &[]),
        other => other.to_owned(),
    }
}

/// What a script value of this kind (the name of a `ValueKind`) looks like, after “should be”.
fn value_kind(lang: &str, kind: &str) -> String {
    let key = match kind {
        "number" => "run.valueKind.number",
        "image" => "run.valueKind.image",
        "area" => "run.valueKind.area",
        "text" => "run.valueKind.text",
        _ => "run.valueKind.boolean",
    };
    text_in(lang, key, &[])
}

/// What a clipboard check expected, as part of a sentence: `test` is the check's name.
fn clipboard_expectation(lang: &str, test: &str, expected: &str) -> String {
    let key = match test {
        "notEquals" => "run.clipboard.notEquals",
        "contains" => "run.clipboard.contains",
        "greaterThan" => "run.clipboard.greaterThan",
        "lessThan" => "run.clipboard.lessThan",
        _ => "run.clipboard.equals",
    };
    text_in(lang, key, &[Arg::from(expected)])
}

/// What the test waited for, as part of a sentence: the screen settling, or a picture.
pub(crate) fn wait_target(lang: &str, target: &str) -> String {
    match target {
        "screenStable" => text_in(lang, "run.waitTarget.screenStable", &[]),
        image => text_in(lang, "run.waitTarget.image", &[Arg::from(image)]),
    }
}
