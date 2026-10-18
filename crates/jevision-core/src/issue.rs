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
        "flow.reference.thresholdNeedsSimilar" => {
            "\"threshold\" works only with \"compare\": \"similar\".".to_owned()
        }
        "flow.field.empty" => "can't be empty.".to_owned(),
        "flow.field.range" => format!("must be from {} to {}.", p("min"), p("max")),
        "flow.app.invalid" => format!(
            "\"{}\" isn't an app bundle identifier like com.apple.TextEdit.",
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
            "\"{}\" isn't a valid placeholder; use ${{secret.NAME}}, ${{env.NAME}} or ${{data.COLUMN}}.",
            p("placeholder")
        ),
        "flow.keys.invalid" => format!(
            "\"{}\" isn't a key combination like cmd+shift+s.",
            p("value")
        ),
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
        "flow.loop.noReturn" => "the blocks inside don't lead back to this loop.".to_owned(),
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
        place.push("Test".to_owned());
    }
    let place = place.join(", ");
    let mut first = place.chars();
    let place = first
        .next()
        .map(|c| c.to_uppercase().chain(first).collect::<String>())
        .unwrap_or_default();
    format!("{place}: {text}")
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
