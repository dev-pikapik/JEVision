use serde::ser::{SerializeMap, Serializer};

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

/// English sentence for an issue, starting with the block and field it concerns, for tools
/// that have no translation at hand, such as the command line and AI assistants.
pub fn describe(issue: &Issue) -> String {
    let p = |name| issue.param(name).unwrap_or_default();
    let text = match issue.code.as_str() {
        "flow.json.syntax" => format!(
            "not valid JSON at line {}, column {}.",
            p("line"),
            p("column")
        ),
        "flow.json.notObject" => "must be a JSON object.".to_owned(),
        "flow.format.missing" => format!(
            "\"format\" is missing; this version writes format {}.",
            crate::FORMAT_VERSION
        ),
        "flow.format.tooNew" => format!(
            "format {} is from a newer JEVision; this version reads up to format {}.",
            p("found"),
            p("supported")
        ),
        "flow.format.unsupported" => format!(
            "format {} is too old; this version reads format {} and newer.",
            p("found"),
            p("oldest")
        ),
        "flow.block.unknownType" => format!("unknown type \"{}\".", p("type")),
        "flow.field.missing" => "required, but missing.".to_owned(),
        "flow.field.type" if p("expected").is_empty() => "wrong kind of value.".to_owned(),
        "flow.field.type" => format!("wrong kind of value; it must be {}.", p("expected")),
        "flow.field.value" => format!(
            "\"{}\" isn't allowed here; use one of: {}.",
            p("value"),
            p("allowed")
        ),
        "flow.field.duration" => format!(
            "\"{}\" isn't a duration; write it like 500ms, 10s or 2m.",
            p("value")
        ),
        "flow.field.oneOf" => format!("use exactly one of: {}.", p("options")),
        "flow.field.unknown" => "not a known field.".to_owned(),
        "flow.reference.expectNeedsMeaning" => {
            "\"expect\" works only with \"compare\": \"meaning\".".to_owned()
        }
        "flow.reference.modelNeedsMeaning" => {
            "\"model\" works only with \"compare\": \"meaning\".".to_owned()
        }
        "flow.reference.unknownProvider" => format!(
            "\"{}\" isn't an AI provider; use one of: {}.",
            p("provider"),
            p("allowed")
        ),
        "flow.reference.providerNeedsMeaning" => {
            "\"provider\" works only with \"compare\": \"meaning\".".to_owned()
        }
        "flow.reference.thresholdNeedsSimilar" => {
            "\"threshold\" works only with \"compare\": \"similar\".".to_owned()
        }
        "flow.field.empty" => "can't be empty.".to_owned(),
        "flow.field.range" => format!("must be from {} to {}.", p("min"), p("max")),
        "flow.app.invalid" => format!(
            "\"{}\" isn't an app bundle identifier like com.apple.TextEdit.",
            p("value")
        ),
        "flow.link.invalid" => format!(
            "\"{}\" isn't a web address; start it with https://.",
            p("value")
        ),
        "flow.tags.duplicate" => format!("the tag \"{}\" is listed twice.", p("tag")),
        "flow.block.duplicateId" => "two blocks have this name.".to_owned(),
        "flow.block.invalidId" => {
            "can't be a block name: use only letters, digits, - and _.".to_owned()
        }
        "flow.start.unknownBlock" if p("target").is_empty() => {
            "the test has no blocks yet.".to_owned()
        }
        "flow.start.unknownBlock" => format!(
            "the test starts at \"{}\", but there is no such block.",
            p("target")
        ),
        "flow.edge.unknownTarget" => {
            format!("leads to \"{}\", but there is no such block.", p("target"))
        }
        "flow.path.invalid" => {
            format!("\"{}\" isn't a path inside the project folder.", p("value"))
        }
        "flow.path.extension" => format!("must name a {} file.", p("expected")),
        "flow.placeholder.invalid" => format!(
            "\"{}\" isn't a valid placeholder; use ${{secret.NAME}}, ${{env.NAME}}, ${{data.COLUMN}} or ${{var.NAME}}.",
            p("placeholder")
        ),
        "flow.input.invalid" => format!(
            "\"{}\" can't name an input; use letters, digits and _, not starting with a digit.",
            p("name")
        ),
        "flow.input.missing" => format!(
            "the block reads the input {}; give it a value.",
            p("name")
        ),
        "flow.keys.invalid" => format!(
            "\"{}\" isn't a key combination like cmd+shift+s.",
            p("value")
        ),
        "flow.drag.samePlace" => {
            "the drag ends where it starts; choose another place to drop on.".to_owned()
        }
        "flow.point.invalid" => {
            "the point must be inside the window: x and y from 0 to 1.".to_owned()
        }
        "flow.area.invalid" => {
            "the area must lie inside the image: x, y, width and height from 0 to 1.".to_owned()
        }
        "flow.loop.emptyBody" => "the loop has nothing inside; add a block on its body.".to_owned(),
        "flow.cycle" => format!(
            "these blocks lead round in a circle: {}. Use a repeat block to run steps again.",
            p("blocks")
        ),
        "flow.block.unreachable" => "nothing leads to this block, so it never runs.".to_owned(),
        "flow.if.sameCondition" => {
            "an earlier condition of this block checks the same thing, so this one never leads anywhere."
                .to_owned()
        }
        "flow.loop.noReturn" => "the blocks inside don't lead back to this loop.".to_owned(),
        "flow.note.unknownBlock" => format!(
            "the note \"{}\" is about the block \"{}\", but there is no such block.",
            p("note"),
            p("block")
        ),
        "flow.note.attachment.invalid" => format!(
            "the note \"{}\" has \"{}\", which isn't a file name or a web address.",
            p("note"),
            p("value")
        ),
        "flow.note.reply.duplicateId" => format!(
            "the note \"{}\" has two replies named \"{}\".",
            p("note"),
            p("reply")
        ),
        "flow.noCheck" => "this test never checks the screen, so it can't catch a wrong result; add a check block.".to_owned(),
        "flow.reference.notSaved" => format!(
            "{} doesn't exist yet; the first run will save the screen there as the reference.",
            p("path")
        ),
        "flow.file.missing" => format!(
            "{} doesn't exist; the run will stop at this block until it's added.",
            p("path")
        ),
        "flow.env.unknown" => format!(
            "no environment has the value {}; add it to {}.",
            p("name"),
            crate::ENVIRONMENTS_FILE
        ),
        "flow.env.partial" => format!(
            "the value {} is missing in these environments: {}.",
            p("name"),
            p("environments")
        ),
        "flow.environments.invalid" => format!("{} can't be read: {}.", p("file"), p("reason")),
        "flow.data.invalid" => format!(
            "{} can't be used as a data table: {}.",
            p("path"),
            p("reason")
        ),
        "flow.data.unknownColumn" => format!(
            "{} has no column \"{}\"; its columns are: {}.",
            p("path"),
            p("column"),
            p("columns")
        ),
        "flow.name.invalid" => format!(
            "\"{}\" can't be a name: start with a letter or _, then use letters, digits and _.",
            p("value")
        ),
        "flow.name.duplicate" => format!("the name \"{}\" is used twice.", p("value")),
        "flow.http.url" => format!(
            "\"{}\" isn't a web address; start it with http:// or https://.",
            p("value")
        ),
        "flow.http.header" => format!("\"{}\" can't be a header name.", p("value")),
        "flow.http.saveFrom" => format!(
            "\"{}\" isn't a part of the answer; use status, body, body.FIELD like body.items[0].id, or header.NAME.",
            p("value")
        ),
        "flow.value.notNumber" => format!(
            "\"{}\" isn't a number, so it can't be compared as greater or less.",
            p("value")
        ),
        "flow.script.inputType" => format!(
            "\"{}\" isn't a valid {} for the input {}: {}.",
            p("value"),
            p("type"),
            p("name"),
            match p("type") {
                "number" => "write a number such as 3.5",
                "boolean" => "write true or false",
                "image" => "write the path of a PNG file",
                _ => "write four numbers between 0 and 1 as x,y,width,height, such as 0.1,0.2,0.5,0.3",
            }
        ),
        "flow.script.inputKind" => format!(
            "the input {} takes a {}, but {} is saved as {}.",
            p("name"),
            p("type"),
            p("variable"),
            p("from")
        ),
        "flow.dateTime.format" => format!(
            "\"{}\" isn't a date format; write letters after %, such as %Y-%m-%d %H:%M.",
            p("value")
        ),
        "flow.var.unknown" => format!(
            "no block of this test saves the value {}; the run stops here unless a test it runs saves it.",
            p("name")
        ),
        "flow.dependency.invalid" => format!(
            "\"{}\" isn't a test file inside the project folder; write its path from this \
             test's folder, such as login.flow.json.",
            p("value")
        ),
        "flow.dependency.duplicate" => format!("{} is listed twice.", p("test")),
        "flow.dependency.missing" => format!("the test {} doesn't exist.", p("test")),
        "flow.dependency.self" => "a test can't depend on itself.".to_owned(),
        "flow.dependency.cycle" => format!(
            "the tests depend on each other in a circle: {}. Remove one of these dependencies.",
            p("cycle")
        ),
        "flow.block.notInstalled" => format!(
            "needs the block {}, which isn't installed in this project; install it with `{}` or remove this step.",
            crate::block_with_version(p("id"), p("version")),
            crate::install_command(p("source"))
        ),
        "flow.package.version" => {
            "the version of the package must look like 1.2.0.".to_owned()
        }
        "flow.package.source" => {
            "the address of the package must start with https://.".to_owned()
        }
        "block.json.syntax" => format!(
            "block.json is not valid JSON at line {}, column {}.",
            p("line"),
            p("column")
        ),
        "block.json.notObject" => "block.json must be a JSON object.".to_owned(),
        "block.format.unsupported" => format!(
            "format {} isn't supported; this version reads format {}.",
            p("found"),
            p("supported")
        ),
        "block.field.missing" => "required, but missing.".to_owned(),
        "block.field.empty" => "can't be empty.".to_owned(),
        "block.field.tooLong" => format!("is too long; the most is {}.", p("max")),
        "block.field.type" => format!("wrong kind of value; it must be {}.", p("expected")),
        "block.field.unknown" => "isn't a field of block.json.".to_owned(),
        "block.id.invalid" => format!(
            "\"{}\" can't be a package id; write three or more parts of lowercase letters, digits \
             and - joined by dots, like com.acme.sign-in.",
            p("value")
        ),
        "block.version.invalid" => format!(
            "\"{}\" isn't a version; write three numbers like 1.2.0.",
            p("value")
        ),
        "block.icon.invalid" => format!(
            "\"{}\" isn't the name of an SF Symbol; write lowercase letters, digits and dots, \
             like person.badge.key.",
            p("value")
        ),
        "block.input.invalid" => format!(
            "\"{}\" can't be an input name: start with a letter or _, then use letters, digits and _.",
            p("name")
        ),
        "block.input.duplicate" => format!("the input \"{}\" is listed twice.", p("name")),
        "block.input.undeclared" => format!(
            "the steps read the input \"{}\", but block.json doesn't list it under inputs.",
            p("name")
        ),
        "block.input.unused" => format!(
            "block.json lists the input \"{}\", but no step reads it.",
            p("name")
        ),
        "block.version.needsNewer" => format!(
            "needs JEVision {} or newer; this is {}. Update JEVision to use this block.",
            p("needs"),
            p("have")
        ),
        "block.path.outside" => format!(
            "{} leads out of the package folder; a block can only use files inside it.",
            p("path")
        ),
        "block.file.missing" => format!("the package has no {}.", p("file")),
        "block.file.unreadable" => format!("{} can't be read: {}.", p("file"), p("reason")),
        "block.folder.notPackage" => format!(
            "{} isn't a block package: it has no block.json and flow.json.",
            p("path")
        ),
        "block.package.tooLarge" => format!("the package is larger than {}.", megabytes(p("max"))),
        "block.package.tooManyFiles" => format!("the package has more than {} files.", p("max")),
        "block.path.unsafe" => format!(
            "{} would end up outside the package folder, so the archive is refused.",
            p("path")
        ),
        "block.install.exists" => format!(
            "{} {} is already installed.",
            p("id"),
            p("version")
        ),
        "block.install.failed" => format!("couldn't install {}: {}.", p("id"), p("reason")),
        "block.remove.failed" => format!("couldn't remove {}: {}.", p("id"), p("reason")),
        "block.notInstalled" => format!("no block with the id {} is installed.", p("id")),
        "block.source.missing" => format!("{} doesn't exist.", p("source")),
        "block.source.unsupported" => format!(
            "{} isn't a block package folder, a .zip file or an https address; GitHub \
             repositories and releases work too.",
            p("source")
        ),
        "block.source.none" => format!(
            "{} wasn't installed from an address, so there is nothing to update it from.",
            p("id")
        ),
        "block.update.otherId" => format!(
            "the address of {} now holds a different block, {}, so it isn't an update.",
            p("id"),
            p("found")
        ),
        "block.download.notHttps" => format!(
            "only https addresses can be downloaded; {} isn't one.",
            p("url")
        ),
        "block.download.failed" => format!("couldn't download {}: {}.", p("url"), p("reason")),
        "block.download.tooLarge" => {
            format!("{} is larger than {}.", p("url"), megabytes(p("max")))
        }
        "block.archive.invalid" => format!("the archive can't be read: {}.", p("reason")),
        "block.archive.empty" => {
            "the archive has no block package: no folder with block.json and flow.json.".to_owned()
        }
        "block.archive.several" => format!(
            "the archive has several block packages ({}); install them one by one.",
            p("ids")
        ),
        "block.lock.invalid" => format!(
            "{} can't be read: {}.",
            crate::PACKAGE_LOCK_FILE,
            p("reason")
        ),
        "block.export.nested" => format!(
            "this block runs {}, and a package can't hold other blocks yet; put the steps of \
             that block into this one.",
            p("test")
        ),
        "block.export.failed" => format!("couldn't write the package: {}.", p("reason")),
        "block.export.notBlock" => format!(
            "{} isn't one of this project's own blocks.",
            p("path")
        ),
        "block.export.skipped" => format!(
            "{} isn't a file a package can hold, so it was left out.",
            p("path")
        ),
        _ => {
            let params: Vec<String> = issue
                .params
                .iter()
                .map(|p| format!("{} {:?}", p.name, p.value))
                .collect();
            format!("{} ({}).", issue.code, params.join(", "))
        }
    };
    let mut place = Vec::new();
    for (name, label) in [("block", "Block"), ("edge", "edge"), ("field", "field")] {
        if let Some(value) = issue.param(name).filter(|v| !v.is_empty()) {
            place.push(format!("{label} \"{value}\""));
        }
    }
    if place.is_empty() {
        place.push(
            if issue.code.starts_with("block.") {
                "Package"
            } else {
                "Test"
            }
            .to_owned(),
        );
    }
    let place = place.join(", ");
    let mut first = place.chars();
    let place = first
        .next()
        .map(|c| c.to_uppercase().chain(first).collect::<String>())
        .unwrap_or_default();
    format!("{place}: {text}")
}

/// A size given in bytes, as whole megabytes.
fn megabytes(bytes: &str) -> String {
    bytes
        .parse::<u64>()
        .map_or_else(|_| bytes.to_owned(), |b| format!("{} MB", b >> 20))
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
            describe(&issue),
            "Block \"wait\", field \"timeout\": must be from 1ms to 10m."
        );
        assert_eq!(
            describe(
                &Issue::error("flow.json.syntax")
                    .with("line", 3)
                    .with("column", 7)
            ),
            "Test: not valid JSON at line 3, column 7."
        );
        assert_eq!(
            describe(&Issue::error("flow.field.unknown").with("field", "colour")),
            "Field \"colour\": not a known field."
        );
    }
}
