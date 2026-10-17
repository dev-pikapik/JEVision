//! Edits that keep a test connected: adding a block on an edge, and removing one so that its
//! neighbours are joined back together.

use std::collections::HashSet;

use crate::decode::DEFAULT_WAIT_TIMEOUT_MS;
use crate::model::{
    BODY, Block, BlockKind, Compare, ELSE, Flow, MouseButton, NEXT, Reference, THEN, Target,
    WaitUntil,
};

/// Name of the slot before the first block, used with an empty [`Slot::from`].
pub const START: &str = "start";

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
        let reference = || Reference {
            image: String::new(),
            compare: Compare::default(),
            area: None,
            ignore: Vec::new(),
            hide: Vec::new(),
        };
        Some(match type_name {
            "openApp" => Self::OpenApp {
                app: None,
                next: None,
            },
            "openFile" => Self::OpenFile {
                files: Vec::new(),
                app: None,
                new_instance: false,
                next: None,
            },
            "click" => Self::Click {
                target: Target::Image(String::new()),
                button: MouseButton::Left,
                double: false,
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
            "check" => Self::Check {
                reference: reference(),
                next: None,
            },
            "if" => Self::If {
                reference: reference(),
                then: None,
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

    fn has_edge(&self, name: &str) -> bool {
        match self {
            Self::If { .. } => name == THEN || name == ELSE,
            Self::Repeat { .. } | Self::UseData { .. } => name == BODY || name == NEXT,
            _ => name == NEXT,
        }
    }

    fn next_mut(&mut self) -> Option<&mut Option<String>> {
        match self {
            Self::If { .. } => None,
            Self::OpenApp { next, .. }
            | Self::OpenFile { next, .. }
            | Self::Click { next, .. }
            | Self::Type { next, .. }
            | Self::PressKeys { next, .. }
            | Self::WaitFor { next, .. }
            | Self::Check { next, .. }
            | Self::Repeat { next, .. }
            | Self::UseData { next, .. }
            | Self::RunTest { next, .. } => Some(next),
        }
    }
}

/// Points edge `name` of `block` at `target`. A loop body without a target is left empty.
fn set_edge(block: &mut Block, name: &str, target: Option<String>) {
    let own = &block.id;
    match (&mut block.kind, name) {
        (BlockKind::If { then, .. }, THEN) => *then = target,
        (BlockKind::If { otherwise, .. }, ELSE) => *otherwise = target,
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

impl Flow {
    fn position(&self, id: &str) -> Option<usize> {
        self.blocks.iter().position(|b| b.id == id)
    }

    /// Ids of the blocks reachable from `start` over any edge.
    fn reachable(&self) -> HashSet<String> {
        let mut seen = HashSet::new();
        let mut stack = vec![self.start.clone()];
        while let Some(id) = stack.pop() {
            let Some(i) = self.position(&id) else {
                continue;
            };
            if seen.insert(id) {
                let edges = self.blocks[i].kind.edges();
                stack.extend(edges.into_iter().map(|(_, t)| t.to_owned()));
            }
        }
        seen
    }

    /// `base`, or `base2`, `base3`… — the first one no block uses yet.
    fn unique_id(&self, base: &str) -> String {
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
        let mut cur = self.start.as_str();
        while let Some(i) = self.position(cur) {
            if !seen.insert(cur) {
                break;
            }
            let block = &self.blocks[i];
            let edge = if matches!(block.kind, BlockKind::If { .. }) {
                THEN
            } else {
                NEXT
            };
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
    /// that same block on both branches. Returns the new block's id, or `None` (and changes
    /// nothing) for an unknown type or slot.
    pub fn insert_block(&mut self, type_name: &str, slot: Option<&Slot>) -> Option<String> {
        let id = self.unique_id(type_name);
        let mut kind = BlockKind::new(type_name, &id)?;
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
                then, otherwise, ..
            } => {
                then.clone_from(&after);
                *otherwise = after;
            }
            other => {
                if let Some(next) = other.next_mut() {
                    *next = after;
                }
            }
        }
        self.blocks.push(Block {
            id: id.clone(),
            kind,
        });
        Some(id)
    }

    /// Removes block `id` and joins its neighbours: whatever led to it now leads to what came
    /// after it. A loop leaves its body in its place; an `if` keeps its `then` branch, and the
    /// blocks that only its `else` branch reached go too. Returns the block to select next, or
    /// `None` when nothing is left to select or there is no such block.
    pub fn remove_block(&mut self, id: &str) -> Option<String> {
        let i = self.position(id)?;
        let before = self.reachable();
        let removed = self.blocks.remove(i);
        // `outer` replaces the block for edges from outside, `inner` for a loop body's way back.
        let (outer, inner) = match &removed.kind {
            BlockKind::If {
                then, otherwise, ..
            } => (then.clone().or_else(|| otherwise.clone()), None),
            BlockKind::Repeat { body, next, .. } | BlockKind::UseData { body, next, .. }
                if body != id =>
            {
                (Some(body.clone()), next.clone())
            }
            kind => (kind.edge(NEXT).map(str::to_owned), None),
        };
        let inside = match &removed.kind {
            BlockKind::Repeat { body, .. } | BlockKind::UseData { body, .. } if body != id => {
                let mut part = self.clone();
                part.start = body.clone();
                part.reachable()
            }
            _ => HashSet::new(),
        };

        let mut predecessor = None;
        for block in &mut self.blocks {
            let names: Vec<&'static str> = block
                .kind
                .edges()
                .into_iter()
                .filter(|(_, t)| *t == id)
                .map(|(name, _)| name)
                .collect();
            for name in names {
                let target = if inside.contains(&block.id) {
                    inner.clone()
                } else {
                    outer.clone()
                };
                // Only a loop's body may lead back to the block itself.
                let target = target.filter(|t| *t != block.id || name == BODY);
                set_edge(block, name, target);
                predecessor.get_or_insert_with(|| block.id.clone());
            }
        }
        if self.start == id {
            self.start = outer.clone().unwrap_or_default();
        }

        let after = self.reachable();
        self.blocks
            .retain(|b| !before.contains(&b.id) || after.contains(&b.id));
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
        assert!(crate::validate(&f).is_empty());
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
