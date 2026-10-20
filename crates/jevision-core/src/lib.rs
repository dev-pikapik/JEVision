//! Core of JEVision: the model of a flowchart UI test, its file format and its validation.
//!
//! A test is one `*.flow.json` file. [`parse`] reads it (upgrading older formats),
//! [`validate`] lists problems as localizable [`Issue`]s, and [`to_canonical_json`] writes it
//! back in the one canonical form, so the same test always produces the same bytes in git.

mod batch;
mod board;
mod canvas;
mod ci;
mod data;
mod decode;
mod diff;
mod edit;
mod env;
mod index;
mod issue;
mod layout;
mod merge;
mod model;
mod project;
mod route;
mod schedule;
mod search;
mod template;
mod validate;

pub use batch::{
    Batch, BatchTest, BlockReason, Blocker, DEFAULT_PARALLEL, Loaded, MAX_PARALLEL, RunMode,
    TestOutcome, TestState, WaitingFor, dependency_issues, resolve_test,
};
pub use board::{
    BOARD_SUFFIX, BOARDS_FOLDER, Board, BoardEdge, BoardLayout, BoardNode, BoardNodeKind,
    CARD_HEIGHT, CARD_WIDTH, FRAME_HEADER, FrameTests, IMAGE_SUFFIXES, IMAGES_FOLDER,
    LaidBoardEdge, LaidNode, OPEN_HEADER, blocks_area, is_image_file,
};
pub use canvas::CanvasItem;
pub use ci::{
    CI_WORKFLOW_FILE, CiWorkflow, CiWorkflowError, ci_workflow, ci_workflow_text, repository_root,
    write_ci_workflow,
};
pub use data::{DataError, DataTable, parse_row_choice, primary_data_file, validate_data};
pub use decode::{DEFAULT_TASK_TIMEOUT_MS, DEFAULT_WAIT_TIMEOUT_MS, parse, parse_block};
pub use diff::{BlockChange, BlockChangeKind, FlowDiff, diff};
pub use edit::{DEFAULT_SCROLL_LINES, START, Slot};
pub use env::{
    AddEnvironmentError, ENVIRONMENTS_FILE, ENVIRONMENTS_FORMAT, EnvironmentValues, Environments,
    required_env_values, validate_environment_values,
};
pub use index::{ARROW_REACH, BoardHeading, BoardIndex, BoardVisible, RectIndex};
pub use issue::{Issue, IssueParam, ParseError, Severity, describe};
pub use layout::{
    BLOCK_HEIGHT, BLOCK_WIDTH, FlowLayout, GRID, LaidBlock, LaidEdge, LaidFrame, LaidSlot, layout,
};
pub use merge::{
    ConflictKind, ConflictPart, MergeChoice, MergeConflict, merge_conflicts, merge_flows,
};
pub use model::*;
pub use project::{FolderRunMode, PROJECT_FILE, PROJECT_FORMAT, ProjectSettings, Quarantined};
pub use route::route_around;
pub use schedule::{
    CalendarInterval, EVERY_HOURS, MAX_LATE_MINUTES, Repeat, SCHEDULE_FILE, SCHEDULE_FORMAT,
    SCHEDULE_HISTORY_FILE, SCHEDULE_HISTORY_KEPT, SCHEDULE_VIDEO_MODES, Schedule, ScheduleHistory,
    ScheduleTests, ScheduledOutcome, ScheduledRun, SkipReason, TimeOfDay, Weekday,
    begin_schedule_check, calendar_intervals, due_schedules, launch_agent_plist, load_schedules,
    parse_schedules, record_scheduled_run, save_schedules, schedule_agent_label, schedules_to_json,
};
pub use search::{SearchCandidate, rank, test_keywords};
pub use template::{BUILTIN_TEMPLATES, BlockInput, InputField};
pub use validate::{
    MAX_REPEAT, MAX_RETRIES, MAX_SCROLL, MAX_TIMEOUT_MS, required_inputs, required_secrets,
    saved_variables, unbreakable_placeholders, validate, validate_in,
};

/// File format version written by this build. Files with an older `format` are upgraded on read.
pub const FORMAT_VERSION: u32 = decode::OLDEST_FORMAT + decode::MIGRATIONS.len() as u32;

/// Writes `flow` in canonical form: fixed key order, blocks in reading order, settings that are
/// off or empty left out but the click button, comparison and wait timeout always written,
/// two-space indentation and a trailing newline.
pub fn to_canonical_json(flow: &Flow) -> String {
    let mut text = serde_json::to_string_pretty(flow).expect("a flow always serializes to JSON");
    text.push('\n');
    text
}

/// Reads a test file and writes it back in canonical form.
pub fn canonicalize(text: &str) -> Result<String, ParseError> {
    parse(text).map(|flow| to_canonical_json(&flow))
}

/// Palette group of a block type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BlockGroup {
    Actions,
    Waiting,
    Checks,
    /// Conditions and loops.
    Logic,
    SubTests,
    /// Test data, and blocks that work without the screen: the clipboard, date and time,
    /// requests, files, databases, commands and scripts.
    Data,
}

/// A block type as offered in the editor's palette.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockSpec {
    /// Value of `type` in the file; also the base of the block's localization keys.
    pub type_name: String,
    pub group: BlockGroup,
    /// Names of the outgoing edges the block can have.
    pub edges: Vec<String>,
}

const CATALOG: &[(&str, BlockGroup, &[&str])] = &[
    ("openApp", BlockGroup::Actions, &[NEXT]),
    ("openFile", BlockGroup::Actions, &[NEXT]),
    ("click", BlockGroup::Actions, &[NEXT]),
    ("hover", BlockGroup::Actions, &[NEXT]),
    ("drag", BlockGroup::Actions, &[NEXT]),
    ("scroll", BlockGroup::Actions, &[NEXT]),
    ("type", BlockGroup::Actions, &[NEXT]),
    ("pressKeys", BlockGroup::Actions, &[NEXT]),
    ("waitFor", BlockGroup::Waiting, &[NEXT]),
    ("pause", BlockGroup::Waiting, &[NEXT]),
    ("check", BlockGroup::Checks, &[NEXT]),
    ("if", BlockGroup::Logic, &[THEN, ELSE]),
    ("repeat", BlockGroup::Logic, &[BODY, NEXT]),
    ("runTest", BlockGroup::SubTests, &[NEXT]),
    ("useData", BlockGroup::Data, &[BODY, NEXT]),
    ("clipboard", BlockGroup::Data, &[NEXT]),
    ("dateTime", BlockGroup::Data, &[NEXT]),
    ("http", BlockGroup::Data, &[NEXT]),
    ("database", BlockGroup::Data, &[NEXT]),
    ("checkFile", BlockGroup::Data, &[NEXT]),
    ("command", BlockGroup::Data, &[NEXT]),
    ("script", BlockGroup::Data, &[NEXT]),
];

/// Every block type, in palette order.
pub fn catalog() -> Vec<BlockSpec> {
    CATALOG
        .iter()
        .map(|(name, group, edges)| BlockSpec {
            type_name: (*name).to_owned(),
            group: *group,
            edges: edges.iter().map(|e| (*e).to_owned()).collect(),
        })
        .collect()
}
