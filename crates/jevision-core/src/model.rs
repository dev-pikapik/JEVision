//! In-memory model of a test: a flowchart of blocks joined by named edges.
//!
//! Field order in this module is the key order of the canonical file, so reordering fields
//! changes every saved test.

use std::borrow::Cow;
use std::collections::HashMap;

use serde::ser::{SerializeMap, Serializer};
use serde::{Deserialize, Serialize};

use crate::FORMAT_VERSION;

/// A UI test: metadata plus a flowchart that starts at `start`.
#[derive(Debug, Clone, PartialEq)]
pub struct Flow {
    pub name: String,
    pub description: Option<String>,
    /// Bundle identifier of the application under test, e.g. `com.apple.TextEdit`.
    pub app: String,
    pub test_type: Option<TestType>,
    pub priority: Option<Priority>,
    pub status: Option<TestStatus>,
    pub tags: Vec<String>,
    /// How many times a failed run is retried before it counts as failed; `None` uses the
    /// project's default retries.
    pub retries: Option<u32>,
    /// Id of the first block to run.
    pub start: String,
    /// Blocks in any order; saved in reading order, see [`Flow::blocks_in_flow_order`].
    pub blocks: Vec<Block>,
    /// How the test is drawn; changes nothing in how it runs.
    pub canvas: CanvasContent,
}

/// The drawing of a test on the editor canvas: blocks placed by hand, section frames and sticky
/// notes. Coordinates are canvas points from the top-left corner.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CanvasContent {
    /// Top-left corners of blocks, by block id. Empty: every block is laid out automatically.
    pub positions: HashMap<String, Point>,
    pub sections: Vec<CanvasSection>,
    pub notes: Vec<Note>,
}

/// A titled frame that groups the blocks drawn inside it, like a heading in a text.
#[derive(Debug, Clone, PartialEq)]
pub struct CanvasSection {
    /// Unique among the sections of the test.
    pub id: String,
    pub title: String,
    pub rect: Rect,
}

/// A sticky note on the canvas.
#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    /// Unique among the notes of the test.
    pub id: String,
    pub text: String,
    /// Who last wrote the note; empty when unknown.
    pub author: String,
    /// When the note was last written, as an RFC 3339 timestamp; empty when unknown.
    pub time: String,
    pub rect: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TestType {
    Smoke,
    Regression,
    Acceptance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Priority {
    Critical,
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TestStatus {
    Draft,
    Ready,
    Outdated,
}

/// One step of the flowchart.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    /// Unique within the test; edges refer to blocks by id.
    pub id: String,
    pub kind: BlockKind,
}

/// What a block does, with its parameters and outgoing edges.
///
/// `next` is the block that runs afterwards; a block without `next` ends the run (or, inside a
/// loop body, the current pass). Paths are relative to the folder of the test file.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum BlockKind {
    /// Launches an application and brings it to the front.
    OpenApp {
        /// Bundle id; `None` opens the application under test.
        #[serde(skip_serializing_if = "Option::is_none")]
        app: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        next: Option<String>,
    },
    /// Opens files or web addresses, each file from a fresh copy so the original stays intact.
    OpenFile {
        /// Relative file paths or `http(s)://` addresses.
        files: Vec<String>,
        /// Bundle id; `None` uses the system default application.
        #[serde(skip_serializing_if = "Option::is_none")]
        app: Option<String>,
        #[serde(skip_serializing_if = "is_false")]
        new_instance: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        next: Option<String>,
    },
    Click {
        target: Target,
        button: MouseButton,
        #[serde(skip_serializing_if = "is_false")]
        double: bool,
        /// After clicking, wait until the screen changes; fail if it does not.
        #[serde(skip_serializing_if = "is_false")]
        wait_for_change: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        next: Option<String>,
    },
    /// Types text, optionally clicking into a field first.
    Type {
        /// May contain `${secret.NAME}`, `${env.NAME}` and `${data.COLUMN}` placeholders.
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        into: Option<Target>,
        #[serde(skip_serializing_if = "Option::is_none")]
        next: Option<String>,
    },
    /// Presses one key combination, e.g. `cmd+shift+s`.
    PressKeys {
        keys: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        next: Option<String>,
    },
    /// Waits for the screen to reach a state; fails after `timeout`.
    WaitFor {
        until: WaitUntil,
        #[serde(rename = "timeout", serialize_with = "duration")]
        timeout_ms: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        next: Option<String>,
    },
    /// Fails the run unless the screen matches the reference.
    Check {
        reference: Reference,
        #[serde(skip_serializing_if = "Option::is_none")]
        next: Option<String>,
    },
    /// Goes to `then` if the condition holds, else to the first of `else_if` whose condition
    /// holds, otherwise to `else`.
    If {
        #[serde(flatten)]
        condition: Condition,
        #[serde(skip_serializing_if = "Option::is_none")]
        then: Option<String>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        else_if: Vec<ElseIf>,
        #[serde(rename = "else", skip_serializing_if = "Option::is_none")]
        otherwise: Option<String>,
    },
    /// Runs the chain starting at `body` `times` times, then goes to `next`.
    /// The body loops by linking back to this block.
    Repeat {
        times: u32,
        body: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        next: Option<String>,
    },
    /// Runs the chain starting at `body` once per row of a CSV file, then goes to `next`.
    /// Row values are available as `${data.COLUMN}`.
    UseData {
        file: String,
        body: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        next: Option<String>,
    },
    /// Runs another test file in place, as a sub-process.
    RunTest {
        test: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        next: Option<String>,
    },
    /// Runs a script. Inputs reach it as environment variables; it writes its outputs as
    /// `name=value` lines to the file named by `$JEV_OUTPUT`, and they become `${var.NAME}`.
    /// A script that exits with a code other than 0 fails the block.
    Script {
        language: ScriptLanguage,
        code: String,
        /// Values may contain placeholders.
        #[serde(skip_serializing_if = "Vec::is_empty")]
        inputs: Vec<NamedValue>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        outputs: Vec<ScriptOutput>,
        #[serde(rename = "timeout", serialize_with = "duration")]
        timeout_ms: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        next: Option<String>,
    },
    /// Runs a command line with `zsh -c`; fails unless it exits with `exit_code`.
    Command {
        /// May contain placeholders.
        command: String,
        /// Folder the command runs in, relative to the folder of the test; `None` runs it there.
        #[serde(skip_serializing_if = "Option::is_none")]
        folder: Option<String>,
        #[serde(skip_serializing_if = "is_zero")]
        exit_code: i32,
        /// Run variable that gets what the command printed, without the last line break.
        #[serde(skip_serializing_if = "Option::is_none")]
        save_output: Option<String>,
        #[serde(rename = "timeout", serialize_with = "duration")]
        timeout_ms: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        next: Option<String>,
    },
    /// Sends an HTTP request; fails unless the answer has the expected status.
    Http {
        method: HttpMethod,
        /// May contain placeholders, as may header values and the body.
        url: String,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        headers: Vec<NamedValue>,
        #[serde(skip_serializing_if = "String::is_empty")]
        body: String,
        #[serde(skip_serializing_if = "ExpectStatus::is_success")]
        expect_status: ExpectStatus,
        /// Parts of the answer kept as run variables.
        #[serde(skip_serializing_if = "Vec::is_empty")]
        save: Vec<SavedValue>,
        #[serde(rename = "timeout", serialize_with = "duration")]
        timeout_ms: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        next: Option<String>,
    },
    /// Waits until a file reaches a state; fails after `timeout`.
    CheckFile {
        /// Relative to the folder of the test, absolute, or from the home folder (`~/`); may
        /// contain placeholders.
        path: String,
        until: FileState,
        #[serde(rename = "timeout", serialize_with = "duration")]
        timeout_ms: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        next: Option<String>,
    },
    /// Runs an SQL query on an SQLite database file. With `expect`, runs it again until the
    /// answer is as expected, and fails after `timeout`.
    Database {
        /// A path as in [`BlockKind::CheckFile`].
        file: String,
        /// May contain placeholders.
        query: String,
        /// Run variable that gets the first value of the first row.
        #[serde(skip_serializing_if = "Option::is_none")]
        save_as: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        expect: Option<DatabaseExpect>,
        #[serde(rename = "timeout", serialize_with = "duration")]
        timeout_ms: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        next: Option<String>,
    },
}

/// What an `if` way out checks.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Condition {
    /// The screen matches the reference.
    #[serde(rename = "reference")]
    Screen(Reference),
    /// A value, usually a run variable, compares with another.
    Value(ValueCheck),
}

impl Condition {
    /// The reference a screen condition compares with.
    pub fn reference(&self) -> Option<&Reference> {
        match self {
            Self::Screen(r) => Some(r),
            Self::Value(_) => None,
        }
    }
}

/// `of` `is` `to`: `${var.status}` equals `200`. Both sides may contain placeholders.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ValueCheck {
    pub of: String,
    pub is: ValueTest,
    pub to: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ValueTest {
    Equals,
    NotEquals,
    Contains,
    /// Both sides read as numbers.
    GreaterThan,
    LessThan,
}

/// A name with a text, such as a script input or an HTTP header.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NamedValue {
    pub name: String,
    pub value: String,
}

/// A value a script reports, kept as the run variable `name`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScriptOutput {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: OutputKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OutputKind {
    Text,
    Number,
    /// Kept as `true` or `false`.
    Boolean,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ScriptLanguage {
    Sh,
    Zsh,
    Python3,
    Node,
    AppleScript,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
}

/// Status an HTTP answer must have for the block to pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpectStatus {
    /// Any of 200–299.
    Success,
    Any,
    Exactly(u16),
}

impl ExpectStatus {
    pub fn is_success(&self) -> bool {
        *self == Self::Success
    }

    pub fn allows(&self, status: u16) -> bool {
        match self {
            Self::Success => (200..300).contains(&status),
            Self::Any => true,
            Self::Exactly(code) => *code == status,
        }
    }
}

impl Serialize for ExpectStatus {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Success => s.serialize_str("success"),
            Self::Any => s.serialize_str("any"),
            Self::Exactly(code) => s.serialize_u16(*code),
        }
    }
}

/// A part of an HTTP answer kept as the run variable `name`. `from` is `status`, `body`, a
/// field of a JSON body such as `body.items[0].id`, or a header such as `header.Location`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SavedValue {
    pub name: String,
    pub from: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FileState {
    Exists,
    Missing,
    /// Changed since the run started, or created during it.
    Changed,
    /// Exists and contains the text, which may contain placeholders.
    Contains(String),
}

/// What a database query must answer.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DatabaseExpect {
    /// The first value of the first row, as text; may contain placeholders.
    Value(String),
    /// How many rows the query returns.
    Rows(u32),
}

/// One more exit of an `if`: taken when the earlier conditions did not hold and this one does.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ElseIf {
    #[serde(flatten)]
    pub condition: Condition,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub then: Option<String>,
}

/// Where on screen an action happens.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Target {
    /// Found by look: the image of the element, located on the current screenshot.
    Image(String),
    /// A fixed point, in fractions of the window size.
    Point(Point),
}

/// A point in fractions (0–1) of the window, from the top-left corner.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

/// A rectangle in fractions (0–1) of the image or window, from the top-left corner.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MouseButton {
    Left,
    Right,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WaitUntil {
    /// The reference shows up on screen.
    Appears(Reference),
    /// The reference is no longer on screen.
    Disappears(Reference),
    /// The screen stops changing.
    ScreenStable,
}

/// A reference image ("how it should look") and how to compare the screen with it.
#[derive(Debug, Clone, PartialEq)]
pub struct Reference {
    /// Path of the reference image.
    pub image: String,
    pub compare: Compare,
    /// Part of the image to compare; `None` compares the whole image.
    pub area: Option<Rect>,
    /// Parts left out of the comparison: clocks, avatars, counters.
    pub ignore: Vec<Rect>,
    /// Parts painted over in grey before a screenshot is sent anywhere or stored in a report.
    pub hide: Vec<Rect>,
}

/// `refs/inbox.png` → `refs/inbox.dark.png`: the reference a run compares with when the app is
/// drawn dark. Without one, the light reference stands in.
pub fn dark_variant(image: &str) -> String {
    let name_start = image.rfind('/').map_or(0, |i| i + 1);
    match image[name_start..].rfind('.') {
        Some(dot) if dot > 0 => {
            let at = name_start + dot;
            format!("{}.dark{}", &image[..at], &image[at..])
        }
        _ => format!("{image}.dark"),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Compare {
    /// Pixel by pixel.
    Exact,
    /// At least `threshold` (0–1] similar.
    Similar { threshold: f64 },
    /// Same meaning, judged by a vision model: the screen shows what `expect` says, or means
    /// the same as the reference image when `expect` is empty.
    Meaning { expect: String },
}

impl Compare {
    pub const DEFAULT_THRESHOLD: f64 = 0.95;
}

impl Default for Compare {
    fn default() -> Self {
        Self::Similar {
            threshold: Self::DEFAULT_THRESHOLD,
        }
    }
}

/// Names of the edges between blocks.
pub const NEXT: &str = "next";
pub const THEN: &str = "then";
pub const ELSE: &str = "else";
pub const BODY: &str = "body";
/// Prefix of the names of the extra exits of an `if`, see [`else_if_edge`].
pub const ELSE_IF: &str = "elseIf";

impl BlockKind {
    /// The `type` value of this block in the file.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::OpenApp { .. } => "openApp",
            Self::OpenFile { .. } => "openFile",
            Self::Click { .. } => "click",
            Self::Type { .. } => "type",
            Self::PressKeys { .. } => "pressKeys",
            Self::WaitFor { .. } => "waitFor",
            Self::Check { .. } => "check",
            Self::If { .. } => "if",
            Self::Repeat { .. } => "repeat",
            Self::UseData { .. } => "useData",
            Self::RunTest { .. } => "runTest",
            Self::Script { .. } => "script",
            Self::Command { .. } => "command",
            Self::Http { .. } => "http",
            Self::CheckFile { .. } => "checkFile",
            Self::Database { .. } => "database",
        }
    }

    /// The block fails the run when the app isn't as expected: a check, a wait for something to
    /// appear or disappear, an HTTP answer status, a file state, or a database answer.
    pub fn checks(&self) -> bool {
        matches!(
            self,
            Self::Check { .. }
                | Self::WaitFor {
                    until: WaitUntil::Appears(_) | WaitUntil::Disappears(_),
                    ..
                }
                | Self::Http { .. }
                | Self::CheckFile { .. }
                | Self::Database {
                    expect: Some(_),
                    ..
                }
        )
    }

    /// Names of the outgoing edges this block can have, in drawing order.
    pub fn edge_names(&self) -> Vec<Cow<'static, str>> {
        match self {
            Self::If { else_if, .. } => std::iter::once(Cow::Borrowed(THEN))
                .chain((0..else_if.len()).map(|n| Cow::Owned(else_if_edge(n))))
                .chain([Cow::Borrowed(ELSE)])
                .collect(),
            Self::Repeat { .. } | Self::UseData { .. } => {
                vec![Cow::Borrowed(BODY), Cow::Borrowed(NEXT)]
            }
            _ => vec![Cow::Borrowed(NEXT)],
        }
    }

    /// Outgoing edges as `(edge name, target block id)`, in drawing order.
    pub fn edges(&self) -> Vec<(Cow<'static, str>, &str)> {
        let mut out = Vec::new();
        match self {
            Self::If {
                then,
                else_if,
                otherwise,
                ..
            } => {
                out.extend(then.as_deref().map(|t| (Cow::Borrowed(THEN), t)));
                for (n, case) in else_if.iter().enumerate() {
                    out.extend(
                        case.then
                            .as_deref()
                            .map(|t| (Cow::Owned(else_if_edge(n)), t)),
                    );
                }
                out.extend(otherwise.as_deref().map(|t| (Cow::Borrowed(ELSE), t)));
            }
            Self::Repeat { body, next, .. } | Self::UseData { body, next, .. } => {
                out.push((Cow::Borrowed(BODY), body.as_str()));
                out.extend(next.as_deref().map(|t| (Cow::Borrowed(NEXT), t)));
            }
            Self::OpenApp { next, .. }
            | Self::OpenFile { next, .. }
            | Self::Click { next, .. }
            | Self::Type { next, .. }
            | Self::PressKeys { next, .. }
            | Self::WaitFor { next, .. }
            | Self::Check { next, .. }
            | Self::RunTest { next, .. }
            | Self::Script { next, .. }
            | Self::Command { next, .. }
            | Self::Http { next, .. }
            | Self::CheckFile { next, .. }
            | Self::Database { next, .. } => {
                out.extend(next.as_deref().map(|t| (Cow::Borrowed(NEXT), t)));
            }
        }
        out
    }
}

/// Name of the exit of an `if` taken when its `else_if[n]` matches: `elseIf1` for the first.
pub fn else_if_edge(n: usize) -> String {
    format!("{ELSE_IF}{}", n + 1)
}

/// Position in `else_if` of the case behind the exit called `name`, as in [`else_if_edge`].
pub fn else_if_index(name: &str) -> Option<usize> {
    let digits = name.strip_prefix(ELSE_IF)?;
    if digits.starts_with('0') || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse::<usize>().ok()?.checked_sub(1)
}

impl Serialize for Flow {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(None)?;
        map.serialize_entry("format", &FORMAT_VERSION)?;
        map.serialize_entry("name", &self.name)?;
        if let Some(d) = &self.description {
            map.serialize_entry("description", d)?;
        }
        map.serialize_entry("app", &self.app)?;
        if let Some(t) = &self.test_type {
            map.serialize_entry("testType", t)?;
        }
        if let Some(p) = &self.priority {
            map.serialize_entry("priority", p)?;
        }
        if let Some(st) = &self.status {
            map.serialize_entry("status", st)?;
        }
        if !self.tags.is_empty() {
            map.serialize_entry("tags", &self.tags)?;
        }
        if let Some(retries) = self.retries {
            map.serialize_entry("retries", &retries)?;
        }
        map.serialize_entry("start", &self.start)?;
        map.serialize_entry("blocks", &BlocksInFlowOrder(self))?;
        let canvas = CanvasOfFlow(self);
        if !canvas.is_empty() {
            map.serialize_entry("canvas", &canvas)?;
        }
        map.end()
    }
}

/// The canvas as saved: positions of blocks that exist, everything sorted by id, whole numbers
/// without a fraction.
struct CanvasOfFlow<'a>(&'a Flow);

impl CanvasOfFlow<'_> {
    fn positions(&self) -> Vec<(&str, &Point)> {
        let mut out: Vec<(&str, &Point)> = self
            .0
            .canvas
            .positions
            .iter()
            .filter(|(id, _)| self.0.blocks.iter().any(|b| &b.id == *id))
            .map(|(id, p)| (id.as_str(), p))
            .collect();
        out.sort_by_key(|(id, _)| *id);
        out
    }

    fn is_empty(&self) -> bool {
        self.positions().is_empty()
            && self.0.canvas.sections.is_empty()
            && self.0.canvas.notes.is_empty()
    }
}

impl Serialize for CanvasOfFlow<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let canvas = &self.0.canvas;
        let mut map = s.serialize_map(None)?;
        let positions = self.positions();
        if !positions.is_empty() {
            let at: Vec<(&str, Coords)> = positions
                .into_iter()
                .map(|(id, p)| (id, Coords(vec![("x", p.x), ("y", p.y)])))
                .collect();
            map.serialize_entry("positions", &Entries(at))?;
        }
        if !canvas.sections.is_empty() {
            let mut sections: Vec<&CanvasSection> = canvas.sections.iter().collect();
            sections.sort_by(|a, b| a.id.cmp(&b.id));
            map.serialize_entry(
                "sections",
                &Entries(sections.into_iter().map(|s| (s.id.as_str(), s)).collect()),
            )?;
        }
        if !canvas.notes.is_empty() {
            let mut notes: Vec<&Note> = canvas.notes.iter().collect();
            notes.sort_by(|a, b| a.id.cmp(&b.id));
            map.serialize_entry(
                "notes",
                &Entries(notes.into_iter().map(|n| (n.id.as_str(), n)).collect()),
            )?;
        }
        map.end()
    }
}

/// Key–value pairs written as a JSON object in the given order.
struct Entries<'a, T>(Vec<(&'a str, T)>);

impl<T: Serialize> Serialize for Entries<'_, T> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(Some(self.0.len()))?;
        for (key, value) in &self.0 {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

/// Canvas coordinates: whole numbers are written without a fraction.
struct Coords(Vec<(&'static str, f64)>);

impl Coords {
    fn of(r: &Rect) -> Self {
        Self(vec![
            ("x", r.x),
            ("y", r.y),
            ("width", r.width),
            ("height", r.height),
        ])
    }
}

impl Serialize for Coords {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(Some(self.0.len()))?;
        for (key, v) in &self.0 {
            if v.fract() == 0.0 && v.abs() < 1e15 {
                map.serialize_entry(key, &(*v as i64))?;
            } else {
                map.serialize_entry(key, v)?;
            }
        }
        map.end()
    }
}

impl Serialize for CanvasSection {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(None)?;
        map.serialize_entry("title", &self.title)?;
        map.serialize_entry("frame", &Coords::of(&self.rect))?;
        map.end()
    }
}

impl Serialize for Note {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(None)?;
        map.serialize_entry("text", &self.text)?;
        if !self.author.is_empty() {
            map.serialize_entry("author", &self.author)?;
        }
        if !self.time.is_empty() {
            map.serialize_entry("time", &self.time)?;
        }
        map.serialize_entry("frame", &Coords::of(&self.rect))?;
        map.end()
    }
}

struct BlocksInFlowOrder<'a>(&'a Flow);

impl Serialize for BlocksInFlowOrder<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let blocks = self.0.blocks_in_flow_order();
        let mut map = s.serialize_map(Some(blocks.len()))?;
        for b in blocks {
            map.serialize_entry(&b.id, &b.kind)?;
        }
        map.end()
    }
}

impl Flow {
    /// Blocks in reading order: depth-first from `start`, following edges in their listed
    /// order, then blocks not reachable from `start` sorted by id. With duplicate ids only the
    /// first block of each id is visited from the graph.
    pub fn blocks_in_flow_order(&self) -> Vec<&Block> {
        let index: HashMap<&str, usize> = self
            .blocks
            .iter()
            .enumerate()
            .rev()
            .map(|(i, b)| (b.id.as_str(), i))
            .collect();
        let mut placed = vec![false; self.blocks.len()];
        let mut order = Vec::with_capacity(self.blocks.len());
        let mut stack = vec![self.start.as_str()];
        while let Some(id) = stack.pop() {
            let Some(&i) = index.get(id) else { continue };
            if std::mem::replace(&mut placed[i], true) {
                continue;
            }
            let block = &self.blocks[i];
            order.push(block);
            stack.extend(block.kind.edges().iter().rev().map(|(_, t)| *t));
        }
        let mut rest: Vec<&Block> = self
            .blocks
            .iter()
            .zip(&placed)
            .filter(|(_, placed)| !**placed)
            .map(|(b, _)| b)
            .collect();
        rest.sort_by(|a, b| a.id.cmp(&b.id));
        order.extend(rest);
        order
    }
}

impl Serialize for Reference {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(None)?;
        if !self.image.is_empty() || !matches!(self.compare, Compare::Meaning { .. }) {
            map.serialize_entry("image", &self.image)?;
        }
        match &self.compare {
            Compare::Exact => map.serialize_entry("compare", "exact")?,
            Compare::Meaning { expect } => {
                map.serialize_entry("compare", "meaning")?;
                if !expect.is_empty() {
                    map.serialize_entry("expect", expect)?;
                }
            }
            Compare::Similar { threshold } => {
                map.serialize_entry("compare", "similar")?;
                map.serialize_entry("threshold", &threshold)?;
            }
        }
        if let Some(area) = &self.area {
            map.serialize_entry("area", area)?;
        }
        if !self.ignore.is_empty() {
            map.serialize_entry("ignore", &self.ignore)?;
        }
        if !self.hide.is_empty() {
            map.serialize_entry("hide", &self.hide)?;
        }
        map.end()
    }
}

/// One step of a path into JSON: a field of an object or a position in an array.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonStep<'a> {
    Field(&'a str),
    Index(usize),
}

/// Splits a path such as `items[0].id` or `[2].name` into steps; `None` when it is malformed.
pub fn json_path(path: &str) -> Option<Vec<JsonStep<'_>>> {
    let mut steps = Vec::new();
    for (n, part) in path.split('.').enumerate() {
        let (name, mut rest) = part.split_at(part.find('[').unwrap_or(part.len()));
        if name.is_empty() && (n > 0 || rest.is_empty()) {
            return None;
        }
        if !name.is_empty() {
            if name.contains(']') {
                return None;
            }
            steps.push(JsonStep::Field(name));
        }
        while !rest.is_empty() {
            let inner = rest.strip_prefix('[')?;
            let end = inner.find(']')?;
            let digits = &inner[..end];
            if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            steps.push(JsonStep::Index(digits.parse().ok()?));
            rest = &inner[end + 1..];
        }
    }
    Some(steps)
}

fn is_false(b: &bool) -> bool {
    !b
}

fn is_zero(n: &i32) -> bool {
    *n == 0
}

fn duration<S: Serializer>(ms: &u64, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&format_duration(*ms))
}

/// Writes milliseconds in the largest unit that divides them exactly: `90s`, `2m`, `1500ms`.
pub fn format_duration(ms: u64) -> String {
    match ms {
        0 => "0s".to_owned(),
        _ if ms % 60_000 == 0 => format!("{}m", ms / 60_000),
        _ if ms % 1000 == 0 => format!("{}s", ms / 1000),
        _ => format!("{ms}ms"),
    }
}

/// Reads `500ms`, `10s` or `2m` into milliseconds.
pub fn parse_duration(text: &str) -> Option<u64> {
    let (digits, factor) = [("ms", 1), ("s", 1000), ("m", 60_000)]
        .into_iter()
        .find_map(|(unit, factor)| Some((text.strip_suffix(unit)?, factor)))?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse::<u64>().ok()?.checked_mul(factor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_variants_go_before_the_extension() {
        assert_eq!(dark_variant("refs/inbox.png"), "refs/inbox.dark.png");
        assert_eq!(dark_variant("a.b/inbox"), "a.b/inbox.dark");
        assert_eq!(dark_variant(".hidden"), ".hidden.dark");
        assert_eq!(dark_variant("refs/.hidden.png"), "refs/.hidden.dark.png");
    }

    #[test]
    fn durations_round_trip_in_largest_exact_unit() {
        for (text, ms, canonical) in [
            ("1500ms", 1500, "1500ms"),
            ("10s", 10_000, "10s"),
            ("120s", 120_000, "2m"),
            ("2000ms", 2000, "2s"),
            ("0s", 0, "0s"),
        ] {
            assert_eq!(parse_duration(text), Some(ms), "{text}");
            assert_eq!(format_duration(ms), canonical);
        }
        for bad in ["", "s", "10", "1.5s", "-1s", "10h", " 5s", "+5s"] {
            assert_eq!(parse_duration(bad), None, "{bad:?}");
        }
    }
}
