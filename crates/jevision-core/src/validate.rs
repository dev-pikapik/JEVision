//! Checks a well-formed test for problems in its meaning: broken edges, forbidden cycles,
//! empty or out-of-range parameters, malformed placeholders.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use crate::data::{DataError, DataTable};
use crate::element::{ELEMENT_ROLES, ElementCheck, ElementQuery, ElementRole, ElementState};
use crate::env::Environments;
use crate::issue::Issue;
use crate::model::{
    AI_PROVIDERS, AttachmentKind, BLOCK_SUFFIX, BODY, Block, BlockKind, CheckOf, ClipboardAction,
    Compare, Condition, DatabaseExpect, ExpectStatus, FileState, Flow, NamedValue, NoteAnchor,
    Point, Rect, Reference, ScriptInput, TEST_SUFFIX, Target, TimeShift, ValueKind, ValueTest,
    WaitUntil, format_duration,
};
use crate::notes::is_safe_name;
use crate::package::{Version, is_block_path, package_of_path};
use crate::resources::resource_id;

pub const MAX_RETRIES: u32 = 10;
pub const MAX_REPEAT: u32 = 10_000;
pub const MAX_TIMEOUT_MS: u64 = 60 * 60 * 1000;
/// Most a scroll block may scroll, in lines or in pixels.
pub const MAX_SCROLL: u32 = 10_000;
/// Most a date and time block may move the date, in any unit.
pub const MAX_TIME_SHIFT: i32 = 100_000;
/// Smallest and largest window side an open-app block may ask for, in points.
pub const MIN_WINDOW_SIDE: u32 = 100;
pub const MAX_WINDOW_SIDE: u32 = 20_000;
/// Farthest from the main display's corner a window may be placed, in points.
pub const MAX_WINDOW_OFFSET: i32 = 20_000;
const MAX_ID_LEN: usize = 64;

/// Lists every problem in `flow`, errors and warnings, in a stable order.
pub fn validate(flow: &Flow) -> Vec<Issue> {
    let mut out = Vec::new();
    root(flow, &mut out);

    let mut blocks: Vec<&Block> = flow.blocks.iter().collect();
    blocks.sort_by(|a, b| a.id.cmp(&b.id));
    let mut seen = HashSet::new();
    for b in &blocks {
        if !seen.insert(b.id.as_str()) {
            out.push(Issue::error("flow.block.duplicateId").with("block", &b.id));
        }
    }
    if !seen.contains(flow.start.as_str()) {
        out.push(Issue::error("flow.start.unknownBlock").with("target", &flow.start));
    }
    for b in &blocks {
        block(b, &seen, &mut out);
    }
    graph(flow, &blocks, &mut out);
    unknown_variables(flow, &blocks, &mut out);
    variable_kinds(flow, &blocks, &mut out);
    notes(flow, &seen, &mut out);
    // A test that runs another one may check the screen there.
    if !flow
        .blocks
        .iter()
        .any(|b| b.kind.checks() || matches!(b.kind, BlockKind::RunTest { .. }))
    {
        out.push(Issue::warning("flow.noCheck"));
    }
    out
}

/// Checks what the notes carry: files and links that can be opened safely, one name for each
/// reply, a block for the note to be about.
fn notes(flow: &Flow, blocks: &HashSet<&str>, out: &mut Vec<Issue>) {
    for note in &flow.canvas.notes {
        if let NoteAnchor::Block { id } = &note.anchor
            && !blocks.contains(id.as_str())
        {
            out.push(
                Issue::warning("flow.note.unknownBlock")
                    .with("note", &note.id)
                    .with("block", id),
            );
        }
        for a in &note.attachments {
            let fine = match a.kind {
                AttachmentKind::File => is_safe_name(&a.value) && is_safe_name(&note.id),
                AttachmentKind::Link => is_web_address(&a.value),
            };
            if !fine {
                out.push(
                    Issue::error("flow.note.attachment.invalid")
                        .with("note", &note.id)
                        .with("value", &a.value),
                );
            }
        }
        let mut seen = HashSet::new();
        for r in &note.replies {
            if !seen.insert(r.id.as_str()) {
                out.push(
                    Issue::error("flow.note.reply.duplicateId")
                        .with("note", &note.id)
                        .with("reply", &r.id),
                );
            }
        }
    }
}

/// Warns about `${var.NAME}` that no block of the test saves. A test it runs may save it, so
/// this is not an error.
fn unknown_variables(flow: &Flow, blocks: &[&Block], out: &mut Vec<Issue>) {
    let saved: HashSet<&str> = flow
        .blocks
        .iter()
        .flat_map(|b| saved_names(&b.kind).into_iter().map(|(_, n)| n))
        .collect();
    for b in blocks {
        let mut reported = HashSet::new();
        for (field, value) in placeholder_fields(&b.kind) {
            for (ns, name) in placeholders(value).into_iter().flatten() {
                if ns == "var" && !saved.contains(name) && reported.insert(name) {
                    out.push(
                        Issue::warning("flow.var.unknown")
                            .with("block", &b.id)
                            .with("field", &field)
                            .with("name", name),
                    );
                }
            }
        }
    }
}

/// `format` without the specifiers for the names of months and days, with or without the `^`
/// and `O` flags: the runner writes those itself, in the language of the date, so the date
/// formatter never sees them.
fn without_name_specifiers(format: &str) -> String {
    let mut out = String::with_capacity(format.len());
    let mut chars = format.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let mut spec = String::from('%');
        while let Some(&f) = chars.peek() {
            if !"-_0^#O".contains(f) {
                break;
            }
            spec.push(f);
            chars.next();
        }
        match chars.next() {
            Some('B' | 'b' | 'h' | 'A' | 'a') => {}
            Some(letter) => {
                spec.push(letter);
                out.push_str(&spec);
            }
            None => out.push_str(&spec),
        }
    }
    out
}

/// The kinds of value each run variable is saved as, by every block that saves it.
fn saved_kinds(flow: &Flow) -> HashMap<&str, Vec<ValueKind>> {
    let mut made: HashMap<&str, Vec<ValueKind>> = HashMap::new();
    for b in &flow.blocks {
        match &b.kind {
            BlockKind::Script { outputs, .. } => {
                for o in outputs {
                    made.entry(o.name.as_str()).or_default().push(o.kind);
                }
            }
            BlockKind::Clipboard {
                action: ClipboardAction::Read { save_as, content },
                ..
            } => made
                .entry(save_as.as_str())
                .or_default()
                .push(content.kind()),
            kind => {
                for (_, name) in saved_names(kind) {
                    made.entry(name).or_default().push(ValueKind::Text);
                }
            }
        }
    }
    made
}

/// Fails a field that gets `${var.NAME}` as a whole when every block that saves NAME saves it
/// as a kind of value the field can't take: a script input of a kind, the image or the area a
/// target looks for, the image a check compares with, the picture a clipboard block puts. A
/// value saved as text may be anything once a run is on, so only a run finds out whether it
/// fits.
fn variable_kinds(flow: &Flow, blocks: &[&Block], out: &mut Vec<Issue>) {
    let made = saved_kinds(flow);
    for b in blocks {
        for (field, kind, text, input) in kind_fields(&b.kind) {
            let Some(name) = whole_variable(text) else {
                continue;
            };
            let Some(kinds) = made.get(name) else {
                continue;
            };
            let fits =
                |k: &ValueKind| [ValueKind::Text, kind].contains(k) || kind == ValueKind::Text;
            if !kinds.iter().any(fits) {
                let from: BTreeSet<&str> = kinds.iter().map(|k| k.name()).collect();
                let mut issue = match input {
                    Some(input) => Issue::error("flow.script.inputKind")
                        .with("block", &b.id)
                        .with("field", &field)
                        .with("name", input),
                    None => Issue::error("flow.var.kind")
                        .with("block", &b.id)
                        .with("field", &field),
                };
                issue = issue
                    .with("type", kind.name())
                    .with("variable", name)
                    .with("from", from.into_iter().collect::<Vec<_>>().join(", "));
                out.push(issue);
            }
        }
    }
}

type KindField<'a> = (String, ValueKind, &'a str, Option<&'a str>);

/// `(field, kind, text, script input name)` of every field of a block that takes a value of a
/// kind other than text, written in full or as `${var.NAME}`.
fn kind_fields(kind: &BlockKind) -> Vec<KindField<'_>> {
    use crate::model::WaitUntil::*;
    fn target<'a>(field: &str, t: Option<&'a Target>) -> Vec<KindField<'a>> {
        match t {
            Some(Target::Image(text)) => {
                vec![(
                    format!("{field}.image"),
                    ValueKind::Image,
                    text.as_str(),
                    None,
                )]
            }
            Some(Target::Area(text)) => {
                vec![(
                    format!("{field}.area"),
                    ValueKind::Area,
                    text.as_str(),
                    None,
                )]
            }
            _ => Vec::new(),
        }
    }
    fn image<'a>(field: &str, text: &'a str) -> Vec<KindField<'a>> {
        vec![(field.to_owned(), ValueKind::Image, text, None)]
    }
    match kind {
        BlockKind::Click { target: t, .. } | BlockKind::Hover { target: t, .. } => {
            target("target", Some(t))
        }
        BlockKind::Drag { from, to, .. } => {
            let mut both = target("from", Some(from));
            both.extend(target("to", Some(to)));
            both
        }
        BlockKind::Scroll { at, .. } => target("at", at.as_ref()),
        BlockKind::Type { into, .. } => target("into", into.as_ref()),
        BlockKind::Script { inputs, .. } => inputs
            .iter()
            .enumerate()
            .map(|(i, input)| {
                (
                    format!("inputs.{i}.value"),
                    input.kind,
                    input.value.as_str(),
                    Some(input.name.as_str()),
                )
            })
            .collect(),
        BlockKind::Check {
            of: CheckOf::Screen(reference),
            ..
        } => image("reference.image", &reference.image),
        BlockKind::WaitFor {
            until: Appears(r), ..
        } => image("until.appears.image", &r.image),
        BlockKind::WaitFor {
            until: Disappears(r),
            ..
        } => image("until.disappears.image", &r.image),
        BlockKind::If {
            condition, else_if, ..
        } => std::iter::once(("reference.image".to_owned(), condition))
            .chain(
                else_if
                    .iter()
                    .enumerate()
                    .map(|(i, c)| (format!("elseIf.{i}.reference.image"), &c.condition)),
            )
            .filter_map(|(field, c)| {
                Some((field, ValueKind::Image, c.reference()?.image.as_str(), None))
            })
            .collect(),
        BlockKind::Clipboard {
            action: ClipboardAction::PutImage { image: text },
            ..
        } => image("image", text),
        _ => Vec::new(),
    }
}

/// NAME of a text that is nothing but `${var.NAME}`.
fn whole_variable(text: &str) -> Option<&str> {
    let text = text.trim();
    let name = text.strip_prefix("${var.")?.strip_suffix('}')?;
    (is_identifier(name) && placeholders(text).len() == 1).then_some(name)
}

/// [`validate`], plus warnings about files the test reads that are not in `dir`, the folder of
/// the test file. A missing reference of a check block is expected before the first run, which
/// saves the screen as that reference; other missing files stop the run at their block.
pub fn validate_in(flow: &Flow, dir: &Path) -> Vec<Issue> {
    validate_in_with(flow, dir, &mut DiskCache::default())
}

/// What [`validate_in_with`] read from disk, so that checking many tests of a project reads
/// each shared file once. Keep it only while none of those files can change.
#[derive(Default)]
pub struct DiskCache {
    exists: HashMap<PathBuf, bool>,
    inputs: HashMap<PathBuf, Vec<String>>,
    unmet: HashMap<(PathBuf, String), Vec<Issue>>,
    environments: HashMap<PathBuf, Result<Environments, crate::SettingsFileError>>,
    tables: HashMap<PathBuf, Result<DataTable, DataError>>,
}

impl DiskCache {
    fn exists(&mut self, path: PathBuf) -> bool {
        *self
            .exists
            .entry(path)
            .or_insert_with_key(|path| path.exists())
    }

    pub(crate) fn environments(
        &mut self,
        dir: &Path,
    ) -> Result<&Environments, &crate::SettingsFileError> {
        (self.environments.entry(dir.to_path_buf()))
            .or_insert_with(|| Environments::find_checked(dir))
            .as_ref()
    }

    /// Reads the table at `path` unless it has been read; [`DiskCache::table`] then gives it.
    pub(crate) fn read_table(&mut self, path: &Path) -> &Result<DataTable, DataError> {
        (self.tables.entry(path.to_path_buf())).or_insert_with(|| DataTable::load(path))
    }

    pub(crate) fn table(&self, path: &Path) -> Option<&DataTable> {
        self.tables.get(path).and_then(|table| table.as_ref().ok())
    }
}

/// [`validate_in`], reading files through `disk`.
pub fn validate_in_with(flow: &Flow, dir: &Path, disk: &mut DiskCache) -> Vec<Issue> {
    let mut out = validate(flow);
    let checked = out.len();
    // Labels are checked where their files are; a label left is one whose file is missing.
    let mut resolved = flow.clone();
    out.extend(crate::resources::resolve_labels(&mut resolved, dir, None).issues);
    let flow = &resolved;
    let mut blocks: Vec<&Block> = flow.blocks.iter().collect();
    blocks.sort_by(|a, b| a.id.cmp(&b.id));
    let mut saved_by_check = HashSet::new();
    let mut read = Vec::new();
    for &b in &blocks {
        if let BlockKind::Check {
            of: CheckOf::Screen(r),
            ..
        } = &b.kind
            && is_relative_path(&r.image)
            && placeholders(&r.image).is_empty()
            && resource_id(&r.image).is_none()
            && !disk.exists(dir.join(&r.image))
        {
            saved_by_check.insert(r.image.as_str());
            out.push(reference_not_saved(&b.id, &r.image));
        }
        read.extend(
            files_read(&b.kind)
                .into_iter()
                .map(|(field, path)| (b, field, path)),
        );
    }
    for (block, field, path) in read {
        // A path with placeholders names a file only once a run fills them in.
        if is_relative_path(path)
            && placeholders(path).is_empty()
            && resource_id(path).is_none()
            && !saved_by_check.contains(path)
            && !disk.exists(dir.join(path))
        {
            if let Some(package) = package_of_path(path).filter(|_| field == "test") {
                out.push(not_installed(block, &field, path, package));
                continue;
            }
            out.push(
                Issue::warning("flow.file.missing")
                    .with("block", &block.id)
                    .with("field", field)
                    .with("path", path),
            );
        }
    }
    for b in &blocks {
        if let BlockKind::RunTest { test, inputs, .. } = &b.kind {
            out.extend(dependencies_unmet(dir, &b.id, test, disk));
            for name in block_inputs(dir, test, disk) {
                if !inputs.contains_key(name) {
                    out.push(
                        Issue::warning("flow.input.missing")
                            .with("block", &b.id)
                            .with("field", format!("inputs.{name}"))
                            .with("name", name),
                    );
                }
            }
        }
    }
    out.extend(crate::env::environment_issues(flow, dir, disk));
    out.extend(crate::data::data_issues(flow, dir, disk));
    let elsewhere = crate::check::path_warnings(flow, dir, &out);
    out.extend(elsewhere);
    out[checked..].sort_by(|a, b| a.param("block").cmp(&b.param("block")));
    out
}

/// The packages that the package in `path`, relative to `dir`, needs, or those need in turn, and
/// that aren't installed in a version that fits; `block` is the step that runs it.
fn dependencies_unmet(dir: &Path, block: &str, path: &str, disk: &mut DiskCache) -> Vec<Issue> {
    let file = dir.join(path);
    let (Some(id), Some(blocks)) = (
        package_of_path(path).filter(|_| is_relative_path(path) && placeholders(path).is_empty()),
        file.parent().and_then(Path::parent),
    ) else {
        return Vec::new();
    };
    (disk.unmet.entry((blocks.to_path_buf(), id.to_owned())))
        .or_insert_with(|| crate::package_files::unmet_in(blocks, id))
        .iter()
        .map(|found| {
            let p = |name| found.param(name).unwrap_or_default();
            let mut issue = Issue::error("flow.block.dependencyUnmet")
                .with("block", block)
                .with("field", "test")
                .with("path", path)
                .with("id", id)
                .with("by", p("by"))
                .with("dependency", p("id"))
                .with("range", p("range"));
            for name in ["installed", "source"] {
                if let Some(value) = found.param(name) {
                    issue = issue.with(name, value);
                }
            }
            issue
        })
        .collect()
}

/// Inputs the custom block in `path`, relative to `dir`, reads and has no default for; none for a
/// test or a file that can't be read.
fn block_inputs<'a>(dir: &Path, path: &str, disk: &'a mut DiskCache) -> &'a [String] {
    if !is_block_path(path) || !is_relative_path(path) || !placeholders(path).is_empty() {
        return &[];
    }
    disk.inputs
        .entry(dir.join(path))
        .or_insert_with_key(|file| {
            let defaults = crate::input_defaults(file);
            std::fs::read_to_string(file)
                .ok()
                .and_then(|text| crate::parse(&text).ok())
                .map(|flow| {
                    let mut names = required_inputs(&flow);
                    names.retain(|name| !defaults.contains_key(name));
                    names
                })
                .unwrap_or_default()
        })
}

/// A check block's reference that isn't there yet: the first run saves the screen as it.
pub(crate) fn reference_not_saved(block: &str, path: &str) -> Issue {
    Issue::warning("flow.reference.notSaved")
        .with("block", block)
        .with("field", "reference.image")
        .with("path", path)
}

/// The step `block` runs the package `id`, in `path` of its `field`, which isn't installed.
pub(crate) fn not_installed(block: &Block, field: &str, path: &str, id: &str) -> Issue {
    let mut issue = Issue::error("flow.block.notInstalled")
        .with("block", &block.id)
        .with("field", field)
        .with("id", id)
        .with("path", path);
    if let BlockKind::RunTest {
        package: Some(need),
        ..
    } = &block.kind
    {
        issue = issue.with("version", &need.version);
        if let Some(source) = &need.source {
            issue = issue.with("source", source);
        }
    }
    issue
}

/// Fields of a block that name a file to read, other than a check block's reference.
pub(crate) fn files_read<'a>(kind: &'a BlockKind) -> Vec<(String, &'a str)> {
    use crate::model::WaitUntil::*;
    let image = |field: &str, t: Option<&'a Target>| match t {
        Some(Target::Image(path)) => vec![(format!("{field}.image"), path)],
        _ => Vec::new(),
    };
    let found: Vec<(String, &'a String)> = match kind {
        BlockKind::OpenFile { files, .. } => files
            .iter()
            .enumerate()
            .filter(|(_, f)| !is_web_address(f))
            .map(|(i, f)| (format!("files.{i}"), f))
            .collect(),
        BlockKind::Click { target, .. } | BlockKind::Hover { target, .. } => {
            image("target", Some(target))
        }
        BlockKind::Drag { from, to, .. } => {
            let mut both = image("from", Some(from));
            both.extend(image("to", Some(to)));
            both
        }
        BlockKind::Scroll { at, .. } => image("at", at.as_ref()),
        BlockKind::Script { inputs, .. } => inputs
            .iter()
            .enumerate()
            .filter(|(_, i)| i.kind == ValueKind::Image)
            .map(|(i, input)| (format!("inputs.{i}.value"), &input.value))
            .collect(),
        BlockKind::Type { into, .. } => image("into", into.as_ref()),
        BlockKind::WaitFor {
            until: Appears(r), ..
        } => vec![("until.appears.image".to_owned(), &r.image)],
        BlockKind::WaitFor {
            until: Disappears(r),
            ..
        } => vec![("until.disappears.image".to_owned(), &r.image)],
        BlockKind::If {
            condition, else_if, ..
        } => std::iter::once(("reference.image".to_owned(), condition))
            .chain(
                else_if
                    .iter()
                    .enumerate()
                    .map(|(i, c)| (format!("elseIf.{i}.reference.image"), &c.condition)),
            )
            .filter_map(|(field, c)| Some((field, &c.reference()?.image)))
            .collect(),
        BlockKind::Clipboard {
            action: ClipboardAction::PutImage { image },
            ..
        } => vec![("image".to_owned(), image)],
        BlockKind::UseData { file, .. } => vec![("file".to_owned(), file)],
        BlockKind::RunTest { test, .. } => vec![("test".to_owned(), test)],
        _ => Vec::new(),
    };
    found.into_iter().map(|(f, p)| (f, p.as_str())).collect()
}

/// Names of the secrets the test reads through `${secret.NAME}`, sorted, without repeats.
pub fn required_secrets(flow: &Flow) -> Vec<String> {
    placeholder_names(flow, "secret")
}

/// Names of the inputs a custom block reads through `${input.NAME}`, sorted, without repeats:
/// the values a test that runs the block gives it.
pub fn required_inputs(flow: &Flow) -> Vec<String> {
    placeholder_names(flow, "input")
}

fn placeholder_names(flow: &Flow, namespace: &str) -> Vec<String> {
    let mut names = BTreeSet::new();
    for b in &flow.blocks {
        // The values a step gives a custom block are filled in where the step runs.
        for (_, value) in placeholder_fields(&b.kind) {
            for p in placeholders(value).into_iter().flatten() {
                if p.0 == namespace {
                    names.insert(p.1.to_owned());
                }
            }
        }
    }
    names.into_iter().collect()
}

/// Fields of a block that may contain `${secret.NAME}`, `${env.NAME}`, `${data.COLUMN}` and
/// `${var.NAME}` placeholders: text to type, the paths and addresses of files the block reads,
/// and what the blocks that work without the screen send and compare.
pub(crate) fn placeholder_fields(kind: &BlockKind) -> Vec<(String, &str)> {
    let mut fields = text_fields(kind);
    // The pictures and areas a block looks for or compares with may be run variables too.
    fields.extend(
        kind_fields(kind)
            .into_iter()
            .filter(|(.., input)| input.is_none())
            .map(|(field, _, text, _)| (field, text)),
    );
    fields
}

/// The texts of an element a block picks: `{at}element.id` and `{at}element.title`.
fn element_fields<'a>(at: &str, e: &'a ElementQuery) -> Vec<(String, &'a str)> {
    vec![
        (format!("{at}element.id"), e.id.as_str()),
        (format!("{at}element.title"), e.title.as_str()),
    ]
}

/// [`element_fields`] and the text the element is compared with.
fn element_check_fields<'a>(at: &str, c: &'a ElementCheck) -> Vec<(String, &'a str)> {
    let mut fields = element_fields(at, &c.element);
    fields.extend(c.is.text().map(|to| (format!("{at}element.to"), to)));
    fields
}

fn target_fields<'a>(field: &str, t: Option<&'a Target>) -> Vec<(String, &'a str)> {
    match t {
        Some(Target::Element(e)) => element_fields(&format!("{field}."), e),
        _ => Vec::new(),
    }
}

fn text_fields(kind: &BlockKind) -> Vec<(String, &str)> {
    match kind {
        BlockKind::OpenApp {
            arguments,
            environment,
            ..
        } => {
            let mut fields: Vec<_> = arguments
                .iter()
                .enumerate()
                .map(|(i, a)| (format!("arguments.{i}"), a.as_str()))
                .collect();
            let mut given: Vec<(&String, &String)> = environment.iter().collect();
            given.sort();
            fields.extend(
                given
                    .into_iter()
                    .map(|(name, value)| (format!("environment.{name}"), value.as_str())),
            );
            fields
        }
        BlockKind::Click { target, .. } | BlockKind::Hover { target, .. } => {
            target_fields("target", Some(target))
        }
        BlockKind::Drag { from, to, .. } => {
            let mut both = target_fields("from", Some(from));
            both.extend(target_fields("to", Some(to)));
            both
        }
        BlockKind::Scroll { at, .. } => target_fields("at", at.as_ref()),
        BlockKind::Type { text, into, .. } => {
            let mut fields = vec![("text".to_owned(), text.as_str())];
            fields.extend(target_fields("into", into.as_ref()));
            fields
        }
        BlockKind::WaitFor {
            until: WaitUntil::Element(e),
            ..
        } => element_check_fields("until.", e),
        BlockKind::Check {
            of: CheckOf::Element(e),
            ..
        } => element_check_fields("", e),
        BlockKind::OpenFile { files, .. } => files
            .iter()
            .enumerate()
            .map(|(i, f)| (format!("files.{i}"), f.as_str()))
            .collect(),
        BlockKind::UseData { file, .. } => vec![("file".to_owned(), file.as_str())],
        BlockKind::RunTest { test, inputs, .. } => {
            let mut fields = vec![("test".to_owned(), test.as_str())];
            let mut given: Vec<(&String, &String)> = inputs.iter().collect();
            given.sort();
            fields.extend(
                given
                    .into_iter()
                    .map(|(name, value)| (format!("inputs.{name}"), value.as_str())),
            );
            fields
        }
        BlockKind::If {
            condition, else_if, ..
        } => {
            let all = std::iter::once((String::new(), condition)).chain(
                else_if
                    .iter()
                    .enumerate()
                    .map(|(i, c)| (format!("elseIf.{i}."), &c.condition)),
            );
            all.flat_map(|(at, c)| match c {
                Condition::Value(v) => vec![
                    (format!("{at}value.of"), v.of.as_str()),
                    (format!("{at}value.to"), v.to.as_str()),
                ],
                Condition::Element(e) => element_check_fields(&at, e),
                Condition::Screen(_) => Vec::new(),
            })
            .collect()
        }
        BlockKind::Script { inputs, .. } => inputs
            .iter()
            .enumerate()
            .map(|(i, v)| (format!("inputs.{i}.value"), v.value.as_str()))
            .collect(),
        BlockKind::Clipboard { action, .. } => match action {
            ClipboardAction::Put { text } => vec![("text".to_owned(), text.as_str())],
            ClipboardAction::PutImage { image } => vec![("image".to_owned(), image.as_str())],
            ClipboardAction::Check { to, .. } => vec![("to".to_owned(), to.as_str())],
            ClipboardAction::Read { .. } => Vec::new(),
        },
        BlockKind::Command {
            command, folder, ..
        } => std::iter::once(("command".to_owned(), command.as_str()))
            .chain(folder.as_deref().map(|f| ("folder".to_owned(), f)))
            .collect(),
        BlockKind::Http {
            url, headers, body, ..
        } => std::iter::once(("url".to_owned(), url.as_str()))
            .chain(values("headers", headers))
            .chain(std::iter::once(("body".to_owned(), body.as_str())))
            .collect(),
        BlockKind::CheckFile { path, until, .. } => {
            let mut found = vec![("path".to_owned(), path.as_str())];
            if let FileState::Contains(text) = until {
                found.push(("until.contains".to_owned(), text.as_str()));
            }
            found
        }
        BlockKind::Database {
            file,
            query,
            expect,
            ..
        } => {
            let mut found = vec![
                ("file".to_owned(), file.as_str()),
                ("query".to_owned(), query.as_str()),
            ];
            if let Some(DatabaseExpect::Value(v)) = expect {
                found.push(("expect.value".to_owned(), v.as_str()));
            }
            found
        }
        _ => Vec::new(),
    }
}

fn values<'a>(key: &str, list: &'a [NamedValue]) -> Vec<(String, &'a str)> {
    list.iter()
        .enumerate()
        .map(|(i, v)| (format!("{key}.{i}.value"), v.value.as_str()))
        .collect()
}

/// Names of the run variables the blocks of `flow` save, sorted, without repeats.
pub fn saved_variables(flow: &Flow) -> Vec<String> {
    let names: BTreeSet<&str> = flow
        .blocks
        .iter()
        .flat_map(|b| saved_names(&b.kind).into_iter().map(|(_, n)| n))
        .filter(|n| is_identifier(n))
        .collect();
    names.into_iter().map(str::to_owned).collect()
}

/// `(field, name)` of every run variable a block saves.
fn saved_names(kind: &BlockKind) -> Vec<(String, &str)> {
    match kind {
        BlockKind::Script { outputs, .. } => outputs
            .iter()
            .enumerate()
            .map(|(i, o)| (format!("outputs.{i}.name"), o.name.as_str()))
            .collect(),
        BlockKind::Command { save_output, .. } => save_output
            .iter()
            .map(|n| ("saveOutput".to_owned(), n.as_str()))
            .collect(),
        BlockKind::Http { save, .. } => save
            .iter()
            .enumerate()
            .map(|(i, s)| (format!("save.{i}.name"), s.name.as_str()))
            .collect(),
        BlockKind::Database { save_as, .. } => save_as
            .iter()
            .map(|n| ("saveAs".to_owned(), n.as_str()))
            .collect(),
        BlockKind::Clipboard {
            action: ClipboardAction::Read { save_as, .. },
            ..
        }
        | BlockKind::DateTime { save_as, .. } => vec![("saveAs".to_owned(), save_as.as_str())],
        _ => Vec::new(),
    }
}

fn root(flow: &Flow, out: &mut Vec<Issue>) {
    if flow.name.trim().is_empty() {
        out.push(Issue::error("flow.field.empty").with("field", "name"));
    }
    if !is_bundle_id(&flow.app) {
        out.push(
            Issue::error("flow.app.invalid")
                .with("field", "app")
                .with("value", &flow.app),
        );
    }
    let mut tags = HashSet::new();
    for tag in &flow.tags {
        if tag.trim().is_empty() {
            out.push(Issue::error("flow.field.empty").with("field", "tags"));
        } else if !tags.insert(tag) {
            out.push(Issue::error("flow.tags.duplicate").with("tag", tag));
        }
    }
    if let Some(link) = flow.link.as_deref().filter(|l| !is_web_address(l)) {
        out.push(
            Issue::error("flow.link.invalid")
                .with("field", "link")
                .with("value", link),
        );
    }
    if flow.retries.is_some_and(|retries| retries > MAX_RETRIES) {
        out.push(range(None, "retries", 0, MAX_RETRIES));
    }
    let mut dependencies = HashSet::new();
    for test in &flow.depends_on {
        if !is_relative_path(test) || !is_test_file_name(test) {
            out.push(
                Issue::error("flow.dependency.invalid")
                    .with("field", "dependsOn")
                    .with("value", test),
            );
        } else if !dependencies.insert(test) {
            out.push(
                Issue::error("flow.dependency.duplicate")
                    .with("field", "dependsOn")
                    .with("test", test),
            );
        }
    }
}

/// `checkout.flow.json` or `login/sign-in.flow.json`: ends in a file name with the test suffix.
pub(crate) fn is_test_file_name(path: &str) -> bool {
    const SUFFIX: &str = ".flow.json";
    path.rsplit('/')
        .next()
        .is_some_and(|name| name.len() > SUFFIX.len() && name.ends_with(SUFFIX))
}

fn block(b: &Block, ids: &HashSet<&str>, out: &mut Vec<Issue>) {
    let id = b.id.as_str();
    let err = |code: &str, field: &str| Issue::error(code).with("block", id).with("field", field);
    if !is_block_id(id) {
        out.push(Issue::error("flow.block.invalidId").with("block", id));
    }
    for (edge, target) in b.kind.edges() {
        if !ids.contains(target) {
            out.push(
                Issue::error("flow.edge.unknownTarget")
                    .with("block", id)
                    .with("edge", edge)
                    .with("target", target),
            );
        }
    }
    match &b.kind {
        BlockKind::OpenApp {
            app,
            window_size,
            window_position,
            environment,
            ..
        } => {
            app_id(id, app.as_deref(), out);
            let mut names: Vec<&String> = environment.keys().collect();
            names.sort();
            for name in names.into_iter().filter(|n| !is_identifier(n)) {
                out.push(
                    err("flow.name.invalid", &format!("environment.{name}")).with("value", name),
                );
            }
            for (field, side) in window_size.iter().flat_map(|s| {
                [
                    ("windowSize.width", s.width),
                    ("windowSize.height", s.height),
                ]
            }) {
                if !(MIN_WINDOW_SIDE..=MAX_WINDOW_SIDE).contains(&side) {
                    out.push(range(Some(id), field, MIN_WINDOW_SIDE, MAX_WINDOW_SIDE));
                }
            }
            for (field, offset) in window_position
                .iter()
                .flat_map(|p| [("windowPosition.x", p.x), ("windowPosition.y", p.y)])
            {
                if offset.unsigned_abs() > MAX_WINDOW_OFFSET.unsigned_abs() {
                    out.push(
                        err("flow.field.range", field)
                            .with("min", -MAX_WINDOW_OFFSET)
                            .with("max", MAX_WINDOW_OFFSET),
                    );
                }
            }
        }
        BlockKind::OpenFile { files, app, .. } => {
            if files.is_empty() {
                out.push(err("flow.field.empty", "files"));
            }
            for (i, f) in files.iter().enumerate() {
                if !is_web_address(f) {
                    relative_path(id, &format!("files.{i}"), f, out);
                }
            }
            app_id(id, app.as_deref(), out);
        }
        BlockKind::Click { target: t, .. } | BlockKind::Hover { target: t, .. } => {
            target(id, "target", t, out)
        }
        BlockKind::Drag { from, to, .. } => {
            target(id, "from", from, out);
            target(id, "to", to, out);
            let same = match (from, to) {
                (Target::Image(a), Target::Image(b)) => !a.is_empty() && a == b,
                (Target::Point(a), Target::Point(b)) => a == b,
                (Target::Description(a), Target::Description(b)) => {
                    !a.trim().is_empty() && a.trim() == b.trim()
                }
                (Target::Element(a), Target::Element(b)) => !a.is_empty() && a == b,
                _ => false,
            };
            if same {
                out.push(err("flow.drag.samePlace", "to"));
            }
        }
        BlockKind::Scroll { amount, at, .. } => {
            if !(1..=MAX_SCROLL).contains(amount) {
                out.push(range(Some(id), "amount", 1, MAX_SCROLL));
            }
            if let Some(t) = at {
                target(id, "at", t, out);
            }
        }
        BlockKind::Type { text, into, .. } => {
            if text.is_empty() {
                out.push(err("flow.field.empty", "text"));
            }
            if let Some(t) = into {
                target(id, "into", t, out);
            }
        }
        BlockKind::PressKeys { keys, .. } => {
            if keys.is_empty() {
                out.push(err("flow.field.empty", "keys"));
            } else if !is_key_combo(keys) {
                out.push(err("flow.keys.invalid", "keys").with("value", keys));
            }
        }
        BlockKind::WaitFor {
            until, timeout_ms, ..
        } => {
            use crate::model::WaitUntil::*;
            match until {
                Appears(r) => reference(id, "until.appears", r, out),
                Disappears(r) => reference(id, "until.disappears", r, out),
                Element(e) => element_check(id, "until.element", e, out),
                ScreenStable => {}
            }
            if !(1..=MAX_TIMEOUT_MS).contains(timeout_ms) {
                out.push(
                    err("flow.field.range", "timeout")
                        .with("min", format_duration(1))
                        .with("max", format_duration(MAX_TIMEOUT_MS)),
                );
            }
        }
        BlockKind::Pause { duration_ms, .. } => {
            if !(1..=MAX_TIMEOUT_MS).contains(duration_ms) {
                out.push(
                    err("flow.field.range", "duration")
                        .with("min", format_duration(1))
                        .with("max", format_duration(MAX_TIMEOUT_MS)),
                );
            }
        }
        BlockKind::Check { of, .. } => match of {
            CheckOf::Screen(r) => reference(id, "reference", r, out),
            CheckOf::Element(e) => element_check(id, "element", e, out),
        },
        BlockKind::If {
            condition: first,
            else_if,
            ..
        } => {
            condition(id, "", first, out);
            for (i, case) in else_if.iter().enumerate() {
                let at = format!("elseIf.{i}.");
                condition(id, &at, &case.condition, out);
                let earlier =
                    std::iter::once(first).chain(else_if[..i].iter().map(|c| &c.condition));
                // A case not filled in yet is reported as empty, not as a repeat.
                let filled = match &case.condition {
                    Condition::Screen(r) => {
                        !r.image.is_empty()
                            || matches!(&r.compare, Compare::Meaning { expect, .. } if !expect.trim().is_empty())
                    }
                    Condition::Value(v) => !v.of.is_empty(),
                    Condition::Element(e) => !e.element.is_empty(),
                };
                if filled && earlier.into_iter().any(|e| *e == case.condition) {
                    let field = match case.condition {
                        Condition::Screen(_) => "reference",
                        Condition::Value(_) => "value",
                        Condition::Element(_) => "element",
                    };
                    out.push(
                        Issue::warning("flow.if.sameCondition")
                            .with("block", id)
                            .with("field", format!("{at}{field}")),
                    );
                }
            }
        }
        BlockKind::Repeat { times, body, .. } => {
            if !(1..=MAX_REPEAT).contains(times) {
                out.push(range(Some(id), "times", 1, MAX_REPEAT));
            }
            loop_body(id, body, out);
        }
        BlockKind::UseData { file, body, .. } => {
            if resource_id(file).is_none() {
                path_with_extension(id, "file", file, ".csv", out);
            }
            loop_body(id, body, out);
        }
        BlockKind::RunTest {
            test,
            package,
            inputs,
            ..
        } => {
            if let Some(need) = package {
                if need.version.parse::<Version>().is_err() {
                    out.push(err("flow.package.version", "package.version"));
                }
                if need
                    .source
                    .as_ref()
                    .is_some_and(|s| !s.starts_with("https://"))
                {
                    out.push(err("flow.package.source", "package.source"));
                }
            }
            if package_of_path(test).is_some() {
                relative_path(id, "test", test, out);
            } else {
                let ext = if test.ends_with(BLOCK_SUFFIX) {
                    BLOCK_SUFFIX
                } else {
                    TEST_SUFFIX
                };
                path_with_extension(id, "test", test, ext, out);
            }
            let mut names: Vec<&String> = inputs.keys().collect();
            names.sort();
            for name in names.into_iter().filter(|n| !is_identifier(n)) {
                out.push(err("flow.input.invalid", &format!("inputs.{name}")).with("name", name));
            }
        }
        BlockKind::Script {
            code,
            inputs,
            timeout_ms,
            ..
        } => {
            if code.trim().is_empty() {
                out.push(err("flow.field.empty", "code"));
            }
            names(id, "inputs", inputs.iter().map(|i| i.name.as_str()), out);
            for (i, input) in inputs.iter().enumerate() {
                input_value(id, i, input, out);
            }
            timeout(id, *timeout_ms, out);
        }
        BlockKind::Command {
            command,
            folder,
            exit_code,
            timeout_ms,
            ..
        } => {
            if command.trim().is_empty() {
                out.push(err("flow.field.empty", "command"));
            }
            if let Some(folder) = folder.as_deref().filter(|f| placeholders(f).is_empty()) {
                relative_path(id, "folder", folder, out);
            }
            if !(0..=255).contains(exit_code) {
                out.push(range(Some(id), "exitCode", 0, 255));
            }
            timeout(id, *timeout_ms, out);
        }
        BlockKind::Http {
            url,
            headers,
            expect_status,
            save,
            timeout_ms,
            ..
        } => {
            if url.trim().is_empty() {
                out.push(err("flow.field.empty", "url"));
            } else if !url.starts_with("${") && !is_web_address(url) {
                out.push(err("flow.http.url", "url").with("value", url));
            }
            for (i, h) in headers.iter().enumerate() {
                let field = format!("headers.{i}.name");
                if h.name.is_empty() {
                    out.push(err("flow.field.empty", &field));
                } else if !is_header_name(&h.name) {
                    out.push(err("flow.http.header", &field).with("value", &h.name));
                }
            }
            if let ExpectStatus::Exactly(code) = expect_status
                && !(100..=599).contains(code)
            {
                out.push(range(Some(id), "expectStatus", 100, 599));
            }
            for (i, s) in save.iter().enumerate() {
                if !is_answer_part(&s.from) {
                    out.push(
                        err("flow.http.saveFrom", &format!("save.{i}.from")).with("value", &s.from),
                    );
                }
            }
            timeout(id, *timeout_ms, out);
        }
        BlockKind::CheckFile {
            path,
            until,
            timeout_ms,
            ..
        } => {
            if path.trim().is_empty() {
                out.push(err("flow.field.empty", "path"));
            }
            if matches!(until, FileState::Contains(text) if text.is_empty()) {
                out.push(err("flow.field.empty", "until.contains"));
            }
            timeout(id, *timeout_ms, out);
        }
        BlockKind::Database {
            file,
            query,
            timeout_ms,
            ..
        } => {
            if file.trim().is_empty() {
                out.push(err("flow.field.empty", "file"));
            }
            if query.trim().is_empty() {
                out.push(err("flow.field.empty", "query"));
            }
            timeout(id, *timeout_ms, out);
        }
        BlockKind::Clipboard { action, .. } => {
            if let ClipboardAction::Check { is, to, timeout_ms } = action {
                if let Some(bad) = not_a_number(*is, to) {
                    out.push(err("flow.value.notNumber", "to").with("value", bad));
                }
                timeout(id, *timeout_ms, out);
            }
        }
        BlockKind::DateTime { format, shift, .. } => {
            if format.is_empty() {
                out.push(err("flow.field.empty", "format"));
            } else if chrono::format::StrftimeItems::new(&without_name_specifiers(format))
                .any(|item| item == chrono::format::Item::Error)
            {
                out.push(err("flow.dateTime.format", "format").with("value", format));
            }
            if let Some(TimeShift { amount, .. }) = shift
                && amount.unsigned_abs() > MAX_TIME_SHIFT.unsigned_abs()
            {
                out.push(
                    err("flow.field.range", "shift.amount")
                        .with("min", -MAX_TIME_SHIFT)
                        .with("max", MAX_TIME_SHIFT),
                );
            }
        }
    }
    let saved = saved_names(&b.kind);
    for (i, (field, name)) in saved.iter().enumerate() {
        if !is_identifier(name) {
            out.push(err("flow.name.invalid", field).with("value", *name));
        } else if saved[..i].iter().any(|(_, n)| n == name) {
            out.push(err("flow.name.duplicate", field).with("value", *name));
        }
    }
    for (field, value) in placeholder_fields(&b.kind) {
        // A shell has `${NAME}` of its own; only placeholders JEVision knows are checked there.
        if matches!(&b.kind, BlockKind::Command { .. }) {
            continue;
        }
        for bad in placeholders(value).into_iter().filter_map(Result::err) {
            out.push(err("flow.placeholder.invalid", &field).with("placeholder", bad));
        }
    }
}

fn condition(block: &str, at: &str, c: &Condition, out: &mut Vec<Issue>) {
    let r = match c {
        Condition::Screen(r) => r,
        Condition::Value(v) => {
            let err = |code: &str, field: &str| {
                Issue::error(code)
                    .with("block", block)
                    .with("field", format!("{at}value.{field}"))
            };
            if v.of.trim().is_empty() {
                out.push(err("flow.field.empty", "of"));
            }
            if let Some(bad) = not_a_number(v.is, &v.to) {
                out.push(err("flow.value.notNumber", "to").with("value", bad));
            }
            return;
        }
        Condition::Element(e) => return element_check(block, &format!("{at}element"), e, out),
    };
    reference(block, &format!("{at}reference"), r, out);
}

/// An element is picked by at least one of its id, kind and label, and the kind is one a test
/// can name.
fn element(block: &str, field: &str, e: &ElementQuery, out: &mut Vec<Issue>) {
    let issue =
        |code: &str, field: String| Issue::error(code).with("block", block).with("field", field);
    if e.is_empty() {
        out.push(issue("flow.element.empty", field.to_owned()));
    }
    if !e.role.is_empty() && ElementRole::named(&e.role).is_none() {
        let allowed: Vec<&str> = ELEMENT_ROLES.iter().map(|r| r.name).collect();
        out.push(
            issue("flow.field.value", format!("{field}.role"))
                .with("value", &e.role)
                .with("allowed", allowed.join(", ")),
        );
    }
}

/// [`element`], and a text to look for in the value or label.
fn element_check(block: &str, field: &str, c: &ElementCheck, out: &mut Vec<Issue>) {
    element(block, field, &c.element, out);
    if let ElementState::ValueContains(to) | ElementState::TitleContains(to) = &c.is
        && to.is_empty()
    {
        out.push(
            Issue::error("flow.field.empty")
                .with("block", block)
                .with("field", format!("{field}.to")),
        );
    }
}

/// The text a greater-or-less test compares with when it is written out in full and is no
/// number.
fn not_a_number(test: ValueTest, to: &str) -> Option<&str> {
    let numeric = matches!(test, ValueTest::GreaterThan | ValueTest::LessThan);
    (numeric && placeholders(to).is_empty() && to.trim().parse::<f64>().is_err()).then_some(to)
}

/// A script input value written out in full must be one of the kind the input is given as: a
/// picture is a PNG file, as the run needs it to be.
fn input_value(block: &str, i: usize, input: &ScriptInput, out: &mut Vec<Issue>) {
    if input.kind.is_text() || !placeholders(&input.value).is_empty() {
        return;
    }
    let issue = |code: &str| {
        Issue::error(code)
            .with("block", block)
            .with("field", format!("inputs.{i}.value"))
    };
    if input.value.trim().is_empty() {
        out.push(issue("flow.field.empty"));
        return;
    }
    let fits = input.kind.parse(&input.value).is_some_and(|v| {
        input.kind != ValueKind::Image
            || resource_id(&v).is_some()
            || v.to_ascii_lowercase().ends_with(".png")
    });
    if !fits {
        out.push(
            issue("flow.script.inputType")
                .with("name", &input.name)
                .with("type", input.kind.name())
                .with("value", &input.value),
        );
    }
}

/// Input names: environment variable names, each once.
fn names<'a>(block: &str, key: &str, names: impl Iterator<Item = &'a str>, out: &mut Vec<Issue>) {
    let mut seen = HashSet::new();
    for (i, name) in names.enumerate() {
        let issue = |code: &str| {
            Issue::error(code)
                .with("block", block)
                .with("field", format!("{key}.{i}.name"))
                .with("value", name)
        };
        if !is_identifier(name) {
            out.push(issue("flow.name.invalid"));
        } else if !seen.insert(name) {
            out.push(issue("flow.name.duplicate"));
        }
    }
}

fn timeout(block: &str, ms: u64, out: &mut Vec<Issue>) {
    if !(1..=MAX_TIMEOUT_MS).contains(&ms) {
        out.push(
            Issue::error("flow.field.range")
                .with("block", block)
                .with("field", "timeout")
                .with("min", format_duration(1))
                .with("max", format_duration(MAX_TIMEOUT_MS)),
        );
    }
}

/// An HTTP header name: letters, digits and `!#$%&'*+-.^_`|~`.
fn is_header_name(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "!#$%&'*+-.^_`|~".contains(c))
}

/// `status`, `body`, `body.` and a path such as `items[0].id`, or `header.` and a header name.
pub(crate) fn is_answer_part(s: &str) -> bool {
    match s {
        "status" | "body" => true,
        _ => {
            if let Some(name) = s.strip_prefix("header.") {
                return is_header_name(name);
            }
            s.strip_prefix("body.")
                .is_some_and(|path| crate::model::json_path(path).is_some())
        }
    }
}

fn range(block: Option<&str>, field: &str, min: u32, max: u32) -> Issue {
    let issue = Issue::error("flow.field.range");
    let issue = match block {
        Some(b) => issue.with("block", b),
        None => issue,
    };
    issue.with("field", field).with("min", min).with("max", max)
}

fn app_id(block: &str, app: Option<&str>, out: &mut Vec<Issue>) {
    if let Some(app) = app.filter(|a| !is_bundle_id(a)) {
        out.push(
            Issue::error("flow.app.invalid")
                .with("block", block)
                .with("field", "app")
                .with("value", app),
        );
    }
}

fn loop_body(block: &str, body: &str, out: &mut Vec<Issue>) {
    if body == block {
        out.push(Issue::error("flow.loop.emptyBody").with("block", block));
    }
}

/// Reports an empty or absolute path; returns whether the path can be used.
fn relative_path(block: &str, field: &str, path: &str, out: &mut Vec<Issue>) -> bool {
    let issue = |code| Issue::error(code).with("block", block).with("field", field);
    if path.is_empty() {
        out.push(issue("flow.field.empty"));
    } else if !is_relative_path(path) {
        out.push(issue("flow.path.invalid").with("value", path));
    } else {
        return true;
    }
    false
}

fn path_with_extension(block: &str, field: &str, path: &str, ext: &str, out: &mut Vec<Issue>) {
    let wrong_extension = !path.ends_with(ext) || path.ends_with(&format!("/{ext}")) || path == ext;
    if relative_path(block, field, path, out) && wrong_extension {
        out.push(
            Issue::error("flow.path.extension")
                .with("block", block)
                .with("field", field)
                .with("value", path)
                .with("expected", ext),
        );
    }
}

fn target(block: &str, field: &str, t: &Target, out: &mut Vec<Issue>) {
    match t {
        Target::Image(path) => {
            relative_path(block, &format!("{field}.image"), path, out);
        }
        Target::Point(p) => point(block, &format!("{field}.point"), p, out),
        Target::Area(text) => {
            let field = format!("{field}.area");
            if text.trim().is_empty() {
                out.push(
                    Issue::error("flow.field.empty")
                        .with("block", block)
                        .with("field", &field),
                );
            } else if placeholders(text).is_empty() && ValueKind::Area.parse(text).is_none() {
                out.push(
                    Issue::error("flow.target.area")
                        .with("block", block)
                        .with("field", &field)
                        .with("value", text),
                );
            }
        }
        Target::Description(text) => {
            if text.trim().is_empty() {
                out.push(
                    Issue::error("flow.field.empty")
                        .with("block", block)
                        .with("field", format!("{field}.description")),
                );
            }
        }
        Target::Element(e) => element(block, &format!("{field}.element"), e, out),
    }
}

fn point(block: &str, field: &str, p: &Point, out: &mut Vec<Issue>) {
    if !(is_fraction(p.x) && is_fraction(p.y)) {
        out.push(
            Issue::error("flow.point.invalid")
                .with("block", block)
                .with("field", field),
        );
    }
}

/// The rectangle has a size and lies inside the window or image.
pub(crate) fn rect_fits(r: &Rect) -> bool {
    [r.x, r.y, r.width, r.height].into_iter().all(is_fraction)
        && r.width > 0.0
        && r.height > 0.0
        && r.x + r.width <= 1.0
        && r.y + r.height <= 1.0
}

fn rect(block: &str, field: &str, r: &Rect, out: &mut Vec<Issue>) {
    if !rect_fits(r) {
        out.push(
            Issue::error("flow.area.invalid")
                .with("block", block)
                .with("field", field),
        );
    }
}

pub(crate) fn reference(block: &str, field: &str, r: &Reference, out: &mut Vec<Issue>) {
    let image_optional =
        matches!(&r.compare, Compare::Meaning { expect, .. } if !expect.trim().is_empty());
    if !(image_optional && r.image.is_empty()) {
        relative_path(block, &format!("{field}.image"), &r.image, out);
    }
    if let Compare::Meaning { provider, .. } = &r.compare
        && !provider.is_empty()
        && !AI_PROVIDERS.contains(&provider.as_str())
    {
        out.push(
            Issue::error("flow.reference.unknownProvider")
                .with("block", block)
                .with("field", format!("{field}.provider"))
                .with("provider", provider)
                .with("allowed", AI_PROVIDERS.join(", ")),
        );
    }
    if let Compare::Similar { threshold } = r.compare
        && !(threshold > 0.0 && threshold <= 1.0)
    {
        out.push(
            Issue::error("flow.field.range")
                .with("block", block)
                .with("field", format!("{field}.threshold"))
                .with("min", 0)
                .with("max", 1),
        );
    }
    if let Some(a) = &r.area {
        rect(block, &format!("{field}.area"), a, out);
    }
    for (name, list) in [("ignore", &r.ignore), ("hide", &r.hide)] {
        for (i, a) in list.iter().enumerate() {
            rect(block, &format!("{field}.{name}.{i}"), a, out);
        }
    }
}

/// Cycles are allowed only through the `body` edge of a loop block; reachability is checked
/// from `start`.
fn graph(flow: &Flow, blocks: &[&Block], out: &mut Vec<Issue>) {
    let index: HashMap<&str, usize> = blocks
        .iter()
        .enumerate()
        .map(|(i, b)| (b.id.as_str(), i))
        .collect();
    let edges = |include_body: bool| -> Vec<Vec<usize>> {
        blocks
            .iter()
            .map(|b| {
                b.kind
                    .edges()
                    .into_iter()
                    .filter(|(name, _)| include_body || *name != BODY)
                    .filter_map(|(_, t)| index.get(t).copied())
                    .collect()
            })
            .collect()
    };
    let all = edges(true);

    for cycle in cycles(&edges(false)) {
        let ids: Vec<&str> = cycle.iter().map(|&i| blocks[i].id.as_str()).collect();
        out.push(
            Issue::error("flow.cycle")
                .with("block", ids[0])
                .with("blocks", ids.join(", ")),
        );
    }

    if let Some(&start) = index.get(flow.start.as_str()) {
        let reached = reachable(&all, start);
        for (i, b) in blocks.iter().enumerate() {
            if !reached[i] {
                out.push(Issue::warning("flow.block.unreachable").with("block", &b.id));
            }
        }
    }

    for (i, b) in blocks.iter().enumerate() {
        let body = match &b.kind {
            BlockKind::Repeat { body, .. } | BlockKind::UseData { body, .. } => body,
            _ => continue,
        };
        if let Some(&body) = index.get(body.as_str())
            && body != i
            && !reachable(&all, body)[i]
        {
            out.push(Issue::warning("flow.loop.noReturn").with("block", &b.id));
        }
    }
}

pub(crate) fn reachable(edges: &[Vec<usize>], from: usize) -> Vec<bool> {
    let mut seen = vec![false; edges.len()];
    let mut queue = VecDeque::from([from]);
    seen[from] = true;
    while let Some(n) = queue.pop_front() {
        for &m in &edges[n] {
            if !seen[m] {
                seen[m] = true;
                queue.push_back(m);
            }
        }
    }
    seen
}

/// One cycle per back edge found by depth-first search, as block indexes in edge order.
fn cycles(edges: &[Vec<usize>]) -> Vec<Vec<usize>> {
    const NEW: u8 = 0;
    const ON_PATH: u8 = 1;
    const DONE: u8 = 2;
    let mut state = vec![NEW; edges.len()];
    let mut found = Vec::new();
    for root in 0..edges.len() {
        if state[root] != NEW {
            continue;
        }
        state[root] = ON_PATH;
        let mut path: Vec<(usize, usize)> = vec![(root, 0)];
        while let Some(top) = path.last_mut() {
            let (node, next_edge) = *top;
            let Some(&to) = edges[node].get(next_edge) else {
                state[node] = DONE;
                path.pop();
                continue;
            };
            top.1 += 1;
            match state[to] {
                NEW => {
                    state[to] = ON_PATH;
                    path.push((to, 0));
                }
                ON_PATH => {
                    let at = path.iter().position(|&(n, _)| n == to).unwrap_or(0);
                    found.push(path[at..].iter().map(|&(n, _)| n).collect());
                }
                _ => {}
            }
        }
    }
    found
}

/// Splits `${namespace.name}` placeholders out of text; malformed ones are returned as `Err`.
pub(crate) fn placeholders(text: &str) -> Vec<Result<(&str, &str), &str>> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find("${") {
        rest = &rest[at..];
        let Some(end) = rest.find('}') else {
            found.push(Err(rest));
            break;
        };
        let whole = &rest[..=end];
        let parsed = whole[2..end].split_once('.').filter(|(ns, name)| {
            matches!(*ns, "secret" | "env" | "data" | "var" | "input") && is_identifier(name)
        });
        found.push(parsed.ok_or(whole));
        rest = &rest[end + 1..];
    }
    found
}

/// `text` for display, with a word joiner (U+2060) between the characters of every `${…}`
/// placeholder, so a line wraps before or after one and never inside: "Hello, ${data.name}!"
/// doesn't end a line with "Hello, $".
pub fn unbreakable_placeholders(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("${") {
        let Some(len) = rest[at..].find('}') else {
            break;
        };
        out.push_str(&rest[..at]);
        for (i, c) in rest[at..=at + len].chars().enumerate() {
            if i > 0 {
                out.push('\u{2060}');
            }
            out.push(c);
        }
        rest = &rest[at + len + 1..];
    }
    out.push_str(rest);
    out
}

fn is_fraction(v: f64) -> bool {
    (0.0..=1.0).contains(&v)
}

pub(crate) fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn is_block_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_ID_LEN
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Reverse-DNS application identifier, e.g. `com.apple.TextEdit`.
fn is_bundle_id(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() >= 2
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
}

pub(crate) fn is_web_address(s: &str) -> bool {
    ["http://", "https://"]
        .iter()
        .any(|scheme| s.len() > scheme.len() && s.starts_with(scheme))
}

/// A portable path relative to the test file's folder, written with `/`.
pub(crate) fn is_relative_path(s: &str) -> bool {
    !s.is_empty()
        && !is_elsewhere(s)
        && !s.ends_with('/')
        && !s.contains('\\')
        && !s.contains('\0')
        && !s.split('/').any(str::is_empty)
}

/// Whether `path` starts outside the project: at the root, in the home folder (`~` and `~/…`)
/// or on a drive (`C:/…`). A run reads `~notes.png`, `~ann/…` and `a:b.png` in the project.
pub(crate) fn is_elsewhere(path: &str) -> bool {
    let drive = matches!(
        path.as_bytes(),
        [letter, b':', b'/' | b'\\', ..] if letter.is_ascii_alphabetic()
    );
    path.starts_with('/') || path == "~" || path.starts_with("~/") || drive
}

const MODIFIERS: &[&str] = &["cmd", "shift", "option", "ctrl"];
const NAMED_KEYS: &[&str] = &[
    "return",
    "tab",
    "space",
    "delete",
    "forwarddelete",
    "escape",
    "up",
    "down",
    "left",
    "right",
    "home",
    "end",
    "pageup",
    "pagedown",
];

/// `cmd+shift+s`: distinct lowercase modifiers, then one key — a character or a named key.
fn is_key_combo(s: &str) -> bool {
    let tokens: Vec<&str> = s.split('+').collect();
    let Some((key, modifiers)) = tokens.split_last() else {
        return false;
    };
    let mut seen = HashSet::new();
    let modifiers_ok = modifiers
        .iter()
        .all(|m| MODIFIERS.contains(m) && seen.insert(*m));
    let mut chars = key.chars();
    let single_char = matches!((chars.next(), chars.next()), (Some(c), None)
        if c.is_ascii_lowercase() || c.is_ascii_digit() || "-=[]\\;',./`".contains(c));
    let function_key = key
        .strip_prefix('f')
        .and_then(|n| n.parse::<u8>().ok())
        .is_some_and(|n| (1..=20).contains(&n) && !key.starts_with("f0"));
    modifiers_ok && (single_char || function_key || NAMED_KEYS.contains(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_combos() {
        for ok in [
            "cmd+s",
            "cmd+shift+s",
            "return",
            "f12",
            "ctrl+option+space",
            "cmd+/",
        ] {
            assert!(is_key_combo(ok), "{ok}");
        }
        for bad in [
            "",
            "cmd+",
            "cmd",
            "cmd+cmd+s",
            "Cmd+S",
            "s+cmd",
            "f0",
            "f21",
            "f01",
            "cmd++",
            "enterr",
        ] {
            assert!(!is_key_combo(bad), "{bad}");
        }
    }

    #[test]
    fn placeholder_parsing() {
        assert_eq!(
            placeholders("${secret.PASS} and ${data.user_1}"),
            vec![Ok(("secret", "PASS")), Ok(("data", "user_1"))]
        );
        assert_eq!(
            placeholders("${env.X} ${host.X} ${secret.} ${secret.A"),
            vec![
                Ok(("env", "X")),
                Err("${host.X}"),
                Err("${secret.}"),
                Err("${secret.A")
            ]
        );
        assert!(placeholders("plain $ text {}").is_empty());
    }

    #[test]
    fn placeholders_never_break_across_lines() {
        let j = "\u{2060}";
        assert_eq!(
            unbreakable_placeholders("Hello, ${data.name}!"),
            format!("Hello, ${j}{{{j}d{j}a{j}t{j}a{j}.{j}n{j}a{j}m{j}e{j}}}!")
        );
        assert_eq!(
            unbreakable_placeholders("${a}${b} $5 {x} ${open"),
            format!("${j}{{{j}a{j}}}${j}{{{j}b{j}}} $5 {{x}} ${{open")
        );
    }

    #[test]
    fn paths_and_ids() {
        for ok in ["refs/a.png", "../shared/login.flow.json", "a.csv"] {
            assert!(is_relative_path(ok), "{ok}");
        }
        for bad in ["", "/etc/x", "~", "~/x", "a//b", "a\\b", "C:/x", "dir/"] {
            assert!(!is_relative_path(bad), "{bad}");
        }
        // As a run reads them: in the project, like `check` says.
        for in_project in ["~notes.png", "~ann/x.png", "a:b.png", "C:x"] {
            assert!(is_relative_path(in_project), "{in_project}");
        }
        assert!(is_bundle_id("com.apple.TextEdit") && is_bundle_id("org.my-app.x1"));
        assert!(!is_bundle_id("TextEdit") && !is_bundle_id("com..x") && !is_bundle_id("com.a b"));
        assert!(is_block_id("open-app_2") && !is_block_id("open app") && !is_block_id(""));
    }

    #[test]
    fn cycles_are_found_once_per_back_edge() {
        // 0 → 1 → 2 → 0, and 3 → 3.
        let found = cycles(&[vec![1], vec![2], vec![0], vec![3]]);
        assert_eq!(found, vec![vec![0, 1, 2], vec![3]]);
        assert!(cycles(&[vec![1, 2], vec![2], vec![]]).is_empty());
    }
}
