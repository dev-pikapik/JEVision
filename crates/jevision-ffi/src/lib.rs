//! UniFFI surface of the JEVision core for the native apps.
//!
//! The model types are declared here as remote types: each declaration mirrors the type in
//! `jevision-core` field by field (the compiler rejects any mismatch), so the bindings expose
//! the core model itself instead of a converted copy.

use std::collections::HashMap;

use jevision_core::{
    Attachment, AttachmentKind, Block, BlockGroup, BlockInput, BlockKind, BlockSpec, CanvasContent,
    CanvasItem, CanvasSection, CheckedTest, ClipboardAction, ClipboardContent, Compare, Condition,
    DatabaseExpect, ElseIf, ExpectStatus, FileState, Flow, FlowLayout, HttpMethod, InputField,
    Issue, IssueParam, LaidBlock, LaidEdge, LaidFrame, LaidSlot, Modifiers, MouseButton,
    NamedValue, Note, NoteAnchor, PackageNeed, Point, Priority, Rect, Reference, Reply, Resolved,
    SavedValue, ScriptInput, ScriptLanguage, ScriptOutput, ScrollDirection, ScrollUnit,
    SearchCandidate, Severity, Slot, Target, TestCard, TestFilter, TestStatus, TestType, TimeShift,
    TimeUnit, ValueCheck, ValueKind, ValueTest, WaitUntil, WindowPosition, WindowSize,
};

uniffi::setup_scaffolding!();

mod about;
mod ai;
mod batch;
mod board;
mod bug;
mod ci;
mod ci_runs;
mod community;
mod env;
mod git;
mod history;
mod last_run;
mod map;
mod mcp;
mod notes;
mod notify;
mod project;
mod projects;
mod references;
mod remote;
mod run;
mod schedule;
mod signin;
mod stats;
mod vision;

pub use about::*;
pub use ai::*;
pub use batch::*;
pub use board::*;
pub use bug::*;
pub use ci::*;
pub use ci_runs::*;
pub use community::*;
pub use env::*;
pub use git::*;
pub use history::*;
pub use last_run::*;
pub use map::*;
pub use mcp::*;
pub use notes::*;
pub use project::*;
pub use projects::*;
pub use references::*;
pub use remote::*;
pub use run::*;
pub use schedule::*;
pub use stats::*;

/// The text is not a readable test file.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum FlowError {
    #[error("{}", jevision_l10n::tr!("flow.file.unreadable"))]
    Invalid { issues: Vec<Issue> },
}

/// Sets the language of every text the core writes, such as `ru` or `en-GB`. A language the
/// catalog has no translation for falls back to English.
#[uniffi::export]
pub fn set_language(tag: String) {
    jevision_l10n::set_language(&tag);
}

/// Sets how numbers are written in the language of [`set_language`]: the system's decimal
/// separator and the one between groups of thousands, such as `,` and a no-break space. An empty
/// `decimal` goes back to the language's own separator.
#[uniffi::export]
pub fn set_number_format(decimal: String, grouping: String) {
    jevision_l10n::set_number_format(&decimal, &grouping);
}

/// The language the core writes its texts in, such as `ru`; English until `set_language`.
#[uniffi::export]
pub fn current_language() -> String {
    jevision_l10n::language()
}

/// The languages the catalog has translations for, English first.
#[uniffi::export]
pub fn catalog_languages() -> Vec<String> {
    jevision_l10n::languages().to_vec()
}

/// A value a localized text refers to: text goes into `%@`, a number into `%lld` and decides the
/// plural form.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum TextArg {
    Text { value: String },
    Number { value: i64 },
}

/// The text of a catalog key in the current language, with its values filled in.
#[uniffi::export]
pub fn localized_text(key: String, args: Vec<TextArg>) -> String {
    let args: Vec<jevision_l10n::Arg> = args
        .into_iter()
        .map(|a| match a {
            TextArg::Text { value } => jevision_l10n::Arg::Text(value),
            TextArg::Number { value } => jevision_l10n::Arg::Number(value),
        })
        .collect();
    jevision_l10n::text(&key, &args)
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

/// The problems of each test and custom block in the folder `project` and its folders, in the
/// order of their paths: files the project lacks, named otherwise than on disk, too big for Git,
/// or found only on this computer.
#[uniffi::export]
pub fn check_project(project: String) -> Vec<CheckedTest> {
    jevision_core::check_project(std::path::Path::new(&project))
}

/// A sentence for a problem in the current language, saying which block and field it concerns.
#[uniffi::export]
pub fn describe_issue(issue: Issue) -> String {
    jevision_core::describe(&issue)
}

/// What a problem says in the current language, without the block and field: the app shows it
/// next to them.
#[uniffi::export]
pub fn issue_sentence(issue: Issue) -> String {
    jevision_core::sentence(&issue)
}

/// Writes a test in canonical form, ready to be saved to its `*.flow.json` file.
#[uniffi::export]
pub fn save_flow(flow: Flow) -> String {
    jevision_core::to_canonical_json(&flow)
}

/// Names the test's blocks save for `${var.NAME}`, for suggestions in the editor.
#[uniffi::export]
pub fn saved_variables(flow: Flow) -> Vec<String> {
    jevision_core::saved_variables(&flow)
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

/// The codes of the languages a date and time block writes month and day names in.
#[uniffi::export]
pub fn date_languages() -> Vec<String> {
    jevision_core::DATE_LANGUAGES
        .iter()
        .map(|l| (*l).to_owned())
        .collect()
}

/// Whether `name` is a time zone a date and time block knows, such as `Europe/Berlin`.
#[uniffi::export]
pub fn time_zone_known(name: String) -> bool {
    jevision_core::time_zone_known(&name)
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

/// Removes block `id`, joining its neighbours, and selects the block that took its place. The
/// notes about it become notes about the test.
#[uniffi::export]
pub fn remove_block(mut flow: Flow, id: String) -> Edited {
    let selection = flow.remove_block(&id);
    Edited { flow, selection }
}

/// Like [`remove_block`], and the notes about the removed blocks go with them.
#[uniffi::export]
pub fn remove_block_with_notes(mut flow: Flow, id: String) -> Edited {
    let selection = flow.remove_block_with_notes(&id);
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

/// Moves `items` by `dx`, `dy` canvas points, snapped to the grid when `snap`. `None` when
/// nothing moves.
#[uniffi::export]
pub fn move_on_canvas(
    mut flow: Flow,
    items: Vec<CanvasItem>,
    dx: f64,
    dy: f64,
    snap: bool,
) -> Option<Flow> {
    flow.move_items(&items, dx, dy, snap).then_some(flow)
}

/// Puts block `id` with its top-left corner at `at`, snapped to the grid.
#[uniffi::export]
pub fn place_block(mut flow: Flow, id: String, at: Point) -> Option<Flow> {
    flow.place_block(&id, at).then_some(flow)
}

/// Gives a section or a note a new frame, snapped to the grid when `snap`.
#[uniffi::export]
pub fn resize_on_canvas(mut flow: Flow, item: CanvasItem, rect: Rect, snap: bool) -> Option<Flow> {
    flow.resize_item(&item, rect, snap).then_some(flow)
}

/// Forgets where blocks were placed by hand, so the test lays out automatically again.
#[uniffi::export]
pub fn arrange_automatically(mut flow: Flow) -> Flow {
    flow.arrange_automatically();
    flow
}

/// Lays the blocks out in rows of three, each row in a frame of its own, so the test reads like
/// text from top to bottom. Frames and notes made by hand stay.
#[uniffi::export]
pub fn arrange_in_rows(mut flow: Flow) -> Flow {
    flow.arrange_in_rows();
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

/// What the sidebar shows and searches of a test file.
#[derive(Debug, Default, PartialEq, Eq, uniffi::Record)]
pub struct TestFileSummary {
    /// Words the test is found by besides its file name: its name, description, details, block
    /// texts and notes.
    pub keywords: String,
    /// Someone marked the test as flaky in its details.
    pub flaky: bool,
    /// The fields of the test's card the sidebar and Run All choose tests by.
    pub card: TestCard,
}

/// The sidebar's summary of the test in `json`; empty when it doesn't read as a test.
#[uniffi::export]
pub fn test_summary(json: String) -> TestFileSummary {
    jevision_core::parse(&json)
        .map(|flow| TestFileSummary {
            keywords: jevision_core::test_keywords(&flow),
            flaky: flow.flaky,
            card: TestCard::of(&flow, false),
        })
        .unwrap_or_default()
}

/// A word for a filter isn't one a test card can have.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum TestFilterError {
    #[error("{reason}")]
    Invalid { reason: String },
}

/// The filter for what a person wrote: the type, priority and status as they appear in test
/// files (`smoke`, `high`, `ready`; case doesn't matter), the author's name, and whether only
/// flaky tests count. A blank word asks nothing.
#[uniffi::export]
pub fn parse_test_filter(
    test_type: Option<String>,
    priority: Option<String>,
    status: Option<String>,
    author: Option<String>,
    flaky: bool,
) -> Result<TestFilter, TestFilterError> {
    TestFilter::from_words(
        test_type.as_deref(),
        priority.as_deref(),
        status.as_deref(),
        author.as_deref(),
        flaky,
    )
    .map_err(|reason| TestFilterError::Invalid { reason })
}

/// Positions of the `cards` that `filter` chooses, in order; all of them for an empty filter.
#[uniffi::export]
pub fn filter_tests(filter: TestFilter, cards: Vec<TestCard>) -> Vec<u32> {
    filter
        .choose(&cards)
        .into_iter()
        .filter_map(|i| u32::try_from(i).ok())
        .collect()
}

/// Whether `filter` asks for nothing.
#[uniffi::export]
pub fn test_filter_is_empty(filter: TestFilter) -> bool {
    filter.is_empty()
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

/// Adds a note about `anchor` at `at` written by `author` at `time` (RFC 3339) and selects it.
#[uniffi::export]
pub fn add_note(
    mut flow: Flow,
    text: String,
    author: String,
    time: String,
    at: Point,
    anchor: NoteAnchor,
) -> CanvasEdit {
    let item = flow.add_note(&text, &author, &time, at, anchor);
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

/// Like [`remove_from_canvas`], and the notes about the removed blocks go with them.
#[uniffi::export]
pub fn remove_from_canvas_with_notes(mut flow: Flow, items: Vec<CanvasItem>) -> Edited {
    let selection = flow.remove_items_with_notes(&items);
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

/// Names of the inputs a custom block reads through `${input.NAME}`, sorted.
#[uniffi::export]
pub fn required_inputs(flow: Flow) -> Vec<String> {
    jevision_core::required_inputs(&flow)
}

/// Templates every project has: `(id, text of the template file)`.
#[uniffi::export]
pub fn builtin_templates() -> Vec<BuiltinTemplate> {
    jevision_core::BUILTIN_TEMPLATES
        .iter()
        .map(|(id, text)| BuiltinTemplate {
            id: (*id).to_owned(),
            text: (*text).to_owned(),
        })
        .collect()
}

#[derive(uniffi::Record)]
pub struct BuiltinTemplate {
    pub id: String,
    pub text: String,
}

/// `items` as a template file named `name`.
#[uniffi::export]
pub fn template_from_canvas(flow: Flow, items: Vec<CanvasItem>, name: String) -> String {
    flow.template(&items, &name)
}

/// Text fields of the chosen blocks that a custom block can ask for each time it is used.
#[uniffi::export]
pub fn input_fields(flow: Flow, items: Vec<CanvasItem>) -> Vec<InputField> {
    flow.input_fields(&items)
}

/// `items` as a custom block file named `name` with the SF Symbol `icon`, asking for `inputs`.
#[uniffi::export]
pub fn custom_block_from_canvas(
    flow: Flow,
    items: Vec<CanvasItem>,
    name: String,
    icon: String,
    inputs: Vec<BlockInput>,
) -> String {
    flow.custom_block(&items, &name, &icon, &inputs)
}

#[uniffi::remote(Record)]
pub struct PackageNeed {
    pub version: String,
    pub source: Option<String>,
}

#[uniffi::remote(Record)]
pub struct InputField {
    pub block: String,
    pub field: String,
    pub value: String,
}

#[uniffi::remote(Record)]
pub struct BlockInput {
    pub block: String,
    pub field: String,
    pub name: String,
}

#[uniffi::remote(Record)]
pub struct Flow {
    pub name: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub app: String,
    pub test_type: Option<TestType>,
    pub priority: Option<Priority>,
    pub status: Option<TestStatus>,
    pub tags: Vec<String>,
    #[uniffi(default = None)]
    pub preconditions: Option<String>,
    #[uniffi(default = None)]
    pub author: Option<String>,
    #[uniffi(default = None)]
    pub platform: Option<String>,
    #[uniffi(default = None)]
    pub link: Option<String>,
    #[uniffi(default = false)]
    pub flaky: bool,
    pub retries: Option<u32>,
    #[uniffi(default = [])]
    pub depends_on: Vec<String>,
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
    pub anchor: NoteAnchor,
    pub attachments: Vec<Attachment>,
    pub replies: Vec<Reply>,
    pub resolved: Option<Resolved>,
    pub rect: Rect,
}

#[uniffi::remote(Enum)]
pub enum NoteAnchor {
    Test,
    Block { id: String },
    Folder,
}

#[uniffi::remote(Enum)]
pub enum AttachmentKind {
    File,
    Link,
}

#[uniffi::remote(Record)]
pub struct Attachment {
    pub kind: AttachmentKind,
    pub value: String,
}

#[uniffi::remote(Record)]
pub struct Reply {
    pub id: String,
    pub author: String,
    pub time: String,
    pub text: String,
}

#[uniffi::remote(Record)]
pub struct Resolved {
    pub by: String,
    pub time: String,
}

#[uniffi::remote(Enum)]
pub enum CanvasItem {
    Block { id: String },
    Section { id: String },
    Note { id: String },
}

#[uniffi::remote(Record)]
pub struct TestCard {
    #[uniffi(default = None)]
    pub test_type: Option<TestType>,
    #[uniffi(default = None)]
    pub priority: Option<Priority>,
    #[uniffi(default = None)]
    pub status: Option<TestStatus>,
    #[uniffi(default = None)]
    pub author: Option<String>,
    #[uniffi(default = false)]
    pub flaky: bool,
}

#[uniffi::remote(Record)]
pub struct TestFilter {
    #[uniffi(default = None)]
    pub test_type: Option<TestType>,
    #[uniffi(default = None)]
    pub priority: Option<Priority>,
    #[uniffi(default = None)]
    pub status: Option<TestStatus>,
    #[uniffi(default = None)]
    pub author: Option<String>,
    #[uniffi(default = false)]
    pub flaky: bool,
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
        reset_state: bool,
        window_size: Option<WindowSize>,
        window_position: Option<WindowPosition>,
        next: Option<String>,
    },
    OpenFile {
        files: Vec<String>,
        app: Option<String>,
        new_instance: bool,
        close_after: bool,
        next: Option<String>,
    },
    Click {
        target: Target,
        button: MouseButton,
        double: bool,
        modifiers: Modifiers,
        wait_for_change: bool,
        next: Option<String>,
    },
    Hover {
        target: Target,
        modifiers: Modifiers,
        wait_for_change: bool,
        next: Option<String>,
    },
    Drag {
        from: Target,
        to: Target,
        button: MouseButton,
        modifiers: Modifiers,
        wait_for_change: bool,
        next: Option<String>,
    },
    Scroll {
        direction: ScrollDirection,
        amount: u32,
        unit: ScrollUnit,
        at: Option<Target>,
        modifiers: Modifiers,
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
    Pause {
        duration_ms: u64,
        next: Option<String>,
    },
    Check {
        reference: Reference,
        next: Option<String>,
    },
    If {
        condition: Condition,
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
        package: Option<PackageNeed>,
        inputs: HashMap<String, String>,
        next: Option<String>,
    },
    Script {
        language: ScriptLanguage,
        code: String,
        inputs: Vec<ScriptInput>,
        outputs: Vec<ScriptOutput>,
        timeout_ms: u64,
        next: Option<String>,
    },
    Command {
        command: String,
        folder: Option<String>,
        exit_code: i32,
        save_output: Option<String>,
        timeout_ms: u64,
        next: Option<String>,
    },
    Http {
        method: HttpMethod,
        url: String,
        headers: Vec<NamedValue>,
        body: String,
        expect_status: ExpectStatus,
        save: Vec<SavedValue>,
        timeout_ms: u64,
        next: Option<String>,
    },
    CheckFile {
        path: String,
        until: FileState,
        timeout_ms: u64,
        next: Option<String>,
    },
    Database {
        file: String,
        query: String,
        save_as: Option<String>,
        expect: Option<DatabaseExpect>,
        timeout_ms: u64,
        next: Option<String>,
    },
    Clipboard {
        action: ClipboardAction,
        next: Option<String>,
    },
    DateTime {
        save_as: String,
        format: String,
        language: Option<String>,
        time_zone: Option<String>,
        shift: Option<TimeShift>,
        next: Option<String>,
    },
}

#[uniffi::remote(Record)]
pub struct Modifiers {
    pub command: bool,
    pub shift: bool,
    pub option: bool,
    pub control: bool,
}

#[uniffi::remote(Enum)]
pub enum ClipboardAction {
    Put {
        text: String,
    },
    PutImage {
        image: String,
    },
    Read {
        save_as: String,
        content: ClipboardContent,
    },
    Check {
        is: ValueTest,
        to: String,
        timeout_ms: u64,
    },
}

#[uniffi::remote(Record)]
pub struct TimeShift {
    pub amount: i32,
    pub unit: TimeUnit,
}

#[uniffi::remote(Enum)]
pub enum TimeUnit {
    Seconds,
    Minutes,
    Hours,
    Days,
    Weeks,
    Months,
    Years,
}

#[uniffi::remote(Record)]
pub struct ElseIf {
    pub condition: Condition,
    pub then: Option<String>,
}

#[uniffi::remote(Enum)]
pub enum Condition {
    Screen(Reference),
    Value(ValueCheck),
}

#[uniffi::remote(Record)]
pub struct ValueCheck {
    pub of: String,
    pub is: ValueTest,
    pub to: String,
}

#[uniffi::remote(Enum)]
pub enum ValueTest {
    Equals,
    NotEquals,
    Contains,
    GreaterThan,
    LessThan,
}

#[uniffi::remote(Record)]
pub struct NamedValue {
    pub name: String,
    pub value: String,
}

#[uniffi::remote(Record)]
pub struct ScriptInput {
    pub name: String,
    pub kind: ValueKind,
    pub value: String,
}

#[uniffi::remote(Record)]
pub struct ScriptOutput {
    pub name: String,
    pub kind: ValueKind,
}

#[uniffi::remote(Enum)]
pub enum ClipboardContent {
    Text,
    Image,
    Files,
}

#[uniffi::remote(Enum)]
pub enum ValueKind {
    Text,
    Number,
    Boolean,
    Image,
    Area,
}

#[uniffi::remote(Enum)]
pub enum ScriptLanguage {
    Sh,
    Zsh,
    Python3,
    Node,
    AppleScript,
}

#[uniffi::remote(Enum)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
}

#[uniffi::remote(Enum)]
pub enum ExpectStatus {
    Success,
    Any,
    Exactly(u16),
}

#[uniffi::remote(Record)]
pub struct SavedValue {
    pub name: String,
    pub from: String,
}

#[uniffi::remote(Enum)]
pub enum FileState {
    Exists,
    Missing,
    Changed,
    Contains(String),
}

#[uniffi::remote(Enum)]
pub enum DatabaseExpect {
    Value(String),
    Rows(u32),
}

#[uniffi::remote(Enum)]
pub enum Target {
    Image(String),
    Point(Point),
    Description(String),
    Area(String),
}

#[uniffi::remote(Record)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[uniffi::remote(Record)]
pub struct WindowSize {
    pub width: u32,
    pub height: u32,
}

#[uniffi::remote(Record)]
pub struct WindowPosition {
    pub x: i32,
    pub y: i32,
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
    Middle,
}

#[uniffi::remote(Enum)]
pub enum ScrollDirection {
    Up,
    Down,
    Left,
    Right,
}

#[uniffi::remote(Enum)]
pub enum ScrollUnit {
    Lines,
    Pixels,
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
    Similar {
        threshold: f64,
    },
    Meaning {
        expect: String,
        model: String,
        provider: String,
    },
}

#[uniffi::remote(Record)]
pub struct Issue {
    pub code: String,
    pub severity: Severity,
    pub params: Vec<IssueParam>,
}

#[uniffi::remote(Record)]
pub struct CheckedTest {
    pub file: String,
    pub issues: Vec<Issue>,
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
    fn the_date_block_names_its_languages_and_knows_zones() {
        assert_eq!(date_languages(), ["en", "ru"]);
        assert!(time_zone_known("Europe/Berlin".to_owned()));
        assert!(!time_zone_known("Mars/Olympus".to_owned()));
    }

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
    fn the_summary_finds_the_test_and_its_flaky_mark() {
        let plain = test_summary(EXAMPLE.to_owned());
        assert!(plain.keywords.contains("nobody is signed in"));
        assert!(!plain.flaky);
        let marked = test_summary(EXAMPLE.replace(r#""retries""#, r#""flaky": true, "retries""#));
        assert!(marked.flaky);
        assert_eq!(test_summary("{".to_owned()), TestFileSummary::default());
    }

    #[test]
    fn tests_are_chosen_by_their_cards() {
        let card = |extra: &str| {
            test_summary(EXAMPLE.replace(r#""retries""#, &format!(r#"{extra}"retries""#))).card
        };
        let cards = vec![
            card(r#""priority": "high", "author": "Anna Lee", "#),
            card(r#""priority": "low", "flaky": true, "#),
            TestCard::default(),
        ];
        let by = |filter| filter_tests(filter, cards.clone());
        let none = TestFilter {
            test_type: None,
            priority: None,
            status: None,
            author: None,
            flaky: false,
        };
        assert!(test_filter_is_empty(none.clone()));
        assert_eq!(by(none.clone()), [0, 1, 2]);
        let high = TestFilter {
            priority: Some(Priority::High),
            ..none.clone()
        };
        assert_eq!(by(high), [0]);
        let anna = TestFilter {
            author: Some("anna lee".into()),
            ..none.clone()
        };
        assert_eq!(by(anna), [0]);
        let flaky = TestFilter {
            flaky: true,
            ..none
        };
        assert!(!test_filter_is_empty(flaky.clone()));
        assert_eq!(by(flaky.clone()), [1]);
        assert_eq!(
            parse_test_filter(None, Some("High".into()), None, None, true),
            Ok(TestFilter {
                priority: Some(Priority::High),
                ..flaky
            })
        );
        assert!(parse_test_filter(Some("fast".into()), None, None, None, false).is_err());
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

    #[test]
    fn the_catalog_has_english_first_and_russian() {
        let languages = catalog_languages();
        assert_eq!(languages.first().map(String::as_str), Some("en"));
        assert!(languages.iter().any(|l| l == "ru"));
    }

    #[test]
    fn a_picture_outside_the_run_reads_in_russian_and_english() {
        let path = "/tmp/other.png";
        assert_eq!(
            jevision_l10n::tr_in!("en", "bug.picture.notInRun", path),
            "/tmp/other.png isn’t a picture of this run"
        );
        assert_eq!(
            jevision_l10n::tr_in!("ru", "bug.picture.notInRun", path),
            "/tmp/other.png — не снимок этого прогона"
        );
    }
}
