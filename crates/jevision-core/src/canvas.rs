//! Edits made by hand on the canvas: moving and resizing, connecting blocks, sections and
//! notes, copy and paste.

use std::collections::{HashMap, HashSet};

use crate::decode::parse;
use crate::edit::set_edge;
use crate::layout::{GRID, layout, rows};
use crate::model::{
    AttachmentKind, BODY, Block, BlockKind, CanvasSection, Flow, NEXT, Note, NoteAnchor, Point,
    Rect, THEN,
};
use crate::to_canonical_json;
use crate::validate::validate;

/// Something drawn on the canvas that can be selected.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CanvasItem {
    Block { id: String },
    Section { id: String },
    Note { id: String },
}

/// How far pasted and duplicated items land from their originals.
const PASTE_OFFSET: f64 = 2.0 * GRID;
/// Id of the frame around a row of blocks, as [`unique`] numbers them: `row`, `row2`, `row3`…
const ROW_ID: &str = "row";
/// The smallest section or note.
const MIN_SIZE: f64 = 4.0 * GRID;

fn is_row_frame(id: &str) -> bool {
    id.strip_prefix(ROW_ID)
        .is_some_and(|n| n.bytes().all(|b| b.is_ascii_digit()))
}

fn snap(v: f64) -> f64 {
    (v / GRID).round() * GRID
}

/// `v` on the grid, or as it is when `snapping` is off.
fn snapped(v: f64, snapping: bool) -> f64 {
    if snapping { snap(v) } else { v }
}

fn contains(outer: &Rect, inner: &Rect) -> bool {
    inner.x >= outer.x
        && inner.y >= outer.y
        && inner.x + inner.width <= outer.x + outer.width
        && inner.y + inner.height <= outer.y + outer.height
}

impl Flow {
    /// Gives every block a position, where the automatic layout draws it, so that moving one
    /// block leaves the others in place.
    pub(crate) fn pin_blocks(&mut self) {
        if !self.canvas.positions.is_empty() {
            return;
        }
        for b in layout(self).blocks {
            let at = Point {
                x: snap(b.rect.x),
                y: snap(b.rect.y),
            };
            self.canvas.positions.insert(b.id, at);
        }
    }

    fn origin(&self, item: &CanvasItem) -> Option<Point> {
        let rect = match item {
            CanvasItem::Block { id } => return self.canvas.positions.get(id).copied(),
            CanvasItem::Section { id } => self.canvas.sections.iter().find(|s| &s.id == id)?.rect,
            CanvasItem::Note { id } => self.canvas.notes.iter().find(|n| &n.id == id)?.rect,
        };
        Some(Point {
            x: rect.x,
            y: rect.y,
        })
    }

    /// `items` and, after them, everything that lies wholly inside a section among them, and
    /// the notes about the blocks among them.
    fn with_contents(&self, items: &[CanvasItem]) -> Vec<CanvasItem> {
        let mut all = self.with_inside(items);
        let moved: HashSet<String> = all
            .iter()
            .filter_map(|i| match i {
                CanvasItem::Block { id } => Some(id.clone()),
                _ => None,
            })
            .collect();
        for note in &self.canvas.notes {
            if matches!(&note.anchor, NoteAnchor::Block { id } if moved.contains(id)) {
                let item = CanvasItem::Note {
                    id: note.id.clone(),
                };
                if !all.contains(&item) {
                    all.push(item);
                }
            }
        }
        all
    }

    /// `items` and, after them, everything that lies wholly inside a section among them.
    fn with_inside(&self, items: &[CanvasItem]) -> Vec<CanvasItem> {
        let frames: Vec<Rect> = items
            .iter()
            .filter_map(|i| match i {
                CanvasItem::Section { id } => self
                    .canvas
                    .sections
                    .iter()
                    .find(|s| &s.id == id)
                    .map(|s| s.rect),
                _ => None,
            })
            .collect();
        let mut all = items.to_vec();
        if frames.is_empty() {
            return all;
        }
        let inside = |r: &Rect| frames.iter().any(|f| contains(f, r));
        let blocks = layout(self)
            .blocks
            .into_iter()
            .filter(|b| inside(&b.rect))
            .map(|b| CanvasItem::Block { id: b.id });
        let sections = self.canvas.sections.iter().filter(|s| inside(&s.rect));
        let sections = sections.map(|s| CanvasItem::Section { id: s.id.clone() });
        let notes = self.canvas.notes.iter().filter(|n| inside(&n.rect));
        let notes = notes.map(|n| CanvasItem::Note { id: n.id.clone() });
        for item in blocks.chain(sections).chain(notes) {
            if !all.contains(&item) {
                all.push(item);
            }
        }
        all
    }

    /// Moves `items` by `dx`, `dy` canvas points, so that the first of them lands on the grid
    /// (with `snapping`) and none goes past the top or left edge. A section takes along
    /// everything wholly inside it. Moving a block places every block by hand from then on.
    /// Returns whether anything moved.
    pub fn move_items(&mut self, items: &[CanvasItem], dx: f64, dy: f64, snapping: bool) -> bool {
        let items = &self.with_contents(items);
        if items.iter().any(|i| matches!(i, CanvasItem::Block { .. })) {
            self.pin_blocks();
        }
        let origins: Vec<Point> = items.iter().filter_map(|i| self.origin(i)).collect();
        let Some(first) = origins.first() else {
            return false;
        };
        let mut dx = snapped(first.x + dx, snapping) - first.x;
        let mut dy = snapped(first.y + dy, snapping) - first.y;
        let left = origins
            .iter()
            .map(|p| p.x + dx)
            .fold(f64::INFINITY, f64::min);
        let top = origins
            .iter()
            .map(|p| p.y + dy)
            .fold(f64::INFINITY, f64::min);
        if left < 0.0 {
            dx -= left;
        }
        if top < 0.0 {
            dy -= top;
        }
        if dx == 0.0 && dy == 0.0 {
            return false;
        }
        for item in items {
            match item {
                CanvasItem::Block { id } => {
                    if let Some(p) = self.canvas.positions.get_mut(id) {
                        p.x += dx;
                        p.y += dy;
                    }
                }
                CanvasItem::Section { id } => {
                    if let Some(s) = self.canvas.sections.iter_mut().find(|s| &s.id == id) {
                        s.rect.x += dx;
                        s.rect.y += dy;
                    }
                }
                CanvasItem::Note { id } => {
                    if let Some(n) = self.canvas.notes.iter_mut().find(|n| &n.id == id) {
                        n.rect.x += dx;
                        n.rect.y += dy;
                    }
                }
            }
        }
        true
    }

    /// Puts block `id` with its top-left corner at `at`, on the grid. Places every block by hand
    /// from then on. Returns `false` when there is no such block.
    pub fn place_block(&mut self, id: &str, at: Point) -> bool {
        if !self.blocks.iter().any(|b| b.id == id) {
            return false;
        }
        self.pin_blocks();
        let at = Point {
            x: snap(at.x).max(0.0),
            y: snap(at.y).max(0.0),
        };
        self.canvas.positions.insert(id.to_owned(), at);
        true
    }

    /// Forgets where blocks were placed by hand, so the test lays out automatically again.
    pub fn arrange_automatically(&mut self) {
        self.canvas.positions.clear();
    }

    /// Arranges the blocks like text: the chain wraps into rows of three, each row in a frame
    /// with no title yet, so the test reads from top to bottom. An `if` or a loop sits between
    /// the rows with what is inside it laid out as the automatic layout does. Blocks and the
    /// frames made by an earlier arrangement are placed anew; other frames and notes stay.
    pub fn arrange_in_rows(&mut self) {
        let (positions, frames) = rows(self);
        self.canvas.positions = positions;
        self.canvas.sections.retain(|s| !is_row_frame(&s.id));
        for rect in frames {
            let id = unique(self.canvas.sections.iter().map(|s| s.id.as_str()), ROW_ID);
            self.canvas.sections.push(CanvasSection {
                id,
                title: String::new(),
                rect,
            });
        }
    }

    /// Gives section or note `item` the frame `rect`, on the grid (with `snapping`) and at least
    /// four grid steps wide and high. Returns `false` for a block or an unknown item.
    pub fn resize_item(&mut self, item: &CanvasItem, rect: Rect, snapping: bool) -> bool {
        let left = snapped(rect.x, snapping).max(0.0);
        let top = snapped(rect.y, snapping).max(0.0);
        let rect = Rect {
            x: left,
            y: top,
            width: snapped(rect.x + rect.width - left, snapping).max(MIN_SIZE),
            height: snapped(rect.y + rect.height - top, snapping).max(MIN_SIZE),
        };
        let frame = match item {
            CanvasItem::Block { .. } => None,
            CanvasItem::Section { id } => self
                .canvas
                .sections
                .iter_mut()
                .find(|s| &s.id == id)
                .map(|s| &mut s.rect),
            CanvasItem::Note { id } => self
                .canvas
                .notes
                .iter_mut()
                .find(|n| &n.id == id)
                .map(|n| &mut n.rect),
        };
        frame.map(|f| *f = rect).is_some()
    }

    /// Adds a section titled `title` with the frame `rect` (snapped like [`Flow::resize_item`]).
    pub fn add_section(&mut self, title: &str, rect: Rect) -> CanvasItem {
        let id = unique(
            self.canvas.sections.iter().map(|s| s.id.as_str()),
            "section",
        );
        self.canvas.sections.push(CanvasSection {
            id: id.clone(),
            title: title.to_owned(),
            rect,
        });
        let item = CanvasItem::Section { id };
        self.resize_item(&item, rect, true);
        item
    }

    /// Adds a note about `anchor` with the default size and its top-left corner at `at`; one
    /// about a block that isn't there is about the test.
    pub fn add_note(
        &mut self,
        text: &str,
        author: &str,
        time: &str,
        at: Point,
        anchor: NoteAnchor,
    ) -> CanvasItem {
        let id = unique(self.canvas.notes.iter().map(|n| n.id.as_str()), "note");
        let rect = Rect {
            x: at.x,
            y: at.y,
            width: 14.0 * GRID,
            height: 9.0 * GRID,
        };
        let anchor = match anchor {
            NoteAnchor::Block { id } if !self.blocks.iter().any(|b| b.id == id) => NoteAnchor::Test,
            anchor => anchor,
        };
        self.canvas.notes.push(Note {
            id: id.clone(),
            text: text.to_owned(),
            author: author.to_owned(),
            time: time.to_owned(),
            anchor,
            attachments: Vec::new(),
            replies: Vec::new(),
            resolved: None,
            rect,
        });
        let item = CanvasItem::Note { id };
        self.resize_item(&item, rect, true);
        item
    }

    /// Points edge `edge` of block `from` at block `to`, or disconnects it when `to` is `None`
    /// (an emptied loop body points back at the loop). Refuses, returning `false`, an edge the
    /// block can't have, an unknown block, and a connection that would make a cycle other than
    /// through a loop's body.
    pub fn connect(&mut self, from: &str, edge: &str, to: Option<&str>) -> bool {
        let Some(i) = self.blocks.iter().position(|b| b.id == from) else {
            return false;
        };
        if !self.blocks[i].kind.has_edge(edge) {
            return false;
        }
        if let Some(to) = to {
            if !self.blocks.iter().any(|b| b.id == to) || (to == from && edge != BODY) {
                return false;
            }
        }
        let cycles = |flow: &Flow| {
            validate(flow)
                .iter()
                .filter(|i| i.code == "flow.cycle")
                .count()
        };
        let before = cycles(self);
        let mut edited = self.clone();
        set_edge(&mut edited.blocks[i], edge, to.map(str::to_owned));
        if edited == *self || cycles(&edited) > before {
            return false;
        }
        *self = edited;
        true
    }

    /// Removes `items`; removed blocks are joined around like [`Flow::remove_block`], and the
    /// notes about them stay on the canvas as notes about the test. Returns the block to select
    /// next.
    pub fn remove_items(&mut self, items: &[CanvasItem]) -> Option<String> {
        self.remove_items_and(items, false)
    }

    /// Like [`Flow::remove_items`], but the notes about the removed blocks go with them.
    pub fn remove_items_with_notes(&mut self, items: &[CanvasItem]) -> Option<String> {
        self.remove_items_and(items, true)
    }

    fn remove_items_and(&mut self, items: &[CanvasItem], with_notes: bool) -> Option<String> {
        let mut select = None;
        for item in items {
            match item {
                CanvasItem::Block { id } => select = self.remove_block_and(id, with_notes),
                CanvasItem::Section { id } => self.canvas.sections.retain(|s| &s.id != id),
                CanvasItem::Note { id } => self.canvas.notes.retain(|n| &n.id != id),
            }
        }
        select.filter(|id| self.blocks.iter().any(|b| &b.id == id))
    }

    /// `items` as the text of a test file, to put on the clipboard: the blocks with the edges
    /// between them, their positions, the sections and notes.
    pub fn copy_items(&self, items: &[CanvasItem]) -> String {
        let mut part = Flow {
            name: self.name.clone(),
            description: None,
            icon: None,
            app: self.app.clone(),
            test_type: None,
            priority: None,
            status: None,
            tags: Vec::new(),
            preconditions: None,
            author: None,
            platform: None,
            link: None,
            flaky: false,
            retries: None,
            depends_on: Vec::new(),
            start: String::new(),
            blocks: Vec::new(),
            canvas: Default::default(),
        };
        for item in items {
            match item {
                CanvasItem::Block { id } => {
                    if let Some(b) = self.blocks.iter().find(|b| &b.id == id) {
                        part.blocks.push(b.clone());
                        if let Some(p) = self.canvas.positions.get(id) {
                            part.canvas.positions.insert(id.clone(), *p);
                        }
                    }
                }
                CanvasItem::Section { id } => {
                    part.canvas
                        .sections
                        .extend(self.canvas.sections.iter().find(|s| &s.id == id).cloned());
                }
                CanvasItem::Note { id } => {
                    part.canvas
                        .notes
                        .extend(self.canvas.notes.iter().find(|n| &n.id == id).cloned());
                }
            }
        }
        let ids: HashSet<String> = part.blocks.iter().map(|b| b.id.clone()).collect();
        for b in &mut part.blocks {
            for (name, target) in b.kind.clone().edges() {
                if !ids.contains(target) {
                    set_edge(b, &name, None);
                }
            }
        }
        // The first block in reading order is the one no other copied block leads to.
        let targets: HashSet<String> = part
            .blocks
            .iter()
            .flat_map(|b| b.kind.edges().into_iter().map(|(_, t)| t.to_owned()))
            .collect();
        part.start = self
            .blocks_in_flow_order()
            .into_iter()
            .map(|b| &b.id)
            .find(|id| ids.contains(*id) && !targets.contains(*id))
            .or(part.blocks.first().map(|b| &b.id))
            .cloned()
            .unwrap_or_default();
        to_canonical_json(&part)
    }

    /// Adds what [`Flow::copy_items`] copied, or any test file: blocks get unused ids and keep
    /// the edges between them. With `after`, a block whose `next` (or `then`) is free to take
    /// them, the copies run after it: the first copied block follows it and the last one leads
    /// to what came after. Otherwise the copies stand apart, moved a little from the
    /// originals. Returns the added items, or `None` for text that isn't a test.
    pub fn paste_items(&mut self, text: &str, after: Option<&str>) -> Option<Vec<CanvasItem>> {
        let part = parse(text).ok()?;
        let mut added = Vec::new();
        let mut ids: HashMap<String, String> = HashMap::new();
        let first = self.blocks.len();
        for b in part.blocks_in_flow_order() {
            if ids.contains_key(&b.id) {
                continue;
            }
            let base = b.id.trim_end_matches(|c: char| c.is_ascii_digit());
            let id = self.unique_id(if base.is_empty() { &b.id } else { base });
            ids.insert(b.id.clone(), id.clone());
            self.canvas.positions.remove(&id);
            self.blocks.push(Block {
                id: id.clone(),
                kind: b.kind.clone(),
            });
            added.push(CanvasItem::Block { id });
        }
        for b in &mut self.blocks[first..] {
            for (name, target) in b.kind.clone().edges() {
                set_edge(b, &name, ids.get(target).cloned());
            }
        }

        let entry = ids.get(&part.start).cloned();
        if self.start.is_empty() && first == 0 {
            // Blocks pasted into an empty test become its start.
            self.start = entry.clone().unwrap_or_default();
        }
        let chained = after
            .zip(entry)
            .is_some_and(|(after, entry)| self.chain(after, &entry, first));
        if !chained && !self.canvas.positions.is_empty() {
            for (old, new) in &ids {
                if let Some(p) = part.canvas.positions.get(old) {
                    let at = Point {
                        x: p.x + PASTE_OFFSET,
                        y: p.y + PASTE_OFFSET,
                    };
                    self.canvas.positions.insert(new.clone(), at);
                }
            }
        }
        let shift = |r: Rect| Rect {
            x: r.x + PASTE_OFFSET,
            y: r.y + PASTE_OFFSET,
            ..r
        };
        for s in part.canvas.sections {
            let id = unique(self.canvas.sections.iter().map(|s| s.id.as_str()), &s.id);
            self.canvas.sections.push(CanvasSection {
                id: id.clone(),
                rect: shift(s.rect),
                ..s
            });
            added.push(CanvasItem::Section { id });
        }
        for n in part.canvas.notes {
            let id = unique(self.canvas.notes.iter().map(|n| n.id.as_str()), &n.id);
            // A copy is a new note: it brings its text and links, not the discussion or the
            // files, which stay with the original. It is about the copy of its block when that
            // was copied too, else about the same block while that is there.
            let anchor = match &n.anchor {
                NoteAnchor::Block { id } => match ids.get(id) {
                    Some(copy) => NoteAnchor::Block { id: copy.clone() },
                    None if self.blocks[..first].iter().any(|b| &b.id == id) => n.anchor.clone(),
                    None => NoteAnchor::Test,
                },
                anchor => anchor.clone(),
            };
            self.canvas.notes.push(Note {
                id: id.clone(),
                anchor,
                attachments: n
                    .attachments
                    .into_iter()
                    .filter(|a| a.kind == AttachmentKind::Link)
                    .collect(),
                replies: Vec::new(),
                resolved: None,
                rect: shift(n.rect),
                ..n
            });
            added.push(CanvasItem::Note { id });
        }
        Some(added)
    }

    /// Runs the blocks from index `first` on, starting at `entry`, after block `after`. Needs a
    /// pasted block with a free `next` to continue with what came after.
    fn chain(&mut self, after: &str, entry: &str, first: usize) -> bool {
        let Some(i) = self.blocks.iter().position(|b| b.id == after) else {
            return false;
        };
        let edge = if matches!(self.blocks[i].kind, BlockKind::If { .. }) {
            THEN
        } else {
            NEXT
        };
        if !self.blocks[i].kind.has_edge(edge) {
            return false;
        }
        let old = self.blocks[i].kind.edge(edge).map(str::to_owned);
        let pasted: HashSet<String> = self.blocks[first..].iter().map(|b| b.id.clone()).collect();
        let mut part = self.clone();
        part.start = entry.to_owned();
        let exit = part
            .blocks_in_flow_order()
            .into_iter()
            .filter(|b| pasted.contains(&b.id))
            .rev()
            .find(|b| b.kind.has_edge(NEXT) && b.kind.edge(NEXT).is_none())
            .map(|b| b.id.clone());
        let Some(exit) = exit else {
            return false;
        };
        set_edge(&mut self.blocks[i], edge, Some(entry.to_owned()));
        let j = self
            .blocks
            .iter()
            .position(|b| b.id == exit)
            .expect("pasted");
        set_edge(&mut self.blocks[j], NEXT, old);
        true
    }
}

/// `base`, or `base2`, `base3`… — the first one not in `taken`.
fn unique<'a>(taken: impl Iterator<Item = &'a str>, base: &str) -> String {
    let taken: HashSet<&str> = taken.collect();
    std::iter::once(base.to_owned())
        .chain((2..).map(|n| format!("{base}{n}")))
        .find(|id| !taken.contains(id.as_str()))
        .expect("an unused id always exists")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{BLOCK_HEIGHT, BLOCK_WIDTH};

    fn flow(blocks: &str, start: &str) -> Flow {
        parse(&format!(
            r#"{{"format": 1, "name": "T", "app": "com.example.App", "start": "{start}", "blocks": {blocks}}}"#
        ))
        .unwrap()
    }

    fn chain() -> Flow {
        flow(
            r#"{"a": {"type": "pressKeys", "keys": "a", "next": "b"},
                "b": {"type": "pressKeys", "keys": "b", "next": "c"},
                "c": {"type": "pressKeys", "keys": "c"}}"#,
            "a",
        )
    }

    fn block(id: &str) -> CanvasItem {
        CanvasItem::Block { id: id.to_owned() }
    }

    fn at(f: &Flow, id: &str) -> Rect {
        layout(f)
            .blocks
            .into_iter()
            .find(|b| b.id == id)
            .unwrap()
            .rect
    }

    #[test]
    fn moving_and_resizing_leave_the_grid_when_snapping_is_off() {
        let mut f = chain();
        assert!(f.move_items(&[block("b")], 301.5, 7.0, false));
        let before = f.canvas.positions["b"];
        assert!(f.move_items(&[block("b")], 3.0, 0.0, false));
        assert_eq!(f.canvas.positions["b"].x, before.x + 3.0);
        assert!(f.move_items(&[block("b")], 3.0, 0.0, true));
        assert_eq!(f.canvas.positions["b"].x % GRID, 0.0);

        let note = f.add_note("x", "me", "now", Point { x: 0.0, y: 0.0 }, NoteAnchor::Test);
        let frame = Rect {
            x: 10.0,
            y: 11.0,
            width: 103.0,
            height: 101.0,
        };
        assert!(f.resize_item(&note, frame, false));
        let CanvasItem::Note { id } = &note else {
            unreachable!()
        };
        assert_eq!(
            f.canvas.notes.iter().find(|n| &n.id == id).unwrap().rect,
            frame
        );
    }

    #[test]
    fn moving_a_block_pins_the_others_where_they_were() {
        let mut f = chain();
        let (a, c) = (at(&f, "a"), at(&f, "c"));
        assert!(f.move_items(&[block("b")], 300.0, 7.0, true));
        assert_eq!(f.canvas.positions.len(), 3);
        let b = f.canvas.positions["b"];
        assert_eq!((b.x % GRID, b.y % GRID), (0.0, 0.0));
        assert!(b.x > a.x + BLOCK_WIDTH);
        // The others stay put, give or take the snap to the grid.
        assert!((at(&f, "a").x - a.x).abs() <= GRID / 2.0);
        assert!((at(&f, "c").y - c.y).abs() <= GRID / 2.0);
        // Nothing goes past the top-left corner; moving by less than half a step does nothing.
        assert!(f.move_items(&[block("a"), block("b")], -10_000.0, 0.0, true));
        assert_eq!(
            f.canvas.positions["a"].x.min(f.canvas.positions["b"].x),
            0.0
        );
        assert!(!f.move_items(&[block("a")], 3.0, -3.0, true));

        let saved = parse(&to_canonical_json(&f)).unwrap();
        assert_eq!(saved, f);
        f.arrange_automatically();
        assert_eq!(layout(&f), layout(&chain()));
    }

    #[test]
    fn hand_placed_blocks_keep_their_place_and_new_ones_go_below() {
        let mut f = chain();
        assert!(f.place_block("c", Point { x: 500.0, y: 20.0 }));
        assert_eq!(at(&f, "c").x, 496.0);
        assert_eq!(at(&f, "c").y, 16.0);
        assert!(!f.place_block("missing", Point { x: 0.0, y: 0.0 }));

        f.insert_block(
            "type",
            Some(&crate::Slot {
                from: "c".into(),
                edge: NEXT.into(),
            }),
        );
        let new = at(&f, "type");
        assert_eq!((new.x, new.y), (496.0, 16.0 + BLOCK_HEIGHT + 48.0));
        let l = layout(&f);
        let edge = l.edges.iter().find(|e| e.from == "c").unwrap();
        assert_eq!(edge.to, "type");
        assert_eq!(
            edge.points.first(),
            Some(&Point {
                x: 600.0,
                y: 16.0 + BLOCK_HEIGHT
            })
        );
        assert_eq!(edge.points.last(), Some(&Point { x: 600.0, y: new.y }));
        // Every edge a block can have has a handle; an open one also has a place to add.
        assert_eq!(l.ports.len(), 4);
        assert!(
            l.slots
                .iter()
                .any(|s| s.slot.from == "type" && s.at.y > new.y + BLOCK_HEIGHT)
        );

        // Removing a block forgets its position.
        f.remove_block("b");
        assert!(!f.canvas.positions.contains_key("b"));
    }

    #[test]
    fn a_hand_placed_loop_is_framed_around_its_body() {
        let mut f = flow(
            r#"{"loop": {"type": "repeat", "times": 2, "body": "x", "next": "after"},
                "x": {"type": "pressKeys", "keys": "x", "next": "loop"},
                "after": {"type": "pressKeys", "keys": "a"}}"#,
            "loop",
        );
        f.place_block("x", Point { x: 400.0, y: 400.0 });
        let l = layout(&f);
        let frame = l.frames[0].rect;
        for id in ["loop", "x"] {
            let r = at(&f, id);
            assert!(
                r.x > frame.x && r.x + r.width < frame.x + frame.width,
                "{id}"
            );
            assert!(
                r.y > frame.y && r.y + r.height < frame.y + frame.height,
                "{id}"
            );
        }
        // The way back up goes around on the right.
        let back = l.edges.iter().find(|e| e.from == "x").unwrap();
        assert!(back.points.iter().any(|p| p.x > 400.0 + BLOCK_WIDTH));
        assert!(l.width >= frame.x + frame.width);
    }

    #[test]
    fn connecting_refuses_edges_that_make_a_cycle() {
        let mut f = chain();
        assert!(f.connect("a", NEXT, Some("c")));
        assert_eq!(f.blocks[0].kind.edge(NEXT), Some("c"));
        assert!(!f.connect("c", NEXT, Some("a")), "a cycle without a loop");
        assert!(!f.connect("a", NEXT, Some("a")));
        assert!(!f.connect("a", THEN, Some("b")), "no such edge");
        assert!(!f.connect("a", NEXT, Some("missing")));
        assert!(!f.connect("a", NEXT, Some("c")), "nothing changes");
        assert!(f.connect("a", NEXT, None));
        assert_eq!(f.blocks[0].kind.edge(NEXT), None);

        let mut f = flow(
            r#"{"loop": {"type": "repeat", "times": 2, "body": "x"},
                "x": {"type": "pressKeys", "keys": "x"}}"#,
            "loop",
        );
        assert!(
            f.connect("x", NEXT, Some("loop")),
            "a loop body goes back to its loop"
        );
        assert!(f.connect("loop", BODY, None));
        assert_eq!(f.blocks[0].kind.edge(BODY), Some("loop"), "an empty body");
    }

    #[test]
    fn copies_keep_the_edges_between_them() {
        let mut f = chain();
        let copied = f.copy_items(&[block("b"), block("c")]);
        let added = f.paste_items(&copied, None).unwrap();
        assert_eq!(added, [block("b2"), block("c2")]);
        let b2 = f.blocks.iter().find(|b| b.id == "b2").unwrap();
        assert_eq!(b2.kind.edge(NEXT), Some("c2"));
        // Standing apart, nothing leads to them yet.
        assert_eq!(
            f.blocks
                .iter()
                .filter(|b| b.kind.edge(NEXT) == Some("b2"))
                .count(),
            0
        );

        // Pasted after a block, they run between it and what came next.
        let mut f = chain();
        let copied = f.copy_items(&[block("a")]);
        f.paste_items(&copied, Some("b")).unwrap();
        let next = |f: &Flow, id: &str| {
            f.blocks
                .iter()
                .find(|b| b.id == id)
                .unwrap()
                .kind
                .edge(NEXT)
                .map(str::to_owned)
        };
        assert_eq!(next(&f, "b").as_deref(), Some("a2"));
        assert_eq!(next(&f, "a2").as_deref(), Some("c"));
        assert!(crate::validate(&f).iter().all(|i| i.code == "flow.noCheck"));

        assert_eq!(f.paste_items("not a test", None), None);

        // Pasted into an empty test, they become its start.
        let mut empty = chain();
        empty.blocks.clear();
        empty.start.clear();
        empty.paste_items(&copied, None).unwrap();
        assert_eq!(empty.start, "a");
    }

    #[test]
    fn a_section_takes_along_what_lies_inside_it() {
        let mut f = chain();
        let a = at(&f, "a");
        let frame = Rect {
            x: a.x - GRID,
            y: a.y - 3.0 * GRID,
            width: BLOCK_WIDTH + 2.0 * GRID,
            height: BLOCK_HEIGHT + 4.0 * GRID,
        };
        let section = f.add_section("Start", frame);
        f.add_note("", "", "", Point { x: 2000.0, y: 0.0 }, NoteAnchor::Test);
        let rect = f.canvas.sections[0].rect;
        assert!(f.move_items(&[section], 32.0, 0.0, true));
        assert_eq!(f.canvas.sections[0].rect.x, rect.x + 32.0);
        assert!((at(&f, "a").x - (a.x + 32.0)).abs() <= GRID / 2.0);
        assert!((at(&f, "b").x - at(&chain(), "b").x).abs() <= GRID / 2.0);
        assert_eq!(f.canvas.notes[0].rect.x, 2000.0);
    }

    #[test]
    fn sections_and_notes_are_added_moved_resized_copied_and_removed() {
        let mut f = chain();
        let section = f.add_section(
            "Sign in",
            Rect {
                x: 10.0,
                y: 10.0,
                width: 300.0,
                height: 20.0,
            },
        );
        assert_eq!(
            section,
            CanvasItem::Section {
                id: "section".into()
            }
        );
        assert_eq!(
            f.canvas.sections[0].rect,
            Rect {
                x: 16.0,
                y: 16.0,
                width: 288.0,
                height: 64.0
            }
        );
        let note = f.add_note(
            "Check twice",
            "Anna",
            "2026-10-18T09:30:00Z",
            Point { x: 592.0, y: 0.0 },
            NoteAnchor::Test,
        );
        assert!(f.move_items(std::slice::from_ref(&note), 16.0, 32.0, true));
        assert_eq!(
            (f.canvas.notes[0].rect.x, f.canvas.notes[0].rect.y),
            (608.0, 32.0)
        );
        assert!(
            f.canvas.positions.is_empty(),
            "moving a note leaves blocks laid out automatically"
        );
        assert!(f.resize_item(
            &note,
            Rect {
                x: 608.0,
                y: 32.0,
                width: 400.0,
                height: 300.0
            },
            true
        ));
        assert!(!f.resize_item(
            &block("a"),
            Rect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0
            },
            true
        ));
        assert!(layout(&f).width >= 608.0 + 400.0);

        let copied = f.copy_items(&[note.clone(), section.clone()]);
        let added = f.paste_items(&copied, None).unwrap();
        assert_eq!(
            added,
            [
                CanvasItem::Section {
                    id: "section2".into()
                },
                CanvasItem::Note { id: "note2".into() }
            ]
        );
        assert_eq!(f.canvas.notes[1].rect.x, 608.0 + PASTE_OFFSET);
        assert_eq!(f.canvas.notes[1].author, "Anna");

        assert_eq!(
            f.remove_items(&[note, section, block("b")]),
            Some("c".into())
        );
        assert_eq!(f.canvas.notes.len(), 1);
        assert_eq!(f.canvas.sections.len(), 1);
        assert_eq!(parse(&to_canonical_json(&f)).unwrap(), f);
    }
    /// `n` pressKeys blocks one after another, `b1` first.
    fn run_of(n: usize) -> Flow {
        let blocks: Vec<String> = (1..=n)
            .map(|i| {
                let next = if i < n {
                    format!(r#", "next": "b{}""#, i + 1)
                } else {
                    String::new()
                };
                format!(r#""b{i}": {{"type": "pressKeys", "keys": "k"{next}}}"#)
            })
            .collect();
        flow(&format!("{{{}}}", blocks.join(",")), "b1")
    }

    fn row_frames(f: &Flow) -> Vec<Rect> {
        f.canvas
            .sections
            .iter()
            .filter(|s| is_row_frame(&s.id))
            .map(|s| s.rect)
            .collect()
    }

    fn inside(outer: &Rect, inner: &Rect) -> bool {
        contains(outer, inner)
    }

    #[test]
    fn a_short_run_fills_one_row_left_to_right() {
        for n in [1, 3] {
            let mut f = run_of(n);
            f.arrange_in_rows();
            let frames = row_frames(&f);
            assert_eq!(frames.len(), 1, "{n} blocks");
            let rects: Vec<Rect> = (1..=n).map(|i| at(&f, &format!("b{i}"))).collect();
            for pair in rects.windows(2) {
                assert_eq!(pair[0].y, pair[1].y);
                assert_eq!(pair[1].x, pair[0].x + BLOCK_WIDTH + 48.0);
            }
            assert!(rects.iter().all(|r| inside(&frames[0], r)));
            // The title bar stays clear above the blocks.
            assert!(rects[0].y >= frames[0].y + 48.0);
            for r in rects {
                assert_eq!((r.x % GRID, r.y % GRID), (0.0, 0.0));
            }
            // Side by side, a connector runs straight from one block to the next.
            let l = layout(&f);
            assert_eq!(l.edges.len(), n - 1);
            for e in &l.edges {
                assert_eq!(e.points.len(), 2);
                assert_eq!(e.points[0].y, e.points[1].y);
                assert!(e.points[0].x < e.points[1].x);
            }
        }
    }

    #[test]
    fn a_long_run_wraps_after_three_blocks_and_reads_like_text() {
        for (n, rows) in [(4, 2), (7, 3)] {
            let mut f = run_of(n);
            f.arrange_in_rows();
            let frames = row_frames(&f);
            assert_eq!(frames.len(), rows, "{n} blocks");
            for pair in frames.windows(2) {
                assert!(
                    pair[1].y >= pair[0].y + pair[0].height,
                    "frames do not overlap"
                );
                assert_eq!(pair[0].x, pair[1].x);
                assert_eq!(pair[0].width, pair[1].width);
            }
            let l = layout(&f);
            for i in 1..=n {
                let r = at(&f, &format!("b{i}"));
                let frame = &frames[(i - 1) / 3];
                assert!(inside(frame, &r), "b{i} sits in its row's frame");
                assert_eq!(
                    r.x,
                    frames[0].x + 16.0 + ((i - 1) % 3) as f64 * (BLOCK_WIDTH + 48.0)
                );
            }
            // The steps the editor numbers by position come out in the order they run.
            let mut by_position: Vec<&crate::layout::LaidBlock> = l.blocks.iter().collect();
            by_position.sort_by(|a, b| {
                (a.rect.y, a.rect.x)
                    .partial_cmp(&(b.rect.y, b.rect.x))
                    .unwrap()
            });
            let ids: Vec<&str> = by_position.iter().map(|b| b.id.as_str()).collect();
            let run: Vec<String> = (1..=n).map(|i| format!("b{i}")).collect();
            assert_eq!(ids, run);
            // The way from the end of a row to the start of the next goes down, then back.
            let wrap = l.edges.iter().find(|e| e.from == "b3").unwrap();
            assert_eq!(wrap.to, "b4");
            assert!(wrap.points.first().unwrap().y < wrap.points.last().unwrap().y);
            assert_eq!(wrap.points.last().unwrap().y, at(&f, "b4").y);
            assert_eq!(l.edges.len(), n - 1);
            assert_eq!(parse(&to_canonical_json(&f)).unwrap(), f);
        }
    }

    #[test]
    fn branches_and_loops_sit_between_the_rows_and_stay_whole() {
        let mut f = flow(
            r#"{"a": {"type": "pressKeys", "keys": "a", "next": "b"},
                "b": {"type": "pressKeys", "keys": "b", "next": "if"},
                "if": {"type": "if", "reference": {"image": "a.png"}, "then": "yes", "else": "no"},
                "yes": {"type": "pressKeys", "keys": "y", "next": "c"},
                "no": {"type": "pressKeys", "keys": "n", "next": "c"},
                "c": {"type": "pressKeys", "keys": "c", "next": "loop"},
                "loop": {"type": "repeat", "times": 2, "body": "x", "next": "d"},
                "x": {"type": "pressKeys", "keys": "x", "next": "loop"},
                "d": {"type": "pressKeys", "keys": "d", "next": "e"},
                "e": {"type": "pressKeys", "keys": "e", "next": "f"},
                "f": {"type": "pressKeys", "keys": "f", "next": "g"},
                "g": {"type": "pressKeys", "keys": "g"}}"#,
            "a",
        );
        let before = layout(&f);
        f.arrange_in_rows();
        let l = layout(&f);
        let mut ids: Vec<&str> = l.blocks.iter().map(|b| b.id.as_str()).collect();
        ids.sort_unstable();
        assert_eq!(
            ids,
            [
                "a", "b", "c", "d", "e", "f", "g", "if", "loop", "no", "x", "yes"
            ]
        );
        // Rows: [a b], the if with its branches, [c], the loop, [d e f], [g].
        assert_eq!(row_frames(&f).len(), 4);
        let (a, b, branch) = (at(&f, "a"), at(&f, "b"), at(&f, "if"));
        let (yes, no, c) = (at(&f, "yes"), at(&f, "no"), at(&f, "c"));
        assert_eq!(a.y, b.y);
        assert!(branch.y > a.y + a.height);
        assert!(yes.x < no.x && yes.y == no.y && yes.y > branch.y);
        assert!(c.y > yes.y + yes.height);
        let (lp, x, d, g) = (at(&f, "loop"), at(&f, "x"), at(&f, "d"), at(&f, "g"));
        assert!(lp.y > c.y && x.y > lp.y && d.y > x.y);
        assert!(at(&f, "g").y > d.y);
        assert!(g.x == d.x);
        // The loop is still a frame around its body, and the branches still join below.
        let frame = l.frames.iter().find(|fr| fr.block == "loop").unwrap().rect;
        assert!(inside(&frame, &lp) && inside(&frame, &x));
        assert!(!inside(&frame, &d));
        for (from, edge, to) in [
            ("if", "then", "yes"),
            ("if", "else", "no"),
            ("yes", "next", "c"),
            ("no", "next", "c"),
            ("x", "next", "loop"),
            ("loop", "next", "d"),
        ] {
            let e = l.edges.iter().find(|e| e.from == from && e.edge == edge);
            assert_eq!(e.map(|e| e.to.as_str()), Some(to), "{from} {edge}");
        }
        assert_eq!(l.edges.len(), before.edges.len());
        // No two blocks touch.
        for (i, p) in l.blocks.iter().enumerate() {
            for q in &l.blocks[i + 1..] {
                let apart = p.rect.x + p.rect.width <= q.rect.x
                    || q.rect.x + q.rect.width <= p.rect.x
                    || p.rect.y + p.rect.height <= q.rect.y
                    || q.rect.y + q.rect.height <= p.rect.y;
                assert!(apart, "{} and {}", p.id, q.id);
            }
        }
    }

    #[test]
    fn arranging_again_replaces_its_own_frames_and_keeps_the_others() {
        let mut f = run_of(5);
        let mine = f.add_section(
            "Sign in",
            Rect {
                x: 700.0,
                y: 16.0,
                width: 160.0,
                height: 160.0,
            },
        );
        f.add_note(
            "check this",
            "Anna",
            "",
            Point { x: 700.0, y: 300.0 },
            NoteAnchor::Test,
        );
        f.arrange_in_rows();
        let once = f.clone();
        f.arrange_in_rows();
        assert_eq!(f, once);
        assert_eq!(row_frames(&f).len(), 2);
        let CanvasItem::Section { id } = mine else {
            unreachable!()
        };
        assert!(
            f.canvas
                .sections
                .iter()
                .any(|s| s.id == id && s.title == "Sign in")
        );
        assert_eq!(f.canvas.notes.len(), 1);
        // After a block goes, arranging closes the gap.
        f.remove_block("b2");
        f.arrange_in_rows();
        assert_eq!(at(&f, "b3").x, at(&f, "b1").x + BLOCK_WIDTH + 48.0);
    }

    #[test]
    fn unreachable_blocks_and_empty_tests_are_arranged_without_loss() {
        let mut f = flow(
            r#"{"a": {"type": "pressKeys", "keys": "a", "next": "b"},
                "b": {"type": "pressKeys", "keys": "b"},
                "lone": {"type": "pressKeys", "keys": "l"}}"#,
            "a",
        );
        f.arrange_in_rows();
        let l = layout(&f);
        assert_eq!(l.blocks.len(), 3);
        assert!(at(&f, "lone").x > row_frames(&f)[0].x + row_frames(&f)[0].width);
        let mut empty = run_of(1);
        empty.blocks.clear();
        empty.arrange_in_rows();
        assert!(empty.canvas.positions.is_empty() && empty.canvas.sections.is_empty());
    }

    #[test]
    fn a_test_not_arranged_keeps_its_automatic_layout() {
        let f = run_of(7);
        assert!(f.canvas.positions.is_empty() && f.canvas.sections.is_empty());
        let l = layout(&f);
        assert!(l.blocks.windows(2).all(|p| p[0].rect.x == p[1].rect.x));
    }

    #[test]
    fn blocks_placed_side_by_side_connect_from_side_to_side() {
        let mut f = chain();
        let a = at(&f, "a");
        f.place_block(
            "b",
            Point {
                x: a.x + 256.0,
                y: a.y,
            },
        );
        f.place_block("c", Point { x: a.x, y: a.y });
        let l = layout(&f);
        let ab = l.edges.iter().find(|e| e.from == "a").unwrap();
        let y = a.y + BLOCK_HEIGHT / 2.0;
        assert_eq!(
            ab.points,
            [
                Point {
                    x: a.x + BLOCK_WIDTH,
                    y
                },
                Point { x: a.x + 256.0, y }
            ]
        );
        // Backwards too, from the left side of the one on the right.
        let bc = l.edges.iter().find(|e| e.from == "b").unwrap();
        assert_eq!(bc.points.first(), Some(&Point { x: a.x + 256.0, y }));
        assert_eq!(
            bc.points.last(),
            Some(&Point {
                x: a.x + BLOCK_WIDTH,
                y
            })
        );
        // The handle to drag a new connector from sits where the connector leaves.
        let port = l.ports.iter().find(|p| p.slot.from == "a").unwrap();
        assert_eq!(port.at, ab.points[0]);

        // A block with two ways out keeps them at its bottom.
        let mut g = flow(
            r#"{"if": {"type": "if", "reference": {"image": "a.png"}, "then": "yes", "else": "no"},
                "yes": {"type": "pressKeys", "keys": "y"},
                "no": {"type": "pressKeys", "keys": "n"}}"#,
            "if",
        );
        let at_if = at(&g, "if");
        g.place_block(
            "yes",
            Point {
                x: at_if.x + 256.0,
                y: at_if.y,
            },
        );
        let then = layout(&g)
            .edges
            .into_iter()
            .find(|e| e.edge == "then")
            .unwrap();
        assert_eq!(then.points[0].y, at_if.y + BLOCK_HEIGHT);
    }
}
