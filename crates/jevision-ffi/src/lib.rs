//! UniFFI surface of the JEVision core for the native apps.
//!
//! The model types are declared here as remote types: each declaration mirrors the type in
//! `jevision-core` field by field (the compiler rejects any mismatch), so the bindings expose
//! the core model itself instead of a converted copy.

use std::collections::HashMap;

use jevision_core::{
    Block, BlockGroup, BlockKind, BlockSpec, CanvasContent, CanvasItem, CanvasSection, Compare,
    ElseIf, Flow, FlowLayout, Issue, IssueParam, LaidBlock, LaidEdge, LaidFrame, LaidSlot,
    MouseButton, Note, Point, Priority, Rect, Reference, SearchCandidate, Severity, Slot, Target,
    TestStatus, TestType, WaitUntil,
};

uniffi::setup_scaffolding!();

mod env;
mod git;
mod history;
mod last_run;
mod map;
mod mcp;
mod project;
mod remote;
mod run;
mod schedule;
mod signin;
mod vision;

pub use env::*;
pub use git::*;
pub use history::*;
pub use last_run::*;
pub use map::*;
pub use mcp::*;
pub use project::*;
pub use remote::*;
pub use run::*;
pub use schedule::*;

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

/// The problems in a test saved in `folder`, including warnings about the images and files it
/// reads that are not there.
#[uniffi::export]
pub fn validate_flow_in(flow: Flow, folder: String) -> Vec<Issue> {
    jevision_core::validate_in(&flow, std::path::Path::new(&folder))
}

/// English sentence for a problem, saying which block and field it concerns.
#[uniffi::export]
pub fn describe_issue(issue: Issue) -> String {
    jevision_core::describe(&issue)
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

/// The picture a run compares with when the app is drawn dark: `refs/inbox.png` →
/// `refs/inbox.dark.png`.
#[uniffi::export]
pub fn reference_dark_variant(image: String) -> String {
    jevision_core::dark_variant(&image)
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

/// Step of the canvas grid, in canvas points.
#[uniffi::export]
pub fn canvas_grid() -> f64 {
    jevision_core::GRID
}

/// A test after an edit on the canvas, and the items to select.
#[derive(uniffi::Record)]
pub struct CanvasEdit {
    pub flow: Flow,
    pub items: Vec<CanvasItem>,
}

/// Moves `items` by `dx`, `dy` canvas points, snapped to the grid. `None` when nothing moves.
#[uniffi::export]
pub fn move_on_canvas(mut flow: Flow, items: Vec<CanvasItem>, dx: f64, dy: f64) -> Option<Flow> {
    flow.move_items(&items, dx, dy).then_some(flow)
}

/// Puts block `id` with its top-left corner at `at`, snapped to the grid.
#[uniffi::export]
pub fn place_block(mut flow: Flow, id: String, at: Point) -> Option<Flow> {
    flow.place_block(&id, at).then_some(flow)
}

/// Gives a section or a note a new frame, snapped to the grid.
#[uniffi::export]
pub fn resize_on_canvas(mut flow: Flow, item: CanvasItem, rect: Rect) -> Option<Flow> {
    flow.resize_item(&item, rect).then_some(flow)
}

/// Forgets where blocks were placed by hand, so the test lays out automatically again.
#[uniffi::export]
pub fn arrange_automatically(mut flow: Flow) -> Flow {
    flow.arrange_automatically();
    flow
}

/// Where a block added after block `id` goes: `then` of an `if`, `next` of any other block.
/// `None` for an unknown block.
#[uniffi::export]
pub fn slot_after_block(flow: Flow, id: String) -> Option<Slot> {
    flow.slot_after(&id)
}

/// Adds one more condition to `selection` when it is an `if`, otherwise a new `if` after it
/// (after the main chain without a selection), and selects that `if`. `None` for an unknown
/// block.
#[uniffi::export]
pub fn add_condition_at(mut flow: Flow, selection: Option<String>) -> Option<Edited> {
    let id = flow.add_condition_at(selection.as_deref())?;
    Some(Edited {
        flow,
        selection: Some(id),
    })
}

/// Removes every link into and out of block `id`, so it stands alone. `None` when it had none.
#[uniffi::export]
pub fn disconnect_block(mut flow: Flow, id: String) -> Option<Flow> {
    flow.disconnect(&id).then_some(flow)
}

/// Words the test in `json` is found by besides its file name: its name, description, block
/// texts and notes. Empty when it doesn't read as a test.
#[uniffi::export]
pub fn test_keywords(json: String) -> String {
    jevision_core::parse(&json)
        .map(|flow| jevision_core::test_keywords(&flow))
        .unwrap_or_default()
}

/// `text` for display with its `${…}` placeholders kept whole across line breaks.
#[uniffi::export]
pub fn unbreakable_placeholders(text: String) -> String {
    jevision_core::unbreakable_placeholders(&text)
}

/// Indices of the candidates that match `query`, best first; all of them for an empty query.
#[uniffi::export]
pub fn rank_search(query: String, candidates: Vec<SearchCandidate>) -> Vec<u32> {
    jevision_core::rank(&query, &candidates)
        .into_iter()
        .filter_map(|i| u32::try_from(i).ok())
        .collect()
}

/// The `if` block `kind` with one more condition, checked after the others; it leads where
/// `else` does until linked elsewhere.
#[uniffi::export]
pub fn add_if_condition(mut kind: BlockKind) -> BlockKind {
    kind.add_condition();
    kind
}

/// The `if` block `kind` without its condition `index` (0 is `reference`); the only condition
/// stays.
#[uniffi::export]
pub fn remove_if_condition(mut kind: BlockKind, index: u32) -> BlockKind {
    kind.remove_condition(index as usize);
    kind
}

/// The `if` block `kind` with its condition `from` moved to place `to`, way out included.
#[uniffi::export]
pub fn move_if_condition(mut kind: BlockKind, from: u32, to: u32) -> BlockKind {
    kind.move_condition(from as usize, to as usize);
    kind
}

/// Position in `elseIf` of the condition behind way out `edge` of an `if`: 0 for `elseIf1`,
/// `None` for any other edge.
#[uniffi::export]
pub fn else_if_index(edge: String) -> Option<u32> {
    jevision_core::else_if_index(&edge).and_then(|n| u32::try_from(n).ok())
}

/// Points edge `edge` of block `from` at `to`, or disconnects it. `None` when the edge can't
/// go there: an unknown edge or block, or a cycle other than through a loop's body.
#[uniffi::export]
pub fn connect_blocks(
    mut flow: Flow,
    from: String,
    edge: String,
    to: Option<String>,
) -> Option<Flow> {
    flow.connect(&from, &edge, to.as_deref()).then_some(flow)
}

/// Adds a section titled `title` with the frame `rect` and selects it.
#[uniffi::export]
pub fn add_section(mut flow: Flow, title: String, rect: Rect) -> CanvasEdit {
    let item = flow.add_section(&title, rect);
    CanvasEdit {
        flow,
        items: vec![item],
    }
}

/// Adds a note at `at` written by `author` at `time` (RFC 3339) and selects it.
#[uniffi::export]
pub fn add_note(
    mut flow: Flow,
    text: String,
    author: String,
    time: String,
    at: Point,
) -> CanvasEdit {
    let item = flow.add_note(&text, &author, &time, at);
    CanvasEdit {
        flow,
        items: vec![item],
    }
}

/// Removes `items`, joining the neighbours of removed blocks, and selects the block that took
/// the place of the last one.
#[uniffi::export]
pub fn remove_from_canvas(mut flow: Flow, items: Vec<CanvasItem>) -> Edited {
    let selection = flow.remove_items(&items);
    Edited { flow, selection }
}

/// `items` as the text of a test file, for the clipboard.
#[uniffi::export]
pub fn copy_from_canvas(flow: Flow, items: Vec<CanvasItem>) -> String {
    flow.copy_items(&items)
}

/// Adds the blocks, sections and notes of a test file's `text`, after block `after` when it
/// can take them, and selects them. `None` when the text isn't a test.
#[uniffi::export]
pub fn paste_on_canvas(mut flow: Flow, text: String, after: Option<String>) -> Option<CanvasEdit> {
    let items = flow.paste_items(&text, after.as_deref())?;
    Some(CanvasEdit { flow, items })
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
    pub retries: Option<u32>,
    pub start: String,
    pub blocks: Vec<Block>,
    pub canvas: CanvasContent,
}

#[uniffi::remote(Record)]
pub struct CanvasContent {
    pub positions: HashMap<String, Point>,
    pub sections: Vec<CanvasSection>,
    pub notes: Vec<Note>,
}

#[uniffi::remote(Record)]
pub struct CanvasSection {
    pub id: String,
    pub title: String,
    pub rect: Rect,
}

#[uniffi::remote(Record)]
pub struct Note {
    pub id: String,
    pub text: String,
    pub author: String,
    pub time: String,
    pub rect: Rect,
}

#[uniffi::remote(Enum)]
pub enum CanvasItem {
    Block { id: String },
    Section { id: String },
    Note { id: String },
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
        else_if: Vec<ElseIf>,
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

#[uniffi::remote(Record)]
pub struct ElseIf {
    pub reference: Reference,
    pub then: Option<String>,
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

#[uniffi::remote(Record)]
pub struct SearchCandidate {
    pub title: String,
    pub keywords: String,
}

#[uniffi::remote(Enum)]
pub enum BlockGroup {
    Actions,
    Waiting,
    Checks,
    Logic,
    SubTests,
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
    pub ports: Vec<LaidSlot>,
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
