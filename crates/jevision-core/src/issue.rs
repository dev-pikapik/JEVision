use jevision_l10n::{language, tr_in};
use serde::ser::{SerializeMap, Serializer};
use serde_json::error::Category;

/// How serious a problem is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    /// The test cannot run as written.
    Error,
    /// The test runs, but probably not the way its author meant.
    Warning,
}

/// A problem found in a test file.
///
/// An issue carries a localization key (`code`) and named parameters instead of a finished
/// sentence, so every frontend shows it in its own language. Parameters keep the order in which
/// they were added.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Issue {
    pub code: String,
    pub severity: Severity,
    #[serde(serialize_with = "params_as_map")]
    pub params: Vec<IssueParam>,
}

/// One named value referenced by an issue's localized text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueParam {
    pub name: String,
    pub value: String,
}

impl Issue {
    pub(crate) fn error(code: &str) -> Self {
        Self {
            code: code.to_owned(),
            severity: Severity::Error,
            params: Vec::new(),
        }
    }

    pub(crate) fn warning(code: &str) -> Self {
        Self {
            severity: Severity::Warning,
            ..Self::error(code)
        }
    }

    pub(crate) fn with(mut self, name: &str, value: impl ToString) -> Self {
        self.params.push(IssueParam {
            name: name.to_owned(),
            value: value.to_string(),
        });
        self
    }

    /// Value of the parameter called `name`, if the issue has one.
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|p| p.name == name)
            .map(|p| p.value.as_str())
    }
}

fn params_as_map<S: Serializer>(params: &[IssueParam], s: S) -> Result<S::Ok, S::Error> {
    let mut map = s.serialize_map(Some(params.len()))?;
    for p in params {
        map.serialize_entry(&p.name, &p.value)?;
    }
    map.end()
}

/// The text is not a readable test file. Holds every problem found while reading it.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{}", .issues.iter().map(describe).collect::<Vec<_>>().join(" "))]
pub struct ParseError {
    pub issues: Vec<Issue>,
}

/// [`describe_in`] in the process language.
pub fn describe(issue: &Issue) -> String {
    describe_in(&language(), issue)
}

/// [`sentence_in`] in the process language.
pub fn sentence(issue: &Issue) -> String {
    sentence_in(&language(), issue)
}

/// What an issue says in `lang`, without where it is: for the app, which shows it next to the
/// block or field it concerns.
pub fn sentence_in(lang: &str, issue: &Issue) -> String {
    let p = |name: &str| issue.param(name).unwrap_or_default();
    match issue.code.as_str() {
        "flow.json.syntax" => tr_in!(lang, "flow.json.syntax", p("line"), p("column")),
        "flow.json.notObject" => tr_in!(lang, "flow.json.notObject"),
        "flow.format.missing" => tr_in!(lang, "flow.format.missing"),
        "flow.format.tooNew" => tr_in!(lang, "flow.format.tooNew", p("found"), p("supported")),
        "flow.format.unsupported" => {
            tr_in!(lang, "flow.format.unsupported", p("found"), p("oldest"))
        }
        "flow.block.unknownType" => {
            tr_in!(lang, "flow.block.unknownType", p("block"), p("type"))
        }
        "flow.field.missing" => tr_in!(lang, "flow.field.missing", p("field")),
        "flow.field.type" if p("expected").is_empty() => {
            tr_in!(lang, "flow.field.type", p("field"))
        }
        "flow.field.type" => tr_in!(
            lang,
            "flow.field.type.expected",
            p("field"),
            expected_kind(lang, p("expected"))
        ),
        "flow.field.value" => tr_in!(lang, "flow.field.value", p("value"), p("allowed")),
        "flow.field.duration" => tr_in!(lang, "flow.field.duration", p("value")),
        "flow.field.oneOf" => tr_in!(lang, "flow.field.oneOf", p("options")),
        "flow.field.unknown" => tr_in!(lang, "flow.field.unknown", p("field")),
        "flow.reference.expectNeedsMeaning" => {
            tr_in!(lang, "flow.reference.expectNeedsMeaning")
        }
        "flow.reference.modelNeedsMeaning" => tr_in!(lang, "flow.reference.modelNeedsMeaning"),
        "flow.reference.unknownProvider" => tr_in!(
            lang,
            "flow.reference.unknownProvider",
            p("provider"),
            p("allowed")
        ),
        "flow.reference.providerNeedsMeaning" => {
            tr_in!(lang, "flow.reference.providerNeedsMeaning")
        }
        "flow.reference.thresholdNeedsSimilar" => {
            tr_in!(lang, "flow.reference.thresholdNeedsSimilar")
        }
        "flow.field.empty" => tr_in!(lang, "flow.field.empty"),
        "flow.field.range" => tr_in!(lang, "flow.field.range", p("min"), p("max")),
        "flow.app.invalid" => tr_in!(lang, "flow.app.invalid", p("value")),
        "flow.link.invalid" => tr_in!(lang, "flow.link.invalid", p("value")),
        "flow.tags.duplicate" => tr_in!(lang, "flow.tags.duplicate", p("tag")),
        "flow.block.duplicateId" => tr_in!(lang, "flow.block.duplicateId", p("block")),
        "flow.block.invalidId" => tr_in!(lang, "flow.block.invalidId", p("block")),
        "flow.start.unknownBlock" if p("target").is_empty() => {
            tr_in!(lang, "flow.start.noBlocks")
        }
        "flow.start.unknownBlock" => tr_in!(lang, "flow.start.unknownBlock", p("target")),
        "flow.edge.unknownTarget" => tr_in!(lang, "flow.edge.unknownTarget", p("target")),
        "flow.path.invalid" => tr_in!(lang, "flow.path.invalid", p("value")),
        "flow.path.extension" => tr_in!(lang, "flow.path.extension", p("expected")),
        "flow.placeholder.invalid" => {
            tr_in!(lang, "flow.placeholder.invalid", p("placeholder"))
        }
        "flow.input.invalid" => tr_in!(lang, "flow.input.invalid", p("name")),
        "flow.input.missing" => tr_in!(lang, "flow.input.missing", p("name")),
        "flow.keys.invalid" => tr_in!(lang, "flow.keys.invalid", p("value")),
        "flow.drag.samePlace" => tr_in!(lang, "flow.drag.samePlace"),
        "flow.point.invalid" => tr_in!(lang, "flow.point.invalid"),
        "flow.area.invalid" => tr_in!(lang, "flow.area.invalid"),
        "flow.loop.emptyBody" => tr_in!(lang, "flow.loop.emptyBody"),
        "flow.cycle" => tr_in!(lang, "flow.cycle", p("blocks")),
        "flow.block.unreachable" => tr_in!(lang, "flow.block.unreachable"),
        "flow.if.sameCondition" => tr_in!(lang, "flow.if.sameCondition"),
        "flow.loop.noReturn" => tr_in!(lang, "flow.loop.noReturn"),
        "flow.note.unknownBlock" => {
            tr_in!(lang, "flow.note.unknownBlock", p("note"), p("block"))
        }
        "flow.note.attachment.invalid" => {
            tr_in!(lang, "flow.note.attachment.invalid", p("note"), p("value"))
        }
        "flow.note.reply.duplicateId" => {
            tr_in!(lang, "flow.note.reply.duplicateId", p("note"), p("reply"))
        }
        "flow.noCheck" => tr_in!(lang, "flow.noCheck"),
        "flow.reference.notSaved" => tr_in!(lang, "flow.reference.notSaved", p("path")),
        "flow.file.missing" => tr_in!(lang, "flow.file.missing", p("path")),
        "flow.env.unknown" => {
            tr_in!(
                lang,
                "flow.env.unknown",
                p("name"),
                crate::ENVIRONMENTS_FILE
            )
        }
        "flow.env.partial" => tr_in!(lang, "flow.env.partial", p("name"), p("environments")),
        "flow.environments.invalid" => {
            tr_in!(lang, "flow.environments.invalid", p("file"), p("reason"))
        }
        "flow.data.invalid" => tr_in!(
            lang,
            "flow.data.invalid",
            p("path"),
            crate::data::data_problem(lang, p("problem"), &|name| p(name).to_owned())
        ),
        "flow.data.unknownColumn" => tr_in!(
            lang,
            "flow.data.unknownColumn",
            p("path"),
            p("column"),
            p("columns")
        ),
        "flow.name.invalid" => tr_in!(lang, "flow.name.invalid", p("value")),
        "flow.name.duplicate" => tr_in!(lang, "flow.name.duplicate", p("value")),
        "flow.http.url" => tr_in!(lang, "flow.http.url", p("value")),
        "flow.http.header" => tr_in!(lang, "flow.http.header", p("value")),
        "flow.http.saveFrom" => tr_in!(lang, "flow.http.saveFrom", p("value")),
        "flow.value.notNumber" => tr_in!(lang, "flow.value.notNumber", p("value")),
        "flow.script.inputType" => tr_in!(
            lang,
            "flow.script.inputType",
            p("value"),
            value_kind(lang, p("type")),
            p("name"),
            input_hint(lang, p("type"))
        ),
        "flow.script.inputKind" => tr_in!(
            lang,
            "flow.script.inputKind",
            p("name"),
            value_kind(lang, p("type")),
            p("variable"),
            p("from")
                .split(", ")
                .map(|kind| value_kind(lang, kind))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        "flow.var.kind" => tr_in!(
            lang,
            "flow.var.kind",
            value_kind(lang, p("type")),
            p("variable"),
            p("from")
                .split(", ")
                .map(|kind| value_kind(lang, kind))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        "flow.target.area" => tr_in!(lang, "flow.target.area", p("value")),
        "flow.dateTime.format" => {
            tr_in!(lang, "flow.dateTime.format", p("value"), "%Y-%m-%d %H:%M")
        }
        "flow.var.unknown" => tr_in!(lang, "flow.var.unknown", p("name")),
        "flow.dependency.invalid" => tr_in!(lang, "flow.dependency.invalid", p("value")),
        "flow.dependency.duplicate" => tr_in!(lang, "flow.dependency.duplicate", p("test")),
        "flow.dependency.missing" => tr_in!(lang, "flow.dependency.missing", p("test")),
        "flow.dependency.self" => tr_in!(lang, "flow.dependency.self"),
        "flow.dependency.cycle" => tr_in!(lang, "flow.dependency.cycle", p("cycle")),
        "flow.block.notInstalled" => tr_in!(
            lang,
            "flow.block.notInstalled.command",
            crate::block_with_version(p("id"), p("version")),
            crate::install_command_in(lang, p("source"))
        ),
        "flow.package.version" => tr_in!(lang, "flow.package.version"),
        "flow.package.source" => tr_in!(lang, "flow.package.source"),
        "block.json.syntax" => tr_in!(lang, "block.json.syntax", p("line"), p("column")),
        "block.json.notObject" => tr_in!(lang, "block.json.notObject"),
        "block.format.unsupported" => {
            tr_in!(lang, "block.format.unsupported", p("found"), p("supported"))
        }
        "block.field.missing" => tr_in!(lang, "block.field.missing", p("field")),
        "block.field.empty" => tr_in!(lang, "block.field.empty", p("field")),
        "block.field.tooLong" => tr_in!(lang, "block.field.tooLong", p("field"), p("max")),
        "block.field.type" => match p("expected") {
            "a list" => tr_in!(lang, "block.field.type.list", p("field")),
            "an object" => tr_in!(lang, "block.field.type.object", p("field")),
            _ => tr_in!(lang, "block.field.type.text", p("field")),
        },
        "block.field.unknown" => tr_in!(lang, "block.field.unknown", p("field")),
        "block.id.invalid" => tr_in!(lang, "block.id.invalid", p("value")),
        "block.version.invalid" => tr_in!(lang, "block.version.invalid", p("value")),
        "block.icon.invalid" => tr_in!(lang, "block.icon.invalid", p("value")),
        "block.input.invalid" => tr_in!(lang, "block.input.invalid", p("name")),
        "block.input.duplicate" => tr_in!(lang, "block.input.duplicate", p("name")),
        "block.input.undeclared" => tr_in!(lang, "block.input.undeclared", p("name")),
        "block.input.unused" => tr_in!(lang, "block.input.unused", p("name")),
        "block.version.needsNewer" => {
            tr_in!(lang, "block.version.needsNewer", p("needs"), p("have"))
        }
        "block.path.outside" => tr_in!(lang, "block.path.outside", p("path")),
        "block.file.missing" => tr_in!(lang, "block.file.missing", p("file")),
        "block.file.unreadable" => {
            tr_in!(lang, "block.file.unreadable", p("file"), p("reason"))
        }
        "block.folder.notPackage" => tr_in!(lang, "block.folder.notPackage", p("path")),
        "block.package.tooLarge" => {
            tr_in!(lang, "block.package.tooLarge", megabytes(lang, p("max")))
        }
        "block.package.tooManyFiles" => tr_in!(lang, "block.package.tooManyFiles", p("max")),
        "block.path.unsafe" => tr_in!(lang, "block.path.unsafe", p("path")),
        "block.install.exists" => tr_in!(lang, "block.install.exists", p("id"), p("version")),
        "block.install.failed" => tr_in!(lang, "block.install.failed", p("id"), p("reason")),
        "block.remove.failed" => tr_in!(lang, "block.remove.failed", p("id"), p("reason")),
        "block.notInstalled" => tr_in!(lang, "block.notInstalled", p("id")),
        "block.source.missing" => tr_in!(lang, "block.source.missing", p("source")),
        "block.source.unsupported" => tr_in!(lang, "block.source.unsupported", p("source")),
        "block.source.none" => tr_in!(lang, "block.source.none", p("id")),
        "block.update.otherId" => tr_in!(lang, "block.update.otherId", p("id"), p("found")),
        "block.download.notHttps" => tr_in!(lang, "block.download.notHttps", p("url")),
        "block.download.failed" => {
            tr_in!(lang, "block.download.failed", p("url"), p("reason"))
        }
        "block.download.tooLarge" => tr_in!(
            lang,
            "block.download.tooLarge",
            p("url"),
            megabytes(lang, p("max"))
        ),
        "block.archive.invalid" => tr_in!(lang, "block.archive.invalid", p("reason")),
        "block.archive.empty" => tr_in!(lang, "block.archive.empty"),
        "block.archive.several" => tr_in!(lang, "block.archive.several", p("ids")),
        "block.lock.invalid" => tr_in!(lang, "block.lock.invalid", p("reason")),
        "block.export.nested" => tr_in!(lang, "block.export.nested", p("test")),
        "block.export.failed" => tr_in!(lang, "block.export.failed", p("reason")),
        "block.export.notBlock" => tr_in!(lang, "block.export.notBlock", p("path")),
        "block.export.skipped" => tr_in!(lang, "block.export.skipped", p("path")),
        _ => {
            let params: Vec<String> = issue
                .params
                .iter()
                .map(|p| format!("{} {:?}", p.name, p.value))
                .collect();
            format!("{} ({}).", issue.code, params.join(", "))
        }
    }
}

/// The sentence for an issue in `lang`, starting with the block and field it concerns, for
/// tools that show issues outside the app, such as the command line and AI assistants.
pub fn describe_in(lang: &str, issue: &Issue) -> String {
    let text = sentence_in(lang, issue);
    let named = |name| issue.param(name).filter(|v| !v.is_empty());
    let mut place: Vec<String> = [
        named("block").map(|v| tr_in!(lang, "flow.place.block", v)),
        named("edge").map(|v| tr_in!(lang, "flow.place.edge", v)),
        named("field").map(|v| tr_in!(lang, "flow.place.field", v)),
    ]
    .into_iter()
    .flatten()
    .collect();
    if place.is_empty() {
        place.push(if issue.code.starts_with("block.") {
            tr_in!(lang, "flow.place.package")
        } else {
            tr_in!(lang, "flow.place.test")
        });
    }
    let place = place.join(", ");
    let mut first = place.chars();
    let place = first
        .next()
        .map(|c| c.to_uppercase().chain(first).collect::<String>())
        .unwrap_or_default();
    format!("{place}: {text}")
}

/// What a field of the wrong kind must hold, from the `expected` of `flow.field.type`.
fn expected_kind(lang: &str, expected: &str) -> String {
    match expected {
        "integer" => tr_in!(lang, "flow.expected.integer"),
        "number" => tr_in!(lang, "flow.expected.number"),
        "string" => tr_in!(lang, "flow.expected.string"),
        "boolean" => tr_in!(lang, "flow.expected.boolean"),
        "object" => tr_in!(lang, "flow.expected.object"),
        "array" => tr_in!(lang, "flow.expected.array"),
        "stringArray" => tr_in!(lang, "flow.expected.stringArray"),
        "status" => tr_in!(lang, "flow.expected.status"),
        other => other.to_owned(),
    }
}

/// A kind of script value (`number`, `image`…) in words, for the middle of a sentence.
fn value_kind(lang: &str, kind: &str) -> String {
    match kind {
        "number" => tr_in!(lang, "flow.valueKind.number"),
        "boolean" => tr_in!(lang, "flow.valueKind.boolean"),
        "image" => tr_in!(lang, "flow.valueKind.image"),
        "area" => tr_in!(lang, "flow.valueKind.area"),
        _ => tr_in!(lang, "flow.valueKind.text"),
    }
}

/// How to write a script input value of this kind.
fn input_hint(lang: &str, kind: &str) -> String {
    match kind {
        "number" => tr_in!(lang, "flow.script.hint.number"),
        "boolean" => tr_in!(lang, "flow.script.hint.boolean"),
        "image" => tr_in!(lang, "flow.script.hint.image"),
        _ => tr_in!(lang, "flow.script.hint.area"),
    }
}

/// A size given in bytes, as whole megabytes.
fn megabytes(lang: &str, bytes: &str) -> String {
    bytes.parse::<u64>().map_or_else(
        |_| bytes.to_owned(),
        |b| tr_in!(lang, "package.size.megabytes", b >> 20),
    )
}

/// What is wrong with a JSON text that isn't JSON at all, in the process language; for any
/// other problem, the reader's own words, which name the fields it expected.
pub(crate) fn json_problem(e: &serde_json::Error) -> String {
    json_problem_in(&language(), e)
}

fn json_problem_in(lang: &str, e: &serde_json::Error) -> String {
    match e.classify() {
        Category::Syntax | Category::Eof => {
            tr_in!(lang, "flow.json.syntax", e.line(), e.column())
        }
        Category::Io | Category::Data => e.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_where_and_what() {
        let issue = Issue::error("flow.field.range")
            .with("block", "wait")
            .with("field", "timeout")
            .with("min", "1ms")
            .with("max", "10m");
        assert_eq!(
            describe_in("en", &issue),
            "Block “wait”, field “timeout”: Must be from 1ms to 10m."
        );
        assert_eq!(
            describe_in("ru", &issue),
            "Блок «wait», поле «timeout»: Допустимо от 1ms до 10m."
        );
        assert_eq!(sentence_in("en", &issue), "Must be from 1ms to 10m.");
        assert_eq!(sentence_in("ru", &issue), "Допустимо от 1ms до 10m.");
        let syntax = Issue::error("flow.json.syntax")
            .with("line", 3)
            .with("column", 7);
        assert_eq!(
            describe_in("en", &syntax),
            "Test: The file isn’t valid JSON. Check line 3, column 7."
        );
        assert_eq!(
            describe_in("ru", &syntax),
            "Тест: Файл не является корректным JSON. Проверьте строку 3, столбец 7."
        );
        assert_eq!(
            describe_in(
                "en",
                &Issue::error("flow.field.unknown").with("field", "colour")
            ),
            "Field “colour”: “colour” isn’t a known setting."
        );
    }

    #[test]
    fn names_the_kinds_of_values_in_words() {
        let issue = Issue::error("flow.field.type")
            .with("field", "files")
            .with("expected", "stringArray");
        assert_eq!(
            describe_in("en", &issue),
            "Field “files”: “files” has the wrong kind of value. It must be a list of texts."
        );
        assert_eq!(
            describe_in("ru", &issue),
            "Поле «files»: У «files» значение не того вида. Ожидается: список строк."
        );
        let script = Issue::error("flow.script.inputType")
            .with("value", "abc")
            .with("type", "number")
            .with("name", "count");
        assert_eq!(
            describe_in("en", &script),
            "Test: “abc” isn’t a valid number for the input count. Write a number such as 3.5."
        );
        assert_eq!(
            describe_in("ru", &script),
            "Тест: «abc» не подходит как число для входного значения count. Напишите число, \
             например 3.5."
        );
    }

    #[test]
    fn sizes_and_install_commands_follow_the_language() {
        let large = Issue::error("block.package.tooLarge").with("max", 50u64 << 20);
        assert_eq!(
            describe_in("en", &large),
            "Package: The block is larger than 50 MB."
        );
        assert_eq!(describe_in("ru", &large), "Пакет: Блок больше, чем 50 МБ.");
        let missing = Issue::error("flow.block.notInstalled")
            .with("block", "sign")
            .with("id", "com.acme.sign-in")
            .with("version", "1.2.0");
        assert!(describe_in("en", &missing).contains(
            "Install it with `jevision blocks install <folder, .zip file or https address>`"
        ));
        assert!(
            describe_in("ru", &missing)
                .contains("`jevision blocks install <папка, файл .zip или адрес https>`")
        );
    }

    #[test]
    fn a_broken_json_text_says_where() {
        let e = serde_json::from_str::<serde_json::Value>("{\n  \"a\": }").unwrap_err();
        assert_eq!(
            json_problem_in("en", &e),
            "The file isn’t valid JSON. Check line 2, column 8."
        );
        assert_eq!(
            json_problem_in("ru", &e),
            "Файл не является корректным JSON. Проверьте строку 2, столбец 8."
        );
    }
}
