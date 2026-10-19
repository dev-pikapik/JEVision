//! Reading a test file: JSON text → format migration → [`Flow`].
//!
//! Decoding is hand-written rather than derived so that every problem becomes an [`Issue`]
//! with a stable code and parameters, and so that all problems are reported at once.

use serde_json::{Map, Value};

use crate::issue::{Issue, ParseError};
use crate::model::{
    Block, BlockKind, CanvasContent, CanvasSection, Compare, ElseIf, Flow, MouseButton, Note,
    Point, Priority, Rect, Reference, ScrollDirection, ScrollUnit, Target, TestStatus, TestType,
    WaitUntil, parse_duration,
};

/// Oldest file format this build can read.
pub(crate) const OLDEST_FORMAT: u32 = 1;

/// Upgrades the root object of a file by one format version.
pub(crate) type Migration = fn(&mut Map<String, Value>);

/// `MIGRATIONS[i]` upgrades a file from format `OLDEST_FORMAT + i` to the next version.
/// Appending a step raises [`crate::FORMAT_VERSION`] by one.
pub(crate) const MIGRATIONS: &[Migration] = &[];

/// Reads a test file, upgrading older formats to the current one.
///
/// Fails with every structural problem found: invalid JSON, unsupported format, missing or
/// mistyped fields, unknown fields and block types. Problems that only concern the meaning of
/// a well-formed test (broken edges, empty text, out-of-range values) are left to
/// [`crate::validate`], so an editor can still open such a test and let the user fix it.
pub fn parse(text: &str) -> Result<Flow, ParseError> {
    let fail = |issue| ParseError {
        issues: vec![issue],
    };
    let mut root: Value = serde_json::from_str(text).map_err(|e| {
        fail(
            Issue::error("flow.json.syntax")
                .with("line", e.line())
                .with("column", e.column()),
        )
    })?;
    let Value::Object(map) = &mut root else {
        return Err(fail(Issue::error("flow.json.notObject")));
    };
    migrate(map, OLDEST_FORMAT, MIGRATIONS).map_err(fail)?;

    let mut issues = Vec::new();
    let flow = flow(Obj::root(map), &mut issues);
    match flow {
        Some(flow) if issues.is_empty() => Ok(flow),
        _ => Err(ParseError { issues }),
    }
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
    match block_kind(Obj::root(map), &mut issues) {
        Some(kind) if issues.is_empty() => Ok(kind),
        _ => Err(ParseError { issues }),
    }
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
            self.wrong_type(key, "array of strings", out);
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

    fn finish(self, out: &mut Out) {
        for key in self.map.keys() {
            if !self.known.contains(&key.as_str()) {
                out.push(self.issue("flow.field.unknown", &self.path_of(key)));
            }
        }
    }
}

const TEST_TYPES: &[(&str, TestType)] = &[
    ("smoke", TestType::Smoke),
    ("regression", TestType::Regression),
    ("acceptance", TestType::Acceptance),
];
const PRIORITIES: &[(&str, Priority)] = &[
    ("critical", Priority::Critical),
    ("high", Priority::High),
    ("medium", Priority::Medium),
    ("low", Priority::Low),
];
const STATUSES: &[(&str, TestStatus)] = &[
    ("draft", TestStatus::Draft),
    ("ready", TestStatus::Ready),
    ("outdated", TestStatus::Outdated),
];
const BUTTONS: &[(&str, MouseButton)] =
    &[("left", MouseButton::Left), ("right", MouseButton::Right)];
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

/// Default for `waitFor.timeout` when the file leaves it out.
pub const DEFAULT_WAIT_TIMEOUT_MS: u64 = 10_000;

fn flow(mut o: Obj<'_>, out: &mut Out) -> Option<Flow> {
    o.get("format");
    let name = o.string("name", out);
    let description = o.opt_string("description", out);
    let app = o.string("app", out);
    let test_type = o.opt_choice("testType", TEST_TYPES, out);
    let priority = o.opt_choice("priority", PRIORITIES, out);
    let status = o.opt_choice("status", STATUSES, out);
    let tags = o.strings("tags", out).unwrap_or_default();
    let retries = o.opt_uint("retries", out);
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
        app: app?,
        test_type,
        priority,
        status,
        tags,
        retries,
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
            let rect = n.object("frame", out).and_then(|f| rect(f, out));
            n.finish(out);
            if let (Some(text), Some(rect)) = (text, rect) {
                canvas.notes.push(Note {
                    id: id.clone(),
                    text,
                    author,
                    time,
                    rect,
                });
            }
        }
    }
    o.finish(out);
    canvas
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
        let block_obj = Obj {
            map,
            block: Some(id),
            path: String::new(),
            known: Vec::new(),
        };
        if let Some(kind) = block_kind(block_obj, out) {
            blocks.push(Block {
                id: id.clone(),
                kind,
            });
        }
    }
    blocks
}

fn block_kind(mut o: Obj<'_>, out: &mut Out) -> Option<BlockKind> {
    let type_name = o.string("type", out)?;
    let next = |o: &mut Obj<'_>, out: &mut Out| o.opt_string("next", out);
    let kind = match type_name.as_str() {
        "openApp" => {
            let app = o.opt_string("app", out);
            let next = next(&mut o, out);
            Some(BlockKind::OpenApp { app, next })
        }
        "openFile" => {
            let files = o.strings("files", out);
            let files = o.require("files", files, out);
            let app = o.opt_string("app", out);
            let new_instance = o.flag("newInstance", out);
            let next = next(&mut o, out);
            all_present(|| {
                Some(BlockKind::OpenFile {
                    files: files?,
                    app,
                    new_instance,
                    next,
                })
            })
        }
        "click" => {
            let target = o.object("target", out).and_then(|t| target(t, out));
            let button = o.opt_choice("button", BUTTONS, out);
            let double = o.flag("double", out);
            let wait_for_change = o.flag("waitForChange", out);
            let next = next(&mut o, out);
            all_present(|| {
                Some(BlockKind::Click {
                    target: target?,
                    button: button.unwrap_or(MouseButton::Left),
                    double,
                    wait_for_change,
                    next,
                })
            })
        }
        "hover" => {
            let target = o.object("target", out).and_then(|t| target(t, out));
            let wait_for_change = o.flag("waitForChange", out);
            let next = next(&mut o, out);
            all_present(|| {
                Some(BlockKind::Hover {
                    target: target?,
                    wait_for_change,
                    next,
                })
            })
        }
        "drag" => {
            let from = o.object("from", out).and_then(|t| target(t, out));
            let to = o.object("to", out).and_then(|t| target(t, out));
            let wait_for_change = o.flag("waitForChange", out);
            let next = next(&mut o, out);
            all_present(|| {
                Some(BlockKind::Drag {
                    from: from?,
                    to: to?,
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
            let at = o.opt_object("at", out).and_then(|t| target(t, out));
            let wait_for_change = o.flag("waitForChange", out);
            let next = next(&mut o, out);
            all_present(|| {
                Some(BlockKind::Scroll {
                    direction: direction?,
                    amount: amount?,
                    unit: unit.unwrap_or(ScrollUnit::Lines),
                    at,
                    wait_for_change,
                    next,
                })
            })
        }
        "type" => {
            let text = o.string("text", out);
            let into = o.opt_object("into", out).and_then(|t| target(t, out));
            let next = next(&mut o, out);
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
            let next = next(&mut o, out);
            all_present(|| Some(BlockKind::PressKeys { keys: keys?, next }))
        }
        "waitFor" => {
            let until = o.get("until");
            let until = o.require("until", until, out);
            let until = until.and_then(|v| wait_until(&o, v, out));
            let timeout_ms = o.duration("timeout", DEFAULT_WAIT_TIMEOUT_MS, out);
            let next = next(&mut o, out);
            all_present(|| {
                Some(BlockKind::WaitFor {
                    until: until?,
                    timeout_ms: timeout_ms?,
                    next,
                })
            })
        }
        "check" => {
            let reference = o.object("reference", out).and_then(|r| reference(r, out));
            let next = next(&mut o, out);
            all_present(|| {
                Some(BlockKind::Check {
                    reference: reference?,
                    next,
                })
            })
        }
        "if" => {
            let reference = o.object("reference", out).and_then(|r| reference(r, out));
            let then = o.opt_string("then", out);
            let else_if = else_ifs(&mut o, out);
            let otherwise = o.opt_string("else", out);
            all_present(|| {
                Some(BlockKind::If {
                    reference: reference?,
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
            let next = next(&mut o, out);
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
            let next = next(&mut o, out);
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
            let next = next(&mut o, out);
            all_present(|| Some(BlockKind::RunTest { test: test?, next }))
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

fn target(mut o: Obj<'_>, out: &mut Out) -> Option<Target> {
    let which = o.one_of(&["image", "point"], out);
    let t = match which {
        Some("image") => o.string("image", out).map(Target::Image),
        Some(_) => o
            .object("point", out)
            .and_then(|p| point(p, out))
            .map(Target::Point),
        None => {
            o.known.extend(["image", "point"]);
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
            let reference = case
                .object("reference", out)
                .and_then(|r| reference(r, out));
            let then = case.opt_string("then", out);
            case.finish(out);
            Some(ElseIf {
                reference: reference?,
                then,
            })
        })
        .collect();
    cases.into_iter().collect()
}

fn reference(mut o: Obj<'_>, out: &mut Out) -> Option<Reference> {
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
        .map
        .contains_key("threshold")
        .then(|| o.number("threshold", out));
    let area = o.opt_object("area", out).and_then(|a| rect(a, out));
    let ignore = rects(&mut o, "ignore", out);
    let hide = rects(&mut o, "hide", out);

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
    let mut until = o.child("until", value, out)?;
    let which = until.one_of(&["appears", "disappears"], out);
    let result = which.and_then(|key| {
        let r = until.object(key, out).and_then(|r| reference(r, out))?;
        Some(if key == "appears" {
            WaitUntil::Appears(r)
        } else {
            WaitUntil::Disappears(r)
        })
    });
    if which.is_none() {
        until.known.extend(["appears", "disappears"]);
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
    fn current_format_needs_no_migration() {
        let mut map = root(&format!(r#"{{"format": {}}}"#, crate::FORMAT_VERSION));
        let before = map.clone();
        migrate(&mut map, OLDEST_FORMAT, MIGRATIONS).unwrap();
        assert_eq!(map, before);
    }
}
