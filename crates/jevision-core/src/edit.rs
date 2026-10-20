//! Edits that keep a test connected: adding a block on an edge, and removing one so that its
//! neighbours are joined back together.

use std::collections::{HashMap, HashSet};

use crate::decode::{DEFAULT_TASK_TIMEOUT_MS, DEFAULT_WAIT_TIMEOUT_MS};
use crate::model::{
    BODY, Block, BlockKind, ClipboardAction, Compare, Condition, DEFAULT_DATE_FORMAT, ELSE, ElseIf,
    ExpectStatus, FileState, Flow, HttpMethod, Modifiers, MouseButton, NEXT, Reference,
    ScriptLanguage, ScrollDirection, ScrollUnit, THEN, Target, ValueCheck, ValueTest, WaitUntil,
    else_if_index,
};

/// How far a new scroll block scrolls, in lines.
pub const DEFAULT_SCROLL_LINES: u32 = 5;

/// Name of the slot before the first block, used with an empty [`Slot::from`].
pub const START: &str = "start";

/// How long a new pause block waits.
const DEFAULT_PAUSE_MS: u64 = 2000;

/// A place where a block can be added: on the edge `edge` leaving block `from`.
/// An empty `from` with the edge [`START`] is the place before the first block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    pub from: String,
    pub edge: String,
}

impl BlockKind {
    /// A block of type `type_name` with default parameters and no outgoing edges, or `None` for
    /// an unknown type. A loop starts empty: its body points at the loop itself (`id`).
    pub fn new(type_name: &str, id: &str) -> Option<Self> {
        let reference = empty_reference;
        Some(match type_name {
            "openApp" => Self::OpenApp {
                app: None,
                reset_state: false,
                window_size: None,
                window_position: None,
                next: None,
            },
            "openFile" => Self::OpenFile {
                files: Vec::new(),
                app: None,
                new_instance: false,
                close_after: false,
                next: None,
            },
            "click" => Self::Click {
                target: Target::Image(String::new()),
                button: MouseButton::Left,
                double: false,
                modifiers: Modifiers::default(),
                wait_for_change: false,
                next: None,
            },
            "hover" => Self::Hover {
                target: Target::Image(String::new()),
                wait_for_change: false,
                next: None,
            },
            "drag" => Self::Drag {
                from: Target::Image(String::new()),
                to: Target::Image(String::new()),
                button: MouseButton::Left,
                modifiers: Modifiers::default(),
                wait_for_change: false,
                next: None,
            },
            "scroll" => Self::Scroll {
                direction: ScrollDirection::Down,
                amount: DEFAULT_SCROLL_LINES,
                unit: ScrollUnit::Lines,
                at: None,
                wait_for_change: false,
                next: None,
            },
            "type" => Self::Type {
                text: String::new(),
                into: None,
                next: None,
            },
            "pressKeys" => Self::PressKeys {
                keys: String::new(),
                next: None,
            },
            "waitFor" => Self::WaitFor {
                until: WaitUntil::ScreenStable,
                timeout_ms: DEFAULT_WAIT_TIMEOUT_MS,
                next: None,
            },
            "pause" => Self::Pause {
                duration_ms: DEFAULT_PAUSE_MS,
                next: None,
            },
            "check" => Self::Check {
                reference: reference(),
                next: None,
            },
            "if" => Self::If {
                condition: Condition::Screen(reference()),
                then: None,
                else_if: Vec::new(),
                otherwise: None,
            },
            "repeat" => Self::Repeat {
                times: 2,
                body: id.to_owned(),
                next: None,
            },
            "useData" => Self::UseData {
                file: String::new(),
                body: id.to_owned(),
                next: None,
            },
            "runTest" => Self::RunTest {
                test: String::new(),
                inputs: HashMap::new(),
                next: None,
            },
            "script" => Self::Script {
                language: ScriptLanguage::Zsh,
                code: String::new(),
                inputs: Vec::new(),
                outputs: Vec::new(),
                timeout_ms: DEFAULT_TASK_TIMEOUT_MS,
                next: None,
            },
            "command" => Self::Command {
                command: String::new(),
                folder: None,
                exit_code: 0,
                save_output: None,
                timeout_ms: DEFAULT_TASK_TIMEOUT_MS,
                next: None,
            },
            "http" => Self::Http {
                method: HttpMethod::Get,
                url: String::new(),
                headers: Vec::new(),
                body: String::new(),
                expect_status: ExpectStatus::Success,
                save: Vec::new(),
                timeout_ms: DEFAULT_TASK_TIMEOUT_MS,
                next: None,
            },
            "checkFile" => Self::CheckFile {
                path: String::new(),
                until: FileState::Exists,
                timeout_ms: DEFAULT_WAIT_TIMEOUT_MS,
                next: None,
            },
            "database" => Self::Database {
                file: String::new(),
                query: String::new(),
                save_as: None,
                expect: None,
                timeout_ms: DEFAULT_WAIT_TIMEOUT_MS,
                next: None,
            },
            "clipboard" => Self::Clipboard {
                action: ClipboardAction::Put {
                    text: String::new(),
                },
                next: None,
            },
            "dateTime" => Self::DateTime {
                save_as: String::new(),
                format: DEFAULT_DATE_FORMAT.to_owned(),
                shift: None,
                next: None,
            },
            _ => return None,
        })
    }

    /// Target of the edge called `name`, if the block has that edge and it leads somewhere.
    pub fn edge(&self, name: &str) -> Option<&str> {
        self.edges()
            .into_iter()
            .find(|(n, _)| *n == name)
            .map(|(_, t)| t)
    }

    /// Adds a condition to an `if`, checked after all the others. Until it is linked elsewhere,
    /// its way out leads where `else` does. After a value check it checks the same value, so
    /// that each way out can stand for one answer. `false` for other blocks.
    pub fn add_condition(&mut self) -> bool {
        let Self::If {
            condition,
            else_if,
            otherwise,
            ..
        } = self
        else {
            return false;
        };
        let last = else_if.last().map_or(&*condition, |c| &c.condition);
        let condition = match last {
            Condition::Value(v) => Condition::Value(ValueCheck {
                of: v.of.clone(),
                is: ValueTest::Equals,
                to: String::new(),
            }),
            Condition::Screen(_) => Condition::Screen(empty_reference()),
        };
        else_if.push(ElseIf {
            condition,
            then: otherwise.clone(),
        });
        true
    }

    /// Removes condition `n` of an `if`, with its way out; 0 is the first, `condition`. The only
    /// condition stays. `false` when nothing changed.
    pub fn remove_condition(&mut self, n: usize) -> bool {
        self.edit_conditions(|all| {
            if all.len() < 2 || n >= all.len() {
                return false;
            }
            all.remove(n);
            true
        })
    }

    /// Moves condition `from` of an `if` to place `to`, with its way out, so it is checked
    /// earlier or later. `false` when nothing changed.
    pub fn move_condition(&mut self, from: usize, to: usize) -> bool {
        self.edit_conditions(|all| {
            if from == to || from >= all.len() || to >= all.len() {
                return false;
            }
            let moved = all.remove(from);
            all.insert(to, moved);
            true
        })
    }

    /// Runs `edit` on the conditions of an `if` in the order they are checked.
    fn edit_conditions(&mut self, edit: impl FnOnce(&mut Vec<ElseIf>) -> bool) -> bool {
        let Self::If {
            condition,
            then,
            else_if,
            ..
        } = self
        else {
            return false;
        };
        let mut all = vec![ElseIf {
            condition: condition.clone(),
            then: then.clone(),
        }];
        all.append(else_if);
        let changed = edit(&mut all);
        let mut all = all.into_iter();
        let first = all.next().expect("an if keeps one condition");
        *condition = first.condition;
        *then = first.then;
        *else_if = all.collect();
        changed
    }

    pub(crate) fn has_edge(&self, name: &str) -> bool {
        self.edge_names().iter().any(|n| n == name)
    }

    fn next_mut(&mut self) -> Option<&mut Option<String>> {
        match self {
            Self::If { .. } => None,
            Self::OpenApp { next, .. }
            | Self::OpenFile { next, .. }
            | Self::Click { next, .. }
            | Self::Hover { next, .. }
            | Self::Drag { next, .. }
            | Self::Scroll { next, .. }
            | Self::Type { next, .. }
            | Self::PressKeys { next, .. }
            | Self::WaitFor { next, .. }
            | Self::Pause { next, .. }
            | Self::Check { next, .. }
            | Self::Repeat { next, .. }
            | Self::UseData { next, .. }
            | Self::RunTest { next, .. }
            | Self::Script { next, .. }
            | Self::Command { next, .. }
            | Self::Http { next, .. }
            | Self::CheckFile { next, .. }
            | Self::Database { next, .. }
            | Self::Clipboard { next, .. }
            | Self::DateTime { next, .. } => Some(next),
        }
    }
}

fn empty_reference() -> Reference {
    Reference {
        image: String::new(),
        compare: Compare::default(),
        area: None,
        ignore: Vec::new(),
        hide: Vec::new(),
    }
}

/// Points edge `name` of `block` at `target`. A loop body without a target is left empty.
pub(crate) fn set_edge(block: &mut Block, name: &str, target: Option<String>) {
    let own = &block.id;
    match (&mut block.kind, name) {
        (BlockKind::If { then, .. }, THEN) => *then = target,
        (BlockKind::If { otherwise, .. }, ELSE) => *otherwise = target,
        (BlockKind::If { else_if, .. }, name) if else_if_index(name).is_some() => {
            if let Some(case) = else_if_index(name).and_then(|n| else_if.get_mut(n)) {
                case.then = target;
            }
        }
        (BlockKind::Repeat { body, .. } | BlockKind::UseData { body, .. }, BODY) => {
            *body = target.unwrap_or_else(|| own.clone());
        }
        (kind, NEXT) => {
            if let Some(next) = kind.next_mut() {
                *next = target;
            }
        }
        _ => {}
    }
}

/// The edge a block added after a block of kind `kind` goes on: `then` of an `if`, `next` of
/// anything else.
fn edge_after(kind: &BlockKind) -> &'static str {
    if matches!(kind, BlockKind::If { .. }) {
        THEN
    } else {
        NEXT
    }
}

impl Flow {
    fn position(&self, id: &str) -> Option<usize> {
        self.blocks.iter().position(|b| b.id == id)
    }

    /// Where each id is in `blocks`; the first block of a duplicate id wins, as in [`Self::position`].
    fn index(&self) -> HashMap<&str, usize> {
        let ids = self.blocks.iter().enumerate().rev();
        ids.map(|(i, b)| (b.id.as_str(), i)).collect()
    }

    /// Ids of the blocks reachable from `start` over any edge.
    fn reachable(&self, start: &str) -> HashSet<String> {
        let index = self.index();
        let mut seen = HashSet::new();
        let mut stack = vec![start];
        while let Some(id) = stack.pop() {
            let Some(&i) = index.get(id) else {
                continue;
            };
            if seen.insert(id.to_owned()) {
                stack.extend(self.blocks[i].kind.edges().into_iter().map(|(_, t)| t));
            }
        }
        seen
    }

    /// `base`, or `base2`, `base3`… — the first one no block uses yet.
    pub(crate) fn unique_id(&self, base: &str) -> String {
        let taken: HashSet<&str> = self.blocks.iter().map(|b| b.id.as_str()).collect();
        std::iter::once(base.to_owned())
            .chain((2..).map(|n| format!("{base}{n}")))
            .find(|id| !taken.contains(id.as_str()))
            .expect("an unused id always exists")
    }

    /// The slot after the last step of the main chain: from `start` along `next` (or `then`).
    fn end_of_chain(&self) -> Slot {
        let mut seen = HashSet::new();
        let mut slot = Slot {
            from: String::new(),
            edge: START.to_owned(),
        };
        let index = self.index();
        let mut cur = self.start.as_str();
        while let Some(&i) = index.get(cur) {
            if !seen.insert(cur) {
                break;
            }
            let block = &self.blocks[i];
            let edge = edge_after(&block.kind);
            slot = Slot {
                from: block.id.clone(),
                edge: edge.to_owned(),
            };
            match block.kind.edge(edge) {
                Some(t) => cur = t,
                None => break,
            }
        }
        slot
    }

    /// Adds a block of type `type_name` on `slot`, between the slot's block and whatever came
    /// next; without a slot, after the last step of the main chain. A new `if` continues with
    /// that same block on all branches. Returns the new block's id, or `None` (and changes
    /// nothing) for an unknown type or slot.
    pub fn insert_block(&mut self, type_name: &str, slot: Option<&Slot>) -> Option<String> {
        let id = self.unique_id(type_name);
        let kind = BlockKind::new(type_name, &id)?;
        self.insert_kind(id, kind, slot)
    }

    /// Adds `kinds` in order as one chain on `slot` (after the main chain without one): each
    /// block leads to the following one and the last to whatever came after the slot; a last
    /// `if` goes there on all branches. A loop with an empty `body` starts empty. Ids come
    /// from the block types. Returns the new ids, or `None` (and changes nothing) when there is
    /// nothing to add, a block before the last has no `next` edge to chain on, or the slot is
    /// unknown.
    pub fn insert_blocks(
        &mut self,
        mut kinds: Vec<BlockKind>,
        slot: Option<&Slot>,
    ) -> Option<Vec<String>> {
        let (_, chained) = kinds.split_last_mut()?;
        if chained.iter_mut().any(|k| k.next_mut().is_none()) {
            return None;
        }
        let mut edited = self.clone();
        let mut ids = Vec::with_capacity(kinds.len());
        let mut slot = slot.cloned();
        for mut kind in kinds {
            let id = edited.unique_id(kind.type_name());
            if let BlockKind::Repeat { body, .. } | BlockKind::UseData { body, .. } = &mut kind {
                if body.is_empty() {
                    body.clone_from(&id);
                }
            }
            let id = edited.insert_kind(id, kind, slot.as_ref())?;
            slot = Some(Slot {
                from: id.clone(),
                edge: NEXT.to_owned(),
            });
            ids.push(id);
        }
        *self = edited;
        Some(ids)
    }

    /// The slot a block added after block `id` goes on: `then` of an `if`, `next` of any other
    /// block, so after a loop rather than inside it. `None` for an unknown block.
    pub fn slot_after(&self, id: &str) -> Option<Slot> {
        let block = &self.blocks[self.position(id)?];
        Some(Slot {
            from: block.id.clone(),
            edge: edge_after(&block.kind).to_owned(),
        })
    }

    /// Adds a condition where the user is working: one more condition on `selected` when it is
    /// an `if`, otherwise a new `if` after it (after the main chain without a selection).
    /// Returns the block to select, or `None` (and changes nothing) for an unknown block.
    pub fn add_condition_at(&mut self, selected: Option<&str>) -> Option<String> {
        let Some(id) = selected else {
            return self.insert_block("if", None);
        };
        let i = self.position(id)?;
        if self.blocks[i].kind.add_condition() {
            return Some(id.to_owned());
        }
        let slot = self.slot_after(id)?;
        self.insert_block("if", Some(&slot))
    }

    /// Removes every link into and out of block `id`, so it stands alone: nothing leads to it
    /// and it leads nowhere. A loop keeps the steps inside it, and the test still starts where
    /// it did. `false` when the block had no such link.
    pub fn disconnect(&mut self, id: &str) -> bool {
        let links: Vec<(String, String)> = self
            .blocks
            .iter()
            .flat_map(|b| {
                b.kind
                    .edges()
                    .into_iter()
                    .filter(|(edge, to)| {
                        let inside = b.id == id && *edge == BODY;
                        let empty_loop = *edge == BODY && *to == b.id;
                        (b.id == id || *to == id) && !inside && !empty_loop
                    })
                    .map(|(edge, _)| (b.id.clone(), edge.into_owned()))
            })
            .collect();
        let mut changed = false;
        for (from, edge) in links {
            changed |= self.connect(&from, &edge, None);
        }
        changed
    }

    fn insert_kind(
        &mut self,
        id: String,
        mut kind: BlockKind,
        slot: Option<&Slot>,
    ) -> Option<String> {
        let slot = slot.cloned().unwrap_or_else(|| self.end_of_chain());
        let after = if slot.from.is_empty() {
            if slot.edge != START {
                return None;
            }
            let old = self.position(&self.start).map(|_| self.start.clone());
            self.start = id.clone();
            old
        } else {
            let i = self.position(&slot.from)?;
            let block = &mut self.blocks[i];
            if !block.kind.has_edge(&slot.edge) {
                return None;
            }
            let old = block.kind.edge(&slot.edge).map(str::to_owned);
            set_edge(block, &slot.edge, Some(id.clone()));
            old
        };
        match &mut kind {
            BlockKind::If {
                then,
                else_if,
                otherwise,
                ..
            } => {
                then.clone_from(&after);
                for case in else_if {
                    case.then.clone_from(&after);
                }
                *otherwise = after;
            }
            other => {
                if let Some(next) = other.next_mut() {
                    *next = after;
                }
            }
        }
        self.canvas.positions.remove(&id);
        self.blocks.push(Block {
            id: id.clone(),
            kind,
        });
        Some(id)
    }

    /// Removes block `id` and joins its neighbours: whatever led to it now leads to what came
    /// after it. A loop leaves its body in its place; an `if` keeps its first branch that leads
    /// anywhere, and the blocks that only its other branches reached go too. Returns the block to select next, or
    /// `None` when nothing is left to select or there is no such block.
    ///
    /// Notes about the removed blocks stay on the canvas as notes about the test, see
    /// [`Flow::remove_block_with_notes`] to remove them too.
    pub fn remove_block(&mut self, id: &str) -> Option<String> {
        self.remove_block_and(id, false)
    }

    /// Like [`Flow::remove_block`], but the notes about the removed blocks go with them.
    pub fn remove_block_with_notes(&mut self, id: &str) -> Option<String> {
        self.remove_block_and(id, true)
    }

    pub(crate) fn remove_block_and(&mut self, id: &str, with_notes: bool) -> Option<String> {
        let i = self.position(id)?;
        let present: HashSet<String> = self.blocks.iter().map(|b| b.id.clone()).collect();
        let before = self.reachable(&self.start);
        let removed = self.blocks.remove(i);
        // `outer` replaces the block for edges from outside, `inner` for a loop body's way back.
        let (outer, inner) = match &removed.kind {
            BlockKind::If {
                then,
                else_if,
                otherwise,
                ..
            } => (
                then.clone()
                    .or_else(|| else_if.iter().find_map(|c| c.then.clone()))
                    .or_else(|| otherwise.clone()),
                None,
            ),
            BlockKind::Repeat { body, next, .. } | BlockKind::UseData { body, next, .. }
                if body != id =>
            {
                (Some(body.clone()), next.clone())
            }
            kind => (kind.edge(NEXT).map(str::to_owned), None),
        };
        let inside = match &removed.kind {
            BlockKind::Repeat { body, .. } | BlockKind::UseData { body, .. } if body != id => {
                self.reachable(body)
            }
            _ => HashSet::new(),
        };

        let mut predecessor = None;
        for block in &mut self.blocks {
            let names: Vec<String> = block
                .kind
                .edges()
                .into_iter()
                .filter(|(_, t)| *t == id)
                .map(|(name, _)| name.into_owned())
                .collect();
            for name in names {
                let target = if inside.contains(&block.id) {
                    inner.clone()
                } else {
                    outer.clone()
                };
                // Only a loop's body may lead back to the block itself.
                let target = target.filter(|t| *t != block.id || name == BODY);
                set_edge(block, &name, target);
                predecessor.get_or_insert_with(|| block.id.clone());
            }
        }
        if self.start == id {
            self.start = outer.clone().unwrap_or_default();
        }

        let after = self.reachable(&self.start);
        self.blocks
            .retain(|b| !before.contains(&b.id) || after.contains(&b.id));
        let kept: HashSet<&str> = self.blocks.iter().map(|b| b.id.as_str()).collect();
        self.canvas
            .positions
            .retain(|id, _| kept.contains(id.as_str()));
        let gone: HashSet<String> = present
            .into_iter()
            .filter(|id| !kept.contains(id.as_str()))
            .collect();
        self.let_go_of_notes(&gone, with_notes);
        [outer, predecessor]
            .into_iter()
            .flatten()
            .find(|s| self.position(s).is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    fn flow(blocks: &str, start: &str) -> Flow {
        parse(&format!(
            r#"{{"format": 1, "name": "T", "app": "com.example.App", "start": "{start}", "blocks": {blocks}}}"#
        ))
        .unwrap()
    }

    fn next_of(f: &Flow, id: &str) -> Option<String> {
        let i = f.position(id).unwrap();
        f.blocks[i].kind.edge(NEXT).map(str::to_owned)
    }

    fn slot(from: &str, edge: &str) -> Slot {
        Slot {
            from: from.into(),
            edge: edge.into(),
        }
    }

    const CHAIN: &str = r#"{"a": {"type": "pressKeys", "keys": "a", "next": "b"},
                            "b": {"type": "pressKeys", "keys": "b"}}"#;

    #[test]
    fn inserts_between_a_block_and_its_next() {
        let mut f = flow(CHAIN, "a");
        let id = f.insert_block("pressKeys", Some(&slot("a", NEXT))).unwrap();
        assert_eq!(id, "pressKeys");
        assert_eq!(next_of(&f, "a").as_deref(), Some("pressKeys"));
        assert_eq!(next_of(&f, "pressKeys").as_deref(), Some("b"));
        let again = f.insert_block("pressKeys", Some(&slot("a", NEXT))).unwrap();
        assert_eq!(again, "pressKeys2");
    }

    #[test]
    fn inserts_at_the_end_and_before_the_start() {
        let mut f = flow(CHAIN, "a");
        let end = f.insert_block("click", None).unwrap();
        assert_eq!(next_of(&f, "b"), Some(end.clone()));
        assert_eq!(next_of(&f, &end), None);
        let first = f.insert_block("openApp", Some(&slot("", START))).unwrap();
        assert_eq!(f.start, first);
        assert_eq!(next_of(&f, &first).as_deref(), Some("a"));

        let mut empty = flow("{}", "");
        let only = empty.insert_block("openApp", None).unwrap();
        assert_eq!(empty.start, only);
        assert_eq!(empty.blocks.len(), 1);
    }

    #[test]
    fn a_new_if_continues_on_every_branch_and_removing_it_keeps_the_first_that_leads_on() {
        let mut f = flow(CHAIN, "a");
        let mut branch = BlockKind::new("if", "if").unwrap();
        assert!(branch.add_condition());
        let ids = f
            .insert_blocks(vec![branch], Some(&slot("a", NEXT)))
            .unwrap();
        let kind = &f.blocks[f.position(&ids[0]).unwrap()].kind;
        for edge in [THEN, "elseIf1", ELSE] {
            assert_eq!(kind.edge(edge), Some("b"), "{edge}");
        }

        let mut f = flow(
            r#"{"a": {"type": "if", "reference": {"image": "a.png"},
                      "elseIf": [{"reference": {"image": "b.png"}, "then": "two"}], "else": "three"},
                "two": {"type": "pressKeys", "keys": "b"},
                "three": {"type": "pressKeys", "keys": "c"}}"#,
            "a",
        );
        assert!(
            f.insert_block("click", Some(&slot("a", "elseIf1")))
                .is_some()
        );
        assert_eq!(f.blocks[0].kind.edge("elseIf1"), Some("click"));
        assert_eq!(f.remove_block("a").as_deref(), Some("click"));
        assert_eq!(f.start, "click");
        assert!(
            f.position("three").is_none(),
            "only the else branch reached it"
        );
    }

    #[test]
    fn conditions_are_added_removed_and_reordered_with_their_ways_out() {
        let f = flow(
            r#"{"a": {"type": "if", "reference": {"image": "1.png"}, "then": "x", "else": "z"},
                "x": {"type": "pressKeys", "keys": "x"},
                "z": {"type": "pressKeys", "keys": "z"}}"#,
            "a",
        );
        let mut kind = f.blocks[0].kind.clone();
        let images = |k: &BlockKind| match k {
            BlockKind::If {
                condition, else_if, ..
            } => std::iter::once(condition)
                .chain(else_if.iter().map(|c| &c.condition))
                .map(|c| c.reference().unwrap().image.clone())
                .collect::<Vec<_>>(),
            _ => unreachable!(),
        };
        assert!(kind.add_condition());
        assert_eq!(
            kind.edge("elseIf1"),
            Some("z"),
            "a new condition leads where else does"
        );
        assert!(kind.move_condition(1, 0));
        assert_eq!(images(&kind), ["", "1.png"]);
        assert_eq!(
            (kind.edge(THEN), kind.edge("elseIf1")),
            (Some("z"), Some("x"))
        );
        assert!(!kind.move_condition(0, 2));
        assert!(kind.remove_condition(0));
        assert_eq!(images(&kind), ["1.png"]);
        assert_eq!(kind.edge(THEN), Some("x"));
        assert!(!kind.remove_condition(0), "the only condition stays");
        let mut keys = BlockKind::new("pressKeys", "k").unwrap();
        assert!(!keys.add_condition());
    }

    #[test]
    fn inserts_a_chain_as_one_edit() {
        let mut f = flow(CHAIN, "a");
        let keys = |k: &str| BlockKind::PressKeys {
            keys: k.into(),
            next: None,
        };
        let ids = f
            .insert_blocks(vec![keys("x"), keys("y")], Some(&slot("a", NEXT)))
            .unwrap();
        assert_eq!(ids, ["pressKeys", "pressKeys2"]);
        assert_eq!(next_of(&f, "a").as_deref(), Some("pressKeys"));
        assert_eq!(next_of(&f, "pressKeys").as_deref(), Some("pressKeys2"));
        assert_eq!(next_of(&f, "pressKeys2").as_deref(), Some("b"));

        let mut empty = flow("{}", "");
        let ids = empty.insert_blocks(vec![keys("x")], None).unwrap();
        assert_eq!(empty.start, ids[0]);

        let original = f.clone();
        assert!(f.insert_blocks(Vec::new(), None).is_none());
        let branch = BlockKind::new("if", "if").unwrap();
        assert!(f.insert_blocks(vec![branch, keys("z")], None).is_none());
        assert!(
            f.insert_blocks(vec![keys("z")], Some(&slot("nope", NEXT)))
                .is_none()
        );
        assert_eq!(f, original);

        let loop_ = BlockKind::Repeat {
            times: 3,
            body: String::new(),
            next: None,
        };
        let branch = BlockKind::new("if", "if").unwrap();
        let ids = f
            .insert_blocks(vec![loop_, branch], Some(&slot("a", NEXT)))
            .unwrap();
        assert_eq!(ids, ["repeat", "if"]);
        let edges = |id: &str| {
            let kind = &f.blocks.iter().find(|b| b.id == id).unwrap().kind;
            let edges: Vec<(String, String)> = kind
                .edges()
                .into_iter()
                .map(|(n, t)| (n.into_owned(), t.to_owned()))
                .collect();
            edges
        };
        let want = |e: &[(&str, &str)]| -> Vec<(String, String)> {
            e.iter()
                .map(|(n, t)| ((*n).to_owned(), (*t).to_owned()))
                .collect()
        };
        assert_eq!(edges("repeat"), want(&[(BODY, "repeat"), (NEXT, "if")]));
        assert_eq!(
            edges("if"),
            want(&[(THEN, "pressKeys"), (ELSE, "pressKeys")])
        );
    }

    #[test]
    fn unknown_types_and_slots_change_nothing() {
        let original = flow(CHAIN, "a");
        let mut f = original.clone();
        assert_eq!(f.insert_block("teleport", None), None);
        assert_eq!(f.insert_block("click", Some(&slot("a", THEN))), None);
        assert_eq!(f.insert_block("click", Some(&slot("zz", NEXT))), None);
        assert_eq!(f.insert_block("click", Some(&slot("", NEXT))), None);
        assert_eq!(f, original);
    }

    #[test]
    fn a_new_if_continues_on_both_branches_and_a_new_loop_is_empty() {
        let mut f = flow(CHAIN, "a");
        let branch = f.insert_block("if", Some(&slot("a", NEXT))).unwrap();
        let i = f.position(&branch).unwrap();
        assert_eq!(f.blocks[i].kind.edge(THEN), Some("b"));
        assert_eq!(f.blocks[i].kind.edge(ELSE), Some("b"));

        let repeat = f.insert_block("repeat", Some(&slot("b", NEXT))).unwrap();
        let i = f.position(&repeat).unwrap();
        assert_eq!(f.blocks[i].kind.edge(BODY), Some(repeat.as_str()));
        let empty = crate::validate(&f)
            .into_iter()
            .filter(|i| i.code == "flow.loop.emptyBody")
            .count();
        assert_eq!(empty, 1);

        // The first block of a body leads back to the loop.
        let inner = f.insert_block("click", Some(&slot(&repeat, BODY))).unwrap();
        assert_eq!(next_of(&f, &inner), Some(repeat.clone()));
        let i = f.position(&repeat).unwrap();
        assert_eq!(f.blocks[i].kind.edge(BODY), Some(inner.as_str()));
    }

    #[test]
    fn removing_joins_the_neighbours() {
        let mut f = flow(
            r#"{"a": {"type": "pressKeys", "keys": "a", "next": "b"},
                "b": {"type": "pressKeys", "keys": "b", "next": "c"},
                "c": {"type": "pressKeys", "keys": "c"}}"#,
            "a",
        );
        assert_eq!(f.remove_block("b").as_deref(), Some("c"));
        assert_eq!(next_of(&f, "a").as_deref(), Some("c"));
        assert_eq!(f.remove_block("a").as_deref(), Some("c"));
        assert_eq!(f.start, "c");
        assert_eq!(f.remove_block("c"), None);
        assert!(f.blocks.is_empty());
        assert_eq!(f.remove_block("c"), None);
    }

    #[test]
    fn removing_a_loop_keeps_its_body_in_place() {
        let mut f = flow(
            r#"{"a": {"type": "pressKeys", "keys": "a", "next": "loop"},
                "loop": {"type": "repeat", "times": 2, "body": "x", "next": "z"},
                "x": {"type": "pressKeys", "keys": "x", "next": "y"},
                "y": {"type": "pressKeys", "keys": "y", "next": "loop"},
                "z": {"type": "pressKeys", "keys": "z"}}"#,
            "a",
        );
        assert_eq!(f.remove_block("loop").as_deref(), Some("x"));
        assert_eq!(next_of(&f, "a").as_deref(), Some("x"));
        assert_eq!(next_of(&f, "y").as_deref(), Some("z"));
        assert!(crate::validate(&f).iter().all(|i| i.code == "flow.noCheck"));
    }

    #[test]
    fn removing_the_last_block_of_a_body_empties_the_loop() {
        let mut f = flow(
            r#"{"loop": {"type": "repeat", "times": 2, "body": "x"},
                "x": {"type": "pressKeys", "keys": "x", "next": "loop"}}"#,
            "loop",
        );
        assert_eq!(f.remove_block("x").as_deref(), Some("loop"));
        assert_eq!(f.blocks[0].kind.edge(BODY), Some("loop"));
    }

    #[test]
    fn removing_an_if_keeps_the_then_branch() {
        let mut f = flow(
            r#"{"if": {"type": "if", "reference": {"image": "a.png"}, "then": "yes", "else": "no"},
                "yes": {"type": "pressKeys", "keys": "y", "next": "end"},
                "no": {"type": "pressKeys", "keys": "n", "next": "end"},
                "end": {"type": "pressKeys", "keys": "e"}}"#,
            "if",
        );
        assert_eq!(f.remove_block("if").as_deref(), Some("yes"));
        let mut ids: Vec<&str> = f.blocks.iter().map(|b| b.id.as_str()).collect();
        ids.sort_unstable();
        assert_eq!(ids, ["end", "yes"]);
        assert_eq!(f.start, "yes");
    }

    #[test]
    fn a_block_added_after_goes_on_then_of_an_if_and_after_a_loop() {
        let f = flow(
            r#"{"if": {"type": "if", "reference": {"image": "a.png"}, "then": "loop"},
                "loop": {"type": "repeat", "times": 2, "body": "loop"}}"#,
            "if",
        );
        assert_eq!(f.slot_after("if"), Some(slot("if", THEN)));
        assert_eq!(f.slot_after("loop"), Some(slot("loop", NEXT)));
        assert_eq!(f.slot_after("nowhere"), None);
    }

    #[test]
    fn adding_a_condition_grows_a_selected_if_or_adds_one_after_the_selection() {
        let mut f = flow(CHAIN, "a");
        let id = f.add_condition_at(Some("a")).unwrap();
        assert_eq!(id, "if");
        assert_eq!(next_of(&f, "a").as_deref(), Some("if"));
        let i = f.position("if").unwrap();
        assert_eq!(f.blocks[i].kind.edge(THEN), Some("b"));

        assert_eq!(f.add_condition_at(Some("if")).as_deref(), Some("if"));
        let BlockKind::If { else_if, .. } = &f.blocks[f.position("if").unwrap()].kind else {
            panic!("not an if");
        };
        assert_eq!(else_if.len(), 1);

        assert_eq!(f.add_condition_at(None).as_deref(), Some("if2"));
        assert_eq!(f.add_condition_at(Some("nowhere")), None);
    }

    #[test]
    fn disconnecting_leaves_the_block_alone_and_its_loop_whole() {
        let mut f = flow(
            r#"{"a": {"type": "pressKeys", "keys": "a", "next": "loop"},
                "loop": {"type": "repeat", "times": 2, "body": "in", "next": "z"},
                "in": {"type": "pressKeys", "keys": "i"},
                "z": {"type": "pressKeys", "keys": "z"}}"#,
            "a",
        );
        assert!(f.disconnect("loop"));
        assert_eq!(next_of(&f, "a"), None);
        assert_eq!(next_of(&f, "loop"), None);
        let i = f.position("loop").unwrap();
        assert_eq!(f.blocks[i].kind.edge(BODY), Some("in"));
        assert_eq!(f.start, "a");
        assert!(!f.disconnect("loop"));

        // The first step inside a loop: the loop is left empty.
        let mut f = flow(
            r#"{"loop": {"type": "repeat", "times": 2, "body": "in"},
                "in": {"type": "pressKeys", "keys": "i"}}"#,
            "loop",
        );
        assert!(f.disconnect("in"));
        let i = f.position("loop").unwrap();
        assert_eq!(f.blocks[i].kind.edge(BODY), Some("loop"));
    }

    #[test]
    fn every_new_block_survives_a_save_and_a_load() {
        let mut f = flow(r#"{}"#, "");
        for spec in crate::catalog() {
            assert!(
                f.insert_block(&spec.type_name, None).is_some(),
                "{}",
                spec.type_name
            );
        }
        let saved = crate::to_canonical_json(&f);
        assert_eq!(crate::to_canonical_json(&parse(&saved).unwrap()), saved);
    }
}
