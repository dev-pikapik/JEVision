//! Blocks that people share. A package is a folder `ID.jevblock` that holds `block.json` (who
//! made the block, which version it is, what it asks for) and `flow.json` (its steps, a test
//! like any other) next to the images the steps use. Installed in a project, the package runs
//! like a block the project made itself.
//!
//! This module is the format: reading and writing `block.json`, versions, the checks a package
//! must pass, the steps a user should look at before installing, and the lock file that
//! remembers where a package came from. The files and folders are in [`crate::package_files`].

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;
use std::str::FromStr;

use serde::Serialize;
use serde_json::{Map, Value};

use crate::issue::{Issue, ParseError, describe};
use crate::model::{
    BlockKind, ClipboardAction, Condition, Flow, Reference, Target, ValueKind, WaitUntil,
};
use crate::validate::{
    is_identifier, is_relative_path, is_web_address, required_inputs, validate_in,
};

/// Ending of the folder of a package.
pub const PACKAGE_SUFFIX: &str = ".jevblock";
/// The file in a package that describes it.
pub const MANIFEST_FILE: &str = "block.json";
/// The file in a package with the steps of the block.
pub const PACKAGE_FLOW_FILE: &str = "flow.json";
/// Format of `block.json` written by this build.
pub const PACKAGE_FORMAT: u32 = 1;
/// The file in the project folder that remembers where installed packages came from; committed
/// with the tests so the whole team can update them.
pub const PACKAGE_LOCK_FILE: &str = "jevision.blocks.json";

const MAX_ID_LEN: usize = 100;
const MAX_NAME_LEN: usize = 60;
const MAX_TEXT_LEN: usize = 500;
const MAX_SHORT_LEN: usize = 100;
const MAX_ICON_LEN: usize = 64;
const MAX_INPUTS: usize = 50;

/// A semantic version, `MAJOR.MINOR.PATCH` with an optional `-pre.release` part: `1.2.0` is newer
/// than `1.2.0-beta.2` and older than `1.10.0`. A `+build` part is accepted and ignored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    major: u64,
    minor: u64,
    patch: u64,
    pre: Vec<String>,
}

impl Version {
    pub fn new(major: u64, minor: u64, patch: u64) -> Self {
        Self {
            major,
            minor,
            patch,
            pre: Vec::new(),
        }
    }
}

impl FromStr for Version {
    type Err = ();

    fn from_str(text: &str) -> Result<Self, ()> {
        let text = text.split_once('+').map_or(text, |(version, _)| version);
        let (numbers, pre) = match text.split_once('-') {
            Some((numbers, pre)) => (numbers, Some(pre)),
            None => (text, None),
        };
        let number = |part: &str| {
            let plain = !part.is_empty()
                && part.bytes().all(|b| b.is_ascii_digit())
                && (part == "0" || !part.starts_with('0'));
            plain.then(|| part.parse::<u64>().ok()).flatten().ok_or(())
        };
        let mut parts = numbers.split('.');
        let (Some(major), Some(minor), Some(patch), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(());
        };
        let pre = match pre {
            None => Vec::new(),
            Some(pre) => pre
                .split('.')
                .map(|part| {
                    let word = !part.is_empty()
                        && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
                    let padded = part.len() > 1
                        && part.starts_with('0')
                        && part.bytes().all(|b| b.is_ascii_digit());
                    (word && !padded).then(|| part.to_owned()).ok_or(())
                })
                .collect::<Result<_, _>>()?,
        };
        Ok(Self {
            major: number(major)?,
            minor: number(minor)?,
            patch: number(patch)?,
            pre,
        })
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if !self.pre.is_empty() {
            write!(f, "-{}", self.pre.join("."))?;
        }
        Ok(())
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        let core =
            (self.major, self.minor, self.patch).cmp(&(other.major, other.minor, other.patch));
        // A version with a pre-release part comes before the release itself.
        core.then_with(|| match (self.pre.is_empty(), other.pre.is_empty()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (false, false) => compare_pre(&self.pre, &other.pre),
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Serialize for Version {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

/// Pre-release parts compare one dot-separated piece at a time: numbers as numbers and below
/// words, words as text, and the shorter list first when all else is equal.
fn compare_pre(a: &[String], b: &[String]) -> Ordering {
    for (x, y) in a.iter().zip(b) {
        let order = match (x.parse::<u64>(), y.parse::<u64>()) {
            (Ok(x), Ok(y)) => x.cmp(&y),
            (Ok(_), Err(_)) => Ordering::Less,
            (Err(_), Ok(_)) => Ordering::Greater,
            (Err(_), Err(_)) => x.cmp(y),
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    a.len().cmp(&b.len())
}

/// The version of this build of JEVision.
pub fn app_version() -> Version {
    env!("CARGO_PKG_VERSION")
        .parse()
        .expect("the crate version is a semantic version")
}

/// What a package asks for each time its block is used: one `${input.NAME}` of its steps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PackageInput {
    pub name: String,
    /// What to call it in the editor, when `name` isn't clear enough.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// What the input holds when the test doesn't say.
    #[serde(rename = "default", skip_serializing_if = "Option::is_none")]
    pub default_value: Option<String>,
}

/// The contents of `block.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockManifest {
    /// Like `com.acme.sign-in`: at least three parts of lowercase letters, digits and `-`,
    /// joined by dots. The folder of the installed package is named after it.
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// SF Symbol the block is shown with.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub version: Version,
    pub author: String,
    /// Name of the license, such as `MIT`.
    pub license: String,
    /// Oldest JEVision that can run the block.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_version: Option<Version>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<PackageInput>,
}

impl BlockManifest {
    /// The file `block.json`: fixed key order, two-space indentation and a trailing newline.
    pub fn to_json(&self) -> String {
        #[derive(Serialize)]
        struct File<'a> {
            format: u32,
            #[serde(flatten)]
            manifest: &'a BlockManifest,
        }
        let mut text = serde_json::to_string_pretty(&File {
            format: PACKAGE_FORMAT,
            manifest: self,
        })
        .expect("a manifest always serializes to JSON");
        text.push('\n');
        text
    }
}

/// Why a package or an operation on packages didn't work. Holds problems as localizable issues,
/// the way [`ParseError`] does for a test file.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{}", .issues.iter().map(describe).collect::<Vec<_>>().join(" "))]
pub struct PackageError {
    pub issues: Vec<Issue>,
}

impl PackageError {
    pub fn new(code: &str, params: &[(&str, &str)]) -> Self {
        Self {
            issues: vec![package_issue(code, params)],
        }
    }
}

impl From<ParseError> for PackageError {
    fn from(e: ParseError) -> Self {
        Self { issues: e.issues }
    }
}

/// An error issue with the parameters `params`, for problems with packages found outside the
/// core, such as in an archive.
pub fn package_issue(code: &str, params: &[(&str, &str)]) -> Issue {
    params
        .iter()
        .fold(Issue::error(code), |issue, (name, value)| {
            issue.with(name, value)
        })
}

/// The path `path`, from a test, names a custom block: one the project made
/// (`NAME.block.json`) or the steps of a package (`ID.jevblock/flow.json`).
pub fn is_block_path(path: &str) -> bool {
    path.ends_with(crate::model::BLOCK_SUFFIX) || package_of_path(path).is_some()
}

/// The id of the package whose steps `path` names, as in `blocks/com.acme.sign-in.jevblock/flow.json`.
pub fn package_of_path(path: &str) -> Option<&str> {
    let (folder, file) = path.rsplit_once('/')?;
    if file != PACKAGE_FLOW_FILE {
        return None;
    }
    let folder = folder.rsplit('/').next()?;
    folder
        .strip_suffix(PACKAGE_SUFFIX)
        .filter(|id| !id.is_empty())
}

/// Whether `id` can be the id of a package.
pub fn is_package_id(id: &str) -> bool {
    let parts: Vec<&str> = id.split('.').collect();
    id.len() <= MAX_ID_LEN
        && parts.len() >= 3
        && parts.iter().all(|part| {
            part.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
                && !part.ends_with('-')
                && part
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        })
}

/// An id to start from for a block called `name` made by `author`, such as
/// `com.anna-smith.sign-in`. Letters that aren't ASCII are left out of the id.
pub fn suggest_id(author: &str, name: &str) -> String {
    fn slug(text: &str, fallback: &str) -> String {
        let mut slug = String::new();
        for c in text.chars().map(|c| c.to_ascii_lowercase()) {
            if c.is_ascii_lowercase() || c.is_ascii_digit() {
                slug.push(c);
            } else if !slug.is_empty() && !slug.ends_with('-') {
                slug.push('-');
            }
        }
        let slug = slug.trim_end_matches('-');
        let slug: String = slug.chars().take(30).collect();
        let slug = slug.trim_end_matches('-');
        if slug.is_empty() {
            fallback.to_owned()
        } else {
            slug.to_owned()
        }
    }
    format!("com.{}.{}", slug(author, "author"), slug(name, "block"))
}

fn is_icon(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_ICON_LEN
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.')
}

/// Reads the text of a `block.json`. Holds every problem found.
pub fn parse_manifest(text: &str) -> Result<BlockManifest, PackageError> {
    let value: Value = serde_json::from_str(text).map_err(|e| {
        PackageError::new(
            "block.json.syntax",
            &[
                ("line", &e.line().to_string()),
                ("column", &e.column().to_string()),
            ],
        )
    })?;
    let Some(object) = value.as_object() else {
        return Err(PackageError::new("block.json.notObject", &[]));
    };
    let mut reader = Reader {
        object,
        issues: Vec::new(),
    };
    reader.format();
    let id = reader.text("id", MAX_ID_LEN, true);
    let name = reader.text("name", MAX_NAME_LEN, true);
    let description = reader.text("description", MAX_TEXT_LEN, false);
    let icon = reader.text("icon", MAX_ICON_LEN, false);
    let version = reader.version("version", true);
    let author = reader.text("author", MAX_SHORT_LEN, true);
    let license = reader.text("license", MAX_SHORT_LEN, true);
    let min_version = reader.version("minVersion", false);
    let inputs = reader.inputs();
    for key in object.keys() {
        if !KEYS.contains(&key.as_str()) {
            reader
                .issues
                .push(Issue::error("block.field.unknown").with("field", key));
        }
    }
    if let Some(id) = id.as_deref().filter(|id| !is_package_id(id)) {
        reader
            .issues
            .push(Issue::error("block.id.invalid").with("value", id));
    }
    if let Some(icon) = icon.as_deref().filter(|icon| !is_icon(icon)) {
        reader
            .issues
            .push(Issue::error("block.icon.invalid").with("value", icon));
    }
    match (id, name, version, author, license) {
        (Some(id), Some(name), Some(version), Some(author), Some(license))
            if reader.issues.is_empty() =>
        {
            Ok(BlockManifest {
                id,
                name,
                description,
                icon,
                version,
                author,
                license,
                min_version,
                inputs,
            })
        }
        _ => Err(PackageError {
            issues: reader.issues,
        }),
    }
}

const KEYS: [&str; 10] = [
    "format",
    "id",
    "name",
    "description",
    "icon",
    "version",
    "author",
    "license",
    "minVersion",
    "inputs",
];

struct Reader<'a> {
    object: &'a Map<String, Value>,
    issues: Vec<Issue>,
}

impl Reader<'_> {
    fn format(&mut self) {
        match self.object.get("format") {
            None => self
                .issues
                .push(Issue::error("block.field.missing").with("field", "format")),
            Some(v) if v.as_u64() == Some(u64::from(PACKAGE_FORMAT)) => {}
            Some(v) => self.issues.push(
                Issue::error("block.format.unsupported")
                    .with("found", v)
                    .with("supported", PACKAGE_FORMAT),
            ),
        }
    }

    /// The text field `key`, trimmed; `None` when it's missing or can't be used, with the reason
    /// kept unless it is missing and `required` is off.
    fn text(&mut self, key: &str, max: usize, required: bool) -> Option<String> {
        match self.object.get(key) {
            None | Some(Value::Null) => {
                if required {
                    self.issues
                        .push(Issue::error("block.field.missing").with("field", key));
                }
                None
            }
            Some(Value::String(s)) if s.trim().is_empty() => {
                self.issues
                    .push(Issue::error("block.field.empty").with("field", key));
                None
            }
            Some(Value::String(s)) if s.trim().chars().count() > max => {
                self.issues.push(
                    Issue::error("block.field.tooLong")
                        .with("field", key)
                        .with("max", max),
                );
                None
            }
            Some(Value::String(s)) => Some(s.trim().to_owned()),
            Some(_) => {
                self.issues.push(
                    Issue::error("block.field.type")
                        .with("field", key)
                        .with("expected", "text"),
                );
                None
            }
        }
    }

    fn version(&mut self, key: &str, required: bool) -> Option<Version> {
        let text = self.text(key, MAX_SHORT_LEN, required)?;
        let version = text.parse().ok();
        if version.is_none() {
            self.issues.push(
                Issue::error("block.version.invalid")
                    .with("field", key)
                    .with("value", text),
            );
        }
        version
    }

    fn inputs(&mut self) -> Vec<PackageInput> {
        let mut inputs: Vec<PackageInput> = Vec::new();
        let list = match self.object.get("inputs") {
            None => return inputs,
            Some(Value::Array(list)) => list,
            Some(_) => {
                self.issues.push(
                    Issue::error("block.field.type")
                        .with("field", "inputs")
                        .with("expected", "a list"),
                );
                return inputs;
            }
        };
        if list.len() > MAX_INPUTS {
            self.issues.push(
                Issue::error("block.field.tooLong")
                    .with("field", "inputs")
                    .with("max", MAX_INPUTS),
            );
            return inputs;
        }
        for (i, item) in list.iter().enumerate() {
            let field = format!("inputs.{i}");
            let Some(entry) = item.as_object() else {
                self.issues.push(
                    Issue::error("block.field.type")
                        .with("field", &field)
                        .with("expected", "an object"),
                );
                continue;
            };
            let mut reader = Reader {
                object: entry,
                issues: Vec::new(),
            };
            let name = reader.text("name", MAX_SHORT_LEN, true);
            let label = reader.text("label", MAX_NAME_LEN, false);
            let default_value = match entry.get("default") {
                None | Some(Value::Null) => None,
                Some(Value::String(s)) if s.chars().count() <= MAX_TEXT_LEN => Some(s.clone()),
                Some(_) => {
                    reader.issues.push(
                        Issue::error("block.field.type")
                            .with("field", "default")
                            .with("expected", "text"),
                    );
                    None
                }
            };
            for key in entry
                .keys()
                .filter(|k| !["name", "label", "default"].contains(&k.as_str()))
            {
                reader
                    .issues
                    .push(Issue::error("block.field.unknown").with("field", key));
            }
            for mut issue in reader.issues {
                issue.params.retain(|p| p.name != "field");
                self.issues.push(issue.with("field", &field));
            }
            let Some(name) = name else { continue };
            if !is_identifier(&name) {
                self.issues
                    .push(Issue::error("block.input.invalid").with("name", &name));
            } else if inputs.iter().any(|i| i.name == name) {
                self.issues
                    .push(Issue::error("block.input.duplicate").with("name", &name));
            } else {
                inputs.push(PackageInput {
                    name,
                    label,
                    default_value,
                });
            }
        }
        inputs
    }
}

/// Everything wrong with the package whose block.json is `manifest` and whose steps are `flow`,
/// the contents of the folder `dir`: for a block that can't be installed, errors; for one that
/// can but has a flaw to know of, warnings.
pub fn check_package(manifest: &BlockManifest, flow: &Flow, dir: &Path) -> Vec<Issue> {
    let mut out = Vec::new();
    if let Some(needs) = manifest
        .min_version
        .as_ref()
        .filter(|needs| **needs > app_version())
    {
        out.push(
            Issue::error("block.version.needsNewer")
                .with("needs", needs)
                .with("have", app_version()),
        );
    }
    // Blocks rarely check the screen themselves; the tests that run them do.
    out.extend(
        validate_in(flow, dir)
            .into_iter()
            .filter(|i| i.code != "flow.noCheck"),
    );
    let read = required_inputs(flow);
    for name in &read {
        if !manifest.inputs.iter().any(|i| &i.name == name) {
            out.push(Issue::error("block.input.undeclared").with("name", name));
        }
    }
    for input in manifest.inputs.iter().filter(|i| !read.contains(&i.name)) {
        out.push(Issue::warning("block.input.unused").with("name", &input.name));
    }
    let mut paths: Vec<(String, String)> = flow
        .blocks
        .iter()
        .flat_map(|b| other_paths(&b.kind).into_iter().map(|p| (b.id.clone(), p)))
        .collect();
    visit_images(&mut flow.clone(), |block, path| {
        paths.push((block.to_owned(), path.clone()));
    });
    for (block, path) in paths {
        if is_relative_path(&path) && path.split('/').any(|part| part == "..") {
            out.push(
                Issue::error("block.path.outside")
                    .with("block", block)
                    .with("path", path),
            );
        }
    }
    out
}

/// Paths the steps of a block read, other than the images: files to open, data tables, and the
/// test a step runs.
fn other_paths(kind: &BlockKind) -> Vec<String> {
    match kind {
        BlockKind::OpenFile { files, .. } => files.clone(),
        BlockKind::UseData { file, .. } => vec![file.clone()],
        BlockKind::RunTest { test, .. } => vec![test.clone()],
        _ => Vec::new(),
    }
}

/// Calls `visit` with the id of the block and each image path in it: what a block clicks on,
/// waits for or compares the screen with.
pub(crate) fn visit_images(flow: &mut Flow, mut visit: impl FnMut(&str, &mut String)) {
    for b in &mut flow.blocks {
        for path in images_mut(&mut b.kind) {
            visit(&b.id, path);
        }
    }
}

/// Like [`visit_images`], over the images and the other files a block takes from beside the
/// test, which are the files to open and the data table; `true` for an image.
pub(crate) fn visit_files(flow: &mut Flow, mut visit: impl FnMut(&str, bool, &mut String)) {
    for b in &mut flow.blocks {
        let others: Vec<&mut String> = match &mut b.kind {
            BlockKind::OpenFile { files, .. } => {
                files.iter_mut().filter(|f| !is_web_address(f)).collect()
            }
            BlockKind::UseData { file, .. } => vec![file],
            _ => Vec::new(),
        };
        for path in others {
            visit(&b.id, false, path);
        }
        for path in images_mut(&mut b.kind) {
            visit(&b.id, true, path);
        }
    }
}

fn images_mut(kind: &mut BlockKind) -> Vec<&mut String> {
    fn target(t: &mut Target) -> Option<&mut String> {
        match t {
            Target::Image(path) => Some(path),
            _ => None,
        }
    }
    fn reference(r: &mut Reference) -> Option<&mut String> {
        (!r.image.is_empty()).then_some(&mut r.image)
    }
    fn condition(c: &mut Condition) -> Option<&mut String> {
        match c {
            Condition::Screen(r) => reference(r),
            Condition::Value(_) => None,
        }
    }
    match kind {
        BlockKind::Click { target: t, .. } | BlockKind::Hover { target: t, .. } => {
            target(t).into_iter().collect()
        }
        BlockKind::Drag { from, to, .. } => {
            [target(from), target(to)].into_iter().flatten().collect()
        }
        BlockKind::Scroll { at: Some(t), .. } | BlockKind::Type { into: Some(t), .. } => {
            target(t).into_iter().collect()
        }
        BlockKind::WaitFor {
            until: WaitUntil::Appears(r) | WaitUntil::Disappears(r),
            ..
        }
        | BlockKind::Check { reference: r, .. } => reference(r).into_iter().collect(),
        BlockKind::If {
            condition: c,
            else_if,
            ..
        } => std::iter::once(condition(c))
            .chain(else_if.iter_mut().map(|e| condition(&mut e.condition)))
            .flatten()
            .collect(),
        BlockKind::Script { inputs, .. } => inputs
            .iter_mut()
            .filter(|i| i.kind == ValueKind::Image)
            .map(|i| &mut i.value)
            .collect(),
        _ => Vec::new(),
    }
}

/// What a step can do that its user may not expect, ordered from the most to the least worrying.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Risk {
    /// Runs code or a shell command on this Mac.
    Script,
    /// Sends a request over the network.
    Http,
    /// Reads or changes a database file.
    Database,
    /// Starts apps, or opens files and web addresses.
    OpensApps,
    /// Types text or presses keys, into whatever has the focus.
    Keyboard,
    /// Replaces or reads what is on the clipboard.
    Clipboard,
}

/// A step that carries a [`Risk`], with what exactly it does as the file says it: the code of a
/// script, the address of a request, the text to type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RiskyStep {
    pub block: String,
    pub risk: Risk,
    pub detail: String,
}

/// The steps of `flow` that run code, reach the network, open apps or files, press keys or use
/// the clipboard, in the order the blocks run.
pub fn risky_steps(flow: &Flow) -> Vec<RiskyStep> {
    flow.blocks_in_flow_order()
        .into_iter()
        .filter_map(|b| {
            let (risk, detail) = match &b.kind {
                BlockKind::Script { code, .. } => (Risk::Script, code.clone()),
                BlockKind::Command {
                    command, folder, ..
                } => (
                    Risk::Script,
                    match folder {
                        Some(folder) => format!("{command}\n{folder}"),
                        None => command.clone(),
                    },
                ),
                BlockKind::Http {
                    method, url, body, ..
                } => {
                    let method = format!("{method:?}").to_uppercase();
                    let detail = if body.is_empty() {
                        format!("{method} {url}")
                    } else {
                        format!("{method} {url}\n{body}")
                    };
                    (Risk::Http, detail)
                }
                BlockKind::Database { file, query, .. } => {
                    (Risk::Database, format!("{file}\n{query}"))
                }
                BlockKind::OpenApp { app, .. } => {
                    (Risk::OpensApps, app.clone().unwrap_or_default())
                }
                BlockKind::OpenFile { files, app, .. } => {
                    let mut detail = files.join("\n");
                    if let Some(app) = app {
                        detail.push_str(&format!("\n{app}"));
                    }
                    (Risk::OpensApps, detail)
                }
                BlockKind::Type { text, .. } => (Risk::Keyboard, text.clone()),
                BlockKind::PressKeys { keys, .. } => (Risk::Keyboard, keys.clone()),
                BlockKind::Clipboard { action, .. } => (
                    Risk::Clipboard,
                    match action {
                        ClipboardAction::Put { text } => text.clone(),
                        ClipboardAction::Read { save_as } => save_as.clone(),
                        ClipboardAction::Check { to, .. } => to.clone(),
                    },
                ),
                _ => return None,
            };
            Some(RiskyStep {
                block: b.id.clone(),
                risk,
                detail,
            })
        })
        .collect()
}

/// Where each installed package came from, kept in [`PACKAGE_LOCK_FILE`]: the address a package
/// was installed from, by id. A package installed from a file has no address.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PackageLock {
    pub sources: BTreeMap<String, String>,
}

impl PackageLock {
    /// Reads the text of the lock file.
    pub fn parse(text: &str) -> Result<Self, String> {
        let value: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let sources = match value.get("sources") {
            None => BTreeMap::new(),
            Some(Value::Object(map)) => map
                .iter()
                .map(|(id, source)| {
                    source
                        .as_str()
                        .map(|s| (id.clone(), s.to_owned()))
                        .ok_or_else(|| format!("the source of {id} must be text"))
                })
                .collect::<Result<_, _>>()?,
            Some(_) => return Err("\"sources\" must be an object".to_owned()),
        };
        Ok(Self { sources })
    }

    /// The lock file: ids in alphabetical order so that git shows clean differences.
    pub fn to_json(&self) -> String {
        #[derive(Serialize)]
        struct File<'a> {
            format: u32,
            sources: &'a BTreeMap<String, String>,
        }
        let mut text = serde_json::to_string_pretty(&File {
            format: PACKAGE_FORMAT,
            sources: &self.sources,
        })
        .expect("a lock always serializes to JSON");
        text.push('\n');
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::parse;

    const MANIFEST: &str = r#"{
  "format": 1,
  "id": "com.acme.sign-in",
  "name": "Sign In",
  "description": "Types an e-mail and a password.",
  "icon": "person.badge.key",
  "version": "1.2.0",
  "author": "Acme QA",
  "license": "MIT",
  "minVersion": "0.1.0",
  "inputs": [
    { "name": "email", "label": "E-mail", "default": "me@example.com" },
    { "name": "password" }
  ]
}"#;

    fn codes(e: PackageError) -> Vec<String> {
        e.issues.into_iter().map(|i| i.code).collect()
    }

    #[test]
    fn versions_are_ordered_by_the_semver_rules() {
        let order = [
            "0.9.9",
            "1.0.0-alpha",
            "1.0.0-alpha.1",
            "1.0.0-alpha.beta",
            "1.0.0-beta",
            "1.0.0-beta.2",
            "1.0.0-beta.11",
            "1.0.0-rc.1",
            "1.0.0",
            "1.0.1",
            "1.2.0",
            "1.10.0",
            "2.0.0",
        ];
        let versions: Vec<Version> = order.iter().map(|v| v.parse().unwrap()).collect();
        assert!(versions.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(
            "1.2.3+build.5".parse::<Version>().unwrap().to_string(),
            "1.2.3"
        );
        for bad in [
            "1",
            "1.2",
            "1.2.3.4",
            "01.2.3",
            "1.2.x",
            "1.2.3-",
            "1.2.3-a..b",
            "",
            "v1.2.3",
        ] {
            assert!(bad.parse::<Version>().is_err(), "{bad}");
        }
    }

    #[test]
    fn a_manifest_is_read_and_written_back_the_same() {
        let manifest = parse_manifest(MANIFEST).unwrap();
        assert_eq!(manifest.id, "com.acme.sign-in");
        assert_eq!(manifest.version, Version::new(1, 2, 0));
        assert_eq!(manifest.inputs.len(), 2);
        assert_eq!(
            manifest.inputs[0].default_value.as_deref(),
            Some("me@example.com")
        );
        assert_eq!(parse_manifest(&manifest.to_json()).unwrap(), manifest);
        let json = manifest.to_json();
        let keys: Vec<&str> = json
            .lines()
            .filter(|l| l.starts_with("  \""))
            .filter_map(|l| l.trim().split('"').nth(1))
            .collect();
        assert_eq!(
            keys,
            [
                "format",
                "id",
                "name",
                "description",
                "icon",
                "version",
                "author",
                "license",
                "minVersion",
                "inputs"
            ]
        );
    }

    #[test]
    fn a_manifest_says_everything_that_is_wrong_with_it() {
        let found = |text: &str| codes(parse_manifest(text).unwrap_err());
        assert_eq!(found("{"), ["block.json.syntax"]);
        assert_eq!(found("[]"), ["block.json.notObject"]);
        assert_eq!(
            found(
                r#"{"id":"acme","name":" ","version":"1.0","license":3,"icon":"A B","extra":1,"format":2}"#
            ),
            [
                "block.format.unsupported",
                "block.field.empty",
                "block.version.invalid",
                "block.field.missing",
                "block.field.type",
                "block.field.unknown",
                "block.id.invalid",
                "block.icon.invalid",
            ]
        );
        let inputs = |list: &str| {
            let text = MANIFEST.replace(
                r#"[
    { "name": "email", "label": "E-mail", "default": "me@example.com" },
    { "name": "password" }
  ]"#,
                list,
            );
            codes(parse_manifest(&text).unwrap_err())
        };
        assert_eq!(
            inputs(
                r#"[{"name":"a b"},{"name":"x"},{"name":"x"},{"label":"L"},{"name":"y","what":1},7]"#
            ),
            [
                "block.input.invalid",
                "block.input.duplicate",
                "block.field.missing",
                "block.field.unknown",
                "block.field.type"
            ]
        );
    }

    #[test]
    fn ids_have_three_parts_and_paths_find_them() {
        for good in ["com.acme.sign-in", "io.github.anna.log-in2", "a.b.c"] {
            assert!(is_package_id(good), "{good}");
        }
        for bad in [
            "acme.sign-in",
            "Com.acme.x",
            "com.acme.",
            "com..x",
            "com.acme.-x",
            "com.acme.x-",
            "com.acme.a_b",
            "com/acme/x",
        ] {
            assert!(!is_package_id(bad), "{bad}");
        }
        assert_eq!(
            suggest_id("Anna Smith", "Sign In!"),
            "com.anna-smith.sign-in"
        );
        assert_eq!(suggest_id("Анна", "Вход"), "com.author.block");
        assert!(is_package_id(&suggest_id("  ", &"x".repeat(200))));
        let flow = "blocks/com.acme.sign-in.jevblock/flow.json";
        assert_eq!(package_of_path(flow), Some("com.acme.sign-in"));
        assert_eq!(
            package_of_path("com.acme.x.jevblock/flow.json"),
            Some("com.acme.x")
        );
        assert_eq!(package_of_path("blocks/x.jevblock/block.json"), None);
        assert_eq!(package_of_path("flow.json"), None);
        assert_eq!(package_of_path("blocks/x/flow.json"), None);
        assert!(is_block_path(flow) && is_block_path("blocks/a.block.json"));
        assert!(!is_block_path("a.flow.json"));
    }

    fn flow(text: &str) -> Flow {
        parse(text).unwrap()
    }

    const STEPS: &str = r#"{"format":1,"name":"Sign In","app":"com.example.App","start":"mail","blocks":{
        "mail":{"type":"type","text":"${input.email}","next":"hit"},
        "hit":{"type":"click","target":{"image":"images/go.png"},"next":"ask"},
        "ask":{"type":"http","method":"POST","url":"https://example.com/in","body":"{}","next":"sh"},
        "sh":{"type":"script","language":"sh","code":"rm -rf x","next":"db"},
        "db":{"type":"database","file":"a.db","query":"delete from t","next":"copy"},
        "copy":{"type":"clipboard","action":"put","text":"x","next":"app"},
        "app":{"type":"openApp","app":"com.apple.Terminal","next":"keys"},
        "keys":{"type":"pressKeys","keys":"return"}}}"#;

    #[test]
    fn the_steps_to_look_at_are_found_in_the_order_they_run() {
        let steps = risky_steps(&flow(STEPS));
        let got: Vec<(&str, Risk)> = steps.iter().map(|s| (s.block.as_str(), s.risk)).collect();
        assert_eq!(
            got,
            [
                ("mail", Risk::Keyboard),
                ("ask", Risk::Http),
                ("sh", Risk::Script),
                ("db", Risk::Database),
                ("copy", Risk::Clipboard),
                ("app", Risk::OpensApps),
                ("keys", Risk::Keyboard),
            ]
        );
        assert_eq!(steps[1].detail, "POST https://example.com/in\n{}");
        assert_eq!(steps[2].detail, "rm -rf x");
        assert!(Risk::Script < Risk::Http && Risk::OpensApps < Risk::Keyboard);
        let quiet = flow(
            r#"{"format":1,"name":"Q","app":"com.example.App","start":"a","blocks":{
                "a":{"type":"click","target":{"image":"a.png"},"next":"b"},
                "b":{"type":"pause","duration":"1s"}}}"#,
        );
        assert!(risky_steps(&quiet).is_empty());
    }

    #[test]
    fn a_package_must_declare_what_it_reads_and_stay_in_its_folder() {
        let dir = std::env::temp_dir().join("jevision-package-check");
        let manifest = parse_manifest(MANIFEST).unwrap();
        let found = |manifest: &BlockManifest, steps: &str| -> Vec<(String, String)> {
            check_package(manifest, &flow(steps), &dir)
                .into_iter()
                .filter(|i| i.code.starts_with("block."))
                .map(|i| {
                    (
                        i.code.clone(),
                        i.param("name")
                            .or(i.param("path"))
                            .unwrap_or_default()
                            .to_owned(),
                    )
                })
                .collect()
        };
        // `password` is declared but never read; `email` is read and declared.
        assert_eq!(
            found(&manifest, STEPS),
            [("block.input.unused".to_owned(), "password".to_owned())]
        );
        let mut bare = manifest.clone();
        bare.inputs.clear();
        assert_eq!(
            found(&bare, STEPS),
            [("block.input.undeclared".to_owned(), "email".to_owned())]
        );
        let outside = r#"{"format":1,"name":"O","app":"com.example.App","start":"a","blocks":{
            "a":{"type":"openFile","files":["../secrets.txt","https://example.com"],"next":"b"},
            "b":{"type":"runTest","test":"../other.block.json"}}}"#;
        assert_eq!(
            found(&bare, outside),
            [
                ("block.path.outside".to_owned(), "../secrets.txt".to_owned()),
                (
                    "block.path.outside".to_owned(),
                    "../other.block.json".to_owned()
                ),
            ]
        );
        let mut newer = manifest;
        newer.min_version = Some(Version::new(99, 0, 0));
        assert_eq!(found(&newer, STEPS)[0].0, "block.version.needsNewer");
    }

    #[test]
    fn images_are_visited_wherever_a_block_keeps_them() {
        let mut f = flow(
            r#"{"format":1,"name":"I","app":"com.example.App","start":"a","blocks":{
                "a":{"type":"click","target":{"image":"a.png"},"next":"b"},
                "b":{"type":"drag","from":{"image":"b.png"},"to":{"point":{"x":0.5,"y":0.5}},"next":"c"},
                "c":{"type":"waitFor","until":{"appears":{"image":"c.png"}},"timeout":"5s","next":"d"},
                "d":{"type":"if","reference":{"image":"d.png"},"elseIf":[{"reference":{"image":"e.png"},"then":"f"}],"then":"f"},
                "f":{"type":"check","reference":{"image":"f.png"}}}}"#,
        );
        let mut seen = Vec::new();
        visit_images(&mut f, |_, p| {
            seen.push(p.clone());
            p.insert_str(0, "images/");
        });
        assert_eq!(seen, ["a.png", "b.png", "c.png", "d.png", "e.png", "f.png"]);
        let mut again = Vec::new();
        visit_images(&mut f, |_, p| again.push(p.clone()));
        assert_eq!(again[0], "images/a.png");
    }

    #[test]
    fn the_json_schema_says_what_the_format_says() {
        let schema: Value =
            serde_json::from_str(include_str!("../../../docs/schemas/block.schema.json")).unwrap();
        let mut described: Vec<&str> = schema["properties"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        described.sort_unstable();
        let mut known = KEYS.to_vec();
        known.sort_unstable();
        assert_eq!(described, known);
        assert_eq!(schema["additionalProperties"], false);

        let required: Vec<&str> = schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|k| k.as_str().unwrap())
            .collect();
        let full: Value = serde_json::from_str(MANIFEST).unwrap();
        for key in known {
            let mut without = full.clone();
            without.as_object_mut().unwrap().remove(key);
            assert_eq!(
                parse_manifest(&without.to_string()).is_err(),
                required.contains(&key),
                "{key}"
            );
        }

        for (key, max) in [
            ("name", MAX_NAME_LEN),
            ("description", MAX_TEXT_LEN),
            ("author", MAX_SHORT_LEN),
            ("license", MAX_SHORT_LEN),
        ] {
            assert_eq!(schema["properties"][key]["maxLength"], max, "{key}");
            let mut longest = full.clone();
            longest[key] = "a".repeat(max).into();
            assert!(parse_manifest(&longest.to_string()).is_ok(), "{key}");
            longest[key] = "a".repeat(max + 1).into();
            assert!(parse_manifest(&longest.to_string()).is_err(), "{key}");
        }
        assert_eq!(schema["properties"]["inputs"]["maxItems"], MAX_INPUTS);
        assert_eq!(schema["properties"]["id"]["maxLength"], MAX_ID_LEN);
        assert_eq!(schema["properties"]["icon"]["maxLength"], MAX_ICON_LEN);
    }

    #[test]
    fn the_lock_keeps_sources_in_alphabetical_order() {
        let mut lock = PackageLock::default();
        lock.sources
            .insert("com.b.x".into(), "https://github.com/b/x".into());
        lock.sources
            .insert("com.a.y".into(), "https://github.com/a/y".into());
        let text = lock.to_json();
        assert!(text.find("com.a.y").unwrap() < text.find("com.b.x").unwrap());
        assert_eq!(PackageLock::parse(&text).unwrap(), lock);
        assert!(PackageLock::parse("{").is_err());
        assert!(PackageLock::parse(r#"{"sources":{"com.a.y":1}}"#).is_err());
        assert_eq!(PackageLock::parse("{}").unwrap(), PackageLock::default());
    }
}
