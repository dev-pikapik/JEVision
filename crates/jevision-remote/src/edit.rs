//! Changes a phone makes to a test. The Mac makes them with the same core editing its editor
//! uses, reads the changed block back as the file would be read and saves the test.

use std::collections::HashMap;
use std::path::Path;

use jevision_core::{Flow, SaveOutcome, Severity};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::project::slash_path;
use crate::wire::{BlockField, FieldKind, PaletteEntry, Problem, TestEdit, cut};
use crate::{RemoteError, hex};

/// Longest setting a phone sees and sends, in bytes. A longer one, such as a big script, is
/// changed on the Mac.
pub(crate) const MAX_FIELD_VALUE: usize = 4000;
/// Longest test name, in bytes.
pub(crate) const MAX_NAME: usize = 200;
/// Most blocks a test can grow to from a phone.
pub(crate) const MAX_BLOCKS: usize = 500;
/// Most problems of a block that reach the phone.
const MAX_PROBLEMS: usize = 10;
/// Longest code and value of a problem, in bytes.
const MAX_PROBLEM_TEXT: usize = 200;

/// When a setting is offered and what an empty value does to it.
#[derive(Clone, Copy, PartialEq)]
enum Rule {
    /// Always offered; an empty value is written as it is.
    Always,
    /// Always offered; an empty value takes it out of the block.
    Optional,
    /// Offered only when the block has it.
    IfPresent,
    /// One way to find an element: `image` or `description` of the object before the dot. The
    /// object holds one of them, so setting one replaces the other. An empty value takes an
    /// optional object out of the block.
    Find { optional: bool },
}

struct Spec {
    path: &'static str,
    kind: FieldKind,
    rule: Rule,
    choices: &'static [&'static str],
}

const fn field(path: &'static str, kind: FieldKind, rule: Rule) -> Spec {
    Spec {
        path,
        kind,
        rule,
        choices: &[],
    }
}

const fn choice(path: &'static str, choices: &'static [&'static str]) -> Spec {
    Spec {
        path,
        kind: FieldKind::Choice,
        rule: Rule::Always,
        choices,
    }
}

const fn find(path: &'static str, optional: bool) -> Spec {
    field(path, FieldKind::Text, Rule::Find { optional })
}

use FieldKind::{Duration, Flag, LongText, Number, Text};
use Rule::{Always, IfPresent, Optional};

const BUTTONS: &[&str] = &["left", "right", "middle"];
const WAIT_FOR_CHANGE: Spec = field("waitForChange", Flag, Always);
const TIMEOUT: Spec = field("timeout", Duration, Optional);
const REFERENCE: [Spec; 2] = [
    field("reference.image", Text, IfPresent),
    field("reference.threshold", Number, IfPresent),
];

/// The settings of a block of `type_name` a phone can change, in the order the Mac's
/// inspector shows them. Lists, conditions, and settings that hold objects of several shapes
/// are changed on the Mac.
fn specs(type_name: &str) -> &'static [Spec] {
    match type_name {
        "openApp" => {
            const {
                &[
                    field("app", Text, Optional),
                    field("resetState", Flag, Always),
                ]
            }
        }
        "openFile" => {
            const {
                &[
                    field("app", Text, Optional),
                    field("newInstance", Flag, Always),
                    field("closeAfter", Flag, Always),
                ]
            }
        }
        "click" => {
            const {
                &[
                    find("target.image", false),
                    find("target.description", false),
                    choice("button", BUTTONS),
                    field("double", Flag, Always),
                    WAIT_FOR_CHANGE,
                ]
            }
        }
        "hover" => {
            const {
                &[
                    find("target.image", false),
                    find("target.description", false),
                    WAIT_FOR_CHANGE,
                ]
            }
        }
        "drag" => {
            const {
                &[
                    find("from.image", false),
                    find("from.description", false),
                    find("to.image", false),
                    find("to.description", false),
                    choice("button", BUTTONS),
                    WAIT_FOR_CHANGE,
                ]
            }
        }
        "scroll" => {
            const {
                &[
                    choice("direction", &["up", "down", "left", "right"]),
                    field("amount", Number, Always),
                    choice("unit", &["lines", "pixels"]),
                    WAIT_FOR_CHANGE,
                ]
            }
        }
        "type" => {
            const {
                &[
                    field("text", LongText, Always),
                    find("into.image", true),
                    find("into.description", true),
                ]
            }
        }
        "pressKeys" => const { &[field("keys", Text, Always)] },
        "waitFor" => const { &[TIMEOUT] },
        "pause" => const { &[field("duration", Duration, Always)] },
        "check" | "if" => &REFERENCE,
        "repeat" => const { &[field("times", Number, Always)] },
        "runTest" => const { &[field("test", Text, Always)] },
        "useData" => const { &[field("file", Text, Always)] },
        "clipboard" => {
            const {
                &[
                    field("text", LongText, IfPresent),
                    field("saveAs", Text, IfPresent),
                    field("timeout", Duration, IfPresent),
                ]
            }
        }
        "dateTime" => {
            const {
                &[
                    field("saveAs", Text, Always),
                    field("format", Text, Optional),
                    field("language", Text, Optional),
                    field("timeZone", Text, Optional),
                ]
            }
        }
        "http" => {
            const {
                &[
                    choice("method", &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD"]),
                    field("url", Text, Always),
                    field("body", LongText, Optional),
                    TIMEOUT,
                ]
            }
        }
        "database" => {
            const {
                &[
                    field("file", Text, Always),
                    field("query", LongText, Always),
                    field("saveAs", Text, Optional),
                    TIMEOUT,
                ]
            }
        }
        "checkFile" => const { &[field("path", Text, Always), TIMEOUT] },
        "command" => {
            const {
                &[
                    field("command", LongText, Always),
                    field("folder", Text, Optional),
                    TIMEOUT,
                ]
            }
        }
        "script" => {
            const {
                &[
                    choice("language", &["sh", "zsh", "python3", "node", "appleScript"]),
                    field("code", LongText, Always),
                    TIMEOUT,
                ]
            }
        }
        _ => const { &[] },
    }
}

/// The other way to find an element, for a `Find` key.
fn other_way(key: &str) -> &'static str {
    if key == "image" {
        "description"
    } else {
        "image"
    }
}

/// The block as the test file writes it.
fn block_json(kind: &jevision_core::BlockKind) -> Map<String, Value> {
    match serde_json::to_value(kind) {
        Ok(Value::Object(map)) => map,
        _ => Map::new(),
    }
}

/// `path` split at its dot: the object it is in, if any, and its key.
fn split(path: &str) -> (Option<&str>, &str) {
    match path.split_once('.') {
        Some((parent, key)) => (Some(parent), key),
        None => (None, path),
    }
}

/// The value of `spec` in `block` as the phone shows it, or `None` when it is not offered.
fn shown(spec: &Spec, block: &Map<String, Value>) -> Option<String> {
    let (parent, key) = split(spec.path);
    let holder = match parent {
        None => Some(block),
        Some(parent) => match block.get(parent) {
            None => None,
            Some(Value::Object(object)) => Some(object),
            Some(_) => return None,
        },
    };
    if let Rule::Find { optional } = spec.rule {
        let Some(object) = holder else {
            return optional.then(String::new);
        };
        return match (object.get(key), object.get(other_way(key)), object.len()) {
            (Some(Value::String(text)), None, 1) => Some(text.clone()),
            (None, Some(Value::String(_)), 1) => Some(String::new()),
            _ => None,
        };
    }
    let value = holder.and_then(|h| h.get(key));
    let text = match (spec.kind, value) {
        (_, None) if spec.rule == Rule::IfPresent || parent.is_some() && holder.is_none() => {
            return None;
        }
        (Flag, None) => "false".to_owned(),
        (_, None) => String::new(),
        (Flag, Some(Value::Bool(on))) => on.to_string(),
        (Number, Some(Value::Number(n))) => n.to_string(),
        (Text | LongText | Duration | FieldKind::Choice, Some(Value::String(text))) => text.clone(),
        _ => return None,
    };
    (text.len() <= MAX_FIELD_VALUE).then_some(text)
}

/// The settings of `kind` a phone can change, with their values.
pub(crate) fn fields(kind: &jevision_core::BlockKind) -> Vec<BlockField> {
    let block = block_json(kind);
    specs(kind.type_name())
        .iter()
        .filter_map(|spec| {
            Some(BlockField {
                path: spec.path.to_owned(),
                kind: spec.kind,
                value: shown(spec, &block)?,
                choices: spec.choices.iter().map(|c| (*c).to_owned()).collect(),
            })
        })
        .collect()
}

/// Every block type, in the order of the Mac's library.
pub(crate) fn palette() -> Vec<PaletteEntry> {
    jevision_core::catalog()
        .into_iter()
        .map(|spec| PaletteEntry {
            group: match serde_json::to_value(spec.group) {
                Ok(Value::String(group)) => group,
                _ => String::new(),
            },
            block_type: spec.type_name,
        })
        .collect()
}

/// Which version of a test file `text` is.
pub(crate) fn revision(text: &str) -> String {
    hex(&Sha256::digest(text.as_bytes()))
}

/// The problems of each block of `flow`, a test in the folder `dir`, by block id.
pub(crate) fn problems(flow: &Flow, dir: &Path) -> HashMap<String, Vec<Problem>> {
    let mut by_block: HashMap<String, Vec<Problem>> = HashMap::new();
    for issue in jevision_core::validate_in(flow, dir) {
        let Some(block) = issue.param("block").map(str::to_owned) else {
            continue;
        };
        let list = by_block.entry(block).or_default();
        if list.len() == MAX_PROBLEMS {
            continue;
        }
        let mut code = issue.code.clone();
        cut(&mut code, MAX_PROBLEM_TEXT);
        list.push(Problem {
            code,
            warning: issue.severity == Severity::Warning,
            params: issue
                .params
                .iter()
                .map(|p| {
                    let mut value = p.value.clone();
                    cut(&mut value, MAX_PROBLEM_TEXT);
                    (p.name.clone(), value)
                })
                .collect(),
        });
    }
    by_block
}

/// Saves `flow` to `file`, a test file inside the project at `root`, the way the Mac's editor
/// does. A change that needs a newer test format than the project has is left for the Mac,
/// which asks first.
pub(crate) fn save(root: &Path, file: &Path, flow: &Flow) -> Result<(), RemoteError> {
    let path = file
        .strip_prefix(root)
        .map_err(|_| RemoteError::PathOutsideProject)?;
    match jevision_core::save_test(root, &slash_path(path), flow, false) {
        Ok(SaveOutcome::Saved) => Ok(()),
        Ok(SaveOutcome::NeedsConfirmation { .. }) => Err(RemoteError::EditNeedsMac),
        Err(_) => Err(RemoteError::HostFailed),
    }
}

/// Makes `edit` on `flow`. Returns the block the phone shows next, if any.
pub(crate) fn apply(flow: &mut Flow, edit: TestEdit) -> Result<Option<String>, RemoteError> {
    match edit {
        TestEdit::SetField {
            block,
            field,
            value,
        } => {
            if value.len() > MAX_FIELD_VALUE {
                return Err(RemoteError::TooLarge);
            }
            let block = flow
                .blocks
                .iter_mut()
                .find(|b| b.id == block)
                .ok_or(RemoteError::EditInvalid)?;
            let mut json = block_json(&block.kind);
            let spec = specs(block.kind.type_name())
                .iter()
                .find(|s| s.path == field && shown(s, &json).is_some())
                .ok_or(RemoteError::EditInvalid)?;
            set(spec, &mut json, value)?;
            block.kind = jevision_core::parse_block(&Value::Object(json))
                .map_err(|_| RemoteError::EditInvalid)?;
            Ok(None)
        }
        TestEdit::AddBlock { after, block_type } => {
            if flow.blocks.len() >= MAX_BLOCKS {
                return Err(RemoteError::TooLarge);
            }
            let slot = match after {
                Some(id) => Some(flow.slot_after(&id).ok_or(RemoteError::EditInvalid)?),
                None => None,
            };
            let id = flow
                .insert_block(&block_type, slot.as_ref())
                .ok_or(RemoteError::EditInvalid)?;
            Ok(Some(id))
        }
        TestEdit::RemoveBlock { block } => {
            if !flow.blocks.iter().any(|b| b.id == block) {
                return Err(RemoteError::EditInvalid);
            }
            Ok(flow.remove_block(&block))
        }
        TestEdit::Rename { name } => {
            let name = name.trim();
            if name.is_empty() || name.chars().any(char::is_control) {
                return Err(RemoteError::EditInvalid);
            }
            if name.len() > MAX_NAME {
                return Err(RemoteError::TooLarge);
            }
            flow.name = name.to_owned();
            Ok(None)
        }
        TestEdit::Unknown => Err(RemoteError::Unsupported),
    }
}

/// Writes `value`, as the phone shows it, into `block` where `spec` says.
fn set(spec: &Spec, block: &mut Map<String, Value>, value: String) -> Result<(), RemoteError> {
    let (parent, key) = split(spec.path);
    if let (Rule::Find { optional }, Some(parent)) = (spec.rule, parent) {
        let active = block
            .get(parent)
            .and_then(|o| o.get(key))
            .is_some_and(Value::is_string);
        match (value.is_empty(), active, optional) {
            (false, _, _) => {
                block.insert(
                    parent.into(),
                    Map::from_iter([(key.into(), value.into())]).into(),
                );
            }
            (true, true, true) => {
                block.remove(parent);
            }
            (true, true, false) => {
                block.insert(
                    parent.into(),
                    Map::from_iter([(key.into(), "".into())]).into(),
                );
            }
            (true, false, _) => {}
        }
        return Ok(());
    }
    let holder = match parent {
        None => block,
        Some(parent) => match block.get_mut(parent) {
            Some(Value::Object(object)) => object,
            _ => return Err(RemoteError::EditInvalid),
        },
    };
    if value.is_empty() && spec.rule == Rule::Optional {
        holder.remove(key);
        return Ok(());
    }
    let value = match spec.kind {
        Flag => match value.as_str() {
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            _ => return Err(RemoteError::EditInvalid),
        },
        Number => number(&value).ok_or(RemoteError::EditInvalid)?,
        FieldKind::Choice if !spec.choices.contains(&value.as_str()) => {
            return Err(RemoteError::EditInvalid);
        }
        _ => Value::String(value),
    };
    holder.insert(key.into(), value);
    Ok(())
}

/// A whole number as one, anything else finite as a decimal; a comma reads as the decimal
/// point, as many phones type it.
fn number(text: &str) -> Option<Value> {
    let text = text.trim();
    if let Ok(whole) = text.parse::<u64>() {
        return Some(whole.into());
    }
    let decimal: f64 = text.replace(',', ".").parse().ok()?;
    serde_json::Number::from_f64(decimal)
        .filter(|_| decimal.is_finite())
        .map(Value::Number)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every choice the phone is offered reads back as the block it was put in.
    #[test]
    fn every_offered_choice_and_path_is_one_the_file_knows() {
        for spec in jevision_core::catalog() {
            let known = jevision_core::block_fields(&spec.type_name);
            let kind = jevision_core::BlockKind::new(&spec.type_name, "a").unwrap();
            for field in specs(&spec.type_name) {
                let (parent, key) = split(field.path);
                assert!(
                    known.contains(&parent.unwrap_or(key)),
                    "{} {}",
                    spec.type_name,
                    field.path
                );
                for value in field.choices {
                    let mut json = block_json(&kind);
                    set(field, &mut json, (*value).to_owned()).unwrap();
                    let back = jevision_core::parse_block(&Value::Object(json));
                    assert!(back.is_ok(), "{} {} {value}", spec.type_name, field.path);
                }
            }
        }
    }

    #[test]
    fn numbers_read_the_way_phones_type_them() {
        assert_eq!(number("5"), Some(5.into()));
        assert_eq!(number("0,9"), Some(0.9.into()));
        assert_eq!(number(" 0.95 "), Some(0.95.into()));
        assert_eq!(number("five"), None);
        assert_eq!(number("inf"), None);
    }
}
