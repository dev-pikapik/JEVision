//! What changed between two versions of a test, block by block, for showing on the flowchart,
//! and which notes and frames on its canvas changed.

use std::collections::{BTreeSet, HashMap};

use serde_json::Value;

use crate::model::{BODY, Block, CanvasSection, ELSE, Flow, NEXT, Note, Rect, THEN};

/// How a block, note or frame differs between the two versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockChangeKind {
    Added,
    Removed,
    Changed,
}

/// One block, note or frame that differs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockChange {
    pub id: String,
    pub kind: BlockChangeKind,
    /// What differs, for a changed item; empty otherwise. For a block, keys of its settings as in
    /// the file (`target`, `timeout`, `type` when the block became another kind). For a note or
    /// frame, `text`, `title` or `author`, and `position` or `size` when it was moved or resized.
    pub fields: Vec<String>,
}

/// The differences between two versions of a test.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FlowDiff {
    /// Blocks of the newer version in reading order, then removed blocks in their old order.
    pub blocks: Vec<BlockChange>,
    /// Keys of the test's own settings that differ (`name`, `app`, `tags`, …).
    pub test_fields: Vec<String>,
    /// Notes of the newer version in their order, then removed notes in their old order.
    pub notes: Vec<BlockChange>,
    /// Frames of the newer version in their order, then removed frames in their old order.
    pub sections: Vec<BlockChange>,
}

/// Compares `before` with `after`; a missing version counts as a test without blocks. Blocks
/// are matched by id. Only what a block does counts as a change: arrows between blocks move
/// whenever blocks are added or removed, which the flowchart shows by itself.
pub fn diff(before: Option<&Flow>, after: Option<&Flow>) -> FlowDiff {
    let old: HashMap<&str, &Block> = before
        .map(|f| f.blocks.iter().map(|b| (b.id.as_str(), b)).collect())
        .unwrap_or_default();
    let new: HashMap<&str, &Block> = after
        .map(|f| f.blocks.iter().map(|b| (b.id.as_str(), b)).collect())
        .unwrap_or_default();
    let mut blocks = Vec::new();
    for block in after.map(Flow::blocks_in_flow_order).unwrap_or_default() {
        let change = match old.get(block.id.as_str()) {
            None => Some((BlockChangeKind::Added, Vec::new())),
            Some(was) => {
                let fields = changed_keys(settings(was), settings(block));
                (!fields.is_empty()).then_some((BlockChangeKind::Changed, fields))
            }
        };
        if let Some((kind, fields)) = change {
            blocks.push(BlockChange {
                id: block.id.clone(),
                kind,
                fields,
            });
        }
    }
    for block in before.map(Flow::blocks_in_flow_order).unwrap_or_default() {
        if !new.contains_key(block.id.as_str()) {
            blocks.push(BlockChange {
                id: block.id.clone(),
                kind: BlockChangeKind::Removed,
                fields: Vec::new(),
            });
        }
    }
    let test_fields = match (before, after) {
        (Some(a), Some(b)) => changed_keys(test_settings(a), test_settings(b)),
        _ => Vec::new(),
    };
    let canvas = |flow: Option<&Flow>| flow.map(|f| f.canvas.clone()).unwrap_or_default();
    let (old_canvas, new_canvas) = (canvas(before), canvas(after));
    FlowDiff {
        blocks,
        test_fields,
        notes: changed_items(&old_canvas.notes, &new_canvas.notes, |n| &n.id, note_fields),
        sections: changed_items(
            &old_canvas.sections,
            &new_canvas.sections,
            |s| &s.id,
            section_fields,
        ),
    }
}

/// Items matched by id between `old` and `new`: added and changed ones in their new order, then
/// removed ones in their old order. `fields` names what differs between two versions of one.
fn changed_items<T>(
    old: &[T],
    new: &[T],
    id: fn(&T) -> &str,
    fields: fn(&T, &T) -> Vec<String>,
) -> Vec<BlockChange> {
    let change = |id: &str, kind, fields| BlockChange {
        id: id.to_owned(),
        kind,
        fields,
    };
    let mut changes: Vec<BlockChange> = new
        .iter()
        .filter_map(|now| match old.iter().find(|was| id(was) == id(now)) {
            None => Some(change(id(now), BlockChangeKind::Added, Vec::new())),
            Some(was) => {
                let fields = fields(was, now);
                (!fields.is_empty()).then(|| change(id(now), BlockChangeKind::Changed, fields))
            }
        })
        .collect();
    changes.extend(
        old.iter()
            .filter(|was| !new.iter().any(|now| id(now) == id(was)))
            .map(|was| change(id(was), BlockChangeKind::Removed, Vec::new())),
    );
    changes
}

/// What differs between two versions of a note. Its time changes with its text, so it isn't
/// named on its own.
fn note_fields(was: &Note, now: &Note) -> Vec<String> {
    let mut fields = Vec::new();
    if was.text != now.text {
        fields.push("text".to_owned());
    }
    if was.author != now.author {
        fields.push("author".to_owned());
    }
    fields.extend(rect_fields(&was.rect, &now.rect));
    fields
}

fn section_fields(was: &CanvasSection, now: &CanvasSection) -> Vec<String> {
    let mut fields = Vec::new();
    if was.title != now.title {
        fields.push("title".to_owned());
    }
    fields.extend(rect_fields(&was.rect, &now.rect));
    fields
}

/// `position` when the rectangle moved, `size` when it was resized.
fn rect_fields(was: &Rect, now: &Rect) -> Vec<String> {
    let mut fields = Vec::new();
    if (was.x, was.y) != (now.x, now.y) {
        fields.push("position".to_owned());
    }
    if (was.width, was.height) != (now.width, now.height) {
        fields.push("size".to_owned());
    }
    fields
}

/// A block's settings as in the file, without its arrows to other blocks.
fn settings(block: &Block) -> serde_json::Map<String, Value> {
    let Ok(Value::Object(mut map)) = serde_json::to_value(&block.kind) else {
        return serde_json::Map::new();
    };
    for edge in [NEXT, THEN, ELSE, BODY] {
        map.remove(edge);
    }
    if let Some(Value::Array(cases)) = map.get_mut("elseIf") {
        for case in cases.iter_mut().filter_map(Value::as_object_mut) {
            case.remove(THEN);
        }
    }
    map
}

/// The test's own settings as in the file, without its blocks and canvas.
fn test_settings(flow: &Flow) -> serde_json::Map<String, Value> {
    let Ok(Value::Object(mut map)) = serde_json::to_value(flow) else {
        return serde_json::Map::new();
    };
    for key in ["format", "start", "blocks", "canvas"] {
        map.remove(key);
    }
    map
}

/// Keys present in only one map or with different values, in key order.
fn changed_keys(
    a: serde_json::Map<String, Value>,
    b: serde_json::Map<String, Value>,
) -> Vec<String> {
    let keys: BTreeSet<&String> = a.keys().chain(b.keys()).collect();
    keys.into_iter()
        .filter(|k| a.get(*k) != b.get(*k))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    fn flow(blocks: &str) -> Flow {
        parse(&format!(
            r#"{{"format":1,"name":"Sign in","app":"com.example.Mail","start":"open","blocks":{{{blocks}}}}}"#
        ))
        .unwrap()
    }

    const OPEN: &str = r#""open":{"type":"openApp","next":"click"}"#;

    fn change(id: &str, kind: BlockChangeKind, fields: &[&str]) -> BlockChange {
        BlockChange {
            id: id.into(),
            kind,
            fields: fields.iter().map(|f| f.to_string()).collect(),
        }
    }

    #[test]
    fn blocks_are_added_removed_and_changed_by_id() {
        let before = flow(&format!(
            r#"{OPEN},
            "click":{{"type":"click","target":{{"image":"refs/ok.png"}},"button":"left","next":"wait"}},
            "wait":{{"type":"waitFor","until":"screenStable","timeout":"5s"}}"#
        ));
        let after = flow(&format!(
            r#"{OPEN},
            "click":{{"type":"click","target":{{"image":"refs/ok.png"}},"button":"right","double":true,"next":"keys"}},
            "keys":{{"type":"pressKeys","keys":"cmd+s"}}"#
        ));
        let diff = diff(Some(&before), Some(&after));
        assert_eq!(
            diff.blocks,
            [
                change("click", BlockChangeKind::Changed, &["button", "double"]),
                change("keys", BlockChangeKind::Added, &[]),
                change("wait", BlockChangeKind::Removed, &[]),
            ]
        );
        assert!(diff.test_fields.is_empty());
    }

    #[test]
    fn only_the_conditions_of_else_if_cases_count_as_a_change() {
        let branch = |then: &str, image: &str| {
            flow(&format!(
                r#""open":{{"type":"if","reference":{{"image":"a.png"}},
                "elseIf":[{{"reference":{{"image":"{image}"}},"then":"{then}"}}]}},
                "x":{{"type":"pressKeys","keys":"x"}},"y":{{"type":"pressKeys","keys":"y"}}"#
            ))
        };
        let before = branch("x", "b.png");
        assert!(
            diff(Some(&before), Some(&branch("y", "b.png")))
                .blocks
                .is_empty()
        );
        assert_eq!(
            diff(Some(&before), Some(&branch("x", "c.png"))).blocks,
            [change("open", BlockChangeKind::Changed, &["elseIf"])]
        );
    }

    #[test]
    fn new_arrows_alone_are_not_a_change() {
        let before = flow(&format!(
            r#"{OPEN},"click":{{"type":"pressKeys","keys":"a"}}"#
        ));
        let after = flow(&format!(
            r#"{OPEN},"click":{{"type":"pressKeys","keys":"a","next":"more"}},
            "more":{{"type":"pressKeys","keys":"b"}}"#
        ));
        assert_eq!(
            diff(Some(&before), Some(&after)).blocks,
            [change("more", BlockChangeKind::Added, &[])]
        );
    }

    #[test]
    fn another_kind_of_block_with_the_same_id_changes_its_type() {
        let before = flow(&format!(
            r#"{OPEN},"click":{{"type":"pressKeys","keys":"a"}}"#
        ));
        let after = flow(&format!(r#"{OPEN},"click":{{"type":"type","text":"a"}}"#));
        assert_eq!(
            diff(Some(&before), Some(&after)).blocks,
            [change(
                "click",
                BlockChangeKind::Changed,
                &["keys", "text", "type"]
            )]
        );
    }

    #[test]
    fn test_settings_and_missing_versions() {
        let before = flow(OPEN);
        let mut after = before.clone();
        after.name = "Sign in again".into();
        after.tags = vec!["smoke".into()];
        assert_eq!(
            diff(Some(&before), Some(&after)).test_fields,
            ["name", "tags"]
        );

        let added = diff(None, Some(&after));
        assert_eq!(added.blocks, [change("open", BlockChangeKind::Added, &[])]);
        assert!(added.test_fields.is_empty());
        let removed = diff(Some(&before), None);
        assert_eq!(
            removed.blocks,
            [change("open", BlockChangeKind::Removed, &[])]
        );
        assert_eq!(diff(None, None), FlowDiff::default());
    }

    #[test]
    fn notes_and_frames_are_added_removed_and_changed_by_id() {
        let rect = |x, width| Rect {
            x,
            y: 0.0,
            width,
            height: 80.0,
        };
        let note = |id: &str, text: &str, rect| Note {
            id: id.into(),
            text: text.into(),
            author: "Ann".into(),
            time: "2026-01-01T00:00:00Z".into(),
            rect,
        };
        let frame = |id: &str, title: &str, rect| CanvasSection {
            id: id.into(),
            title: title.into(),
            rect,
        };
        let mut before = flow(OPEN);
        before.canvas.notes = vec![
            note("kept", "Same", rect(0.0, 200.0)),
            note("edited", "Old", rect(0.0, 200.0)),
            note("gone", "Bye", rect(0.0, 200.0)),
        ];
        before.canvas.sections = vec![
            frame("moved", "Sign in", rect(0.0, 300.0)),
            frame("renamed", "Old", rect(0.0, 300.0)),
        ];
        let mut after = before.clone();
        after.canvas.notes[1].text = "New".into();
        after.canvas.notes[1].time = "2026-02-01T00:00:00Z".into();
        after.canvas.notes[2] = note("new", "Hi", rect(40.0, 200.0));
        after.canvas.sections[0].rect = rect(40.0, 400.0);
        after.canvas.sections[1].title = "New".into();
        after
            .canvas
            .positions
            .insert("open".into(), crate::model::Point { x: 40.0, y: 40.0 });

        let diff = diff(Some(&before), Some(&after));
        assert_eq!(
            diff.notes,
            [
                change("edited", BlockChangeKind::Changed, &["text"]),
                change("new", BlockChangeKind::Added, &[]),
                change("gone", BlockChangeKind::Removed, &[]),
            ]
        );
        assert_eq!(
            diff.sections,
            [
                change("moved", BlockChangeKind::Changed, &["position", "size"]),
                change("renamed", BlockChangeKind::Changed, &["title"]),
            ]
        );
        assert!(diff.blocks.is_empty());
        assert!(
            diff.test_fields.is_empty(),
            "the canvas is not a test setting"
        );
        assert_eq!(
            super::diff(None, Some(&before)).notes.len(),
            3,
            "every note of a new test is added"
        );
    }
}
