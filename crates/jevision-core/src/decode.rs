//! Reading a test file: JSON text → format migration → [`Flow`].
//!
//! Decoding is hand-written rather than derived so that every problem becomes an [`Issue`]
//! with a stable code and parameters, and so that all problems are reported at once.

use std::collections::HashMap;

use serde_json::{Map, Value};

use crate::element::{ELEMENT_STATES, ElementCheck, ElementQuery, ElementState};
use crate::issue::{Issue, ParseError};
use crate::model::{
    Attachment, AttachmentKind, Block, BlockKind, CanvasContent, CanvasSection, CheckOf,
    ClipboardAction, ClipboardContent, Compare, Condition, DATE_LANGUAGES, DEFAULT_DATE_FORMAT,
    DatabaseExpect, ElseIf, ExpectStatus, FileState, Flow, HttpMethod, Modifiers, MouseButton,
    NamedValue, Note, NoteAnchor, PackageNeed, Point, Priority, Rect, Reference, Reply, Resolved,
    SavedValue, ScriptInput, ScriptLanguage, ScriptOutput, ScrollDirection, ScrollUnit, Target,
    TestStatus, TestType, TimeShift, TimeUnit, ValueCheck, ValueKind, ValueTest, WaitUntil,
    WindowPosition, WindowSize, parse_duration, time_zone_known,
};
use crate::package::{Version, app_version};
use crate::resources::{RESOURCE_KINDS, ResourceMeta, is_content_hash, is_resource_id};

/// Oldest file format this build can read.
pub(crate) const OLDEST_FORMAT: u32 = 1;

/// Upgrades the root object of a file by one format version.
pub(crate) type Migration = fn(&mut Map<String, Value>);

/// `MIGRATIONS[i]` upgrades a file from format `OLDEST_FORMAT + i` to the next version, and
/// `FORMAT_CHANGES[i]` is the catalog key of a sentence saying what that next version adds.
/// Appending a step raises [`crate::FORMAT_VERSION`] by one.
pub(crate) const MIGRATIONS: &[Migration] = &[to_labels];
pub(crate) const FORMAT_CHANGES: &[&str] = &["flow.format.2"];
const _: () = assert!(MIGRATIONS.len() == FORMAT_CHANGES.len());

/// Format 2 lets a test name an image or file by its label (`res:…`, see [`crate::resources`])
/// instead of its path. A format-1 file holds no labels, so nothing in it changes.
fn to_labels(_: &mut Map<String, Value>) {}

/// The steps that upgrade a `.jevmeta` file, the label of a resource, like [`MIGRATIONS`] for a
/// test, and the newest format of it.
pub(crate) const META_MIGRATIONS: &[Migration] = &[];
pub(crate) const META_FORMAT: u32 = OLDEST_FORMAT + META_MIGRATIONS.len() as u32;

/// Reads a test file, upgrading older formats to the current one.
///
/// Fails with every structural problem found: invalid JSON, unsupported format, missing or
/// mistyped fields, unknown fields and block types. Problems that only concern the meaning of
/// a well-formed test (broken edges, empty text, out-of-range values) are left to
/// [`crate::validate`], so an editor can still open such a test and let the user fix it.
pub fn parse(text: &str) -> Result<Flow, ParseError> {
    let map = versioned(text, MIGRATIONS)?;
    let mut issues = Vec::new();
    let flow = flow(Obj::root(&map), &mut issues);
    match flow {
        Some(flow) if issues.is_empty() => Ok(flow),
        _ => Err(ParseError { issues }),
    }
}

/// Reads a `.jevmeta` file, the label of a resource. Strict like [`parse`], and with the same
/// rules for versions.
pub fn parse_meta(text: &str) -> Result<ResourceMeta, ParseError> {
    let map = versioned(text, META_MIGRATIONS)?;
    let mut issues = Vec::new();
    let meta = meta(Obj::root(&map), &mut issues);
    match meta {
        Some(meta) if issues.is_empty() => Ok(meta),
        _ => Err(ParseError { issues }),
    }
}

/// The root object of a file whose versions follow the rules of tests, brought to the newest
/// format by `steps`. A file that says it needs a newer JEVision fails with that one problem,
/// before anything else in it is looked at: a newer version may add fields this one rejects.
fn versioned(text: &str, steps: &[Migration]) -> Result<Map<String, Value>, ParseError> {
    let fail = |issue| ParseError {
        issues: vec![issue],
    };
    let root: Value = serde_json::from_str(text).map_err(|e| {
        fail(
            Issue::error("flow.json.syntax")
                .with("line", e.line())
                .with("column", e.column()),
        )
    })?;
    let Value::Object(mut map) = root else {
        return Err(fail(Issue::error("flow.json.notObject")));
    };
    let needs = map
        .get("minVersion")
        .and_then(Value::as_str)
        .and_then(|text| text.parse::<Version>().ok())
        .filter(|needs| *needs > app_version());
    if let Some(needs) = needs {
        return Err(fail(Issue::error("flow.needsNewer").with("version", needs)));
    }
    migrate(&mut map, OLDEST_FORMAT, steps).map_err(fail)?;
    Ok(map)
}

/// Reads one block (`type` and its fields, as under `blocks` in a file), for adding it to a
/// test. Like [`parse`], fails with every structural problem; issues name no block.
pub fn parse_block(value: &Value) -> Result<BlockKind, ParseError> {
    let Value::Object(map) = value else {
        return Err(ParseError {
            issues: vec![Issue::error("flow.json.notObject")],
        });
    };
    let mut issues = Vec::new();
    match block_kind(&mut Obj::root(map), &mut issues) {
        Some(kind) if issues.is_empty() => Ok(kind),
        _ => Err(ParseError { issues }),
    }
}

/// Every field a block of `type_name` may have, `type` and its edges included; just `type` for
/// an unknown type. For descriptions of the format that must not miss a field.
pub fn block_fields(type_name: &str) -> Vec<&'static str> {
    read_fields(
        Map::from_iter([("type".into(), type_name.into())]),
        |o, out| {
            block_kind(o, out);
        },
    )
}

/// Every field a target (`target`, `from`, `to`, `at`, `into`) may have.
pub fn target_fields() -> Vec<&'static str> {
    read_fields(Map::new(), |o, out| {
        target(o, out);
    })
}

/// Every field a reference (`check`, `waitFor`, `if`, `repeat`) may have.
pub fn reference_fields() -> Vec<&'static str> {
    read_fields(Map::new(), |o, out| {
        reference(o, out);
    })
}

/// The keys `read` asks for when decoding `map`, in order and without repeats.
fn read_fields(
    map: Map<String, Value>,
    read: impl FnOnce(&mut Obj<'_>, &mut Out),
) -> Vec<&'static str> {
    let mut o = Obj::root(&map);
    read(&mut o, &mut Vec::new());
    let mut fields = Vec::new();
    for key in o.known {
        if !fields.contains(&key) {
            fields.push(key);
        }
    }
    fields
}

/// Brings `map` from its `format` to `oldest + steps.len()` by running the missing steps.
pub(crate) fn migrate(
    map: &mut Map<String, Value>,
    oldest: u32,
    steps: &[Migration],
) -> Result<(), Issue> {
    let current = oldest + steps.len() as u32;
    let found = match map.get("format") {
        None => return Err(Issue::error("flow.format.missing")),
        Some(v) => v
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| {
                Issue::error("flow.field.type")
                    .with("field", "format")
                    .with("expected", "integer")
            })?,
    };
    if found > current {
        return Err(Issue::error("flow.format.tooNew")
            .with("found", found)
            .with("supported", current));
    }
    if found < oldest {
        return Err(Issue::error("flow.format.unsupported")
            .with("found", found)
            .with("oldest", oldest));
    }
    for step in &steps[(found - oldest) as usize..] {
        step(map);
    }
    map.insert("format".into(), current.into());
    Ok(())
}

type Out = Vec<Issue>;

/// A JSON object being decoded: remembers which keys were read to report the rest as unknown.
struct Obj<'v> {
    map: &'v Map<String, Value>,
    block: Option<&'v str>,
    path: String,
    known: Vec<&'static str>,
}

impl<'v> Obj<'v> {
    fn root(map: &'v Map<String, Value>) -> Self {
        Self {
            map,
            block: None,
            path: String::new(),
            known: Vec::new(),
        }
    }

    fn path_of(&self, key: &str) -> String {
        if self.path.is_empty() {
            key.to_owned()
        } else {
            format!("{}.{key}", self.path)
        }
    }

    fn issue(&self, code: &str, field: &str) -> Issue {
        let issue = Issue::error(code);
        let issue = match self.block {
            Some(b) => issue.with("block", b),
            None => issue,
        };
        issue.with("field", field)
    }

    fn get(&mut self, key: &'static str) -> Option<&'v Value> {
        self.known.push(key);
        self.map.get(key)
    }

    fn wrong_type(&self, key: &str, expected: &str, out: &mut Out) {
        out.push(
            self.issue("flow.field.type", &self.path_of(key))
                .with("expected", expected),
        );
    }

    fn require<T>(&self, key: &str, value: Option<T>, out: &mut Out) -> Option<T> {
        if value.is_none() && !self.map.contains_key(key) {
            out.push(self.issue("flow.field.missing", &self.path_of(key)));
        }
        value
    }

    fn opt_string(&mut self, key: &'static str, out: &mut Out) -> Option<String> {
        match self.get(key)? {
            Value::String(s) => Some(s.clone()),
            _ => {
                self.wrong_type(key, "string", out);
                None
            }
        }
    }

    fn string(&mut self, key: &'static str, out: &mut Out) -> Option<String> {
        let v = self.opt_string(key, out);
        self.require(key, v, out)
    }

    fn strings(&mut self, key: &'static str, out: &mut Out) -> Option<Vec<String>> {
        let items = self.opt_array(key, out)?;
        let strings: Vec<String> = items
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect();
        if strings.len() != items.len() {
            self.wrong_type(key, "stringArray", out);
            return None;
        }
        Some(strings)
    }

    fn flag(&mut self, key: &'static str, out: &mut Out) -> bool {
        match self.get(key) {
            None => false,
            Some(Value::Bool(b)) => *b,
            Some(_) => {
                self.wrong_type(key, "boolean", out);
                false
            }
        }
    }

    fn opt_uint(&mut self, key: &'static str, out: &mut Out) -> Option<u32> {
        let v = self.get(key)?;
        let n = v.as_u64().and_then(|n| u32::try_from(n).ok());
        if n.is_none() {
            self.wrong_type(key, "integer", out);
        }
        n
    }

    /// A whole number that must be there.
    fn int(&mut self, key: &'static str, out: &mut Out) -> Option<i32> {
        let n = self
            .get(key)
            .map(|v| v.as_i64().and_then(|n| i32::try_from(n).ok()));
        if n == Some(None) {
            self.wrong_type(key, "integer", out);
        }
        self.require(key, n.flatten(), out)
    }

    /// A non-negative whole number that must be there.
    fn uint(&mut self, key: &'static str, out: &mut Out) -> Option<u32> {
        let n = self.opt_uint(key, out);
        self.require(key, n, out)
    }

    /// A whole number that may be left out; `0` then.
    fn opt_int(&mut self, key: &'static str, out: &mut Out) -> Option<i32> {
        let Some(v) = self.get(key) else {
            return Some(0);
        };
        let n = v.as_i64().and_then(|n| i32::try_from(n).ok());
        if n.is_none() {
            self.wrong_type(key, "integer", out);
        }
        n
    }

    fn number(&mut self, key: &'static str, out: &mut Out) -> Option<f64> {
        let n = match self.get(key) {
            None => None,
            Some(Value::Number(n)) => n.as_f64(),
            Some(_) => {
                self.wrong_type(key, "number", out);
                None
            }
        };
        self.require(key, n, out)
    }

    fn opt_array(&mut self, key: &'static str, out: &mut Out) -> Option<&'v Vec<Value>> {
        match self.get(key)? {
            Value::Array(a) => Some(a),
            _ => {
                self.wrong_type(key, "array", out);
                None
            }
        }
    }

    fn child(&self, key: &str, value: &'v Value, out: &mut Out) -> Option<Obj<'v>> {
        match value {
            Value::Object(map) => Some(Obj {
                map,
                block: self.block,
                path: self.path_of(key),
                known: Vec::new(),
            }),
            _ => {
                self.wrong_type(key, "object", out);
                None
            }
        }
    }

    fn opt_object(&mut self, key: &'static str, out: &mut Out) -> Option<Obj<'v>> {
        let v = self.get(key)?;
        self.child(key, v, out)
    }

    fn object(&mut self, key: &'static str, out: &mut Out) -> Option<Obj<'v>> {
        let o = self.opt_object(key, out);
        self.require(key, o, out)
    }

    fn opt_choice<T: Copy>(
        &mut self,
        key: &'static str,
        options: &[(&str, T)],
        out: &mut Out,
    ) -> Option<T> {
        let text = self.opt_string(key, out)?;
        let found = options.iter().find(|(name, _)| *name == text).map(|o| o.1);
        if found.is_none() {
            let allowed: Vec<&str> = options.iter().map(|o| o.0).collect();
            out.push(
                self.issue("flow.field.value", &self.path_of(key))
                    .with("value", text)
                    .with("allowed", allowed.join(", ")),
            );
        }
        found
    }

    fn duration(&mut self, key: &'static str, default_ms: u64, out: &mut Out) -> Option<u64> {
        let Some(text) = self.opt_string(key, out) else {
            return (!self.map.contains_key(key)).then_some(default_ms);
        };
        let ms = parse_duration(&text);
        if ms.is_none() {
            out.push(
                self.issue("flow.field.duration", &self.path_of(key))
                    .with("value", text),
            );
        }
        ms
    }

    /// Exactly one of `keys` must be present; returns which one.
    fn one_of(&self, keys: &[&'static str], out: &mut Out) -> Option<&'static str> {
        let present: Vec<&'static str> = keys
            .iter()
            .copied()
            .filter(|k| self.map.contains_key(*k))
            .collect();
        if let [key] = present[..] {
            return Some(key);
        }
        let field = if self.path.is_empty() {
            keys.join("|")
        } else {
            self.path.clone()
        };
        out.push(
            self.issue("flow.field.oneOf", &field)
                .with("options", keys.join(", ")),
        );
        None
    }

    fn finish(&self, out: &mut Out) {
        for key in self.map.keys() {
            if !self.known.contains(&key.as_str()) {
                out.push(self.issue("flow.field.unknown", &self.path_of(key)));
            }
        }
    }
}

pub(crate) const TEST_TYPES: &[(&str, TestType)] = &[
    ("smoke", TestType::Smoke),
    ("regression", TestType::Regression),
    ("acceptance", TestType::Acceptance),
];
pub(crate) const PRIORITIES: &[(&str, Priority)] = &[
    ("critical", Priority::Critical),
    ("high", Priority::High),
    ("medium", Priority::Medium),
    ("low", Priority::Low),
];
pub(crate) const STATUSES: &[(&str, TestStatus)] = &[
    ("draft", TestStatus::Draft),
    ("ready", TestStatus::Ready),
    ("outdated", TestStatus::Outdated),
];
const BUTTONS: &[(&str, MouseButton)] = &[
    ("left", MouseButton::Left),
    ("right", MouseButton::Right),
    ("middle", MouseButton::Middle),
];
const SCROLL_DIRECTIONS: &[(&str, ScrollDirection)] = &[
    ("up", ScrollDirection::Up),
    ("down", ScrollDirection::Down),
    ("left", ScrollDirection::Left),
    ("right", ScrollDirection::Right),
];
const SCROLL_UNITS: &[(&str, ScrollUnit)] =
    &[("lines", ScrollUnit::Lines), ("pixels", ScrollUnit::Pixels)];

#[derive(Clone, Copy)]
enum CompareMode {
    Exact,
    Similar,
    Meaning,
}
const COMPARE: &[(&str, CompareMode)] = &[
    ("exact", CompareMode::Exact),
    ("similar", CompareMode::Similar),
    ("meaning", CompareMode::Meaning),
];

const LANGUAGES: &[(&str, ScriptLanguage)] = &[
    ("sh", ScriptLanguage::Sh),
    ("zsh", ScriptLanguage::Zsh),
    ("python3", ScriptLanguage::Python3),
    ("node", ScriptLanguage::Node),
    ("appleScript", ScriptLanguage::AppleScript),
];
const VALUE_KINDS: &[(&str, ValueKind)] = &[
    ("text", ValueKind::Text),
    ("number", ValueKind::Number),
    ("boolean", ValueKind::Boolean),
    ("image", ValueKind::Image),
    ("area", ValueKind::Area),
];
const MODIFIERS: &[&str] = &["cmd", "shift", "option", "ctrl"];
#[derive(Clone, Copy)]
enum ClipboardMode {
    Put,
    Read,
    Check,
}
const CLIPBOARD_CONTENTS: &[(&str, ClipboardContent)] = &[
    ("text", ClipboardContent::Text),
    ("image", ClipboardContent::Image),
    ("files", ClipboardContent::Files),
];
const CLIPBOARD_MODES: &[(&str, ClipboardMode)] = &[
    ("put", ClipboardMode::Put),
    ("read", ClipboardMode::Read),
    ("check", ClipboardMode::Check),
];
const TIME_UNITS: &[(&str, TimeUnit)] = &[
    ("seconds", TimeUnit::Seconds),
    ("minutes", TimeUnit::Minutes),
    ("hours", TimeUnit::Hours),
    ("days", TimeUnit::Days),
    ("weeks", TimeUnit::Weeks),
    ("months", TimeUnit::Months),
    ("years", TimeUnit::Years),
];
const METHODS: &[(&str, HttpMethod)] = &[
    ("GET", HttpMethod::Get),
    ("POST", HttpMethod::Post),
    ("PUT", HttpMethod::Put),
    ("PATCH", HttpMethod::Patch),
    ("DELETE", HttpMethod::Delete),
    ("HEAD", HttpMethod::Head),
];
const VALUE_TESTS: &[(&str, ValueTest)] = &[
    ("equals", ValueTest::Equals),
    ("notEquals", ValueTest::NotEquals),
    ("contains", ValueTest::Contains),
    ("greaterThan", ValueTest::GreaterThan),
    ("lessThan", ValueTest::LessThan),
];

/// Default for `waitFor.timeout`, and for the time `checkFile` and `database` wait, when the
/// file leaves it out.
pub const DEFAULT_WAIT_TIMEOUT_MS: u64 = 10_000;
/// Default for the time a script, command or HTTP request may take.
pub const DEFAULT_TASK_TIMEOUT_MS: u64 = 30_000;

/// The fields every versioned file starts with: where its JSON Schema is, which format it is in
/// (checked by [`migrate`]) and the oldest JEVision that reads it.
fn header(o: &mut Obj<'_>, out: &mut Out) {
    o.opt_string("$schema", out);
    o.get("format");
    if let Some(text) = o.opt_string("minVersion", out)
        && text.parse::<Version>().is_err()
    {
        out.push(
            o.issue("flow.minVersion.invalid", "minVersion")
                .with("value", text),
        );
    }
}

fn meta(mut o: Obj<'_>, out: &mut Out) -> Option<ResourceMeta> {
    header(&mut o, out);
    let id = o.string("id", out);
    let kind = o.opt_choice("kind", RESOURCE_KINDS, out);
    let kind = o.require("kind", kind, out);
    let hash = o.string("hash", out);
    let size = o.get("size").map(|v| v.as_u64());
    if size == Some(None) {
        o.wrong_type("size", "integer", out);
    }
    let size = o.require("size", size.flatten(), out);
    let name = o.string("name", out);
    let bad_name =
        |n: &&str| n.is_empty() || *n == "." || *n == ".." || n.contains(['/', '\\', '\0']);
    for (field, invalid) in [
        ("id", id.as_deref().filter(|v| !is_resource_id(v))),
        ("hash", hash.as_deref().filter(|v| !is_content_hash(v))),
        ("name", name.as_deref().filter(bad_name)),
    ] {
        if let Some(value) = invalid {
            out.push(o.issue("resource.meta.invalid", field).with("value", value));
        }
    }
    o.finish(out);
    Some(ResourceMeta {
        id: id?,
        kind: kind?,
        hash: hash?,
        size: size?,
        name: name?,
    })
}

fn flow(mut o: Obj<'_>, out: &mut Out) -> Option<Flow> {
    header(&mut o, out);
    let name = o.string("name", out);
    let description = o.opt_string("description", out);
    let icon = o.opt_string("icon", out);
    let app = o.string("app", out);
    let test_type = o.opt_choice("testType", TEST_TYPES, out);
    let priority = o.opt_choice("priority", PRIORITIES, out);
    let status = o.opt_choice("status", STATUSES, out);
    let tags = o.strings("tags", out).unwrap_or_default();
    let preconditions = o.opt_string("preconditions", out);
    let author = o.opt_string("author", out);
    let platform = o.opt_string("platform", out);
    let link = o.opt_string("link", out);
    let flaky = o.flag("flaky", out);
    let retries = o.opt_uint("retries", out);
    let depends_on = o.strings("dependsOn", out).unwrap_or_default();
    let start = o.string("start", out);
    let blocks = o.object("blocks", out).map(|b| blocks(b, out));
    let canvas = o
        .opt_object("canvas", out)
        .map(|c| canvas(c, out))
        .unwrap_or_default();
    o.finish(out);
    Some(Flow {
        name: name?,
        description,
        icon,
        app: app?,
        test_type,
        priority,
        status,
        tags,
        preconditions,
        author,
        platform,
        link,
        flaky,
        retries,
        depends_on,
        start: start?,
        blocks: blocks?,
        canvas,
    })
}

fn canvas(mut o: Obj<'_>, out: &mut Out) -> CanvasContent {
    let mut canvas = CanvasContent::default();
    if let Some(positions) = o.opt_object("positions", out) {
        for (id, value) in positions.map {
            if let Some(p) = positions.child(id, value, out).and_then(|p| point(p, out)) {
                canvas.positions.insert(id.clone(), p);
            }
        }
    }
    if let Some(sections) = o.opt_object("sections", out) {
        for (id, value) in sections.map {
            let Some(mut s) = sections.child(id, value, out) else {
                continue;
            };
            let title = s.string("title", out);
            let rect = s.object("frame", out).and_then(|f| rect(f, out));
            s.finish(out);
            if let (Some(title), Some(rect)) = (title, rect) {
                canvas.sections.push(CanvasSection {
                    id: id.clone(),
                    title,
                    rect,
                });
            }
        }
    }
    if let Some(notes) = o.opt_object("notes", out) {
        for (id, value) in notes.map {
            let Some(mut n) = notes.child(id, value, out) else {
                continue;
            };
            let text = n.string("text", out);
            let author = n.opt_string("author", out).unwrap_or_default();
            let time = n.opt_string("time", out).unwrap_or_default();
            let anchor = n
                .opt_object("anchor", out)
                .and_then(|a| note_anchor(a, out))
                .unwrap_or_default();
            let attachments = entries(&mut n, "attachments", out, attachment).unwrap_or_default();
            let replies = entries(&mut n, "replies", out, reply).unwrap_or_default();
            let resolved = n.opt_object("resolved", out).and_then(|r| resolved(r, out));
            let rect = n.object("frame", out).and_then(|f| rect(f, out));
            n.finish(out);
            if let (Some(text), Some(rect)) = (text, rect) {
                canvas.notes.push(Note {
                    id: id.clone(),
                    text,
                    author,
                    time,
                    anchor,
                    attachments,
                    replies,
                    resolved,
                    rect,
                });
            }
        }
    }
    o.finish(out);
    canvas
}

#[derive(Clone, Copy)]
enum AnchorKind {
    Test,
    Block,
    Folder,
}

const ANCHOR_KINDS: &[(&str, AnchorKind)] = &[
    ("test", AnchorKind::Test),
    ("block", AnchorKind::Block),
    ("folder", AnchorKind::Folder),
];

fn note_anchor(mut o: Obj<'_>, out: &mut Out) -> Option<NoteAnchor> {
    let kind = o.opt_choice("type", ANCHOR_KINDS, out);
    let anchor = match kind {
        Some(AnchorKind::Test) => Some(NoteAnchor::Test),
        Some(AnchorKind::Folder) => Some(NoteAnchor::Folder),
        Some(AnchorKind::Block) => o.string("block", out).map(|id| NoteAnchor::Block { id }),
        None => {
            o.require("type", None::<()>, out);
            None
        }
    };
    o.finish(out);
    anchor
}

fn attachment(mut o: Obj<'_>, out: &mut Out) -> Option<Attachment> {
    let kind = o.one_of(&["file", "link"], out);
    let value = match kind {
        Some(key) => o.string(key, out),
        None => None,
    };
    o.finish(out);
    let kind = match kind? {
        "file" => AttachmentKind::File,
        _ => AttachmentKind::Link,
    };
    Some(Attachment {
        kind,
        value: value?,
    })
}

fn reply(mut o: Obj<'_>, out: &mut Out) -> Option<Reply> {
    let id = o.string("id", out);
    let author = o.opt_string("author", out).unwrap_or_default();
    let time = o.opt_string("time", out).unwrap_or_default();
    let text = o.string("text", out);
    o.finish(out);
    Some(Reply {
        id: id?,
        author,
        time,
        text: text?,
    })
}

fn resolved(mut o: Obj<'_>, out: &mut Out) -> Option<Resolved> {
    let by = o.string("by", out);
    let time = o.opt_string("time", out).unwrap_or_default();
    o.finish(out);
    Some(Resolved { by: by?, time })
}

fn blocks(o: Obj<'_>, out: &mut Out) -> Vec<Block> {
    let mut blocks = Vec::new();
    for (id, value) in o.map {
        let Value::Object(map) = value else {
            out.push(
                Issue::error("flow.field.type")
                    .with("block", id)
                    .with("expected", "object"),
            );
            continue;
        };
        let mut block_obj = Obj {
            map,
            block: Some(id),
            path: String::new(),
            known: Vec::new(),
        };
        if let Some(kind) = block_kind(&mut block_obj, out) {
            blocks.push(Block {
                id: id.clone(),
                kind,
            });
        }
    }
    blocks
}

fn block_kind(o: &mut Obj<'_>, out: &mut Out) -> Option<BlockKind> {
    let type_name = o.string("type", out)?;
    let next = |o: &mut Obj<'_>, out: &mut Out| o.opt_string("next", out);
    let kind = match type_name.as_str() {
        "openApp" => {
            let app = o.opt_string("app", out);
            let reset_state = o.flag("resetState", out);
            let window_size = o
                .opt_object("windowSize", out)
                .and_then(|s| window_size(s, out));
            let window_position = o
                .opt_object("windowPosition", out)
                .and_then(|p| window_position(p, out));
            let arguments = o.strings("arguments", out).unwrap_or_default();
            let environment = texts(o, "environment", out);
            let next = next(o, out);
            Some(BlockKind::OpenApp {
                app,
                reset_state,
                window_size,
                window_position,
                arguments,
                environment,
                next,
            })
        }
        "openFile" => {
            let files = o.strings("files", out);
            let files = o.require("files", files, out);
            let app = o.opt_string("app", out);
            let new_instance = o.flag("newInstance", out);
            let close_after = o.flag("closeAfter", out);
            let next = next(o, out);
            all_present(|| {
                Some(BlockKind::OpenFile {
                    files: files?,
                    app,
                    new_instance,
                    close_after,
                    next,
                })
            })
        }
        "click" => {
            let target = o
                .object("target", out)
                .and_then(|mut t| target(&mut t, out));
            let button = o.opt_choice("button", BUTTONS, out);
            let double = o.flag("double", out);
            let modifiers = modifiers(o, out);
            let wait_for_change = o.flag("waitForChange", out);
            let next = next(o, out);
            all_present(|| {
                Some(BlockKind::Click {
                    target: target?,
                    button: button.unwrap_or(MouseButton::Left),
                    double,
                    modifiers,
                    wait_for_change,
                    next,
                })
            })
        }
        "hover" => {
            let target = o
                .object("target", out)
                .and_then(|mut t| target(&mut t, out));
            let modifiers = modifiers(o, out);
            let wait_for_change = o.flag("waitForChange", out);
            let next = next(o, out);
            all_present(|| {
                Some(BlockKind::Hover {
                    target: target?,
                    modifiers,
                    wait_for_change,
                    next,
                })
            })
        }
        "drag" => {
            let from = o.object("from", out).and_then(|mut t| target(&mut t, out));
            let to = o.object("to", out).and_then(|mut t| target(&mut t, out));
            let button = o.opt_choice("button", BUTTONS, out);
            let modifiers = modifiers(o, out);
            let wait_for_change = o.flag("waitForChange", out);
            let next = next(o, out);
            all_present(|| {
                Some(BlockKind::Drag {
                    from: from?,
                    to: to?,
                    button: button.unwrap_or(MouseButton::Left),
                    modifiers,
                    wait_for_change,
                    next,
                })
            })
        }
        "scroll" => {
            let direction = o.opt_choice("direction", SCROLL_DIRECTIONS, out);
            let direction = o.require("direction", direction, out);
            let amount = o.opt_uint("amount", out);
            let amount = o.require("amount", amount, out);
            let unit = o.opt_choice("unit", SCROLL_UNITS, out);
            let at = o
                .opt_object("at", out)
                .and_then(|mut t| target(&mut t, out));
            let modifiers = modifiers(o, out);
            let wait_for_change = o.flag("waitForChange", out);
            let next = next(o, out);
            all_present(|| {
                Some(BlockKind::Scroll {
                    direction: direction?,
                    amount: amount?,
                    unit: unit.unwrap_or(ScrollUnit::Lines),
                    at,
                    modifiers,
                    wait_for_change,
                    next,
                })
            })
        }
        "type" => {
            let text = o.string("text", out);
            let into = o
                .opt_object("into", out)
                .and_then(|mut t| target(&mut t, out));
            let next = next(o, out);
            all_present(|| {
                Some(BlockKind::Type {
                    text: text?,
                    into,
                    next,
                })
            })
        }
        "pressKeys" => {
            let keys = o.string("keys", out);
            let next = next(o, out);
            all_present(|| Some(BlockKind::PressKeys { keys: keys?, next }))
        }
        "waitFor" => {
            let until = o.get("until");
            let until = o.require("until", until, out);
            let until = until.and_then(|v| wait_until(o, v, out));
            let timeout_ms = o.duration("timeout", DEFAULT_WAIT_TIMEOUT_MS, out);
            let next = next(o, out);
            all_present(|| {
                Some(BlockKind::WaitFor {
                    until: until?,
                    timeout_ms: timeout_ms?,
                    next,
                })
            })
        }
        "pause" => {
            let given = o.map.contains_key("duration");
            let duration_ms = o.duration("duration", 0, out).filter(|_| given);
            let duration_ms = o.require("duration", duration_ms, out);
            let next = next(o, out);
            all_present(|| {
                Some(BlockKind::Pause {
                    duration_ms: duration_ms?,
                    next,
                })
            })
        }
        "check" => {
            let of = match o.one_of(&["reference", "element"], out) {
                Some("reference") => o
                    .object("reference", out)
                    .and_then(|mut r| reference(&mut r, out))
                    .map(CheckOf::Screen),
                Some(_) => o
                    .object("element", out)
                    .and_then(|e| element_check(e, out))
                    .map(CheckOf::Element),
                None => {
                    o.known.extend(["reference", "element"]);
                    None
                }
            };
            let next = next(o, out);
            all_present(|| Some(BlockKind::Check { of: of?, next }))
        }
        "if" => {
            let condition = condition(o, out);
            let then = o.opt_string("then", out);
            let else_if = else_ifs(o, out);
            let otherwise = o.opt_string("else", out);
            all_present(|| {
                Some(BlockKind::If {
                    condition: condition?,
                    then,
                    else_if: else_if?,
                    otherwise,
                })
            })
        }
        "repeat" => {
            let times = o.opt_uint("times", out);
            let times = o.require("times", times, out);
            let body = o.string("body", out);
            let next = next(o, out);
            all_present(|| {
                Some(BlockKind::Repeat {
                    times: times?,
                    body: body?,
                    next,
                })
            })
        }
        "useData" => {
            let file = o.string("file", out);
            let body = o.string("body", out);
            let next = next(o, out);
            all_present(|| {
                Some(BlockKind::UseData {
                    file: file?,
                    body: body?,
                    next,
                })
            })
        }
        "runTest" => {
            let test = o.string("test", out);
            let package = match o.opt_object("package", out) {
                None => Some(None),
                Some(p) => package_need(p, out).map(Some),
            };
            let inputs = texts(o, "inputs", out);
            let next = next(o, out);
            all_present(|| {
                Some(BlockKind::RunTest {
                    test: test?,
                    package: package?,
                    inputs,
                    next,
                })
            })
        }
        "script" => {
            let language = o.opt_choice("language", LANGUAGES, out);
            let language = o.require("language", language, out);
            let code = o.string("code", out);
            let inputs = entries(o, "inputs", out, |mut item, out| {
                let name = item.string("name", out);
                let kind = item.opt_choice("type", VALUE_KINDS, out);
                let value = item.string("value", out);
                item.finish(out);
                Some(ScriptInput {
                    name: name?,
                    kind: kind.unwrap_or(ValueKind::Text),
                    value: value?,
                })
            });
            let outputs = entries(o, "outputs", out, |mut item, out| {
                let name = item.string("name", out);
                let kind = item.opt_choice("type", VALUE_KINDS, out);
                let kind = item.require("type", kind, out);
                item.finish(out);
                Some(ScriptOutput {
                    name: name?,
                    kind: kind?,
                })
            });
            let timeout_ms = o.duration("timeout", DEFAULT_TASK_TIMEOUT_MS, out);
            let next = next(o, out);
            all_present(|| {
                Some(BlockKind::Script {
                    language: language?,
                    code: code?,
                    inputs: inputs?,
                    outputs: outputs?,
                    timeout_ms: timeout_ms?,
                    next,
                })
            })
        }
        "command" => {
            let command = o.string("command", out);
            let folder = o.opt_string("folder", out);
            let exit_code = o.opt_int("exitCode", out);
            let save_output = o.opt_string("saveOutput", out);
            let timeout_ms = o.duration("timeout", DEFAULT_TASK_TIMEOUT_MS, out);
            let next = next(o, out);
            all_present(|| {
                Some(BlockKind::Command {
                    command: command?,
                    folder,
                    exit_code: exit_code?,
                    save_output,
                    timeout_ms: timeout_ms?,
                    next,
                })
            })
        }
        "http" => {
            let method = o.opt_choice("method", METHODS, out);
            let url = o.string("url", out);
            let headers = named_values(o, "headers", "value", out);
            let body = o.opt_string("body", out).unwrap_or_default();
            let expect_status = expect_status(o, out);
            let save = named_values(o, "save", "from", out).map(|all| {
                all.into_iter()
                    .map(|v| SavedValue {
                        name: v.name,
                        from: v.value,
                    })
                    .collect()
            });
            let timeout_ms = o.duration("timeout", DEFAULT_TASK_TIMEOUT_MS, out);
            let next = next(o, out);
            all_present(|| {
                Some(BlockKind::Http {
                    method: method.unwrap_or(HttpMethod::Get),
                    url: url?,
                    headers: headers?,
                    body,
                    expect_status: expect_status?,
                    save: save?,
                    timeout_ms: timeout_ms?,
                    next,
                })
            })
        }
        "checkFile" => {
            let path = o.string("path", out);
            let until = o.get("until");
            let until = o.require("until", until, out);
            let until = until.and_then(|v| file_state(o, v, out));
            let timeout_ms = o.duration("timeout", DEFAULT_WAIT_TIMEOUT_MS, out);
            let next = next(o, out);
            all_present(|| {
                Some(BlockKind::CheckFile {
                    path: path?,
                    until: until?,
                    timeout_ms: timeout_ms?,
                    next,
                })
            })
        }
        "database" => {
            let file = o.string("file", out);
            let query = o.string("query", out);
            let save_as = o.opt_string("saveAs", out);
            let expect = match o.opt_object("expect", out) {
                None => Some(None),
                Some(e) => database_expect(e, out).map(Some),
            };
            let timeout_ms = o.duration("timeout", DEFAULT_WAIT_TIMEOUT_MS, out);
            let next = next(o, out);
            all_present(|| {
                Some(BlockKind::Database {
                    file: file?,
                    query: query?,
                    save_as,
                    expect: expect?,
                    timeout_ms: timeout_ms?,
                    next,
                })
            })
        }
        "clipboard" => {
            let mode = o.opt_choice("action", CLIPBOARD_MODES, out);
            let action = match o.require("action", mode, out) {
                Some(ClipboardMode::Put) => match o.one_of(&["text", "image"], out) {
                    Some("image") => o
                        .string("image", out)
                        .map(|image| ClipboardAction::PutImage { image }),
                    Some(_) => o
                        .string("text", out)
                        .map(|text| ClipboardAction::Put { text }),
                    None => {
                        o.known.extend(["text", "image"]);
                        None
                    }
                },
                Some(ClipboardMode::Read) => {
                    let save_as = o.string("saveAs", out);
                    let content = o.opt_choice("content", CLIPBOARD_CONTENTS, out);
                    let content = if o.map.contains_key("content") {
                        content
                    } else {
                        Some(ClipboardContent::Text)
                    };
                    all_present(|| {
                        Some(ClipboardAction::Read {
                            save_as: save_as?,
                            content: content?,
                        })
                    })
                }
                Some(ClipboardMode::Check) => {
                    let is = o.opt_choice("is", VALUE_TESTS, out);
                    let is = o.require("is", is, out);
                    let to = o.string("to", out);
                    let timeout_ms = o.duration("timeout", DEFAULT_WAIT_TIMEOUT_MS, out);
                    all_present(|| {
                        Some(ClipboardAction::Check {
                            is: is?,
                            to: to?,
                            timeout_ms: timeout_ms?,
                        })
                    })
                }
                None => {
                    o.known
                        .extend(["text", "image", "saveAs", "content", "is", "to", "timeout"]);
                    None
                }
            };
            let next = next(o, out);
            all_present(|| {
                Some(BlockKind::Clipboard {
                    action: action?,
                    next,
                })
            })
        }
        "dateTime" => {
            let save_as = o.string("saveAs", out);
            let format = o.opt_string("format", out);
            let languages: Vec<(&str, &str)> = DATE_LANGUAGES.iter().map(|l| (*l, *l)).collect();
            let language = o.opt_choice("language", &languages, out);
            let language = if o.map.contains_key("language") {
                language.map(|l| Some(l.to_owned()))
            } else {
                Some(None)
            };
            let time_zone = o.opt_string("timeZone", out);
            if let Some(zone) = time_zone.as_deref().filter(|z| !time_zone_known(z)) {
                out.push(
                    o.issue("flow.field.value", &o.path_of("timeZone"))
                        .with("value", zone)
                        .with("allowed", "UTC, Europe/Berlin, America/New_York …"),
                );
            }
            let shift = match o.opt_object("shift", out) {
                None => Some(None),
                Some(s) => time_shift(s, out).map(Some),
            };
            let next = next(o, out);
            all_present(|| {
                Some(BlockKind::DateTime {
                    save_as: save_as?,
                    format: format.unwrap_or_else(|| DEFAULT_DATE_FORMAT.to_owned()),
                    language: language?,
                    time_zone: time_zone.filter(|z| time_zone_known(z)),
                    shift: shift?,
                    next,
                })
            })
        }
        other => {
            out.push(
                Issue::error("flow.block.unknownType")
                    .with("block", o.block.unwrap_or_default())
                    .with("type", other),
            );
            return None;
        }
    };
    o.finish(out);
    kind
}

/// Builds a value from fields that were all read beforehand; `?` inside stops only the build.
fn all_present<T>(build: impl FnOnce() -> Option<T>) -> Option<T> {
    build()
}

fn target(o: &mut Obj<'_>, out: &mut Out) -> Option<Target> {
    const KEYS: [&str; 5] = ["image", "point", "description", "area", "element"];
    let which = o.one_of(&KEYS, out);
    let t = match which {
        Some("image") => o.string("image", out).map(Target::Image),
        Some("area") => o.string("area", out).map(Target::Area),
        Some("description") => o.string("description", out).map(Target::Description),
        Some("element") => o
            .object("element", out)
            .and_then(|e| element(e, out))
            .map(Target::Element),
        Some(_) => o
            .object("point", out)
            .and_then(|p| point(p, out))
            .map(Target::Point),
        None => {
            o.known.extend(KEYS);
            None
        }
    };
    o.finish(out);
    t
}

fn point(mut o: Obj<'_>, out: &mut Out) -> Option<Point> {
    let x = o.number("x", out);
    let y = o.number("y", out);
    o.finish(out);
    Some(Point { x: x?, y: y? })
}

fn window_size(mut o: Obj<'_>, out: &mut Out) -> Option<WindowSize> {
    let width = o.uint("width", out);
    let height = o.uint("height", out);
    o.finish(out);
    Some(WindowSize {
        width: width?,
        height: height?,
    })
}

fn window_position(mut o: Obj<'_>, out: &mut Out) -> Option<WindowPosition> {
    let x = o.int("x", out);
    let y = o.int("y", out);
    o.finish(out);
    Some(WindowPosition { x: x?, y: y? })
}

fn rect(mut o: Obj<'_>, out: &mut Out) -> Option<Rect> {
    let x = o.number("x", out);
    let y = o.number("y", out);
    let width = o.number("width", out);
    let height = o.number("height", out);
    o.finish(out);
    Some(Rect {
        x: x?,
        y: y?,
        width: width?,
        height: height?,
    })
}

fn rects(o: &mut Obj<'_>, key: &'static str, out: &mut Out) -> Vec<Rect> {
    let Some(items) = o.opt_array(key, out) else {
        return Vec::new();
    };
    items
        .iter()
        .enumerate()
        .filter_map(|(i, v)| {
            o.child(&format!("{key}.{i}"), v, out)
                .and_then(|r| rect(r, out))
        })
        .collect()
}

/// The `elseIf` cases of an `if`; `None` when one of them is broken.
fn else_ifs(o: &mut Obj<'_>, out: &mut Out) -> Option<Vec<ElseIf>> {
    let Some(items) = o.opt_array("elseIf", out) else {
        return Some(Vec::new());
    };
    let cases: Vec<Option<ElseIf>> = items
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let mut case = o.child(&format!("elseIf.{i}"), v, out)?;
            let condition = condition(&mut case, out);
            let then = case.opt_string("then", out);
            case.finish(out);
            Some(ElseIf {
                condition: condition?,
                then,
            })
        })
        .collect();
    cases.into_iter().collect()
}

/// What an `if`, or one of its `elseIf` cases, checks: `reference`, `value` or `element`.
fn condition(o: &mut Obj<'_>, out: &mut Out) -> Option<Condition> {
    const KEYS: [&str; 3] = ["reference", "value", "element"];
    match o.one_of(&KEYS, out) {
        Some("reference") => o
            .object("reference", out)
            .and_then(|mut r| reference(&mut r, out))
            .map(Condition::Screen),
        Some("element") => o
            .object("element", out)
            .and_then(|e| element_check(e, out))
            .map(Condition::Element),
        Some(_) => o
            .object("value", out)
            .and_then(|v| value_check(v, out))
            .map(Condition::Value),
        None => {
            o.known.extend(KEYS);
            None
        }
    }
}

/// The fields that pick an element: `id`, `role`, `title` and `index`, all optional here.
fn element_query(o: &mut Obj<'_>, out: &mut Out) -> ElementQuery {
    ElementQuery {
        id: o.opt_string("id", out).unwrap_or_default(),
        role: o.opt_string("role", out).unwrap_or_default(),
        title: o.opt_string("title", out).unwrap_or_default(),
        index: o.opt_uint("index", out),
    }
}

fn element(mut o: Obj<'_>, out: &mut Out) -> Option<ElementQuery> {
    let query = element_query(&mut o, out);
    o.finish(out);
    Some(query)
}

/// An element and what is expected of it: `is`, with `to` for the states that compare text.
fn element_check(mut o: Obj<'_>, out: &mut Out) -> Option<ElementCheck> {
    let element = element_query(&mut o, out);
    let is = o.opt_string("is", out);
    let is = o.require("is", is, out);
    let to = o.opt_string("to", out);
    let is = is.and_then(|name| {
        if !ELEMENT_STATES.contains(&name.as_str()) {
            out.push(
                o.issue("flow.field.value", &o.path_of("is"))
                    .with("value", name)
                    .with("allowed", ELEMENT_STATES.join(", ")),
            );
            return None;
        }
        if ElementState::compares_text(&name) {
            let to = o.require("to", to.clone(), out)?;
            return ElementState::named(&name, to);
        }
        if to.is_some() {
            out.push(o.issue("flow.element.toNeedsText", &o.path_of("to")));
            return None;
        }
        ElementState::named(&name, String::new())
    });
    o.finish(out);
    Some(ElementCheck { element, is: is? })
}

fn value_check(mut o: Obj<'_>, out: &mut Out) -> Option<ValueCheck> {
    let of = o.string("of", out);
    let is = o.opt_choice("is", VALUE_TESTS, out);
    let is = o.require("is", is, out);
    let to = o.string("to", out);
    o.finish(out);
    Some(ValueCheck {
        of: of?,
        is: is?,
        to: to?,
    })
}

/// An object of texts by name, such as the inputs of a test it runs; absent is empty.
fn texts(o: &mut Obj<'_>, key: &'static str, out: &mut Out) -> HashMap<String, String> {
    let Some(map) = o.opt_object(key, out) else {
        return HashMap::new();
    };
    let mut values = HashMap::new();
    for (name, value) in map.map {
        match value {
            Value::String(v) => {
                values.insert(name.clone(), v.clone());
            }
            _ => map.wrong_type(name, "string", out),
        }
    }
    values
}

/// A list of `{"name": …, value_key: …}` objects, such as script inputs or HTTP headers.
fn named_values(
    o: &mut Obj<'_>,
    key: &'static str,
    value_key: &'static str,
    out: &mut Out,
) -> Option<Vec<NamedValue>> {
    entries(o, key, out, |mut item, out| {
        let name = item.string("name", out);
        let value = item.string(value_key, out);
        item.finish(out);
        Some(NamedValue {
            name: name?,
            value: value?,
        })
    })
}

/// The objects in the array `key`, each read by `read`; an absent array is an empty list.
fn entries<'v, T>(
    o: &mut Obj<'v>,
    key: &'static str,
    out: &mut Out,
    read: impl Fn(Obj<'v>, &mut Out) -> Option<T>,
) -> Option<Vec<T>> {
    let Some(items) = o.opt_array(key, out) else {
        return (!o.map.contains_key(key)).then(Vec::new);
    };
    let found: Vec<Option<T>> = items
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let item = o.child(&format!("{key}.{i}"), v, out)?;
            read(item, out)
        })
        .collect();
    found.into_iter().collect()
}

/// `modifiers`: absent or a list of the keys held down.
fn modifiers(o: &mut Obj<'_>, out: &mut Out) -> Modifiers {
    let mut held = Modifiers::default();
    for key in o.strings("modifiers", out).unwrap_or_default() {
        match key.as_str() {
            "cmd" => held.command = true,
            "shift" => held.shift = true,
            "option" => held.option = true,
            "ctrl" => held.control = true,
            _ => out.push(
                o.issue("flow.field.value", &o.path_of("modifiers"))
                    .with("value", key)
                    .with("allowed", MODIFIERS.join(", ")),
            ),
        }
    }
    held
}

fn package_need(mut o: Obj<'_>, out: &mut Out) -> Option<PackageNeed> {
    let version = o.string("version", out);
    let source = o.opt_string("source", out);
    o.finish(out);
    Some(PackageNeed {
        version: version?,
        source,
    })
}

fn time_shift(mut o: Obj<'_>, out: &mut Out) -> Option<TimeShift> {
    let amount = o.opt_int("amount", out);
    let amount = o.require("amount", amount, out);
    let unit = o.opt_choice("unit", TIME_UNITS, out);
    let unit = o.require("unit", unit, out);
    o.finish(out);
    Some(TimeShift {
        amount: amount?,
        unit: unit?,
    })
}

/// `expectStatus`: absent or `"success"`, `"any"`, or a status code.
fn expect_status(o: &mut Obj<'_>, out: &mut Out) -> Option<ExpectStatus> {
    match o.get("expectStatus") {
        None => Some(ExpectStatus::Success),
        Some(Value::String(s)) if s == "success" => Some(ExpectStatus::Success),
        Some(Value::String(s)) if s == "any" => Some(ExpectStatus::Any),
        Some(v) => {
            let code = v.as_u64().and_then(|n| u16::try_from(n).ok());
            if code.is_none() {
                o.wrong_type("expectStatus", "status", out);
            }
            code.map(ExpectStatus::Exactly)
        }
    }
}

fn file_state(o: &Obj<'_>, value: &Value, out: &mut Out) -> Option<FileState> {
    match value.as_str() {
        Some("exists") => return Some(FileState::Exists),
        Some("missing") => return Some(FileState::Missing),
        Some("changed") => return Some(FileState::Changed),
        Some(other) => {
            out.push(
                o.issue("flow.field.value", &o.path_of("until"))
                    .with("value", other)
                    .with("allowed", "exists, missing, changed, {\"contains\": …}"),
            );
            return None;
        }
        None => {}
    }
    let mut until = o.child("until", value, out)?;
    let text = until.string("contains", out);
    until.finish(out);
    text.map(FileState::Contains)
}

fn database_expect(mut o: Obj<'_>, out: &mut Out) -> Option<DatabaseExpect> {
    let result = match o.one_of(&["value", "rows"], out) {
        Some("value") => o.string("value", out).map(DatabaseExpect::Value),
        Some(_) => {
            let rows = o.opt_uint("rows", out);
            o.require("rows", rows, out).map(DatabaseExpect::Rows)
        }
        None => {
            o.known.extend(["value", "rows"]);
            None
        }
    };
    o.finish(out);
    result
}

fn reference(o: &mut Obj<'_>, out: &mut Out) -> Option<Reference> {
    let mode = o.opt_choice("compare", COMPARE, out);
    let expect = o.opt_string("expect", out);
    let model = o.opt_string("model", out);
    let provider = o.opt_string("provider", out);
    // A meaning check that says what to expect needs no reference image.
    let image = match (mode, &expect) {
        (Some(CompareMode::Meaning), Some(e)) if !e.trim().is_empty() => {
            Some(o.opt_string("image", out).unwrap_or_default())
        }
        _ => o.string("image", out),
    };
    let threshold = o
        .get("threshold")
        .is_some()
        .then(|| o.number("threshold", out));
    let area = o.opt_object("area", out).and_then(|a| rect(a, out));
    let ignore = rects(o, "ignore", out);
    let hide = rects(o, "hide", out);

    let compare = match (mode.unwrap_or(CompareMode::Similar), threshold) {
        (CompareMode::Similar, None) => Some(Compare::default()),
        (CompareMode::Similar, Some(t)) => t.map(|threshold| Compare::Similar { threshold }),
        (CompareMode::Exact, None) => Some(Compare::Exact),
        (CompareMode::Meaning, None) => Some(Compare::Meaning {
            expect: expect.clone().unwrap_or_default(),
            model: model.clone().unwrap_or_default().trim().to_owned(),
            provider: provider.clone().unwrap_or_default().trim().to_owned(),
        }),
        (_, Some(_)) => {
            out.push(o.issue(
                "flow.reference.thresholdNeedsSimilar",
                &o.path_of("threshold"),
            ));
            None
        }
    };
    let compare = if expect.is_some() && !matches!(mode, Some(CompareMode::Meaning)) {
        out.push(o.issue("flow.reference.expectNeedsMeaning", &o.path_of("expect")));
        None
    } else if model.is_some() && !matches!(mode, Some(CompareMode::Meaning)) {
        out.push(o.issue("flow.reference.modelNeedsMeaning", &o.path_of("model")));
        None
    } else if provider.is_some() && !matches!(mode, Some(CompareMode::Meaning)) {
        out.push(o.issue(
            "flow.reference.providerNeedsMeaning",
            &o.path_of("provider"),
        ));
        None
    } else {
        compare
    };
    o.finish(out);
    Some(Reference {
        image: image?,
        compare: compare?,
        area,
        ignore,
        hide,
    })
}

fn wait_until(o: &Obj<'_>, value: &Value, out: &mut Out) -> Option<WaitUntil> {
    if value.as_str() == Some("screenStable") {
        return Some(WaitUntil::ScreenStable);
    }
    const KEYS: [&str; 3] = ["appears", "disappears", "element"];
    let mut until = o.child("until", value, out)?;
    let which = until.one_of(&KEYS, out);
    let result = which.and_then(|key| {
        if key == "element" {
            let e = until.object(key, out)?;
            return element_check(e, out).map(WaitUntil::Element);
        }
        let r = until
            .object(key, out)
            .and_then(|mut r| reference(&mut r, out))?;
        Some(if key == "appears" {
            WaitUntil::Appears(r)
        } else {
            WaitUntil::Disappears(r)
        })
    });
    if which.is_none() {
        until.known.extend(KEYS);
    }
    until.finish(out);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bump_to_2(map: &mut Map<String, Value>) {
        if let Some(name) = map.remove("title") {
            map.insert("name".into(), name);
        }
    }

    fn bump_to_3(map: &mut Map<String, Value>) {
        map.insert("touched".into(), true.into());
    }

    fn root(json: &str) -> Map<String, Value> {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn migration_runs_only_missing_steps_in_order() {
        let steps: &[Migration] = &[bump_to_2, bump_to_3];

        let mut from_1 = root(r#"{"format": 1, "title": "t"}"#);
        migrate(&mut from_1, 1, steps).unwrap();
        assert_eq!(
            from_1,
            root(r#"{"format": 3, "name": "t", "touched": true}"#)
        );

        let mut from_2 = root(r#"{"format": 2, "title": "kept"}"#);
        migrate(&mut from_2, 1, steps).unwrap();
        assert_eq!(
            from_2,
            root(r#"{"format": 3, "title": "kept", "touched": true}"#)
        );
    }

    #[test]
    fn migration_rejects_versions_out_of_range() {
        let steps: &[Migration] = &[bump_to_2];
        let code = |json: &str| migrate(&mut root(json), 1, steps).unwrap_err().code;
        assert_eq!(code(r#"{"format": 3}"#), "flow.format.tooNew");
        assert_eq!(code(r#"{"format": 0}"#), "flow.format.unsupported");
        assert_eq!(code(r#"{}"#), "flow.format.missing");
        assert_eq!(code(r#"{"format": "1"}"#), "flow.field.type");
    }

    #[test]
    fn format_2_changes_nothing_in_a_format_1_test() {
        let mut map = root(r#"{"format": 1, "name": "t", "blocks": {"a": {"file": "x.csv"}}}"#);
        migrate(&mut map, OLDEST_FORMAT, &MIGRATIONS[..1]).unwrap();
        assert_eq!(
            map,
            root(r#"{"format": 2, "name": "t", "blocks": {"a": {"file": "x.csv"}}}"#)
        );
        assert_eq!(FORMAT_CHANGES[0], "flow.format.2");
    }

    #[test]
    fn current_format_needs_no_migration() {
        let mut map = root(&format!(r#"{{"format": {}}}"#, crate::FORMAT_VERSION));
        let before = map.clone();
        migrate(&mut map, OLDEST_FORMAT, MIGRATIONS).unwrap();
        assert_eq!(map, before);
    }
}
