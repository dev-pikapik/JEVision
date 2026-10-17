//! In-memory model of a test: a flowchart of blocks joined by named edges.
//!
//! Field order in this module is the key order of the canonical file, so reordering fields
//! changes every saved test.

use std::collections::HashMap;

use serde::Serialize;
use serde::ser::{SerializeMap, Serializer};

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
    /// How many times a failed run is retried before it counts as failed.
    pub retries: u32,
    /// Id of the first block to run.
    pub start: String,
    /// Blocks in any order; saved in reading order, see [`Flow::blocks_in_flow_order`].
    pub blocks: Vec<Block>,
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
        /// May contain `${secret.NAME}` and `${data.COLUMN}` placeholders.
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
    /// Goes to `then` if the screen matches the reference, otherwise to `else`.
    If {
        reference: Reference,
        #[serde(skip_serializing_if = "Option::is_none")]
        then: Option<String>,
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
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
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
    /// Parts blurred out before a screenshot is sent anywhere or stored in a report.
    pub hide: Vec<Rect>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Compare {
    /// Pixel by pixel.
    Exact,
    /// At least `threshold` (0–1] similar.
    Similar { threshold: f64 },
    /// Same meaning, judged by a vision-language model.
    Meaning,
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
        }
    }

    /// Outgoing edges as `(edge name, target block id)`.
    pub fn edges(&self) -> Vec<(&'static str, &str)> {
        let mut out = Vec::new();
        match self {
            Self::If {
                then, otherwise, ..
            } => {
                out.extend(then.as_deref().map(|t| (THEN, t)));
                out.extend(otherwise.as_deref().map(|t| (ELSE, t)));
            }
            Self::Repeat { body, next, .. } | Self::UseData { body, next, .. } => {
                out.push((BODY, body.as_str()));
                out.extend(next.as_deref().map(|t| (NEXT, t)));
            }
            Self::OpenApp { next, .. }
            | Self::OpenFile { next, .. }
            | Self::Click { next, .. }
            | Self::Type { next, .. }
            | Self::PressKeys { next, .. }
            | Self::WaitFor { next, .. }
            | Self::Check { next, .. }
            | Self::RunTest { next, .. } => out.extend(next.as_deref().map(|t| (NEXT, t))),
        }
        out
    }

    /// Whether the block loops over its `body` edge.
    pub fn is_loop(&self) -> bool {
        matches!(self, Self::Repeat { .. } | Self::UseData { .. })
    }
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
        if self.retries != 0 {
            map.serialize_entry("retries", &self.retries)?;
        }
        map.serialize_entry("start", &self.start)?;
        map.serialize_entry("blocks", &BlocksInFlowOrder(self))?;
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
        map.serialize_entry("image", &self.image)?;
        match self.compare {
            Compare::Exact => map.serialize_entry("compare", "exact")?,
            Compare::Meaning => map.serialize_entry("compare", "meaning")?,
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

fn is_false(b: &bool) -> bool {
    !b
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
