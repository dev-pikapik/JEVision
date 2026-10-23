//! JSON Schemas of the files this crate reads and writes, made from the types that hold them and
//! kept in `docs/schemas`: the parts of the types the derive can't see are written here.

use std::borrow::Cow;

use jevision_schema::{Kind, SEMVER_PATTERN, Validator, without_null};
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde_json::{Value, json};

use crate::board::{Board, ENDS, OURS, SIDES};
use crate::element::{ELEMENT_ROLES, ELEMENT_STATES, ElementCheck, ElementQuery};
use crate::model::{
    AI_PROVIDERS, Attachment, Compare, DEFAULT_DATE_FORMAT, ExpectStatus, Flow, Modifiers, Rect,
    Reference,
};
use crate::package::{PACKAGE_FORMAT, PackageLock, Version, VersionReq};
use crate::project::PROJECT_FORMAT;
use crate::resources::{RESOURCE_KINDS, ResourceMeta};
use crate::schedule::{SCHEDULE_FORMAT, TimeOfDay};
use crate::{DEFAULT_TASK_TIMEOUT_MS, DEFAULT_WAIT_TIMEOUT_MS, ENVIRONMENTS_FORMAT};

// The formats each kind of file can have: the constants of their readers, or for tests and
// labels a list that the tests below check against their readers.
const TEST: Kind = Kind {
    name: "test",
    title: "JEVision test",
    description: "A JEVision test: a flowchart of blocks that drive and check an application. \
        Tests are `*.flow.json` files; `*.block.json` files are blocks of the project's own, \
        `*.template.json` files are templates, and `flow.json` is the test inside a community \
        block.",
    formats: &[1, 2],
    header: true,
};
const JEVMETA: Kind = Kind {
    name: "jevmeta",
    title: "JEVision resource label",
    description: "The label of an image, table or other file of a project, which a test uses \
        to name the file as `res:` and the label's `id` instead of by its path. It sits next to \
        the file, named after it with `.jevmeta` added, such as `login.png.jevmeta`: move or \
        rename the two together. JEVision writes it itself.",
    formats: &[1],
    header: true,
};
const PROJECT: Kind = Kind {
    name: "project",
    title: "JEVision project settings",
    description: "Settings a project shares with everyone who runs its tests, in \
        `jevision.project.json` in the project folder: retries, tests in quarantine, areas \
        covered in every screenshot, the AI the project asks and the folders whose tests run \
        side by side. API keys never go in this file.",
    formats: &[PROJECT_FORMAT],
    header: false,
};
const ENVIRONMENTS: Kind = Kind {
    name: "environments",
    title: "JEVision environments",
    description: "Environments of a project, such as `staging` and `production`, in \
        `jevision.environments.json` in the project folder: named sets of values that tests \
        read as `${env.NAME}`, like the address of a server. Secrets never go here; tests read \
        them as `${secret.NAME}`.",
    formats: &[ENVIRONMENTS_FORMAT],
    header: false,
};
const SCHEDULES: Kind = Kind {
    name: "schedules",
    title: "JEVision schedules",
    description: "When the tests of a project run by themselves, in `jevision.schedule.json` in \
        the project folder: each schedule names the tests, the days or hours, and the time of \
        day.",
    formats: &[SCHEDULE_FORMAT],
    header: false,
};
const SCHEDULE_HISTORY: Kind = Kind {
    name: "schedule-history",
    title: "JEVision schedule history",
    description: "What came of the scheduled runs of a project on this computer, in \
        `.jevision/schedule-history.json`. JEVision keeps it itself; it isn't shared.",
    formats: &[crate::SCHEDULE_HISTORY_FORMAT],
    header: false,
};
const BOARD: Kind = Kind {
    name: "board",
    title: "JEVision board",
    description: "A board of a project: a `*.canvas` file in its `boards` folder, in the open \
        JSON Canvas 1.0 format (jsoncanvas.org), so Obsidian opens it too. A test is a `file` \
        node that points at its `*.flow.json`; what only JEVision uses sits under `jevision` \
        keys. Keys JEVision doesn't know are kept as they are.",
    formats: &[],
    header: false,
};
const BLOCK: Kind = Kind {
    name: "block",
    title: "JEVision block package",
    description: "A block that people share: `block.json` in the folder `ID.jevblock`, next to \
        `flow.json` with the steps of the block.",
    formats: &[PACKAGE_FORMAT],
    header: false,
};
const BLOCKS_LOCK: Kind = Kind {
    name: "blocks-lock",
    title: "JEVision installed blocks",
    description: "Where the shared blocks installed in a project came from, in \
        `jevision.blocks.json` in the project folder, so the whole team can update them. \
        JEVision keeps it itself.",
    formats: &[PACKAGE_FORMAT],
    header: false,
};

pub(crate) fn wait_timeout() -> u64 {
    DEFAULT_WAIT_TIMEOUT_MS
}

pub(crate) fn task_timeout() -> u64 {
    DEFAULT_TASK_TIMEOUT_MS
}

pub(crate) fn date_format() -> String {
    DEFAULT_DATE_FORMAT.to_owned()
}

/// A time as tests write it.
pub(crate) fn duration(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "A time: a whole number and `ms`, `s` or `m`, such as `500ms`, `10s` or \
            `2m`.",
        "type": "string",
        "pattern": "^[0-9]+(ms|s|m)$",
    })
}

fn canvas_point() -> Value {
    json!({
        "type": "object",
        "properties": { "x": { "type": "number" }, "y": { "type": "number" } },
        "required": ["x", "y"],
        "additionalProperties": false,
    })
}

/// Places of blocks on the canvas, by block id.
pub(crate) fn canvas_points(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "type": "object",
        "additionalProperties": canvas_point(),
    })
}

/// A frame on the canvas, in canvas points.
pub(crate) fn canvas_rect(_: &mut SchemaGenerator) -> Schema {
    let number = json!({ "type": "number" });
    json_schema!({
        "description": "Where it is drawn, in canvas points from the top-left corner.",
        "type": "object",
        "properties": { "x": number, "y": number, "width": number, "height": number },
        "required": ["x", "y", "width", "height"],
        "additionalProperties": false,
    })
}

/// Says that a path to a file of the test may be written as a label of the file instead.
pub(crate) fn path_or_label(schema: &mut Schema) {
    const LABEL: &str = "It may instead be `res:` and the 16 lowercase hex digits of a label, \
        which finds the file wherever it moves inside the project. A test that names a file by \
        its label is saved in format 2.";
    let text = match schema.get("description").and_then(Value::as_str) {
        Some(text) => format!("{text} {LABEL}"),
        None => LABEL.to_owned(),
    };
    schema.insert("description".into(), text.into());
}

impl JsonSchema for Modifiers {
    fn schema_name() -> Cow<'static, str> {
        "Modifiers".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let all = Modifiers {
            command: true,
            shift: true,
            option: true,
            control: true,
        };
        json_schema!({
            "description": "Keys held down: a list of `cmd`, `shift`, `option` and `ctrl`.",
            "type": "array",
            "items": { "enum": all.names() },
        })
    }
}

impl JsonSchema for ExpectStatus {
    fn schema_name() -> Cow<'static, str> {
        "ExpectStatus".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "Status the answer must have: `success` for any of 200–299, `any`, or \
                one status code.",
            "oneOf": [
                { "enum": ["success", "any"] },
                { "type": "integer", "minimum": 0, "maximum": u16::MAX },
            ],
        })
    }
}

impl JsonSchema for Attachment {
    fn schema_name() -> Cow<'static, str> {
        "Attachment".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let one = |key: &str, description: &str| {
            json!({
                "type": "object",
                "properties": { key: { "type": "string", "description": description } },
                "required": [key],
                "additionalProperties": false,
            })
        };
        json_schema!({
            "description": "A file or link that comes with a note.",
            "oneOf": [
                one("file", "Name of a file saved next to the test."),
                one("link", "A web address, such as a page or a GitHub issue."),
            ],
        })
    }
}

impl JsonSchema for ElementQuery {
    fn schema_name() -> Cow<'static, str> {
        "ElementQuery".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "minLength": 1 },
                "role": { "enum": ELEMENT_ROLES.iter().map(|role| role.name).collect::<Vec<_>>() },
                "title": { "type": "string", "minLength": 1 },
                "index": { "type": "integer", "minimum": 0, "maximum": u32::MAX },
            },
            "anyOf": [{ "required": ["id"] }, { "required": ["role"] }, { "required": ["title"] }],
            "additionalProperties": false,
        })
    }
}

impl JsonSchema for ElementCheck {
    fn schema_name() -> Cow<'static, str> {
        "ElementCheck".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let mut schema = ElementQuery::json_schema(generator).to_value();
        schema["properties"]["is"] = json!({ "enum": ELEMENT_STATES });
        schema["properties"]["to"] = json!({ "type": "string" });
        schema["required"] = json!(["is"]);
        schema["allOf"] = json!([{
            "if": { "properties": { "is": { "enum": ["valueEquals", "valueContains", "titleEquals", "titleContains"] } } },
            "then": { "required": ["to"] },
            "else": { "not": { "required": ["to"] } },
        }]);
        schema.try_into().expect("an object schema")
    }
}

impl JsonSchema for Reference {
    fn schema_name() -> Cow<'static, str> {
        "Reference".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let rect = generator.subschema_for::<Rect>().to_value();
        let with = |mut schema: Value, description: &str| {
            schema["description"] = description.into();
            schema
        };
        let mut image = json_schema!({
            "description": "Path of the reference image, relative to the folder of the test. A \
                check by meaning with `expect` may leave it out.",
            "type": "string",
        });
        path_or_label(&mut image);
        let meaning = json!({
            "properties": { "compare": { "const": "meaning" } },
            "required": ["compare"],
        });
        json_schema!({
            "description": "A reference image (how the screen should look) and how to compare the \
                screen with it.",
            "type": "object",
            "properties": {
                "image": image,
                "compare": {
                    "description": "`similar`: at least `threshold` alike; `exact`: pixel by \
                        pixel; `meaning`: a vision model judges that the screen shows what \
                        `expect` says, or means the same as the image when there is no `expect`.",
                    "enum": ["similar", "exact", "meaning"],
                    "default": "similar",
                },
                "threshold": {
                    "description": "How alike the screen must be for `similar`, above 0 and at \
                        most 1.",
                    "type": "number",
                    "exclusiveMinimum": 0,
                    "maximum": 1,
                    "default": Compare::DEFAULT_THRESHOLD,
                },
                "expect": {
                    "description": "For `meaning`: what the screen must show, in words.",
                    "type": "string",
                },
                "provider": {
                    "description": "For `meaning`: the AI provider to ask instead of the one \
                        chosen in the settings.",
                    "enum": AI_PROVIDERS,
                },
                "model": {
                    "description": "For `meaning`: the model of that provider to ask instead of \
                        its chosen one.",
                    "type": "string",
                },
                "area": with(rect.clone(), "Part of the image to compare; without it, the \
                    whole image."),
                "ignore": {
                    "description": "Parts left out of the comparison: clocks, avatars, counters.",
                    "type": "array",
                    "items": rect,
                },
                "hide": {
                    "description": "Parts painted over in grey before a screenshot is sent \
                        anywhere or stored in a report.",
                    "type": "array",
                    "items": rect,
                },
            },
            "additionalProperties": false,
            "if": {
                "properties": {
                    "compare": { "const": "meaning" },
                    "expect": { "pattern": "\\S" },
                },
                "required": ["compare", "expect"],
            },
            "else": { "required": ["image"] },
            "dependentSchemas": {
                "threshold": { "properties": { "compare": { "const": "similar" } } },
                "expect": meaning,
                "provider": meaning,
                "model": meaning,
            },
        })
    }
}

/// The names of environments: letters, digits, `-` and `_`.
pub(crate) fn environment_name() -> String {
    format!("^[A-Za-z0-9_-]{{1,{}}}$", crate::env::MAX_NAME_LEN)
}

/// A path inside the project folder that ends in `ending`: parts joined by `/`, none of them
/// empty, `.` or `..`, and no `\`.
pub(crate) fn inside_project(ending: &str) -> String {
    const PART: &str = r"([^./\\][^/\\]*|\.[^./\\][^/\\]*|\.\.[^/\\]+)";
    if ending.is_empty() {
        format!("^{PART}(/{PART})*$")
    } else {
        format!(r"^({PART}/)*[^/\\]*{ending}$")
    }
}

impl JsonSchema for ResourceMeta {
    fn schema_name() -> Cow<'static, str> {
        "ResourceMeta".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let kinds: Vec<&str> = RESOURCE_KINDS.iter().map(|(name, _)| *name).collect();
        json_schema!({
            "type": "object",
            "properties": {
                "id": {
                    "description": "The label, 16 lowercase hex digits: a test names the file \
                        as `res:` and the label.",
                    "type": "string",
                    "pattern": "^[0-9a-f]{16}$",
                },
                "kind": {
                    "description": "What the file is: an `image`, a `table` (a CSV or TSV \
                        file) or another `file`.",
                    "enum": kinds,
                },
                "hash": {
                    "description": "`sha256:` and the SHA-256 of the file's content in 64 \
                        lowercase hex digits. With `size`, it finds the file again when it was \
                        moved without its `.jevmeta`.",
                    "type": "string",
                    "pattern": "^sha256:[0-9a-f]{64}$",
                },
                "size": {
                    "description": "Size of the file in bytes.",
                    "type": "integer",
                    "minimum": 0,
                },
                "name": {
                    "description": "The name the file had when it got the label, without its \
                        folder; shown when the file can't be found.",
                    "type": "string",
                    "pattern": r"^[^/\\\x00]+$",
                    "not": { "enum": [".", ".."] },
                },
            },
            "required": ["id", "kind", "hash", "size", "name"],
            "additionalProperties": false,
        })
    }
}

impl JsonSchema for TimeOfDay {
    fn schema_name() -> Cow<'static, str> {
        "TimeOfDay".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A time of day as `HH:MM`, from `00:00` to `23:59`.",
            "type": "string",
            "pattern": "^([01][0-9]|2[0-3]):[0-5][0-9]$",
        })
    }
}

impl JsonSchema for Version {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        "Version".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({ "type": "string", "pattern": SEMVER_PATTERN })
    }
}

impl JsonSchema for VersionReq {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        "VersionReq".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({ "type": "string", "pattern": r"\S" })
    }
}

impl JsonSchema for PackageLock {
    fn schema_name() -> Cow<'static, str> {
        "PackageLock".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "object",
            "properties": {
                "sources": {
                    "description": "The address each package was installed from, by its id. A \
                        package installed from a file has none.",
                    "type": "object",
                    "additionalProperties": { "type": "string" },
                },
                "versions": {
                    "description": "The version each package had when it was installed, by its \
                        id.",
                    "type": "object",
                    "additionalProperties": { "type": "string", "pattern": SEMVER_PATTERN },
                },
            },
            "additionalProperties": false,
        })
    }
}

impl JsonSchema for Board {
    fn schema_name() -> Cow<'static, str> {
        "Board".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let id = json!({
            "description": "Unique among the nodes and arrows of the board.",
            "type": "string",
            "pattern": r"\S",
        });
        let color = json!({
            "description": "One of the preset colors `1` to `6` (red, orange, yellow, green, \
                cyan, purple), or `#rrggbb`; empty or left out for none.",
            "type": "string",
            "pattern": "^([1-6]|#[0-9A-Fa-f]{6})?$",
        });
        let number = |description: &str| json!({ "description": description, "type": "number" });
        let size = |description: &str| json!({ "description": description, "type": "number", "minimum": 0.5 });
        let ours = |properties: Value| {
            json!({
                "description": "What only JEVision uses.",
                "type": "object",
                "properties": properties,
                "additionalProperties": false,
            })
        };
        let flag = |description: &str| json!({ "description": description, "type": "boolean" });
        let text = |description: &str| json!({ "description": description, "type": "string" });
        let of_type = |kind: &str, then: Value| {
            json!({
                "if": { "properties": { "type": { "const": kind } }, "required": ["type"] },
                "then": then,
            })
        };
        let mut sides = vec![""];
        sides.extend(SIDES);
        let mut ends = vec![""];
        ends.extend(ENDS);
        let node = json!({
            "description": "A thing on the board. Keys JEVision doesn't know are kept.",
            "type": "object",
            "properties": {
                "id": id,
                "type": {
                    "description": "`text` for a note in Markdown, `file` for a test, another \
                        board or a picture, `link` for a web page and `group` for a frame.",
                    "enum": ["text", "file", "link", "group"],
                },
                "x": number("Board points from the board's origin, to the right."),
                "y": number("Board points from the board's origin, downwards."),
                "width": size("Board points; rounded to whole ones."),
                "height": size("Board points; rounded to whole ones."),
                "color": color,
                OURS: { "type": "object" },
            },
            "required": ["id", "type", "x", "y", "width", "height"],
            "allOf": [
                of_type("text", json!({
                    "properties": {
                        "text": text("The note, in Markdown."),
                        OURS: { "maxProperties": 0 },
                    },
                    "required": ["text"],
                })),
                of_type("file", json!({
                    "properties": {
                        "file": {
                            "description": "A file of the project, relative to the project \
                                folder with `/` between folders: a test when it ends in \
                                `.flow.json`, another board when it ends in `.canvas`.",
                            "type": "string",
                            "pattern": r"\S",
                        },
                        OURS: ours(json!({
                            "open": flag("The card shows the blocks of the test inside it."),
                        })),
                    },
                    "required": ["file"],
                })),
                of_type("link", json!({
                    "properties": {
                        "url": text("The address of the web page."),
                        OURS: { "maxProperties": 0 },
                    },
                    "required": ["url"],
                })),
                of_type("group", json!({
                    "properties": {
                        "label": text("The title of the frame."),
                        OURS: ours(json!({
                            "collapsed": flag("Folded to its title bar; what lies inside isn't \
                                shown."),
                            "folder": text("The test folder it holds on the board of all \
                                tests, relative to the project; empty for the project folder."),
                        })),
                    },
                })),
            ],
        });
        let edge = json!({
            "description": "An arrow between two nodes. Keys JEVision doesn't know are kept.",
            "type": "object",
            "properties": {
                "id": id,
                "fromNode": text("The id of the node the arrow leaves."),
                "fromSide": {
                    "description": "The side of the node the arrow leaves; empty or left out \
                        for the side that faces the other node.",
                    "enum": sides,
                },
                "fromEnd": {
                    "description": "`arrow` for an arrowhead at the start; there is none \
                        unless the file says so.",
                    "enum": ends,
                },
                "toNode": text("The id of the node the arrow points at."),
                "toSide": {
                    "description": "The side of the node the arrow arrives at; empty or left \
                        out for the side that faces the other node.",
                    "enum": sides,
                },
                "toEnd": {
                    "description": "`none` for no arrowhead at the end; there is one unless \
                        the file says not.",
                    "enum": ends,
                },
                "label": text("Text on the arrow."),
                "color": color,
            },
            "required": ["id", "fromNode", "toNode"],
        });
        json_schema!({
            "type": "object",
            "properties": {
                "nodes": {
                    "description": "In drawing order: later nodes are drawn over earlier ones, \
                        so frames come before what they hold.",
                    "type": ["array", "null"],
                    "items": node,
                },
                "edges": { "type": ["array", "null"], "items": edge },
                OURS: ours(json!({
                    "allTests": flag("Every test of the project is on this board: new tests \
                        appear on it by themselves, in the frame of their folder, and deleted \
                        ones go away."),
                })),
            },
        })
    }
}

mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::compat::TEST_SCHEMA;
    use crate::decode::{META_FORMAT, OLDEST_FORMAT};
    use crate::resources::{META_SCHEMA, META_SUFFIX};
    use crate::{
        AiChoice, BlockManifest, Dependency, Environments, FolderRunMode, PackageInput,
        ProjectSettings, Quarantined, Repeat, RunMode, Schedule, ScheduleHistory, ScheduleTests,
        ScheduledOutcome, ScheduledRun, SkipReason, Weekday,
    };

    /// Every block type with every setting it has, and a file named by its label, as the reader
    /// takes it.
    const EVERY_BLOCK: &str = r#"{
  "$schema": "https://dev-pikapik.github.io/JEVision/schemas/test.schema.json",
  "format": 2,
  "minVersion": "0.1.0",
  "name": "Every block",
  "description": "Uses every block with every setting.",
  "icon": "star",
  "app": "com.apple.TextEdit",
  "testType": "smoke",
  "priority": "high",
  "status": "ready",
  "tags": ["all"],
  "preconditions": "Signed in",
  "author": "QA",
  "platform": "macOS 26",
  "link": "https://example.com/issues/1",
  "flaky": true,
  "retries": 2,
  "dependsOn": ["sign-in.flow.json"],
  "start": "open",
  "blocks": {
    "open": { "type": "openApp", "app": "com.apple.TextEdit", "resetState": true, "windowSize": { "width": 800, "height": 600 }, "windowPosition": { "x": -100, "y": 50 }, "next": "file" },
    "file": { "type": "openFile", "files": ["fixtures/a.txt", "https://example.com"], "app": "com.apple.TextEdit", "newInstance": true, "closeAfter": true, "next": "click" },
    "click": { "type": "click", "target": { "image": "refs/menu.png" }, "button": "right", "double": true, "modifiers": ["cmd", "shift", "option", "ctrl"], "waitForChange": true, "next": "hover" },
    "hover": { "type": "hover", "target": { "description": "the blue Save button" }, "modifiers": ["option"], "waitForChange": true, "next": "drag" },
    "drag": { "type": "drag", "from": { "point": { "x": 0.1, "y": 0.2 } }, "to": { "area": "0.5,0.5,0.1,0.1" }, "button": "middle", "modifiers": ["shift"], "waitForChange": true, "next": "scroll" },
    "scroll": { "type": "scroll", "direction": "down", "amount": 240, "unit": "pixels", "at": { "point": { "x": 0.5, "y": 0.6 } }, "modifiers": ["cmd"], "waitForChange": true, "next": "type" },
    "type": { "type": "type", "text": "${data.name}", "into": { "image": "refs/field.png" }, "next": "keys" },
    "keys": { "type": "pressKeys", "keys": "cmd+s", "next": "appears" },
    "appears": { "type": "waitFor", "until": { "appears": { "image": "refs/done.png", "compare": "exact", "area": { "x": 0, "y": 0, "width": 1, "height": 0.5 }, "ignore": [{ "x": 0.9, "y": 0, "width": 0.1, "height": 0.1 }], "hide": [{ "x": 0, "y": 0.9, "width": 0.2, "height": 0.1 }] } }, "timeout": "2m", "next": "gone" },
    "gone": { "type": "waitFor", "until": { "disappears": { "image": "refs/spinner.png" } }, "next": "still" },
    "still": { "type": "waitFor", "until": "screenStable", "next": "pause" },
    "pause": { "type": "pause", "duration": "500ms", "next": "check" },
    "check": { "type": "check", "reference": { "image": "refs/done.png", "compare": "similar", "threshold": 0.8 }, "next": "branch" },
    "branch": { "type": "if", "reference": { "compare": "meaning", "expect": "A dialog asks whether to save", "provider": "openai", "model": "gpt-5" }, "then": "again", "elseIf": [{ "value": { "of": "${var.status}", "is": "equals", "to": "200" }, "then": "rows" }, { "reference": { "image": "refs/error.png", "compare": "meaning" } }], "else": "decide" },
    "decide": { "type": "if", "value": { "of": "${var.count}", "is": "greaterThan", "to": "1" }, "then": "again" },
    "again": { "type": "repeat", "times": 3, "body": "keys", "next": "rows" },
    "rows": { "type": "useData", "file": "data/users.csv", "body": "type", "next": "run" },
    "run": { "type": "runTest", "test": "blocks/org.example.add.jevblock/flow.json", "package": { "version": "1.2.0", "source": "https://github.com/example/add" }, "inputs": { "text": "hello" }, "next": "script" },
    "script": { "type": "script", "language": "python3", "code": "print(1)", "inputs": [{ "name": "A", "value": "1" }, { "name": "PIC", "type": "image", "value": "refs/a.png" }], "outputs": [{ "name": "B", "type": "number" }], "timeout": "45s", "next": "command" },
    "command": { "type": "command", "command": "ls", "folder": "fixtures", "exitCode": 1, "saveOutput": "listing", "timeout": "1500ms", "next": "post" },
    "post": { "type": "http", "method": "POST", "url": "https://example.com/api", "headers": [{ "name": "Accept", "value": "application/json" }], "body": "{}", "expectStatus": 201, "save": [{ "name": "id", "from": "body.id" }], "timeout": "5s", "next": "get" },
    "get": { "type": "http", "url": "https://example.com", "expectStatus": "any", "next": "report" },
    "report": { "type": "checkFile", "path": "~/Downloads/report.txt", "until": { "contains": "Total" }, "timeout": "20s", "next": "removed" },
    "removed": { "type": "checkFile", "path": "out.txt", "until": "missing", "next": "count" },
    "count": { "type": "database", "file": "app.sqlite", "query": "select count(*) from t", "saveAs": "count", "expect": { "rows": 1 }, "timeout": "3s", "next": "one" },
    "one": { "type": "database", "file": "app.sqlite", "query": "select 1", "expect": { "value": "1" }, "next": "copy" },
    "copy": { "type": "clipboard", "action": "put", "text": "hello", "next": "picture" },
    "picture": { "type": "clipboard", "action": "put", "image": "res:3f9c2a7b81d04e65", "next": "paste" },
    "paste": { "type": "clipboard", "action": "read", "saveAs": "copied", "content": "files", "next": "copied" },
    "copied": { "type": "clipboard", "action": "check", "is": "contains", "to": "hello", "timeout": "2s", "next": "date" },
    "date": { "type": "dateTime", "saveAs": "today", "format": "%d.%m.%Y", "language": "ru", "timeZone": "Europe/Berlin", "shift": { "amount": -2, "unit": "days" } }
  },
  "canvas": {
    "positions": { "open": { "x": 0, "y": 0 } },
    "sections": { "start": { "title": "Start", "frame": { "x": 0, "y": 0, "width": 400, "height": 300 } } },
    "notes": {
      "why": {
        "text": "**Why** this test exists",
        "author": "QA",
        "time": "2026-10-20T10:00:00Z",
        "anchor": { "type": "block", "block": "open" },
        "attachments": [{ "file": "log.txt" }, { "link": "https://example.com" }],
        "replies": [{ "id": "first", "author": "Dev", "time": "2026-10-20T11:00:00Z", "text": "Fixed" }],
        "resolved": { "by": "QA", "time": "2026-10-20T12:00:00Z" },
        "frame": { "x": 10, "y": 10, "width": 200, "height": 100 }
      }
    }
  }
}"#;

    fn root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    /// Every file under `dir` whose name ends in one of `endings`.
    fn files(dir: &Path, endings: &[&str], found: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if path.is_dir() {
                files(&path, endings, found);
            } else if endings.iter().any(|end| name.ends_with(end)) {
                found.push(path);
            }
        }
    }

    fn read(path: &Path) -> Value {
        serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
    }

    /// `file` without `key`.
    fn without(file: &Value, key: &str) -> Value {
        let mut changed = file.clone();
        changed.as_object_mut().unwrap().remove(key);
        changed
    }

    fn passes(validator: &Validator, file: &Value, what: &str) {
        if let Err(problem) = validator.check(file) {
            panic!("{what} doesn't fit its schema: {problem}");
        }
    }

    fn fails(validator: &Validator, file: &Value, what: &str) {
        assert!(
            validator.check(file).is_err(),
            "the schema lets {what} through"
        );
    }

    fn test_schema() -> Value {
        without_null(TEST.schema::<Flow>())
    }

    #[test]
    fn the_published_schemas_are_up_to_date() {
        for (kind, schema) in schemas() {
            kind.check_published(&schema);
        }
    }

    #[test]
    fn tests_fit_the_test_schema() {
        assert_eq!(TEST.url(), TEST_SCHEMA);
        let formats: Vec<u32> = (OLDEST_FORMAT..=crate::FORMAT_VERSION).collect();
        assert_eq!(TEST.formats, formats);
        let tests = Validator::new(&test_schema());
        let mut found = Vec::new();
        files(&root().join("examples"), &["flow.json"], &mut found);
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/test");
        files(&fixtures, &["flow.json"], &mut found);
        files(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("templates"),
            &[".template.json"],
            &mut found,
        );
        assert!(found.len() > 5, "{found:?}");
        for path in &found {
            passes(&tests, &read(path), &path.display().to_string());
        }

        let flow = crate::parse(EVERY_BLOCK).expect("the reader takes the sample");
        passes(
            &tests,
            &serde_json::from_str(EVERY_BLOCK).unwrap(),
            "the sample",
        );
        let canonical = serde_json::from_str(&crate::to_canonical_json(&flow)).unwrap();
        passes(&tests, &canonical, "the sample as JEVision writes it");

        let sample: Value = serde_json::from_str(EVERY_BLOCK).unwrap();
        let bare = without(&without(&sample, "$schema"), "minVersion");
        assert!(crate::parse(&bare.to_string()).is_ok());
        passes(&tests, &bare, "a test without `$schema` and `minVersion`");
        // The reader takes a label in a test of format 1 too.
        let first = with(&sample, "/format", 1.into());
        assert!(crate::parse(&first.to_string()).is_ok());
        passes(&tests, &first, "a test of format 1 with a label");
    }

    #[test]
    fn the_sample_uses_every_block_and_setting() {
        let schema = test_schema();
        let sample: Value = serde_json::from_str(EVERY_BLOCK).unwrap();
        for variant in schema["$defs"]["BlockKind"]["oneOf"].as_array().unwrap() {
            let kind = variant["properties"]["type"]["const"].as_str().unwrap();
            let used: Vec<&String> = sample["blocks"]
                .as_object()
                .unwrap()
                .values()
                .filter(|block| block["type"] == kind)
                .flat_map(|block| block.as_object().unwrap().keys())
                .collect();
            let branches = variant["oneOf"].as_array().into_iter().flatten();
            for key in [variant]
                .into_iter()
                .chain(branches)
                .flat_map(|s| s["properties"].as_object().unwrap().keys())
            {
                let last = kind == "dateTime" && key == "next";
                assert!(
                    last || used.contains(&key),
                    "no `{kind}` block sets `{key}`"
                );
            }
        }
    }

    #[test]
    fn the_test_schema_turns_away_what_the_reader_does() {
        let tests = Validator::new(&test_schema());
        let sample: Value = serde_json::from_str(EVERY_BLOCK).unwrap();
        let mut misspelled = sample.clone();
        misspelled["nmae"] = "Typo".into();
        let mut in_block = sample.clone();
        in_block["blocks"]["click"]["dubble"] = true.into();
        let mut wrong_type = sample.clone();
        wrong_type["retries"] = "2".into();
        let mut wrong_in_block = sample.clone();
        wrong_in_block["blocks"]["again"]["times"] = "3".into();
        let mut null = sample.clone();
        null["description"] = Value::Null;
        for (file, what) in [
            (misspelled, "a misspelled key"),
            (in_block, "a misspelled key in a block"),
            (wrong_type, "a number written as text"),
            (wrong_in_block, "a number written as text in a block"),
            (null, "null for a key that may be left out"),
            (without(&sample, "format"), "a test without format"),
            (with(&sample, "/format", 3.into()), "a newer format"),
            (with(&sample, "/format", 0.into()), "format 0"),
            (
                with(&sample, "/minVersion", "1.4".into()),
                "a version that is no version",
            ),
            (
                with(&sample, "/$schema", 1.into()),
                "a number for the address of the schema",
            ),
        ] {
            assert!(
                crate::parse(&file.to_string()).is_err(),
                "the reader takes {what}"
            );
            fails(&tests, &file, what);
        }
    }

    #[test]
    fn labels_fit_the_jevmeta_schema() {
        assert_eq!(JEVMETA.url(), META_SCHEMA);
        let formats: Vec<u32> = (OLDEST_FORMAT..=META_FORMAT).collect();
        assert_eq!(JEVMETA.formats, formats);
        let schema = validator(&JEVMETA);
        let reads = |file: &Value| crate::parse_meta(&file.to_string()).is_ok();
        let mut found = Vec::new();
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        files(&fixtures, &[META_SUFFIX], &mut found);
        assert!(found.len() > 5, "{found:?}");
        for path in &found {
            let file = read(path);
            assert!(reads(&file), "the reader doesn't take {}", path.display());
            passes(&schema, &file, &path.display().to_string());
        }

        let project = folder("jevmeta");
        let mut written = Vec::new();
        for name in ["login.png", "users.csv", "notes.txt"] {
            fs::write(project.join(name), name).unwrap();
            crate::label_resource(&project, &project.join(name)).unwrap();
            written.push(read(&project.join(format!("{name}{META_SUFFIX}"))));
        }
        let kinds: Vec<&Value> = written.iter().map(|file| &file["kind"]).collect();
        assert_eq!(kinds, ["image", "table", "file"]);
        for file in &written {
            passes(&schema, file, "a label as JEVision writes it");
        }
        let file = &written[0];
        let bare = with(&without(file, "$schema"), "/minVersion", "0.1.0".into());
        assert!(reads(&bare));
        passes(
            &schema,
            &bare,
            "a label with `minVersion` and without `$schema`",
        );

        let missing = ["format", "id", "kind", "hash", "size", "name"]
            .map(|key| (without(file, key), format!("a label without `{key}`")));
        let wrong = [
            (with(file, "/label", "x".into()), "a key it doesn't know"),
            (with(file, "/name", Value::Null), "null for a name"),
            (with(file, "/format", 2.into()), "a newer format"),
            (
                with(file, "/minVersion", "1.4".into()),
                "a version that is no version",
            ),
            (
                with(file, "/$schema", 1.into()),
                "a number for the address of the schema",
            ),
            (
                with(file, "/id", "3F9C2A7B81D04E65".into()),
                "an id in capitals",
            ),
            (
                with(file, "/id", "3f9c2a7b81d04e6".into()),
                "an id one digit short",
            ),
            (with(file, "/kind", "picture".into()), "an unknown kind"),
            (
                with(file, "/hash", "md5:0cc175b9c0f1b6a831c399e269772661".into()),
                "a hash that is no SHA-256",
            ),
            (with(file, "/size", "4".into()), "text for the size"),
            (with(file, "/size", (-1).into()), "a negative size"),
            (
                with(file, "/name", "images/login.png".into()),
                "a name with a folder",
            ),
            (
                with(file, "/name", "..".into()),
                "the parent folder for a name",
            ),
            (with(file, "/name", "".into()), "an empty name"),
        ]
        .map(|(file, what)| (file, what.to_owned()));
        for (file, what) in wrong.iter().chain(&missing) {
            assert!(!reads(file), "the reader takes {what}");
            fails(&schema, file, what);
        }
    }

    /// The schema of each kind of file this crate reads.
    fn schemas() -> Vec<(&'static Kind, Value)> {
        vec![
            (&TEST, test_schema()),
            (&JEVMETA, JEVMETA.schema::<ResourceMeta>()),
            (&PROJECT, PROJECT.schema::<crate::project::File>()),
            (&ENVIRONMENTS, ENVIRONMENTS.schema::<crate::env::File>()),
            (&SCHEDULES, SCHEDULES.schema::<crate::schedule::File>()),
            (
                &SCHEDULE_HISTORY,
                SCHEDULE_HISTORY.schema::<ScheduleHistory>(),
            ),
            (&BOARD, BOARD.schema::<Board>()),
            (&BLOCK, BLOCK.schema::<BlockManifest>()),
            (&BLOCKS_LOCK, BLOCKS_LOCK.schema::<PackageLock>()),
        ]
    }

    fn validator(kind: &Kind) -> Validator {
        let (_, schema) = schemas()
            .into_iter()
            .find(|(k, _)| k.name == kind.name)
            .unwrap();
        Validator::new(&schema)
    }

    fn folder(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("jevision-schema-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A file of settings as JEVision writes it, and what its reader takes.
    struct Sample {
        kind: &'static Kind,
        file: Value,
        reads: fn(&str) -> bool,
        /// The reader turns away keys it doesn't know, and a file without `format`.
        strict: bool,
        /// A key at the top whose value must not be text.
        not_text: &'static str,
        /// Files both the reader and the schema turn away, and why.
        wrong: Vec<(Value, &'static str)>,
    }

    /// `file` with `value` at `pointer`, a key it may not have yet.
    fn with(file: &Value, pointer: &str, value: Value) -> Value {
        let mut changed = file.clone();
        let (parent, key) = pointer.rsplit_once('/').unwrap();
        match changed.pointer_mut(parent).unwrap() {
            Value::Array(items) => items[key.parse::<usize>().unwrap()] = value,
            object => object[key] = value,
        }
        changed
    }

    fn settings_samples() -> Vec<Sample> {
        let project = ProjectSettings {
            clean_home: true,
            retries: Some(2),
            quarantine: vec![
                Quarantined {
                    test: "checkout/pay.flow.json".into(),
                    reason: Some("Bug 12".into()),
                    until: Some("2026-11-01".into()),
                },
                Quarantined {
                    test: "sign-in.flow.json".into(),
                    reason: None,
                    until: None,
                },
            ],
            suggest_quarantine_after: Some(5),
            local_only: true,
            hide: vec![Rect {
                x: 0.1,
                y: 0.1,
                width: 0.5,
                height: 0.2,
            }],
            ai_screen: Some(AiChoice {
                provider: "jev".into(),
                model: String::new(),
            }),
            ai_text: Some(AiChoice {
                provider: "anthropic".into(),
                model: "claude-sonnet".into(),
            }),
            run_modes: vec![
                FolderRunMode {
                    folder: ".".into(),
                    mode: RunMode::Sequential,
                },
                FolderRunMode {
                    folder: "checkout".into(),
                    mode: RunMode::Parallel { limit: 4 },
                },
            ],
        };
        let project_text = project.to_json();
        assert_eq!(ProjectSettings::parse(&project_text).unwrap(), project);
        let project: Value = serde_json::from_str(&project_text).unwrap();

        let dir = folder("environments");
        for (name, server) in [
            ("staging", "https://staging.acme.com"),
            ("production", "https://acme.com"),
        ] {
            let values = BTreeMap::from([("SERVER".to_owned(), server.to_owned())]);
            Environments::add(&dir, name, values).unwrap();
        }
        let mut environments = read(&dir.join(crate::ENVIRONMENTS_FILE));
        environments["default"] = "staging".into();

        let at = |hour, minute| TimeOfDay::new(hour, minute).unwrap();
        let schedules = crate::schedule::schedules_to_json(&[
            Schedule {
                name: "Nightly".into(),
                tests: ScheduleTests::All,
                repeat: Repeat::Daily,
                at: at(2, 30),
                environment: Some("staging".into()),
                video: Some("on-failure".into()),
            },
            Schedule {
                name: "Checkout".into(),
                tests: ScheduleTests::Folder("shop/checkout".into()),
                repeat: Repeat::Days(vec![Weekday::Mon, Weekday::Fri]),
                at: at(9, 0),
                environment: None,
                video: None,
            },
            Schedule {
                name: "Sign-in".into(),
                tests: ScheduleTests::Files(vec![
                    "sign-in.flow.json".into(),
                    ".hidden/a.flow.json".into(),
                ]),
                repeat: Repeat::EveryHours(4),
                at: at(0, 15),
                environment: None,
                video: None,
            },
        ])
        .unwrap();
        let schedules: Value = serde_json::from_str(&schedules).unwrap();

        let history = ScheduleHistory {
            format: crate::SCHEDULE_HISTORY_FORMAT,
            checked_at: Some(29_000_000),
            runs: vec![
                ScheduledRun {
                    schedule: "Nightly".into(),
                    started_at_ms: 1_760_000_000_000,
                    duration_ms: 5_000,
                    outcome: ScheduledOutcome::Finished {
                        passed: 3,
                        failed: 1,
                    },
                },
                ScheduledRun {
                    schedule: "Nightly".into(),
                    started_at_ms: 1_760_086_400_000,
                    duration_ms: 0,
                    outcome: ScheduledOutcome::Skipped {
                        reason: SkipReason::ScreenLocked,
                    },
                },
                ScheduledRun {
                    schedule: "Checkout".into(),
                    started_at_ms: 1_760_090_000_000,
                    duration_ms: 12,
                    outcome: ScheduledOutcome::Error {
                        message: "No environment staging".into(),
                    },
                },
            ],
        };
        let history = serde_json::to_value(&history).unwrap();

        let id = "org.jevision.add-date";
        let lock = PackageLock {
            sources: BTreeMap::from([(
                id.to_owned(),
                "https://example.com/add-date.jevblock".to_owned(),
            )]),
            versions: BTreeMap::from([(id.to_owned(), "1.2.0-beta.2".parse().unwrap())]),
        };
        let lock_text = lock.to_json();
        assert_eq!(PackageLock::parse(&lock_text).unwrap(), lock);
        let lock: Value = serde_json::from_str(&lock_text).unwrap();

        vec![
            Sample {
                wrong: vec![
                    (
                        with(&project, "/ai/text/provider", "jev".into()),
                        "jev for writing tests",
                    ),
                    (
                        with(&project, "/ai/screen/provider", "skynet".into()),
                        "an unknown AI",
                    ),
                    (
                        with(&project, "/runModes/0/limit", 4.into()),
                        "a limit for one by one",
                    ),
                    (
                        with(&project, "/runModes/1/limit", 1.into()),
                        "a limit of one",
                    ),
                    (
                        with(&project, "/quarantine/0/test", "pay.json".into()),
                        "a quarantined file that is no test",
                    ),
                    (
                        with(&project, "/quarantine/0/until", "1 Nov".into()),
                        "an end that is no date",
                    ),
                ],
                kind: &PROJECT,
                file: project,
                reads: |text| ProjectSettings::parse(text).is_ok(),
                strict: true,
                not_text: "retries",
            },
            Sample {
                wrong: vec![
                    (
                        with(&environments, "/environments", json!({ "my env": {} })),
                        "a space in a name",
                    ),
                    (
                        with(
                            &environments,
                            "/environments/staging",
                            json!({ "1st": "x" }),
                        ),
                        "a value name with a digit first",
                    ),
                    (
                        with(&environments, "/environments/staging/SERVER", 8080.into()),
                        "a value that is no text",
                    ),
                ],
                kind: &ENVIRONMENTS,
                file: environments,
                reads: |text| Environments::parse(text).is_ok(),
                strict: true,
                not_text: "environments",
            },
            Sample {
                wrong: vec![
                    (
                        with(&schedules, "/schedules/0/at", "24:00".into()),
                        "a time that is no time",
                    ),
                    (
                        with(
                            &schedules,
                            "/schedules/0/repeat",
                            json!({ "everyHours": 5 }),
                        ),
                        "hours that don't divide a day",
                    ),
                    (
                        with(&schedules, "/schedules/0/video", "sometimes".into()),
                        "an unknown video choice",
                    ),
                    (
                        with(
                            &schedules,
                            "/schedules/1/tests",
                            json!({ "folder": "../shop" }),
                        ),
                        "a folder outside the project",
                    ),
                    (
                        with(&schedules, "/schedules/1/repeat", json!({ "days": [] })),
                        "no days",
                    ),
                    (
                        with(
                            &schedules,
                            "/schedules/2/tests",
                            json!({ "files": ["a.json"] }),
                        ),
                        "a file that is no test",
                    ),
                    (
                        with(&schedules, "/schedules/0/name", " Nightly".into()),
                        "a name with a space first",
                    ),
                ],
                kind: &SCHEDULES,
                file: schedules,
                reads: |text| crate::schedule::parse_schedules(text).is_ok(),
                strict: true,
                not_text: "schedules",
            },
            Sample {
                wrong: vec![(
                    with(&history, "/runs/1/outcome/reason", "tired".into()),
                    "an unknown reason",
                )],
                kind: &SCHEDULE_HISTORY,
                file: history,
                reads: |text| serde_json::from_str::<ScheduleHistory>(text).is_ok(),
                strict: false,
                not_text: "runs",
            },
            Sample {
                wrong: vec![(
                    with(&lock, &format!("/versions/{id}"), "1.2".into()),
                    "a version that is no version",
                )],
                kind: &BLOCKS_LOCK,
                file: lock,
                reads: |text| PackageLock::parse(text).is_ok(),
                strict: false,
                not_text: "sources",
            },
        ]
    }

    #[test]
    fn settings_fit_their_schemas_and_are_turned_away_as_their_readers_do() {
        for sample in settings_samples() {
            let name = sample.kind.name;
            let schema = validator(sample.kind);
            let read = |file: &Value| (sample.reads)(&file.to_string());
            assert!(
                read(&sample.file),
                "the {name} reader doesn't take its sample"
            );
            passes(&schema, &sample.file, &format!("the {name} sample"));

            // Only tests and labels have `$schema` and `minVersion`: to the readers of the other
            // files they are keys like any other.
            for (key, value) in [
                ("$schema", sample.kind.url()),
                ("minVersion", "0.1.0".into()),
            ] {
                let headed = with(&sample.file, &format!("/{key}"), value.into());
                fails(&schema, &headed, &format!("`{key}` in a {name} file"));
                assert_eq!(read(&headed), !sample.strict, "{name}: `{key}`");
            }
            if !sample.kind.formats.is_empty() {
                let no_format = without(&sample.file, "format");
                fails(
                    &schema,
                    &no_format,
                    &format!("a {name} file without format"),
                );
                assert_eq!(
                    read(&no_format),
                    !sample.strict,
                    "{name}: a file without format"
                );
            }

            let mut misspelled = sample.file.clone();
            misspelled["fromat"] = 1.into();
            fails(
                &schema,
                &misspelled,
                &format!("a misspelled key in a {name} file"),
            );
            assert_eq!(
                read(&misspelled),
                !sample.strict,
                "{name}: a misspelled key"
            );
            let not_text = with(&sample.file, &format!("/{}", sample.not_text), "2".into());
            fails(
                &schema,
                &not_text,
                &format!("text for {} in a {name} file", sample.not_text),
            );
            assert!(
                !read(&not_text),
                "the {name} reader takes text for {}",
                sample.not_text
            );
            for (file, what) in &sample.wrong {
                fails(&schema, file, &format!("{what} in a {name} file"));
                assert!(!read(file), "the {name} reader takes {what}");
            }
        }
    }

    #[test]
    fn blocks_fit_the_block_schema() {
        let schema = validator(&BLOCK);
        let mut found = Vec::new();
        files(&root().join("examples"), &["block.json"], &mut found);
        assert!(!found.is_empty());
        for path in &found {
            passes(&schema, &read(path), &path.display().to_string());
        }

        let manifest = BlockManifest {
            id: "com.acme.sign-in".into(),
            name: "Sign in".into(),
            description: Some("Signs in with the account of the test.".into()),
            icon: Some("person.crop.circle".into()),
            version: "1.2.0-beta.2".parse().unwrap(),
            author: "Acme".into(),
            license: "MIT".into(),
            min_version: Some("1.4.0".parse().unwrap()),
            inputs: vec![PackageInput {
                name: "USER".into(),
                label: Some("User name".into()),
                default_value: Some(String::new()),
            }],
            dependencies: vec![Dependency {
                id: "com.acme.open-app".into(),
                version: ">=1.2.0 <2.0.0".parse().unwrap(),
                source: Some("https://example.com/open-app.jevblock".into()),
            }],
        };
        let text = manifest.to_json();
        assert_eq!(crate::package::parse_manifest(&text).unwrap(), manifest);
        let file: Value = serde_json::from_str(&text).unwrap();
        passes(&schema, &file, "the sample block");
        let mut short = file.clone();
        short.as_object_mut().unwrap().remove("dependencies");
        short["dependencies"] = json!([{ "id": "com.acme.open-app" }]);
        passes(&schema, &short, "a dependency without a version");

        for (wrong, what) in [
            (with(&file, "/licence", "MIT".into()), "a misspelled key"),
            (
                with(&file, "/$schema", BLOCK.url().into()),
                "the address of a schema",
            ),
            (without(&file, "format"), "a block without format"),
            (with(&file, "/version", 1.into()), "a number for a version"),
            (
                with(&file, "/version", "1.2".into()),
                "a version that is no version",
            ),
            (with(&file, "/id", "acme".into()), "an id of one part"),
            (with(&file, "/name", "  ".into()), "a blank name"),
            (
                with(&file, "/inputs/0/name", "1st".into()),
                "an input name with a digit first",
            ),
            (
                with(&file, "/dependencies/0/source", "http://example.com".into()),
                "a plain http source",
            ),
        ] {
            assert!(
                crate::package::parse_manifest(&wrong.to_string()).is_err(),
                "the reader takes {what}"
            );
            fails(&schema, &wrong, what);
        }
    }

    #[test]
    fn boards_fit_the_board_schema() {
        let schema = validator(&BOARD);
        let text = r##"{
  "nodes": [
    {"id": "g", "type": "group", "label": "Checkout", "x": 0, "y": 0, "width": 600, "height": 400, "jevision": {"collapsed": true, "folder": "checkout"}},
    {"id": "t", "type": "file", "file": "checkout/pay.flow.json", "x": 20, "y": 40, "width": 280, "height": 120, "color": "4", "jevision": {"open": true}},
    {"id": "n", "type": "text", "text": "**Read me**", "x": 20, "y": 200, "width": 200, "height": 60, "color": "#ff8800", "obsidian": 1},
    {"id": "l", "type": "link", "url": "https://jsoncanvas.org", "x": 300, "y": 200, "width": 200, "height": 60}
  ],
  "edges": [
    {"id": "e", "fromNode": "n", "fromSide": "right", "fromEnd": "arrow", "toNode": "l", "toSide": "left", "toEnd": "none", "label": "see", "color": "1", "styleAttributes": {}}
  ],
  "jevision": {"allTests": true},
  "metadata": {"version": "1.0"}
}"##;
        let board = Board::parse(text).unwrap();
        let file: Value = serde_json::from_str(text).unwrap();
        passes(&schema, &file, "the sample board");
        let written: Value = serde_json::from_str(&board.to_json()).unwrap();
        passes(&schema, &written, "the sample board as JEVision writes it");
        let empty = json!({ "nodes": null, "edges": null });
        assert!(Board::parse(&empty.to_string()).is_ok());
        passes(&schema, &empty, "a board without nodes");

        for (wrong, what) in [
            (
                with(&file, "/jevision/alltests", true.into()),
                "a misspelled key of ours",
            ),
            (
                with(&file, "/nodes/1/jevision/collapsed", true.into()),
                "a frame's key on a test",
            ),
            (
                with(&file, "/nodes/2/jevision", json!({ "open": true })),
                "a key of ours on a note",
            ),
            (with(&file, "/nodes/1/x", "20".into()), "text for a number"),
            (
                with(&file, "/nodes/1/width", 0.4.into()),
                "a node without size",
            ),
            (
                with(&file, "/nodes/1/color", "7".into()),
                "an unknown color",
            ),
            (
                with(&file, "/nodes/1/type", "card".into()),
                "an unknown type",
            ),
            (
                with(&file, "/nodes/1/file", " ".into()),
                "a test without a file",
            ),
            (
                with(&file, "/edges/0/toEnd", "circle".into()),
                "an unknown arrowhead",
            ),
            (
                with(&file, "/edges/0/fromSide", "middle".into()),
                "an unknown side",
            ),
        ] {
            assert!(
                Board::parse(&wrong.to_string()).is_err(),
                "the reader takes {what}"
            );
            fails(&schema, &wrong, what);
        }
    }
}
