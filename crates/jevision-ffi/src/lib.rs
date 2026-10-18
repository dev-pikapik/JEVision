//! UniFFI surface of the JEVision core for the native apps.
//!
//! The model types are declared here as remote types: each declaration mirrors the type in
//! `jevision-core` field by field (the compiler rejects any mismatch), so the bindings expose
//! the core model itself instead of a converted copy.

use jevision_core::{
    Block, BlockGroup, BlockKind, BlockSpec, Compare, Flow, FlowLayout, Issue, IssueParam,
    LaidBlock, LaidEdge, LaidFrame, LaidSlot, MouseButton, Point, Priority, Rect, Reference,
    Severity, Slot, Target, TestStatus, TestType, WaitUntil,
};

uniffi::setup_scaffolding!();

mod history;
mod mcp;
mod run;
mod vision;

pub use history::*;
pub use mcp::*;
pub use run::*;

/// The text is not a readable test file.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum FlowError {
    #[error("test file cannot be read")]
    Invalid { issues: Vec<Issue> },
}

/// File format version written by this build.
#[uniffi::export]
pub fn format_version() -> u32 {
    jevision_core::FORMAT_VERSION
}

/// Version of this build of JEVision, such as `0.1.0`.
#[uniffi::export]
pub fn jevision_version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

/// Reads a test file's text, upgrading older formats.
#[uniffi::export]
pub fn load_flow(json: String) -> Result<Flow, FlowError> {
    jevision_core::parse(&json).map_err(|e| FlowError::Invalid { issues: e.issues })
}

/// Lists the problems in a test, errors and warnings, in a stable order.
#[uniffi::export]
pub fn validate_flow(flow: Flow) -> Vec<Issue> {
    jevision_core::validate(&flow)
}

/// Writes a test in canonical form, ready to be saved to its `*.flow.json` file.
#[uniffi::export]
pub fn save_flow(flow: Flow) -> String {
    jevision_core::to_canonical_json(&flow)
}

/// Names of the secrets the test reads, so the app can show which ones are set.
#[uniffi::export]
pub fn required_secrets(flow: Flow) -> Vec<String> {
    jevision_core::required_secrets(&flow)
}

/// Every block type, in palette order.
#[uniffi::export]
pub fn block_catalog() -> Vec<BlockSpec> {
    jevision_core::catalog()
}

/// Where the editor draws the blocks, frames and connectors of a test.
#[uniffi::export]
pub fn layout_flow(flow: Flow) -> FlowLayout {
    jevision_core::layout(&flow)
}

/// A test after an edit, and the block to select.
#[derive(uniffi::Record)]
pub struct Edited {
    pub flow: Flow,
    pub selection: Option<String>,
}

/// Adds a block of type `type_name` on `slot` (after the main chain without one) and selects
/// it. `None` for an unknown type or slot.
#[uniffi::export]
pub fn insert_block(mut flow: Flow, type_name: String, slot: Option<Slot>) -> Option<Edited> {
    let id = flow.insert_block(&type_name, slot.as_ref())?;
    Some(Edited {
        flow,
        selection: Some(id),
    })
}

/// Adds `blocks` as one chain on `slot` (after the main chain without one) and selects the
/// last of them. `None` when there is nothing to add or the blocks can't be chained there.
#[uniffi::export]
pub fn insert_blocks(mut flow: Flow, blocks: Vec<BlockKind>, slot: Option<Slot>) -> Option<Edited> {
    let ids = flow.insert_blocks(blocks, slot.as_ref())?;
    Some(Edited {
        flow,
        selection: ids.last().cloned(),
    })
}

/// Removes block `id`, joining its neighbours, and selects the block that took its place.
#[uniffi::export]
pub fn remove_block(mut flow: Flow, id: String) -> Edited {
    let selection = flow.remove_block(&id);
    Edited { flow, selection }
}

#[uniffi::remote(Record)]
pub struct Flow {
    pub name: String,
    pub description: Option<String>,
    pub app: String,
    pub test_type: Option<TestType>,
    pub priority: Option<Priority>,
    pub status: Option<TestStatus>,
    pub tags: Vec<String>,
    pub retries: u32,
    pub start: String,
    pub blocks: Vec<Block>,
}

#[uniffi::remote(Enum)]
pub enum TestType {
    Smoke,
    Regression,
    Acceptance,
}

#[uniffi::remote(Enum)]
pub enum Priority {
    Critical,
    High,
    Medium,
    Low,
}

#[uniffi::remote(Enum)]
pub enum TestStatus {
    Draft,
    Ready,
    Outdated,
}

#[uniffi::remote(Record)]
pub struct Block {
    pub id: String,
    pub kind: BlockKind,
}

#[uniffi::remote(Enum)]
pub enum BlockKind {
    OpenApp {
        app: Option<String>,
        next: Option<String>,
    },
    OpenFile {
        files: Vec<String>,
        app: Option<String>,
        new_instance: bool,
        next: Option<String>,
    },
    Click {
        target: Target,
        button: MouseButton,
        double: bool,
        wait_for_change: bool,
        next: Option<String>,
    },
    Type {
        text: String,
        into: Option<Target>,
        next: Option<String>,
    },
    PressKeys {
        keys: String,
        next: Option<String>,
    },
    WaitFor {
        until: WaitUntil,
        timeout_ms: u64,
        next: Option<String>,
    },
    Check {
        reference: Reference,
        next: Option<String>,
    },
    If {
        reference: Reference,
        then: Option<String>,
        otherwise: Option<String>,
    },
    Repeat {
        times: u32,
        body: String,
        next: Option<String>,
    },
    UseData {
        file: String,
        body: String,
        next: Option<String>,
    },
    RunTest {
        test: String,
        next: Option<String>,
    },
}

#[uniffi::remote(Enum)]
pub enum Target {
    Image(String),
    Point(Point),
}

#[uniffi::remote(Record)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[uniffi::remote(Record)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[uniffi::remote(Enum)]
pub enum MouseButton {
    Left,
    Right,
}

#[uniffi::remote(Enum)]
pub enum WaitUntil {
    Appears(Reference),
    Disappears(Reference),
    ScreenStable,
}

#[uniffi::remote(Record)]
pub struct Reference {
    pub image: String,
    pub compare: Compare,
    pub area: Option<Rect>,
    pub ignore: Vec<Rect>,
    pub hide: Vec<Rect>,
}

#[uniffi::remote(Enum)]
pub enum Compare {
    Exact,
    Similar { threshold: f64 },
    Meaning { expect: String },
}

#[uniffi::remote(Record)]
pub struct Issue {
    pub code: String,
    pub severity: Severity,
    pub params: Vec<IssueParam>,
}

#[uniffi::remote(Record)]
pub struct IssueParam {
    pub name: String,
    pub value: String,
}

#[uniffi::remote(Enum)]
pub enum Severity {
    Error,
    Warning,
}

#[uniffi::remote(Record)]
pub struct BlockSpec {
    pub type_name: String,
    pub group: BlockGroup,
    pub edges: Vec<String>,
}

#[uniffi::remote(Enum)]
pub enum BlockGroup {
    Actions,
    WaitAndCheck,
    Logic,
    Data,
}

#[uniffi::remote(Record)]
pub struct Slot {
    pub from: String,
    pub edge: String,
}

#[uniffi::remote(Record)]
pub struct FlowLayout {
    pub width: f64,
    pub height: f64,
    pub blocks: Vec<LaidBlock>,
    pub frames: Vec<LaidFrame>,
    pub edges: Vec<LaidEdge>,
    pub slots: Vec<LaidSlot>,
}

#[uniffi::remote(Record)]
pub struct LaidBlock {
    pub id: String,
    pub rect: Rect,
}

#[uniffi::remote(Record)]
pub struct LaidFrame {
    pub block: String,
    pub rect: Rect,
}

#[uniffi::remote(Record)]
pub struct LaidEdge {
    pub from: String,
    pub edge: String,
    pub to: String,
    pub points: Vec<Point>,
}

#[uniffi::remote(Record)]
pub struct LaidSlot {
    pub slot: Slot,
    pub at: Point,
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = include_str!("../../../examples/sign-in.flow.json");

    #[test]
    fn load_validate_save_round_trip() {
        let flow = load_flow(EXAMPLE.to_owned()).unwrap();
        assert!(validate_flow(flow.clone()).is_empty());
        assert_eq!(save_flow(flow), EXAMPLE);
    }

    #[test]
    fn edits_return_the_block_to_select() {
        let flow = load_flow(EXAMPLE.to_owned()).unwrap();
        let after_open = Slot {
            from: "open".into(),
            edge: "next".into(),
        };
        let added = insert_block(flow.clone(), "click".into(), Some(after_open)).unwrap();
        assert_eq!(added.selection.as_deref(), Some("click"));
        assert_eq!(layout_flow(added.flow.clone()).blocks.len(), 7);
        let removed = remove_block(added.flow, "click".into());
        assert_eq!(removed.selection.as_deref(), Some("email"));
        assert_eq!(removed.flow, flow);
        assert!(insert_block(flow, "nope".into(), None).is_none());
    }

    #[test]
    fn version_is_the_workspace_version() {
        let workspace = include_str!("../../../Cargo.toml");
        assert!(workspace.contains(&format!("\nversion = \"{}\"\n", jevision_version())));
    }

    #[test]
    fn unreadable_text_reports_issues() {
        let FlowError::Invalid { issues } = load_flow("{".to_owned()).unwrap_err();
        assert_eq!(issues[0].code, "flow.json.syntax");
    }
}
