//! What changed between two versions of a test, block by block, for showing on the flowchart.

use std::collections::{BTreeSet, HashMap};

use serde_json::Value;

use crate::model::{BODY, Block, ELSE, Flow, NEXT, THEN};

/// How a block differs between the two versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockChangeKind {
    Added,
    Removed,
    Changed,
}

/// One block that differs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockChange {
    pub id: String,
    pub kind: BlockChangeKind,
    /// Keys of the settings that differ, as in the file (`target`, `timeout`, `type` when the
    /// block became another kind), for a changed block; empty otherwise.
    pub fields: Vec<String>,
}

/// The differences between two versions of a test.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FlowDiff {
    /// Blocks of the newer version in reading order, then removed blocks in their old order.
    pub blocks: Vec<BlockChange>,
    /// Keys of the test's own settings that differ (`name`, `app`, `tags`, …).
    pub test_fields: Vec<String>,
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
    FlowDiff {
        blocks,
        test_fields,
    }
}

/// A block's settings as in the file, without its arrows to other blocks.
fn settings(block: &Block) -> serde_json::Map<String, Value> {
    let Ok(Value::Object(mut map)) = serde_json::to_value(&block.kind) else {
        return serde_json::Map::new();
    };
    for edge in [NEXT, THEN, ELSE, BODY] {
        map.remove(edge);
    }
    map
}

/// The test's own settings as in the file, without its blocks.
fn test_settings(flow: &Flow) -> serde_json::Map<String, Value> {
    let Ok(Value::Object(mut map)) = serde_json::to_value(flow) else {
        return serde_json::Map::new();
    };
    for key in ["format", "start", "blocks"] {
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
}
