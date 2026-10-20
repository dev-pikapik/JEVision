//! Placement of a test's blocks on the editor canvas.
//!
//! A test without positions in its file lays out automatically, always the same way, so diffs
//! show only real changes: the chain reads top to bottom, the branches of an `if` sit side by
//! side, and a loop is a frame around its body. Once blocks are moved by hand, they stay where
//! they were put and the connectors are routed between them. Coordinates are canvas points from
//! the top-left corner, not fractions of a window.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet, VecDeque};

use crate::edit::{START, Slot};
use crate::model::{BODY, BlockKind, Flow, NEXT, Point, Rect};

pub const BLOCK_WIDTH: f64 = 208.0;
pub const BLOCK_HEIGHT: f64 = 64.0;
/// Step of the canvas grid that moved blocks, sections and notes snap to.
pub const GRID: f64 = 16.0;
/// Vertical space between blocks; it holds the connector and the place to add a block.
const GAP: f64 = 48.0;
const COLUMN_GAP: f64 = 48.0;
/// Space between a loop’s frame and its blocks, and around the whole canvas.
const PAD: f64 = 16.0;
/// How far before a block a connector makes its last turn.
const TURN: f64 = 16.0;
/// Distance between connectors that jump to a block placed elsewhere.
const LANE: f64 = 12.0;

/// Where everything goes on the canvas.
#[derive(Debug, Clone, PartialEq)]
pub struct FlowLayout {
    pub width: f64,
    pub height: f64,
    /// Every placed block once, in reading order.
    pub blocks: Vec<LaidBlock>,
    pub frames: Vec<LaidFrame>,
    pub edges: Vec<LaidEdge>,
    pub slots: Vec<LaidSlot>,
    /// Where each edge a block can have leaves it, connected or not: the handle to drag a
    /// connector from.
    pub ports: Vec<LaidSlot>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LaidBlock {
    pub id: String,
    pub rect: Rect,
}

/// The frame around a loop block and its body.
#[derive(Debug, Clone, PartialEq)]
pub struct LaidFrame {
    pub block: String,
    pub rect: Rect,
}

/// A connector from block `from` along its edge `edge` to block `to`: a polyline of straight
/// segments, the arrow at the last point.
#[derive(Debug, Clone, PartialEq)]
pub struct LaidEdge {
    pub from: String,
    pub edge: String,
    pub to: String,
    pub points: Vec<Point>,
}

/// A place where the editor offers to add a block, at point `at` on the connector.
#[derive(Debug, Clone, PartialEq)]
pub struct LaidSlot {
    pub slot: Slot,
    pub at: Point,
}

enum Item {
    Block(usize),
    /// An `if` with a column per exit, left to right: `then`, each `elseIf`, `else`.
    Branch {
        at: usize,
        lanes: Vec<(Cow<'static, str>, Vec<Item>)>,
    },
    Loop {
        at: usize,
        body: Vec<Item>,
    },
}

impl Item {
    fn first(&self) -> usize {
        match self {
            Self::Block(at) | Self::Branch { at, .. } | Self::Loop { at, .. } => *at,
        }
    }
}

/// An edge whose source is placed but whose target is not connected yet.
struct Exit {
    from: usize,
    edge: Cow<'static, str>,
    target: String,
    points: Vec<Point>,
}

fn pt(x: f64, y: f64) -> Point {
    Point { x, y }
}

/// Lays out `flow`: automatically, or around the blocks placed by hand when there are any. In
/// both cases blocks that cannot be reached from `start` get columns of their own on the right,
/// and only the first block of each duplicate id is placed. The canvas covers sections and
/// notes too.
pub fn layout(flow: &Flow) -> FlowLayout {
    let auto = auto_layout(flow);
    let pinned = auto
        .blocks
        .iter()
        .any(|b| flow.canvas.positions.contains_key(&b.id));
    let mut out = if pinned {
        free_layout(flow, &auto)
    } else {
        auto
    };
    let drawn = flow.canvas.sections.iter().map(|s| &s.rect);
    for r in drawn.chain(flow.canvas.notes.iter().map(|n| &n.rect)) {
        out.width = out.width.max(r.x + r.width + PAD);
        out.height = out.height.max(r.y + r.height + PAD);
    }
    out
}

/// Exit point of each edge a block can have, spread evenly along its bottom: one in the
/// middle, two at a quarter from either side.
pub(crate) fn port_xs(kind: &BlockKind, rect: &Rect) -> Vec<(Cow<'static, str>, f64)> {
    let names = kind.edge_names();
    let step = rect.width / (2.0 * names.len() as f64);
    names
        .into_iter()
        .enumerate()
        .map(|(n, name)| (name, rect.x + step * (2 * n + 1) as f64))
        .collect()
}

fn auto_layout(flow: &Flow) -> FlowLayout {
    let index: HashMap<&str, usize> = flow
        .blocks
        .iter()
        .enumerate()
        .rev()
        .map(|(i, b)| (b.id.as_str(), i))
        .collect();
    let mut sources = vec![Vec::new(); flow.blocks.len()];
    for (i, block) in flow.blocks.iter().enumerate() {
        if index[block.id.as_str()] != i {
            continue; // a later block with the same id is never reached
        }
        for (name, target) in block.kind.edges() {
            if let Some(&t) = index.get(target).filter(|_| name != BODY) {
                sources[t].push(i);
            }
        }
    }
    let mut b = Builder {
        flow,
        index,
        sources,
        placed: vec![false; flow.blocks.len()],
        rects: vec![None; flow.blocks.len()],
        jumps: Vec::new(),
        out: FlowLayout {
            width: 0.0,
            height: 0.0,
            blocks: Vec::new(),
            frames: Vec::new(),
            edges: Vec::new(),
            slots: Vec::new(),
            ports: Vec::new(),
        },
    };
    let mut columns = vec![b.column(Some(flow.start.as_str()), &[])];
    for block in flow.blocks_in_flow_order() {
        if b.index
            .get(block.id.as_str())
            .is_some_and(|&i| !b.placed[i])
        {
            columns.push(b.column(Some(block.id.as_str()), &[]));
        }
    }

    let mut left = PAD;
    for (n, items) in columns.iter().enumerate() {
        let (width, _) = b.column_size(items);
        let cx = left + width / 2.0;
        if n == 0 {
            b.slot_at(String::new(), START, pt(cx, GAP / 2.0));
        }
        let open = b.place_column(items, cx, GAP);
        b.jumps.extend(open);
        left += width + 2.0 * COLUMN_GAP;
    }
    b.route_jumps();
    b.finish()
}

struct Builder<'a> {
    flow: &'a Flow,
    index: HashMap<&'a str, usize>,
    /// The blocks with an edge to each block, except a loop's edge into its body.
    sources: Vec<Vec<usize>>,
    placed: Vec<bool>,
    rects: Vec<Option<Rect>>,
    jumps: Vec<Exit>,
    out: FlowLayout,
}

impl<'a> Builder<'a> {
    fn id(&self, i: usize) -> &'a str {
        let flow: &'a Flow = self.flow;
        &flow.blocks[i].id
    }

    fn kind(&self, i: usize) -> &'a BlockKind {
        let flow: &'a Flow = self.flow;
        &flow.blocks[i].kind
    }

    /// The chain from `start` up to a block in `stops`, a block placed already or the end.
    fn column(&mut self, start: Option<&'a str>, stops: &[&'a str]) -> Vec<Item> {
        let mut items = Vec::new();
        let mut cur = start;
        while let Some(id) = cur {
            let Some(&i) = self.index.get(id) else { break };
            if stops.contains(&id) || self.placed[i] {
                break;
            }
            self.placed[i] = true;
            match self.kind(i) {
                kind @ BlockKind::If { .. } => {
                    let exits: Vec<(Cow<'static, str>, Option<&'a str>)> = kind
                        .edge_names()
                        .into_iter()
                        .map(|name| {
                            let target = kind.edge(&name);
                            (name, target)
                        })
                        .collect();
                    let targets: Vec<Option<&'a str>> = exits.iter().map(|(_, t)| *t).collect();
                    let merge = self.merge(&targets, stops);
                    let inner: Vec<&str> = stops.iter().copied().chain(merge).collect();
                    let lanes = exits
                        .into_iter()
                        .map(|(name, target)| (name, self.column(target, &inner)))
                        .collect();
                    items.push(Item::Branch { at: i, lanes });
                    cur = merge;
                }
                BlockKind::Repeat { body, next, .. } | BlockKind::UseData { body, next, .. } => {
                    let inner: Vec<&str> = stops.iter().copied().chain([id]).collect();
                    let body = self.column(Some(body.as_str()), &inner);
                    items.push(Item::Loop { at: i, body });
                    cur = next.as_deref();
                }
                kind => {
                    items.push(Item::Block(i));
                    cur = kind.edge(NEXT);
                }
            }
        }
        items
    }

    /// The first block every branch leads to, where they join again: of the blocks they all lead
    /// to, the one a depth-first walk from the first branch finishes last. The walk stops at
    /// those blocks, so its cost stays with the branches, not with the rest of the test.
    fn merge(&self, branches: &[Option<&'a str>], stops: &[&'a str]) -> Option<&'a str> {
        let stops: Vec<usize> = stops
            .iter()
            .filter_map(|id| self.index.get(id).copied())
            .collect();
        let branches: Vec<usize> = branches
            .iter()
            .map(|b| {
                b.and_then(|id| self.index.get(id).copied())
                    .filter(|i| !stops.contains(i))
            })
            .collect::<Option<_>>()?;
        let (&first, rest) = branches.split_first()?;
        let mut known = vec![HashMap::new(); rest.len()];
        let mut shared = |i: usize| {
            rest.iter()
                .zip(&mut known)
                .all(|(&from, known)| self.leads_to(&[from], i, &stops, known))
        };
        if shared(first) {
            return Some(self.id(first));
        }
        // The blocks where the walk enters the shared ones: each entered later finishes later.
        let mut seen = HashSet::from([first]);
        let mut entries = Vec::new();
        let mut walk = vec![(self.targets(first, &stops), 0)];
        while let Some((targets, n)) = walk.last_mut() {
            let Some(&i) = targets.get(*n) else {
                walk.pop();
                continue;
            };
            *n += 1;
            if !seen.insert(i) {
                continue;
            }
            if shared(i) {
                entries.push(i);
            } else {
                walk.push((self.targets(i, &stops), 0));
            }
        }
        // An entry that an earlier one leads to was walked as part of that one already.
        (0..entries.len())
            .rev()
            .find(|&n| !self.leads_to(&entries[..n], entries[n], &stops, &mut HashMap::new()))
            .map(|n| self.id(entries[n]))
    }

    /// Whether one of `from` leads to `to`. Searches forward from `from` and back from `to` in
    /// turns, so the answer costs about as much as the shorter search. `known` holds what earlier
    /// searches from the same `from` found out, and gains what this one does.
    fn leads_to(
        &self,
        from: &[usize],
        to: usize,
        stops: &[usize],
        known: &mut HashMap<usize, bool>,
    ) -> bool {
        if let Some(&answer) = known.get(&to) {
            return answer;
        }
        let mut ahead: HashSet<usize> = from.iter().copied().collect();
        let mut behind = HashSet::from([to]);
        let mut forward: VecDeque<usize> = from.iter().copied().collect();
        let mut back = VecDeque::from([to]);
        let found = ahead.contains(&to)
            || 'search: loop {
                let Some(i) = forward.pop_front() else {
                    break false;
                };
                for j in self.targets(i, stops) {
                    if behind.contains(&j) {
                        break 'search true;
                    }
                    if ahead.insert(j) {
                        forward.push_back(j);
                    }
                }
                let Some(j) = back.pop_front() else {
                    break false;
                };
                for &i in &self.sources[j] {
                    if self.placed[i] || stops.contains(&i) || known.get(&i) == Some(&false) {
                        continue;
                    }
                    if ahead.contains(&i) || known.get(&i) == Some(&true) {
                        break 'search true;
                    }
                    if behind.insert(i) {
                        back.push_back(i);
                    }
                }
            };
        known.extend(ahead.into_iter().map(|i| (i, true)));
        if !found {
            known.extend(behind.into_iter().map(|i| (i, false)));
        }
        found
    }

    /// The blocks `i` leads to, in the order of its edges: none once it is placed, and none into
    /// a loop's body or at a stop.
    fn targets(&self, i: usize, stops: &[usize]) -> Vec<usize> {
        if self.placed[i] {
            return Vec::new();
        }
        self.kind(i)
            .edges()
            .into_iter()
            .filter(|(name, _)| name != BODY)
            .filter_map(|(_, target)| self.index.get(target).copied())
            .filter(|t| !stops.contains(t))
            .collect()
    }

    fn size(&self, item: &Item) -> (f64, f64) {
        match item {
            Item::Block(_) => (BLOCK_WIDTH, BLOCK_HEIGHT),
            Item::Branch { lanes, .. } => {
                let height = lanes
                    .iter()
                    .map(|(_, items)| self.column_size(items).1)
                    .fold(0.0, f64::max);
                (self.lanes_width(lanes), BLOCK_HEIGHT + GAP + height)
            }
            Item::Loop { body, .. } => {
                let (bw, bh) = self.column_size(body);
                let inner = if body.is_empty() { 0.0 } else { bh + GAP };
                (bw + 2.0 * PAD, PAD + BLOCK_HEIGHT + GAP + inner)
            }
        }
    }

    /// Width of an `if` with its branches side by side, at least the width of the block.
    fn lanes_width(&self, lanes: &[(Cow<'static, str>, Vec<Item>)]) -> f64 {
        let content: f64 = lanes.iter().map(|(_, items)| self.lane_width(items)).sum();
        let gaps = COLUMN_GAP * lanes.len().saturating_sub(1) as f64;
        (content + gaps).max(BLOCK_WIDTH)
    }

    /// Width of a branch's column: an empty one is just the arrow past it.
    fn lane_width(&self, items: &[Item]) -> f64 {
        if items.is_empty() {
            0.0
        } else {
            self.column_size(items).0
        }
    }

    fn column_size(&self, items: &[Item]) -> (f64, f64) {
        let sizes: Vec<(f64, f64)> = items.iter().map(|i| self.size(i)).collect();
        let width = sizes.iter().map(|s| s.0).fold(BLOCK_WIDTH, f64::max);
        let gaps = GAP * sizes.len().saturating_sub(1) as f64;
        (width, sizes.iter().map(|s| s.1).sum::<f64>() + gaps)
    }

    /// Places a column centred on `cx` from `top`; returns its edges still to connect.
    fn place_column(&mut self, items: &[Item], cx: f64, top: f64) -> Vec<Exit> {
        let mut y = top;
        let mut open: Vec<Exit> = Vec::new();
        for item in items {
            let first = self.id(item.first());
            for exit in std::mem::take(&mut open) {
                if exit.target == first {
                    self.connect(exit, pt(cx, y));
                } else {
                    self.jumps.push(exit);
                }
            }
            open = self.place_item(item, cx, y);
            y += self.size(item).1 + GAP;
        }
        open
    }

    fn place_item(&mut self, item: &Item, cx: f64, top: f64) -> Vec<Exit> {
        match item {
            Item::Block(i) => {
                self.block(*i, cx, top);
                self.exit_down(*i, NEXT, cx, top + BLOCK_HEIGHT)
            }
            Item::Branch { at, lanes } => {
                let width = self.lanes_width(lanes);
                let widths: Vec<f64> = lanes.iter().map(|(_, l)| self.lane_width(l)).collect();
                // Room the block's width leaves over goes evenly between the branches.
                let spare = width - widths.iter().sum::<f64>();
                let gap = spare / lanes.len().saturating_sub(1).max(1) as f64;
                let mut left = cx - width / 2.0;
                self.block(*at, cx, top);
                let bottom = top + BLOCK_HEIGHT;
                let lanes_top = bottom + GAP;
                let mut exits = Vec::new();
                let rect = self.rects[*at].expect("the block was just placed");
                let ports = port_xs(self.kind(*at), &rect);
                for (((edge, items), w), (_, port)) in lanes.iter().zip(widths).zip(ports) {
                    let lane = left + w / 2.0;
                    left += w + gap;
                    self.port(*at, edge, pt(port, bottom));
                    let lead = vec![
                        pt(port, bottom),
                        pt(port, bottom + TURN),
                        pt(lane, bottom + TURN),
                        pt(lane, lanes_top),
                    ];
                    self.slot(*at, edge, pt(lane, bottom + (TURN + GAP) / 2.0));
                    if let Some(first) = items.first() {
                        let to = self.id(first.first());
                        self.edge(*at, edge, to, lead);
                        exits.extend(self.place_column(items, lane, lanes_top));
                    } else if let Some(target) = self.kind(*at).edge(edge) {
                        exits.push(Exit {
                            from: *at,
                            edge: edge.clone(),
                            target: target.to_owned(),
                            points: lead,
                        });
                    }
                }
                exits
            }
            Item::Loop { at, body } => {
                let (width, height) = self.size(item);
                let frame = Rect {
                    x: cx - width / 2.0,
                    y: top,
                    width,
                    height,
                };
                self.out.frames.push(LaidFrame {
                    block: self.id(*at).to_owned(),
                    rect: frame,
                });
                let node_top = top + PAD;
                self.block(*at, cx, node_top);
                let node_bottom = node_top + BLOCK_HEIGHT;
                let body_top = node_bottom + GAP;
                self.slot(*at, BODY, pt(cx, node_bottom + GAP / 2.0));
                self.port(*at, BODY, pt(cx, node_bottom));
                let own = self.id(*at);
                if let Some(first) = body.first() {
                    let to = self.id(first.first());
                    self.edge(*at, BODY, to, vec![pt(cx, node_bottom), pt(cx, body_top)]);
                    let back = top + height - TURN;
                    let lane = frame.x + PAD / 2.0;
                    let middle = node_top + BLOCK_HEIGHT / 2.0;
                    for exit in self.place_column(body, cx, body_top) {
                        if exit.target != own {
                            self.jumps.push(exit);
                            continue;
                        }
                        let mut points = exit.points;
                        let end = points[points.len() - 1];
                        points.extend([
                            pt(end.x, back),
                            pt(lane, back),
                            pt(lane, middle),
                            pt(cx - BLOCK_WIDTH / 2.0, middle),
                        ]);
                        self.edge(exit.from, &exit.edge, own, points);
                    }
                } else if let Some(target) = self.kind(*at).edge(BODY).filter(|t| *t != own) {
                    self.jumps.push(Exit {
                        from: *at,
                        edge: Cow::Borrowed(BODY),
                        target: target.to_owned(),
                        points: vec![pt(cx, node_bottom)],
                    });
                }
                self.exit_down(*at, NEXT, cx, top + height)
            }
        }
    }

    fn block(&mut self, i: usize, cx: f64, top: f64) {
        let rect = Rect {
            x: cx - BLOCK_WIDTH / 2.0,
            y: top,
            width: BLOCK_WIDTH,
            height: BLOCK_HEIGHT,
        };
        self.rects[i] = Some(rect);
        self.out.blocks.push(LaidBlock {
            id: self.id(i).to_owned(),
            rect,
        });
    }

    /// The slot below a block and its edge `edge`, still to connect, if it leads anywhere.
    fn exit_down(&mut self, i: usize, edge: &'static str, cx: f64, bottom: f64) -> Vec<Exit> {
        self.slot(i, edge, pt(cx, bottom + GAP / 2.0));
        self.port(i, edge, pt(cx, bottom));
        self.kind(i)
            .edge(edge)
            .map(|target| Exit {
                from: i,
                edge: Cow::Borrowed(edge),
                target: target.to_owned(),
                points: vec![pt(cx, bottom)],
            })
            .into_iter()
            .collect()
    }

    /// Joins `exit` to the top of the block at `to`, turning just above it if needed.
    fn connect(&mut self, exit: Exit, to: Point) {
        let mut points = exit.points;
        let from = points[points.len() - 1];
        if from.x != to.x {
            points.extend([pt(from.x, to.y - TURN), pt(to.x, to.y - TURN)]);
        }
        points.push(to);
        let target = self.index[exit.target.as_str()];
        let to = self.id(target);
        self.edge(exit.from, &exit.edge, to, points);
    }

    /// Routes edges to blocks placed elsewhere along lanes right of everything else, into the
    /// right side of their target.
    fn route_jumps(&mut self) {
        let right = self
            .rects
            .iter()
            .flatten()
            .chain(self.out.frames.iter().map(|f| &f.rect))
            .map(|r| r.x + r.width)
            .fold(0.0, f64::max);
        let mut lane = right + COLUMN_GAP / 2.0;
        for exit in std::mem::take(&mut self.jumps) {
            let Some(target) = self
                .index
                .get(exit.target.as_str())
                .and_then(|&i| self.rects[i])
            else {
                continue;
            };
            let mut points = exit.points;
            let from = points[points.len() - 1];
            let middle = target.y + target.height / 2.0;
            points.extend([
                pt(from.x, from.y + TURN),
                pt(lane, from.y + TURN),
                pt(lane, middle),
                pt(target.x + target.width, middle),
            ]);
            lane += LANE;
            self.edge(exit.from, &exit.edge, &exit.target, points);
        }
    }

    fn edge(&mut self, from: usize, edge: &str, to: &str, points: Vec<Point>) {
        self.out.edges.push(LaidEdge {
            from: self.id(from).to_owned(),
            edge: edge.to_owned(),
            to: to.to_owned(),
            points: straighten(points),
        });
    }

    fn slot(&mut self, i: usize, edge: &str, at: Point) {
        self.slot_at(self.id(i).to_owned(), edge, at);
    }

    fn port(&mut self, i: usize, edge: &str, at: Point) {
        self.out.ports.push(LaidSlot {
            slot: Slot {
                from: self.id(i).to_owned(),
                edge: edge.to_owned(),
            },
            at,
        });
    }

    fn slot_at(&mut self, from: String, edge: &str, at: Point) {
        self.out.slots.push(LaidSlot {
            slot: Slot {
                from,
                edge: edge.to_owned(),
            },
            at,
        });
    }

    fn finish(mut self) -> FlowLayout {
        let corners = self
            .out
            .blocks
            .iter()
            .map(|b| &b.rect)
            .chain(self.out.frames.iter().map(|f| &f.rect))
            .map(|r| pt(r.x + r.width, r.y + r.height));
        let points = self.out.edges.iter().flat_map(|e| e.points.iter().copied());
        let slots = self
            .out
            .slots
            .iter()
            .map(|s| pt(s.at.x, s.at.y + GAP / 2.0));
        for p in corners.chain(points).chain(slots) {
            self.out.width = self.out.width.max(p.x);
            self.out.height = self.out.height.max(p.y);
        }
        self.out.width += PAD;
        self.out
    }
}

/// Lays out a test with blocks placed by hand. A block without a position goes below the first
/// block that leads to it, or where the automatic layout put it, moved right until it overlaps
/// nothing. Connectors leave the bottom of a block and enter the top of the next one; one going
/// back up passes on the right of both blocks. A loop's frame surrounds its body.
fn free_layout(flow: &Flow, auto: &FlowLayout) -> FlowLayout {
    let index: HashMap<&str, &BlockKind> = flow
        .blocks
        .iter()
        .rev()
        .map(|b| (b.id.as_str(), &b.kind))
        .collect();
    let placed: HashSet<&str> = auto.blocks.iter().map(|b| b.id.as_str()).collect();
    let mut order: Vec<&str> = Vec::new();
    for b in flow.blocks_in_flow_order() {
        if placed.contains(b.id.as_str()) && !order.contains(&b.id.as_str()) {
            order.push(&b.id);
        }
    }
    let mut rects: HashMap<&str, Rect> = HashMap::new();
    for &id in &order {
        if let Some(p) = flow.canvas.positions.get(id) {
            rects.insert(id, block_rect(p.x, p.y));
        }
    }
    for &id in &order {
        if rects.contains_key(id) {
            continue;
        }
        let before = order
            .iter()
            .find(|&&o| rects.contains_key(o) && index[o].edges().iter().any(|(_, t)| *t == id));
        let mut r = match before {
            Some(o) => block_rect(rects[o].x, rects[o].y + BLOCK_HEIGHT + GAP),
            None => {
                auto.blocks
                    .iter()
                    .find(|b| b.id == id)
                    .expect("placed")
                    .rect
            }
        };
        while rects.values().any(|o| overlaps(o, &r)) {
            r.x += BLOCK_WIDTH + COLUMN_GAP;
        }
        rects.insert(id, r);
    }

    let mut out = FlowLayout {
        width: 0.0,
        height: 0.0,
        blocks: order
            .iter()
            .map(|&id| LaidBlock {
                id: id.to_owned(),
                rect: rects[id],
            })
            .collect(),
        frames: Vec::new(),
        edges: Vec::new(),
        slots: Vec::new(),
        ports: Vec::new(),
    };
    if let Some(start) = rects.get(flow.start.as_str()) {
        let at = pt(start.x + start.width / 2.0, start.y - GAP / 2.0);
        out.slots.push(slot(String::new(), START, at));
    }
    for &id in &order {
        let kind = index[id];
        let rect = rects[id];
        if let BlockKind::Repeat { body, .. } | BlockKind::UseData { body, .. } = kind {
            let mut frame = rect;
            for inside in loop_body(&index, id, body) {
                if let Some(r) = rects.get(inside) {
                    frame = union(&frame, r);
                }
            }
            out.frames.push(LaidFrame {
                block: id.to_owned(),
                rect: Rect {
                    x: frame.x - PAD,
                    y: frame.y - PAD,
                    width: frame.width + 2.0 * PAD,
                    height: frame.height + 2.0 * PAD,
                },
            });
        }
        for (edge, x) in port_xs(kind, &rect) {
            let from = pt(x, rect.y + rect.height);
            out.ports.push(slot(id.to_owned(), &edge, from));
            let target = kind.edge(&edge).filter(|t| *t != id || edge != BODY);
            match target.and_then(|t| Some((t, rects.get(t)?))) {
                Some((to, target)) => {
                    let points = route(from, &rect, target);
                    out.slots.push(slot(id.to_owned(), &edge, middle(&points)));
                    out.edges.push(LaidEdge {
                        from: id.to_owned(),
                        edge: edge.to_string(),
                        to: to.to_owned(),
                        points,
                    });
                }
                None => out
                    .slots
                    .push(slot(id.to_owned(), &edge, pt(from.x, from.y + GAP / 2.0))),
            }
        }
    }

    let corners = (out.blocks.iter().map(|b| &b.rect))
        .chain(out.frames.iter().map(|f| &f.rect))
        .map(|r| pt(r.x + r.width, r.y + r.height));
    let points = out.edges.iter().flat_map(|e| e.points.iter().copied());
    let slots = out.slots.iter().map(|s| pt(s.at.x, s.at.y + GAP / 2.0));
    let far: Vec<Point> = corners.chain(points).chain(slots).collect();
    for p in far {
        out.width = out.width.max(p.x + PAD);
        out.height = out.height.max(p.y + PAD);
    }
    out
}

fn block_rect(x: f64, y: f64) -> Rect {
    Rect {
        x,
        y,
        width: BLOCK_WIDTH,
        height: BLOCK_HEIGHT,
    }
}

fn slot(from: String, edge: &str, at: Point) -> LaidSlot {
    LaidSlot {
        slot: Slot {
            from,
            edge: edge.to_owned(),
        },
        at,
    }
}

/// Whether two blocks are closer than a connector needs.
fn overlaps(a: &Rect, b: &Rect) -> bool {
    let room = GRID;
    a.x < b.x + b.width + room
        && b.x < a.x + a.width + room
        && a.y < b.y + b.height + room
        && b.y < a.y + a.height + room
}

fn union(a: &Rect, b: &Rect) -> Rect {
    let (x, y) = (a.x.min(b.x), a.y.min(b.y));
    Rect {
        x,
        y,
        width: (a.x + a.width).max(b.x + b.width) - x,
        height: (a.y + a.height).max(b.y + b.height) - y,
    }
}

/// Blocks reachable from a loop's `body` without passing the loop itself.
fn loop_body<'a>(
    index: &HashMap<&'a str, &'a BlockKind>,
    own: &str,
    body: &'a str,
) -> Vec<&'a str> {
    let mut seen: Vec<&str> = Vec::new();
    let mut stack = vec![body];
    while let Some(id) = stack.pop() {
        if id == own || seen.contains(&id) {
            continue;
        }
        let Some(kind) = index.get(id) else { continue };
        seen.push(id);
        stack.extend(kind.edges().into_iter().map(|(_, t)| t));
    }
    seen
}

/// A connector from `from`, on the bottom of `source`, to the top middle of `target`.
fn route(from: Point, source: &Rect, target: &Rect) -> Vec<Point> {
    let to = pt(target.x + target.width / 2.0, target.y);
    let below = pt(from.x, from.y + TURN);
    let points = if to.y - TURN >= below.y {
        vec![from, below, pt(to.x, below.y), pt(to.x, to.y - TURN), to]
    } else {
        let lane = (source.x + source.width).max(target.x + target.width) + COLUMN_GAP / 2.0;
        vec![
            from,
            below,
            pt(lane, below.y),
            pt(lane, to.y - TURN),
            pt(to.x, to.y - TURN),
            to,
        ]
    };
    straighten(points)
}

/// `points` without repeated points and without the middle one of three in a straight line.
fn straighten(points: Vec<Point>) -> Vec<Point> {
    let mut clean: Vec<Point> = Vec::with_capacity(points.len());
    for p in points {
        if clean.last() == Some(&p) {
            continue;
        }
        if let [.., a, b] = clean[..] {
            if (a.x == b.x && b.x == p.x) || (a.y == b.y && b.y == p.y) {
                clean.pop();
            }
        }
        clean.push(p);
    }
    clean
}

/// The point halfway along a polyline.
fn middle(points: &[Point]) -> Point {
    let length = |a: &Point, b: &Point| (b.x - a.x).hypot(b.y - a.y);
    let mut left = points.windows(2).map(|w| length(&w[0], &w[1])).sum::<f64>() / 2.0;
    for w in points.windows(2) {
        let l = length(&w[0], &w[1]);
        if l >= left && l > 0.0 {
            let t = left / l;
            return pt(
                w[0].x + (w[1].x - w[0].x) * t,
                w[0].y + (w[1].y - w[0].y) * t,
            );
        }
        left -= l;
    }
    points[0]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ELSE, THEN};
    use crate::parse;

    fn flow(blocks: &str, start: &str) -> Flow {
        parse(&format!(
            r#"{{"format": 1, "name": "T", "app": "com.example.App", "start": "{start}", "blocks": {blocks}}}"#
        ))
        .unwrap()
    }

    fn rect<'l>(l: &'l FlowLayout, id: &str) -> &'l Rect {
        &l.blocks.iter().find(|b| b.id == id).unwrap().rect
    }

    fn edge<'l>(l: &'l FlowLayout, from: &str, name: &str) -> &'l LaidEdge {
        l.edges
            .iter()
            .find(|e| e.from == from && e.edge == name)
            .unwrap()
    }

    #[test]
    fn a_chain_reads_top_to_bottom() {
        let f = parse(include_str!("../../../examples/sign-in.flow.json")).unwrap();
        let l = layout(&f);
        let ids: Vec<&str> = l.blocks.iter().map(|b| b.id.as_str()).collect();
        assert_eq!(
            ids,
            ["open", "email", "password", "submit", "inbox", "messages"]
        );
        for pair in l.blocks.windows(2) {
            assert_eq!(pair[0].rect.x, pair[1].rect.x);
            assert_eq!(pair[1].rect.y, pair[0].rect.y + BLOCK_HEIGHT + GAP);
        }
        assert_eq!(l.edges.len(), 5);
        assert!(l.edges.iter().all(|e| e.points.len() == 2));
        // Before the first block, and after each one.
        assert_eq!(l.slots.len(), 7);
        assert_eq!(l.slots[0].slot.edge, START);
        assert!(l.height > rect(&l, "messages").y + BLOCK_HEIGHT);
    }

    #[test]
    fn branches_sit_side_by_side_and_join_below() {
        let f = flow(
            r#"{"if": {"type": "if", "reference": {"image": "a.png"}, "then": "yes", "else": "no"},
                "yes": {"type": "pressKeys", "keys": "y", "next": "more"},
                "more": {"type": "pressKeys", "keys": "m", "next": "end"},
                "no": {"type": "pressKeys", "keys": "n", "next": "end"},
                "end": {"type": "pressKeys", "keys": "e"}}"#,
            "if",
        );
        let l = layout(&f);
        let (branch, yes, no, more, end) = (
            rect(&l, "if"),
            rect(&l, "yes"),
            rect(&l, "no"),
            rect(&l, "more"),
            rect(&l, "end"),
        );
        assert!(yes.x < no.x);
        assert_eq!(yes.y, no.y);
        assert!(more.y > yes.y);
        assert!(end.y > more.y + more.height);
        assert_eq!(end.x, branch.x);
        // Both branches end on top of the block where they join.
        let top = pt(end.x + end.width / 2.0, end.y);
        assert_eq!(edge(&l, "more", NEXT).points.last(), Some(&top));
        assert_eq!(edge(&l, "no", NEXT).points.last(), Some(&top));
        assert_eq!(edge(&l, "if", ELSE).to, "no");
    }

    #[test]
    fn every_exit_of_an_if_gets_its_own_lane_and_port() {
        let f = flow(
            r#"{"if": {"type": "if", "reference": {"image": "a.png"}, "then": "one",
                       "elseIf": [{"reference": {"image": "b.png"}, "then": "two"}], "else": "three"},
                "one": {"type": "pressKeys", "keys": "a", "next": "end"},
                "two": {"type": "pressKeys", "keys": "b", "next": "end"},
                "three": {"type": "pressKeys", "keys": "c", "next": "end"},
                "end": {"type": "pressKeys", "keys": "e"}}"#,
            "if",
        );
        let l = layout(&f);
        let xs: Vec<f64> = ["one", "two", "three"]
            .iter()
            .map(|id| rect(&l, id).x)
            .collect();
        assert!(xs[0] < xs[1] && xs[1] < xs[2]);
        assert_eq!(rect(&l, "one").y, rect(&l, "three").y);
        assert_eq!(
            rect(&l, "if").x,
            xs[1],
            "the block sits over its middle lane"
        );
        assert_eq!(rect(&l, "end").x, rect(&l, "if").x);
        assert_eq!(edge(&l, "if", THEN).to, "one");
        assert_eq!(edge(&l, "if", "elseIf1").to, "two");
        assert_eq!(edge(&l, "if", ELSE).to, "three");
        let ports: Vec<&str> = l
            .ports
            .iter()
            .filter(|p| p.slot.from == "if")
            .map(|p| p.slot.edge.as_str())
            .collect();
        assert_eq!(ports, [THEN, "elseIf1", ELSE]);
    }

    #[test]
    fn an_empty_branch_runs_straight_to_the_join() {
        let f = flow(
            r#"{"if": {"type": "if", "reference": {"image": "a.png"}, "then": "yes", "else": "end"},
                "yes": {"type": "pressKeys", "keys": "y", "next": "end"},
                "end": {"type": "pressKeys", "keys": "e"}}"#,
            "if",
        );
        let l = layout(&f);
        let e = edge(&l, "if", ELSE);
        assert_eq!(e.to, "end");
        // The arrow passes right beside the other branch, and the canvas has room around it.
        let beside = rect(&l, "yes").x + BLOCK_WIDTH + COLUMN_GAP;
        assert_eq!(e.points[2].x, beside);
        assert!(rect(&l, "yes").x >= PAD);
        assert_eq!(l.width, beside + PAD);
        assert!(
            l.slots
                .iter()
                .any(|s| s.slot.from == "if" && s.slot.edge == ELSE)
        );
    }

    #[test]
    fn a_loop_is_a_frame_around_its_body() {
        let f = flow(
            r#"{"loop": {"type": "repeat", "times": 2, "body": "x", "next": "after"},
                "x": {"type": "pressKeys", "keys": "x", "next": "loop"},
                "after": {"type": "pressKeys", "keys": "a"}}"#,
            "loop",
        );
        let l = layout(&f);
        let frame = l.frames[0].rect;
        for id in ["loop", "x"] {
            let r = rect(&l, id);
            assert!(r.x > frame.x && r.x + r.width < frame.x + frame.width);
            assert!(r.y > frame.y && r.y + r.height < frame.y + frame.height);
        }
        assert!(rect(&l, "after").y > frame.y + frame.height);
        let back = edge(&l, "x", NEXT);
        let node = rect(&l, "loop");
        assert_eq!(back.to, "loop");
        assert_eq!(
            back.points.last(),
            Some(&pt(node.x, node.y + node.height / 2.0))
        );
        assert_eq!(edge(&l, "loop", NEXT).points[0].y, frame.y + frame.height);
    }

    #[test]
    fn every_block_is_placed_once_and_the_result_does_not_depend_on_block_order() {
        let mut f = flow(
            r#"{"a": {"type": "if", "reference": {"image": "a.png"}, "then": "b", "else": "a2"},
                "a2": {"type": "pressKeys", "keys": "c", "next": "a"},
                "b": {"type": "repeat", "times": 2, "body": "c", "next": "missing"},
                "c": {"type": "pressKeys", "keys": "c"},
                "orphan": {"type": "pressKeys", "keys": "o", "next": "b"}}"#,
            "a",
        );
        let l = layout(&f);
        let mut ids: Vec<&str> = l.blocks.iter().map(|b| b.id.as_str()).collect();
        ids.sort_unstable();
        assert_eq!(ids, ["a", "a2", "b", "c", "orphan"]);
        assert!(rect(&l, "orphan").x > rect(&l, "a").x + BLOCK_WIDTH);
        // The jump back to `a` and the orphan's edge into the loop are drawn too.
        assert_eq!(edge(&l, "a2", NEXT).to, "a");
        assert_eq!(edge(&l, "orphan", NEXT).to, "b");

        f.blocks.reverse();
        assert_eq!(layout(&f), l);
    }
}
