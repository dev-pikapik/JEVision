//! Core of JEVision: the model of a flowchart UI test, its file format and its validation.
//!
//! A test is one `*.flow.json` file. [`parse`] reads it (upgrading older formats),
//! [`validate`] lists problems as localizable [`Issue`]s, and [`to_canonical_json`] writes it
//! back in the one canonical form, so the same test always produces the same bytes in git.

mod decode;
mod diff;
mod edit;
mod issue;
mod layout;
mod model;
mod validate;

pub use decode::{DEFAULT_WAIT_TIMEOUT_MS, parse, parse_block};
pub use diff::{BlockChange, BlockChangeKind, FlowDiff, diff};
pub use edit::{START, Slot};
pub use issue::{Issue, IssueParam, ParseError, Severity, describe};
pub use layout::{
    BLOCK_HEIGHT, BLOCK_WIDTH, FlowLayout, LaidBlock, LaidEdge, LaidFrame, LaidSlot, layout,
};
pub use model::*;
pub use validate::{MAX_REPEAT, MAX_RETRIES, MAX_TIMEOUT_MS, required_secrets, validate};

/// File format version written by this build. Files with an older `format` are upgraded on read.
pub const FORMAT_VERSION: u32 = decode::OLDEST_FORMAT + decode::MIGRATIONS.len() as u32;

/// Writes `flow` in canonical form: fixed key order, blocks sorted by id, defaults left out,
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
    WaitAndCheck,
    Logic,
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
    ("type", BlockGroup::Actions, &[NEXT]),
    ("pressKeys", BlockGroup::Actions, &[NEXT]),
    ("waitFor", BlockGroup::WaitAndCheck, &[NEXT]),
    ("check", BlockGroup::WaitAndCheck, &[NEXT]),
    ("if", BlockGroup::Logic, &[THEN, ELSE]),
    ("repeat", BlockGroup::Logic, &[BODY, NEXT]),
    ("runTest", BlockGroup::Logic, &[NEXT]),
    ("useData", BlockGroup::Data, &[BODY, NEXT]),
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
