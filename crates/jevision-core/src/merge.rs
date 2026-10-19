//! Combining two versions of a test that were changed apart, such as here and on the server,
//! block by block rather than line by line: whatever only one side changed is taken as it is,
//! and only what both changed differently is a conflict for the user to settle — per block,
//! per setting and per arrow.

use std::collections::{BTreeMap, HashSet};

use serde_json::{Map, Value};

use crate::model::{BODY, ELSE, ELSE_IF, Flow, NEXT, THEN};
use crate::{ParseError, parse};

type Object = Map<String, Value>;

/// Which part of a test a conflict is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictPart {
    /// The test's own settings: its name, app, tags, first block…
    Test,
    Block,
    Note,
    Frame,
}

/// What both sides did to the same thing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictKind {
    /// Both changed the same setting differently; the whole block when it has no field, as when
    /// either side turned it into another kind of block.
    Changed,
    /// Both sent the same arrow to different blocks.
    Link,
    /// Removed here, changed on the other side.
    RemovedHere,
    /// Removed on the other side, changed here.
    RemovedThere,
    /// Both added one with the same id, each different.
    AddedBoth,
}

/// One thing both sides changed differently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeConflict {
    pub part: ConflictPart,
    /// Id of the block, note or frame; empty for the test's own settings.
    pub id: String,
    /// The setting or arrow as named in the file (`timeout`, `next`, `tags`…); `None` when the
    /// conflict is about the whole block, note or frame.
    pub field: Option<String>,
    pub kind: ConflictKind,
    /// Whether keeping both makes sense: both added steps in the same place, both added a note
    /// or frame, or both added different tags.
    pub can_keep_both: bool,
}

/// How a conflict is settled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeChoice {
    Mine,
    Theirs,
    /// Both versions: theirs added after mine, or the tags of both. Counts as `Mine` where the
    /// conflict can't keep both.
    Both,
}

/// What both `mine` and `theirs` changed differently since `base`, the version they both
/// started from; a missing `base` means both added the test on their own. In reading order:
/// the test's settings, then blocks, frames and notes.
pub fn merge_conflicts(base: Option<&Flow>, mine: &Flow, theirs: &Flow) -> Vec<MergeConflict> {
    Merger::run(base, mine, theirs, &[]).1
}

/// `mine` and `theirs` combined, with each conflict of [`merge_conflicts`] settled by the choice
/// at its position (`Mine` where there is none). A block removed by one side is removed the way
/// the editor removes it — what led to it now leads to what came after it — so that new steps
/// the other side linked to it stay connected. Where blocks sit and the size of notes and
/// frames follow `mine` when both moved them.
pub fn merge_flows(
    base: Option<&Flow>,
    mine: &Flow,
    theirs: &Flow,
    choices: &[MergeChoice],
) -> Result<Flow, ParseError> {
    let (value, _) = Merger::run(base, mine, theirs, choices);
    let mut flow = parse(&value.to_string())?;
    join_around_removed(&mut flow, [Some(mine), Some(theirs), base]);
    Ok(flow)
}

struct Merger<'a> {
    choices: &'a [MergeChoice],
    conflicts: Vec<MergeConflict>,
    /// Blocks one side removed and the other kept, by id: `true` when kept as theirs.
    kept: Vec<(String, bool)>,
}

impl Merger<'_> {
    fn run(
        base: Option<&Flow>,
        mine: &Flow,
        theirs: &Flow,
        choices: &[MergeChoice],
    ) -> (Value, Vec<MergeConflict>) {
        let object = |flow: Option<&Flow>| match flow.map(serde_json::to_value) {
            Some(Ok(Value::Object(map))) => map,
            _ => Object::new(),
        };
        let (b, m, t) = (object(base), object(Some(mine)), object(Some(theirs)));
        let mut merger = Merger {
            choices,
            conflicts: Vec::new(),
            kept: Vec::new(),
        };

        let settings = |o: &Object| -> Object {
            o.iter()
                .filter(|(k, _)| !matches!(k.as_str(), "blocks" | "canvas"))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        };
        let mut out = merger.fields(
            ConflictPart::Test,
            "",
            &settings(&b),
            &settings(&m),
            &settings(&t),
        );

        let order: Vec<&str> = mine
            .blocks_in_flow_order()
            .into_iter()
            .chain(theirs.blocks_in_flow_order())
            .chain(base.map(Flow::blocks_in_flow_order).unwrap_or_default())
            .map(|b| b.id.as_str())
            .collect();
        let versions = [&b, &m, &t].map(|o| child(o, &["blocks"]));
        let mut blocks = merger.items(ConflictPart::Block, &order, versions.clone());
        merger.reconnect_kept([&b, &m, &t], &mut out, &mut blocks, &versions);
        out.insert("blocks".into(), Value::Object(blocks));

        let mut canvas = Object::new();
        let positions = soft_items([&b, &m, &t].map(|o| child(o, &["canvas", "positions"])));
        for (key, part) in [
            ("sections", ConflictPart::Frame),
            ("notes", ConflictPart::Note),
        ] {
            let maps = [&b, &m, &t].map(|o| child(o, &["canvas", key]));
            let order: Vec<&str> = maps[1]
                .keys()
                .chain(maps[2].keys())
                .chain(maps[0].keys())
                .map(String::as_str)
                .collect();
            let items = merger.items(part, &order, maps.clone());
            if !items.is_empty() {
                canvas.insert(key.into(), Value::Object(items));
            }
        }
        if !positions.is_empty() {
            canvas.insert("positions".into(), Value::Object(positions));
        }
        if !canvas.is_empty() {
            out.insert("canvas".into(), Value::Object(canvas));
        }
        (Value::Object(out), merger.conflicts)
    }

    /// Arrows the removing side moved away from a block that is kept after all lead to it
    /// again, unless the keeping side moved them too.
    fn reconnect_kept(
        &self,
        tops: [&Object; 3],
        settings: &mut Object,
        blocks: &mut Object,
        [b, m, t]: &[Object; 3],
    ) {
        for (id, as_theirs) in &self.kept {
            if !blocks.contains_key(id) {
                continue;
            }
            let (keeper, remover) = if *as_theirs { (t, m) } else { (m, t) };
            let to_kept = Some(Value::String(id.clone()));
            let edge = |o: &Object, from: &str, edge: &str| o.get(from)?.get(edge).cloned();
            for (from, block) in blocks.iter_mut() {
                let Some(block) = block.as_object_mut() else {
                    continue;
                };
                for name in [NEXT, THEN, ELSE, BODY] {
                    let kept = edge(keeper, from, name);
                    if kept == to_kept
                        && edge(b, from, name) == kept
                        && block.get(name).cloned() == edge(remover, from, name)
                    {
                        block.insert(name.into(), Value::String(id.clone()));
                    }
                }
            }
            let start = |o: &Object| o.get("start").cloned();
            let (keeper_start, remover_start) = if *as_theirs {
                (start(tops[2]), start(tops[1]))
            } else {
                (start(tops[1]), start(tops[2]))
            };
            if keeper_start == to_kept
                && start(tops[0]) == keeper_start
                && start(settings) == remover_start
            {
                settings.insert("start".into(), Value::String(id.clone()));
            }
        }
    }

    /// Notes a conflict and says how it is settled.
    fn conflict(
        &mut self,
        part: ConflictPart,
        id: &str,
        field: Option<&str>,
        kind: ConflictKind,
        can_keep_both: bool,
    ) -> MergeChoice {
        let choice = self
            .choices
            .get(self.conflicts.len())
            .copied()
            .unwrap_or(MergeChoice::Mine);
        self.conflicts.push(MergeConflict {
            part,
            id: id.to_owned(),
            field: field.map(str::to_owned),
            kind,
            can_keep_both,
        });
        match choice {
            MergeChoice::Both if !can_keep_both => MergeChoice::Mine,
            choice => choice,
        }
    }

    /// Items keyed by id, visited in `order` (ids may repeat), from the base, mine and theirs.
    fn items(&mut self, part: ConflictPart, order: &[&str], [b, m, t]: [Object; 3]) -> Object {
        let mut taken: HashSet<String> = [&b, &m, &t]
            .into_iter()
            .flat_map(|o| o.keys().cloned())
            .collect();
        let mut seen = HashSet::new();
        let mut out = Object::new();
        for &id in order {
            if !seen.insert(id) {
                continue;
            }
            let object = |o: &Object| o.get(id).and_then(Value::as_object).cloned();
            for (id, item) in self.item(part, id, object(&b), object(&m), object(&t), &mut taken) {
                out.insert(id, Value::Object(item));
            }
        }
        out
    }

    /// One block, note or frame as merged: none when it's removed, two when both are kept.
    fn item(
        &mut self,
        part: ConflictPart,
        id: &str,
        b: Option<Object>,
        m: Option<Object>,
        t: Option<Object>,
        taken: &mut HashSet<String>,
    ) -> Vec<(String, Object)> {
        let one = |o: Option<Object>| o.map(|o| (id.to_owned(), o)).into_iter().collect();
        if m == t || b == t {
            return one(m);
        }
        if b == m {
            return one(t);
        }
        match (b, m, t) {
            (None, Some(m), Some(t)) => {
                let both = keep_both(part, id, &m, &t, taken);
                match self.conflict(part, id, None, ConflictKind::AddedBoth, both.is_some()) {
                    MergeChoice::Mine => one(Some(m)),
                    MergeChoice::Theirs => one(Some(t)),
                    MergeChoice::Both => both.unwrap_or_default(),
                }
            }
            (Some(_), None, t) => {
                match self.conflict(part, id, None, ConflictKind::RemovedHere, false) {
                    MergeChoice::Theirs => {
                        self.kept.push((id.to_owned(), true));
                        one(t)
                    }
                    _ => Vec::new(),
                }
            }
            (Some(_), m, None) => {
                match self.conflict(part, id, None, ConflictKind::RemovedThere, false) {
                    MergeChoice::Theirs => Vec::new(),
                    _ => {
                        if part == ConflictPart::Block {
                            self.kept.push((id.to_owned(), false));
                        }
                        one(m)
                    }
                }
            }
            (Some(b), Some(m), Some(t)) => {
                let kind_of = |o: &Object| o.get("type").cloned();
                if part == ConflictPart::Block
                    && (kind_of(&m) != kind_of(&b) || kind_of(&t) != kind_of(&b))
                {
                    match self.conflict(part, id, None, ConflictKind::Changed, false) {
                        MergeChoice::Theirs => one(Some(t)),
                        _ => one(Some(m)),
                    }
                } else {
                    one(Some(self.fields(part, id, &b, &m, &t)))
                }
            }
            // Both sides agree whenever no more than one of the three versions exists.
            (b, m, _) => one(m.or(b)),
        }
    }

    /// One object merged setting by setting.
    fn fields(
        &mut self,
        part: ConflictPart,
        id: &str,
        b: &Object,
        m: &Object,
        t: &Object,
    ) -> Object {
        let mut groups: BTreeMap<&str, Vec<&String>> = BTreeMap::new();
        for key in b.keys().chain(m.keys()).chain(t.keys()) {
            let keys = groups.entry(group(part, key)).or_default();
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
        let mut out = m.clone();
        for (group, keys) in groups {
            let values = |o: &Object| -> Vec<Option<Value>> {
                keys.iter().map(|k| o.get(*k).cloned()).collect()
            };
            let (vb, vm, vt) = (values(b), values(m), values(t));
            let theirs = if vm == vt || vb == vt {
                false
            } else if vb == vm {
                true
            } else if group == "frame" {
                false
            } else {
                let is_link = matches!(
                    (part, group),
                    (ConflictPart::Test, "start")
                        | (ConflictPart::Block, NEXT | THEN | ELSE | BODY)
                );
                let kind = if is_link {
                    ConflictKind::Link
                } else {
                    ConflictKind::Changed
                };
                let both = part == ConflictPart::Test && group == "tags";
                match self.conflict(part, id, Some(group), kind, both) {
                    MergeChoice::Mine => false,
                    MergeChoice::Theirs => true,
                    MergeChoice::Both => {
                        let mut tags: Vec<Value> = Vec::new();
                        for tag in [m, t]
                            .into_iter()
                            .filter_map(|o| o.get("tags")?.as_array())
                            .flatten()
                        {
                            if !tags.contains(tag) {
                                tags.push(tag.clone());
                            }
                        }
                        out.insert("tags".into(), Value::Array(tags));
                        false
                    }
                }
            };
            if theirs {
                for key in keys {
                    match t.get(key) {
                        Some(value) => out.insert(key.clone(), value.clone()),
                        None => out.remove(key),
                    };
                }
            }
        }
        out
    }
}

/// The name a conflict about `key` goes by: a note's time changes with its text, and a frame's
/// or note's place and size come as one.
fn group(part: ConflictPart, key: &str) -> &str {
    match (part, key) {
        (ConflictPart::Note, "time") => "text",
        _ => key,
    }
}

/// Both added items with the same id, side by side: theirs under a new id. Steps are kept both
/// only when both went into the same place, where theirs then follows mine.
fn keep_both(
    part: ConflictPart,
    id: &str,
    m: &Object,
    t: &Object,
    taken: &mut HashSet<String>,
) -> Option<Vec<(String, Object)>> {
    if part == ConflictPart::Block {
        let only_next = |o: &Object| {
            ![THEN, ELSE, BODY, ELSE_IF]
                .iter()
                .any(|k| o.contains_key(*k))
        };
        if !only_next(m) || !only_next(t) || m.get(NEXT) != t.get(NEXT) {
            return None;
        }
    }
    let new_id = (2..)
        .map(|n| format!("{id}{n}"))
        .find(|candidate| !taken.contains(candidate))
        .expect("an unused id always exists");
    taken.insert(new_id.clone());
    let mut mine = m.clone();
    if part == ConflictPart::Block {
        mine.insert(NEXT.into(), Value::String(new_id.clone()));
    }
    Some(vec![(id.to_owned(), mine), (new_id, t.clone())])
}

/// Items where only the latest move counts, such as where blocks sit: theirs when only they
/// changed one, else mine.
fn soft_items([b, m, t]: [Object; 3]) -> Object {
    let mut out = m.clone();
    for (id, value) in &t {
        if m.get(id) != Some(value) && m.get(id) == b.get(id) {
            out.insert(id.clone(), value.clone());
        }
    }
    for id in b.keys() {
        if !t.contains_key(id) && m.get(id) == b.get(id) {
            out.remove(id);
        }
    }
    out
}

/// The object at `path` inside `o`; empty when there is none.
fn child(o: &Object, path: &[&str]) -> Object {
    let mut value = o;
    for key in path {
        match value.get(*key).and_then(Value::as_object) {
            Some(next) => value = next,
            None => return Object::new(),
        }
    }
    value.clone()
}

/// Removes, the way the editor does, blocks that are gone but still have arrows leading to
/// them, taking each from the first version in `versions` that has it.
fn join_around_removed(flow: &mut Flow, versions: [Option<&Flow>; 3]) {
    let mut joined = HashSet::new();
    loop {
        let present: HashSet<&str> = flow.blocks.iter().map(|b| b.id.as_str()).collect();
        let missing = flow
            .blocks
            .iter()
            .flat_map(|b| b.kind.edges().into_iter().map(|(_, to)| to))
            .chain(std::iter::once(flow.start.as_str()))
            .find(|to| !present.contains(to) && !joined.contains(*to))
            .map(str::to_owned);
        let Some(id) = missing else { break };
        let block = versions
            .into_iter()
            .flatten()
            .find_map(|f| f.blocks.iter().find(|b| b.id == id));
        if let Some(block) = block {
            flow.blocks.push(block.clone());
            flow.remove_block(&id);
        }
        joined.insert(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Severity, validate};

    fn flow(blocks: &str) -> Flow {
        flow_with("", blocks)
    }

    fn flow_with(settings: &str, blocks: &str) -> Flow {
        parse(&format!(
            r#"{{"format":1,"name":"Sign in","app":"com.example.Mail",{settings}"start":"open","blocks":{{{blocks}}}}}"#
        ))
        .unwrap()
    }

    const OPEN: &str = r#""open":{"type":"openApp","next":"click"}"#;
    const CLICK: &str = r#""click":{"type":"waitFor","until":"screenStable","timeout":"5s"}"#;

    fn conflict(
        part: ConflictPart,
        id: &str,
        field: Option<&str>,
        kind: ConflictKind,
        both: bool,
    ) -> MergeConflict {
        MergeConflict {
            part,
            id: id.into(),
            field: field.map(Into::into),
            kind,
            can_keep_both: both,
        }
    }

    /// Merged with `choices`, checked to be a valid test in canonical form.
    fn merged(base: &Flow, mine: &Flow, theirs: &Flow, choices: &[MergeChoice]) -> Flow {
        let flow = merge_flows(Some(base), mine, theirs, choices).unwrap();
        let errors: Vec<_> = validate(&flow)
            .into_iter()
            .filter(|i| i.severity == Severity::Error)
            .collect();
        assert!(errors.is_empty(), "{errors:?}");
        flow
    }

    fn block(flow: &Flow, id: &str) -> Option<Value> {
        let block = flow.blocks.iter().find(|b| b.id == id)?;
        serde_json::to_value(&block.kind).ok()
    }

    #[test]
    fn changes_made_on_one_side_are_taken_without_asking() {
        let base = flow(&format!("{OPEN},{CLICK}"));
        let mine = flow(&format!(
            r#"{OPEN},"click":{{"type":"waitFor","until":"screenStable","timeout":"9s"}}"#
        ));
        let theirs = flow_with(
            r#""tags":["smoke"],"#,
            &format!(
                r#"{OPEN},"click":{{"type":"waitFor","until":{{"appears":{{"image":"new.png"}}}},"timeout":"5s"}}"#
            ),
        );
        assert!(merge_conflicts(Some(&base), &mine, &theirs).is_empty());
        let flow = merged(&base, &mine, &theirs, &[]);
        assert_eq!(flow.tags, ["smoke"]);
        let click = block(&flow, "click").unwrap();
        assert_eq!(click["timeout"], "9s");
        assert_eq!(click["until"]["appears"]["image"], "new.png");
    }

    #[test]
    fn the_same_setting_changed_on_both_sides_is_settled_by_choice() {
        let base = flow(&format!("{OPEN},{CLICK}"));
        let with = |timeout: &str| {
            flow(&format!(
                r#"{OPEN},"click":{{"type":"waitFor","until":"screenStable","timeout":"{timeout}"}}"#
            ))
        };
        let (mine, theirs) = (with("7s"), with("9s"));
        assert_eq!(
            merge_conflicts(Some(&base), &mine, &theirs),
            [conflict(
                ConflictPart::Block,
                "click",
                Some("timeout"),
                ConflictKind::Changed,
                false
            )]
        );
        let timeout = |choices: &[MergeChoice]| {
            block(&merged(&base, &mine, &theirs, choices), "click").unwrap()["timeout"].clone()
        };
        assert_eq!(timeout(&[]), "7s");
        assert_eq!(timeout(&[MergeChoice::Theirs]), "9s");
        assert_eq!(
            timeout(&[MergeChoice::Both]),
            "7s",
            "a setting can't keep both"
        );
    }

    #[test]
    fn removed_on_one_side_and_changed_on_the_other() {
        let base = flow(&format!(
            r#""open":{{"type":"openApp","next":"click"}},{CLICK}"#
        ));
        let mine = flow(r#""open":{"type":"openApp"}"#);
        let theirs = flow(&format!(
            r#"{OPEN},"click":{{"type":"waitFor","until":"screenStable","timeout":"9s"}}"#
        ));
        assert_eq!(
            merge_conflicts(Some(&base), &mine, &theirs),
            [conflict(
                ConflictPart::Block,
                "click",
                None,
                ConflictKind::RemovedHere,
                false
            )]
        );
        assert!(block(&merged(&base, &mine, &theirs, &[]), "click").is_none());
        let kept = merged(&base, &mine, &theirs, &[MergeChoice::Theirs]);
        assert_eq!(block(&kept, "click").unwrap()["timeout"], "9s");
        assert_eq!(
            block(&kept, "open").unwrap()["next"],
            "click",
            "kept blocks stay connected"
        );

        assert_eq!(
            merge_conflicts(Some(&base), &theirs, &mine),
            [conflict(
                ConflictPart::Block,
                "click",
                None,
                ConflictKind::RemovedThere,
                false
            )]
        );
    }

    #[test]
    fn steps_both_added_in_the_same_place_can_be_kept_both() {
        let base = flow(&format!("{OPEN},{CLICK}"));
        let added = |keys: &str| {
            flow(&format!(
                r#""open":{{"type":"openApp","next":"keys"}},"keys":{{"type":"pressKeys","keys":"{keys}","next":"click"}},{CLICK}"#
            ))
        };
        let (mine, theirs) = (added("cmd+a"), added("cmd+b"));
        assert_eq!(
            merge_conflicts(Some(&base), &mine, &theirs),
            [conflict(
                ConflictPart::Block,
                "keys",
                None,
                ConflictKind::AddedBoth,
                true
            )]
        );
        let flow = merged(&base, &mine, &theirs, &[MergeChoice::Both]);
        let order: Vec<&str> = flow
            .blocks_in_flow_order()
            .iter()
            .map(|b| b.id.as_str())
            .collect();
        assert_eq!(order, ["open", "keys", "keys2", "click"]);
        assert_eq!(block(&flow, "keys2").unwrap()["keys"], "cmd+b");
        assert_eq!(
            block(
                &merged(&base, &mine, &theirs, &[MergeChoice::Theirs]),
                "keys"
            )
            .unwrap()["keys"],
            "cmd+b"
        );
    }

    #[test]
    fn different_blocks_added_on_both_sides_are_all_kept() {
        let base = flow(OPEN.replace(r#","next":"click""#, "").as_str());
        let mine =
            flow(r#""open":{"type":"openApp","next":"a"},"a":{"type":"pressKeys","keys":"a"}"#);
        let theirs = flow(
            r#""open":{"type":"openApp"},"b":{"type":"pressKeys","keys":"b"},"c":{"type":"pressKeys","keys":"c"}"#,
        );
        assert!(merge_conflicts(Some(&base), &mine, &theirs).is_empty());
        let flow = merge_flows(Some(&base), &mine, &theirs, &[]).unwrap();
        let ids: HashSet<&str> = flow.blocks.iter().map(|b| b.id.as_str()).collect();
        assert_eq!(ids, HashSet::from(["open", "a", "b", "c"]));
    }

    #[test]
    fn arrows_sent_to_different_blocks_are_a_link_conflict() {
        let base = flow(&format!(
            r#"{OPEN},{CLICK},"a":{{"type":"pressKeys","keys":"a"}}"#
        ));
        let to = |target: &str| {
            flow(&format!(
                r#""open":{{"type":"openApp","next":"{target}"}},{CLICK},"a":{{"type":"pressKeys","keys":"a"}}"#
            ))
        };
        let (mine, theirs) = (to("a"), to("open"));
        assert_eq!(
            merge_conflicts(Some(&base), &mine, &theirs),
            [conflict(
                ConflictPart::Block,
                "open",
                Some("next"),
                ConflictKind::Link,
                false
            )]
        );
        let flow = merge_flows(Some(&base), &mine, &theirs, &[MergeChoice::Theirs]).unwrap();
        assert_eq!(block(&flow, "open").unwrap()["next"], "open");
    }

    #[test]
    fn new_steps_leading_to_a_removed_block_are_joined_to_what_came_after_it() {
        let base = flow(&format!(
            r#""open":{{"type":"openApp","next":"click"}},{},"end":{{"type":"pressKeys","keys":"q"}}"#,
            CLICK.replace(r#""timeout":"5s""#, r#""timeout":"5s","next":"end""#)
        ));
        // Here the click goes; there a step is added before it.
        let mine =
            flow(r#""open":{"type":"openApp","next":"end"},"end":{"type":"pressKeys","keys":"q"}"#);
        let theirs = flow(&format!(
            r#""open":{{"type":"openApp","next":"keys"}},"keys":{{"type":"pressKeys","keys":"k","next":"click"}},{},"end":{{"type":"pressKeys","keys":"q"}}"#,
            CLICK.replace(r#""timeout":"5s""#, r#""timeout":"5s","next":"end""#)
        ));
        // `open.next` changed on both sides.
        let conflicts = merge_conflicts(Some(&base), &mine, &theirs);
        assert_eq!(
            conflicts,
            [conflict(
                ConflictPart::Block,
                "open",
                Some("next"),
                ConflictKind::Link,
                false
            )]
        );
        let flow = merged(&base, &mine, &theirs, &[MergeChoice::Theirs]);
        let order: Vec<&str> = flow
            .blocks_in_flow_order()
            .iter()
            .map(|b| b.id.as_str())
            .collect();
        assert_eq!(order, ["open", "keys", "end"]);
    }

    #[test]
    fn a_block_turned_into_another_kind_is_one_conflict() {
        let base = flow(&format!("{OPEN},{CLICK}"));
        let mine = flow(&format!(r#"{OPEN},"click":{{"type":"type","text":"hi"}}"#));
        let theirs = flow(&format!(
            r#"{OPEN},"click":{{"type":"waitFor","until":"screenStable","timeout":"9s"}}"#
        ));
        assert_eq!(
            merge_conflicts(Some(&base), &mine, &theirs),
            [conflict(
                ConflictPart::Block,
                "click",
                None,
                ConflictKind::Changed,
                false
            )]
        );
        let flow = merged(&base, &mine, &theirs, &[MergeChoice::Theirs]);
        assert_eq!(block(&flow, "click").unwrap()["type"], "waitFor");
    }

    #[test]
    fn test_settings_tags_and_the_first_block() {
        let base = flow_with(r#""tags":["a"],"#, &format!("{OPEN},{CLICK}"));
        let mine = flow_with(
            r#""name":"Mine","tags":["a","b"],"#,
            &format!("{OPEN},{CLICK}"),
        );
        let mut theirs = flow_with(
            r#""name":"Theirs","tags":["a","c"],"#,
            &format!("{OPEN},{CLICK}"),
        );
        theirs.start = "click".into();
        let mut mine = mine;
        mine.start = "open".into();
        assert_eq!(
            merge_conflicts(Some(&base), &mine, &theirs),
            [
                conflict(
                    ConflictPart::Test,
                    "",
                    Some("name"),
                    ConflictKind::Changed,
                    false
                ),
                conflict(
                    ConflictPart::Test,
                    "",
                    Some("tags"),
                    ConflictKind::Changed,
                    true
                ),
            ]
        );
        let flow = merged(
            &base,
            &mine,
            &theirs,
            &[MergeChoice::Theirs, MergeChoice::Both],
        );
        assert_eq!(flow.name, "Theirs");
        assert_eq!(flow.tags, ["a", "b", "c"]);
        assert_eq!(flow.start, "click", "only they changed the first block");
    }

    #[test]
    fn notes_and_frames_merge_by_id_and_places_follow_mine() {
        let canvas = |notes: &str, positions: &str| {
            parse(&format!(
                r#"{{"format":1,"name":"Sign in","app":"com.example.Mail","start":"open","blocks":{{{OPEN},{CLICK}}},
                "canvas":{{"positions":{{{positions}}},"notes":{{{notes}}}}}}}"#
            ))
            .unwrap()
        };
        let note = |id: &str, text: &str, x: u32| {
            format!(
                r#""{id}":{{"text":"{text}","frame":{{"x":{x},"y":0,"width":200,"height":80}}}}"#
            )
        };
        let base = canvas(&note("n", "Hi", 0), r#""open":{"x":0,"y":0}"#);
        let mine = canvas(
            &format!("{},{}", note("n", "Mine", 10), note("m", "New", 0)),
            r#""open":{"x":10,"y":0}"#,
        );
        let theirs = canvas(
            &format!("{},{}", note("n", "Theirs", 20), note("m", "Other", 0)),
            r#""open":{"x":20,"y":0}"#,
        );
        assert_eq!(
            merge_conflicts(Some(&base), &mine, &theirs),
            [
                conflict(ConflictPart::Note, "m", None, ConflictKind::AddedBoth, true),
                conflict(
                    ConflictPart::Note,
                    "n",
                    Some("text"),
                    ConflictKind::Changed,
                    false
                ),
            ]
        );
        let flow = merged(
            &base,
            &mine,
            &theirs,
            &[MergeChoice::Both, MergeChoice::Theirs],
        );
        let note = |id: &str| flow.canvas.notes.iter().find(|n| n.id == id).unwrap();
        assert_eq!(
            (note("m").text.as_str(), note("m2").text.as_str()),
            ("New", "Other")
        );
        assert_eq!(
            (note("n").text.as_str(), note("n").rect.x),
            ("Theirs", 10.0)
        );
        assert_eq!(flow.canvas.positions["open"].x, 10.0);
    }

    #[test]
    fn a_test_both_added_on_their_own() {
        let mine = flow(&format!("{OPEN},{CLICK}"));
        let theirs = flow_with(
            r#""tags":["x"],"#,
            OPEN.replace(r#","next":"click""#, "").as_str(),
        );
        assert_eq!(
            merge_conflicts(None, &mine, &theirs),
            [conflict(
                ConflictPart::Block,
                "open",
                None,
                ConflictKind::AddedBoth,
                false
            )]
        );
        let flow = merge_flows(None, &mine, &theirs, &[]).unwrap();
        assert_eq!(flow.tags, ["x"]);
        assert_eq!(flow.blocks.len(), 2);
    }
}
