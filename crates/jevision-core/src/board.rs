//! Boards: an endless canvas with the project's tests, frames, notes, links and other boards on
//! it. A board is a `*.canvas` file in the project's `boards` folder, in the open JSON Canvas
//! 1.0 format (jsoncanvas.org), so Obsidian opens it too: a test is a `file` node that points at
//! its `*.flow.json`, another board a `file` node that points at its `*.canvas`, a note a `text`
//! node in Markdown, a link a `link` node and a frame a `group` node. What only JEVision uses
//! sits under a `jevision` key; keys this build doesn't know are kept as they were. A picture is
//! a `file` node that points at the image, as in Obsidian. Files are written in one canonical
//! form, a line per node and edge, so a change shows as a small diff.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};

use serde_json::{Map, Value};

use crate::index::RectIndex;
use crate::layout::GRID;
use crate::model::{Point, Rect};

/// Folder of the project that holds its boards.
pub const BOARDS_FOLDER: &str = "boards";

/// Folder of the boards folder that holds the pictures put on boards.
pub const IMAGES_FOLDER: &str = "images";
/// Ending of a board's file name.
pub const BOARD_SUFFIX: &str = ".canvas";
const TEST_SUFFIX: &str = ".flow.json";
/// Key of the fields only JEVision uses, on the board and on its nodes.
const OURS: &str = "jevision";

/// Size of a test's card.
pub const CARD_WIDTH: f64 = 14.0 * GRID;
pub const CARD_HEIGHT: f64 = 6.0 * GRID;
/// Height of a frame's title bar: all that shows of a folded frame.
pub const FRAME_HEADER: f64 = 3.0 * GRID;
/// Height of the title bar of a test whose blocks are shown inside its card.
pub const OPEN_HEADER: f64 = 3.0 * GRID;
/// The most a test with its blocks shown takes up on the board, per side; a bigger test shows
/// its blocks smaller.
const OPEN_LIMIT: f64 = 150.0 * GRID;
/// Longest side of a picture put on a board.
const IMAGE_SIDE: f64 = 24.0 * GRID;
/// Endings of the files a board shows as pictures, in lower case.
pub const IMAGE_SUFFIXES: [&str; 10] = [
    "png", "jpg", "jpeg", "gif", "webp", "heic", "heif", "tif", "tiff", "bmp",
];
/// Space around and between the cards in a folder's frame on the board of all tests.
const PAD: f64 = GRID;
/// Space between the folders' frames on the board of all tests.
const COLUMN_GAP: f64 = 2.0 * GRID;
/// The smallest node.
const MIN_SIZE: f64 = 3.0 * GRID;

/// A board, as read from its file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Board {
    /// In drawing order: later nodes are drawn over earlier ones, so frames come before what
    /// they hold.
    pub nodes: Vec<BoardNode>,
    pub edges: Vec<BoardEdge>,
    /// Every test of the project is on this board: new tests appear on it by themselves, in the
    /// frame of their folder, and deleted ones go away.
    pub all_tests: bool,
    /// Keys of the file this build doesn't use, as a JSON object, kept as they were; empty
    /// when there are none.
    pub extra: String,
}

/// A thing on a board.
#[derive(Debug, Clone, PartialEq)]
pub struct BoardNode {
    /// Unique among the nodes and edges of the board.
    pub id: String,
    pub kind: BoardNodeKind,
    /// Board points from the board's origin; y grows downwards.
    pub rect: Rect,
    /// One of the preset colors `1` to `6` (red, orange, yellow, green, cyan, purple) or a
    /// `#rrggbb` color; empty for none.
    pub color: String,
    /// A test's card opened to show the blocks inside it, drawn closer to the test itself the
    /// nearer the board is looked at; only a `file` node can be open.
    pub open: bool,
    /// Keys this build doesn't use, as a JSON object; empty when there are none.
    pub extra: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BoardNodeKind {
    /// A note, in Markdown.
    Text {
        text: String,
    },
    /// A file of the project, relative to the project folder with `/` between folders: a test
    /// when it ends in `.flow.json`, another board when it ends in `.canvas`.
    File {
        file: String,
    },
    Link {
        url: String,
    },
    /// A frame around the nodes that lie inside it; they move with it.
    Group {
        label: String,
        /// Folded to its title bar; what lies inside isn't shown.
        collapsed: bool,
        /// The test folder it holds on the board of all tests, relative to the project, empty
        /// for the project folder itself; `None` for a frame drawn by hand.
        folder: Option<String>,
    },
}

/// An arrow between two nodes.
#[derive(Debug, Clone, PartialEq)]
pub struct BoardEdge {
    pub id: String,
    pub from: String,
    /// Side of the node the arrow leaves: `top`, `right`, `bottom` or `left`; empty for the side
    /// that faces the other node.
    pub from_side: String,
    pub to: String,
    pub to_side: String,
    /// Arrowhead at the start; JSON Canvas draws none there unless the file says so.
    pub from_arrow: bool,
    /// Arrowhead at the end; JSON Canvas draws one there unless the file says not to.
    pub to_arrow: bool,
    pub label: String,
    pub color: String,
    pub extra: String,
}

/// The tests a frame holds.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameTests {
    pub frame: String,
    /// Test files relative to the project.
    pub tests: Vec<String>,
}

/// What is shown of a board, worked out once per change instead of on every redraw.
#[derive(Debug, Clone, PartialEq)]
pub struct BoardLayout {
    /// Every node not inside a folded frame, in drawing order, with the part of it that shows:
    /// a folded frame only shows its title bar.
    pub nodes: Vec<LaidNode>,
    /// The arrows between shown nodes.
    pub edges: Vec<LaidBoardEdge>,
    /// What all shown nodes cover; `None` for an empty board.
    pub bounds: Option<Rect>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LaidNode {
    pub id: String,
    pub rect: Rect,
    /// A frame: it only takes a click on its title bar.
    pub frame: bool,
}

/// Where an arrow leaves and arrives: the middle of a side of each node.
#[derive(Debug, Clone, PartialEq)]
pub struct LaidBoardEdge {
    pub id: String,
    pub from: Point,
    pub from_side: String,
    pub to: Point,
    pub to_side: String,
}

fn snap(v: f64) -> f64 {
    (v / GRID).round() * GRID
}

pub(crate) fn contains(outer: &Rect, inner: &Rect) -> bool {
    inner.x >= outer.x
        && inner.y >= outer.y
        && inner.x + inner.width <= outer.x + outer.width
        && inner.y + inner.height <= outer.y + outer.height
}

fn union(a: Rect, b: &Rect) -> Rect {
    let (x, y) = (a.x.min(b.x), a.y.min(b.y));
    Rect {
        x,
        y,
        width: (a.x + a.width).max(b.x + b.width) - x,
        height: (a.y + a.height).max(b.y + b.height) - y,
    }
}

/// The folder of a file relative to the project: `a/b/c.flow.json` → `a/b`.
fn folder_of(file: &str) -> &str {
    file.rsplit_once('/').map_or("", |(folder, _)| folder)
}

/// `path` with its start `old` replaced by `new`, when it is `old` or inside the folder `old`.
fn renamed(path: &str, old: &str, new: &str) -> Option<String> {
    if path == old {
        return Some(new.to_owned());
    }
    path.strip_prefix(old)
        .filter(|rest| rest.starts_with('/'))
        .map(|rest| format!("{new}{rest}"))
}

/// Whether `file` is a picture a board shows, going by its ending.
pub fn is_image_file(file: &str) -> bool {
    file.rsplit_once('.')
        .is_some_and(|(_, ending)| IMAGE_SUFFIXES.contains(&ending.to_ascii_lowercase().as_str()))
}

/// Where the blocks of a test whose card is `card` are drawn when it shows them: the middle of
/// the card under its title bar, as large as fits with the blocks' proportions kept. `content`
/// is the size the blocks take up on the test's own canvas.
pub fn blocks_area(card: &Rect, content: (f64, f64)) -> Rect {
    let (width, height) = (content.0.max(1.0), content.1.max(1.0));
    let room = (card.width, (card.height - OPEN_HEADER).max(1.0));
    let fit = (room.0 / width).min(room.1 / height);
    Rect {
        x: card.x + (room.0 - width * fit) / 2.0,
        y: card.y + OPEN_HEADER,
        width: width * fit,
        height: height * fit,
    }
}

const SIDES: [&str; 4] = ["top", "right", "bottom", "left"];
const ENDS: [&str; 2] = ["none", "arrow"];

impl BoardNode {
    /// The test file this node shows, when it is a test's card.
    pub fn test(&self) -> Option<&str> {
        match &self.kind {
            BoardNodeKind::File { file } if file.ends_with(TEST_SUFFIX) => Some(file),
            _ => None,
        }
    }

    /// The picture file this node shows, when it is a picture.
    pub fn image(&self) -> Option<&str> {
        match &self.kind {
            BoardNodeKind::File { file } if is_image_file(file) => Some(file),
            _ => None,
        }
    }

    fn is_frame(&self) -> bool {
        matches!(self.kind, BoardNodeKind::Group { .. })
    }

    fn is_folded(&self) -> bool {
        matches!(
            self.kind,
            BoardNodeKind::Group {
                collapsed: true,
                ..
            }
        )
    }

    /// The part that shows: a folded frame only shows its title bar.
    fn shown(&self) -> Rect {
        let mut rect = self.rect;
        if self.is_folded() {
            rect.height = rect.height.min(FRAME_HEADER);
        }
        rect
    }

    /// The size a new node of `kind` gets.
    fn size(kind: &BoardNodeKind) -> (f64, f64) {
        match kind {
            BoardNodeKind::Text { .. } => (16.0 * GRID, 10.0 * GRID),
            BoardNodeKind::File { file } if file.ends_with(TEST_SUFFIX) => {
                (CARD_WIDTH, CARD_HEIGHT)
            }
            BoardNodeKind::File { .. } | BoardNodeKind::Link { .. } => (CARD_WIDTH, 4.0 * GRID),
            BoardNodeKind::Group { .. } => (30.0 * GRID, 20.0 * GRID),
        }
    }
}

impl Board {
    /// Reads the text of a board file; `Err` says, in English, what is wrong with it.
    pub fn parse(text: &str) -> Result<Self, String> {
        let value: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let Value::Object(mut top) = value else {
            return Err("a board file holds one JSON object with \"nodes\" and \"edges\"".into());
        };
        let nodes = take_list(&mut top, "nodes")?;
        let edges = take_list(&mut top, "edges")?;
        let mut ours = take_ours(&mut top, "the board")?;
        let all_tests = take_flag(&mut ours, "allTests", "the board")?;
        no_more(&ours, "the board")?;

        let mut board = Board {
            nodes: Vec::with_capacity(nodes.len()),
            edges: Vec::with_capacity(edges.len()),
            all_tests,
            extra: extra(top),
        };
        let mut ids = HashSet::new();
        for (n, node) in nodes.into_iter().enumerate() {
            let node = parse_node(node, n)?;
            if !ids.insert(node.id.clone()) {
                return Err(format!("the id \"{}\" is used more than once", node.id));
            }
            board.nodes.push(node);
        }
        let node_ids: HashSet<String> = ids.clone();
        for (n, edge) in edges.into_iter().enumerate() {
            let edge = parse_edge(edge, n)?;
            for end in [&edge.from, &edge.to] {
                if !node_ids.contains(end) {
                    return Err(format!(
                        "arrow \"{}\" leads to \"{end}\", which isn't a node of the board",
                        edge.id
                    ));
                }
            }
            if !ids.insert(edge.id.clone()) {
                return Err(format!("the id \"{}\" is used more than once", edge.id));
            }
            board.edges.push(edge);
        }
        Ok(board)
    }

    /// The board as the text of its file: a line per node and per edge, keys in the order of
    /// the JSON Canvas spec, coordinates in whole points, settings that are off left out, tab
    /// indentation as Obsidian writes it, and a trailing newline.
    pub fn to_json(&self) -> String {
        let list = |lines: Vec<String>| {
            if lines.is_empty() {
                "[]".to_owned()
            } else {
                format!("[\n\t\t{}\n\t]", lines.join(",\n\t\t"))
            }
        };
        let mut out = format!(
            "{{\n\t\"nodes\":{},\n\t\"edges\":{}",
            list(self.nodes.iter().map(write_node).collect()),
            list(self.edges.iter().map(write_edge).collect()),
        );
        for (key, value) in extra_map(&self.extra) {
            out.push_str(&format!(",\n\t{}:{value}", json(&key)));
        }
        if self.all_tests {
            out.push_str(&format!(",\n\t\"{OURS}\":{{\"allTests\":true}}"));
        }
        out.push_str("\n}\n");
        out
    }

    /// Adds a node of `kind` centered on `at`, snapped to the grid, and returns its id. A
    /// frame goes behind everything so it doesn't cover what it is drawn around.
    pub fn add(&mut self, kind: BoardNodeKind, at: Point) -> String {
        let size = BoardNode::size(&kind);
        self.add_sized(kind, size, at)
    }

    /// Adds a picture of `width` by `height` pixels, shown from the file `file` relative to the
    /// project, centered on `at`, in its own proportions: its longer side is as it is, but no
    /// less than 3 and no more than 24 grid steps.
    pub fn add_image(&mut self, file: &str, width: f64, height: f64, at: Point) -> String {
        let (width, height) = (width.max(1.0), height.max(1.0));
        let long = width.max(height);
        let fit = long.clamp(MIN_SIZE, IMAGE_SIDE) / long;
        let size = (
            (width * fit).round().max(GRID),
            (height * fit).round().max(GRID),
        );
        let kind = BoardNodeKind::File {
            file: file.to_owned(),
        };
        self.add_sized(kind, size, at)
    }

    fn add_sized(&mut self, kind: BoardNodeKind, (width, height): (f64, f64), at: Point) -> String {
        let id = self.new_id(&format!("{kind:?}{}{}", at.x, at.y));
        let node = BoardNode {
            id: id.clone(),
            rect: Rect {
                x: snap(at.x - width / 2.0),
                y: snap(at.y - height / 2.0),
                width,
                height,
            },
            kind,
            color: String::new(),
            open: false,
            extra: String::new(),
        };
        if node.is_frame() {
            self.nodes.insert(0, node);
        } else {
            self.nodes.push(node);
        }
        id
    }

    /// The ids of the nodes that lie inside the frame `group`, at any depth.
    pub fn contents(&self, group: &str) -> Vec<String> {
        let Some(frame) = self
            .nodes
            .iter()
            .find(|n| n.id == group && matches!(n.kind, BoardNodeKind::Group { .. }))
        else {
            return Vec::new();
        };
        self.nodes
            .iter()
            .filter(|n| n.id != group && contains(&frame.rect, &n.rect))
            .map(|n| n.id.clone())
            .collect()
    }

    /// The nodes `ids` with everything inside the frames among them.
    pub fn with_contents(&self, ids: &[String]) -> Vec<String> {
        let mut seen: HashSet<String> = HashSet::new();
        let mut moved: Vec<String> = Vec::new();
        for id in ids {
            for id in std::iter::once(id.clone()).chain(self.contents(id)) {
                if seen.insert(id.clone()) {
                    moved.push(id);
                }
            }
        }
        moved
    }

    /// Moves the nodes `ids`, and what lies inside the frames among them, by `by`. With `snap`
    /// the first of them lands on the grid and the rest keep their places around it.
    pub fn move_nodes(&mut self, ids: &[String], by: Point, snap_to_grid: bool) {
        let moved: HashSet<String> = self.with_contents(ids).into_iter().collect();
        let first = ids
            .iter()
            .find_map(|id| self.nodes.iter().find(|n| &n.id == id));
        let Some(first) = first else { return };
        let (dx, dy) = if snap_to_grid {
            (
                snap(first.rect.x + by.x) - first.rect.x,
                snap(first.rect.y + by.y) - first.rect.y,
            )
        } else {
            (by.x.round(), by.y.round())
        };
        for node in self.nodes.iter_mut().filter(|n| moved.contains(&n.id)) {
            node.rect.x += dx;
            node.rect.y += dy;
        }
    }

    /// Gives the node `id` the place and size `rect`, snapped to the grid and no smaller than
    /// the smallest node.
    pub fn resize(&mut self, id: &str, rect: Rect) {
        let Some(node) = self.nodes.iter_mut().find(|n| n.id == id) else {
            return;
        };
        node.rect = Rect {
            x: snap(rect.x),
            y: snap(rect.y),
            width: snap(rect.width).max(MIN_SIZE),
            height: snap(rect.height).max(MIN_SIZE),
        };
    }

    /// Removes the nodes `ids` and the arrows to and from them. What lies inside a removed
    /// frame stays.
    pub fn remove(&mut self, ids: &[String]) {
        let gone: HashSet<&str> = ids.iter().map(String::as_str).collect();
        self.nodes.retain(|n| !gone.contains(n.id.as_str()));
        self.edges
            .retain(|e| !gone.contains(e.from.as_str()) && !gone.contains(e.to.as_str()));
    }

    /// Opens the card of the test `id` to show the blocks inside it: the card grows, from its
    /// top left corner, to hold `content`, the size the blocks take up on the test's own
    /// canvas, and comes to the front. The frames around it grow with it and make way for it,
    /// so it stays in them and covers nothing. A test bigger than the board's limit shows its
    /// blocks smaller. `false` when `id` isn't a test's card or is open already.
    pub fn open_test(&mut self, id: &str, content: (f64, f64)) -> bool {
        let Some(at) = self.nodes.iter().position(|n| n.id == id) else {
            return false;
        };
        if self.nodes[at].test().is_none() || self.nodes[at].open {
            return false;
        }
        let (width, height) = (content.0.max(1.0), content.1.max(1.0));
        let fit = (OPEN_LIMIT / width.max(height)).min(1.0);
        self.resize_card(
            at,
            (
                snap(width * fit).max(CARD_WIDTH),
                (OPEN_HEADER + snap(height * fit)).max(CARD_HEIGHT),
            ),
        );
        let mut node = self.nodes.remove(at);
        node.open = true;
        self.nodes.push(node);
        true
    }

    /// Folds the open test `id` back into its card, and the frames around it give back the
    /// room it took. `false` when it isn't open.
    pub fn close_test(&mut self, id: &str) -> bool {
        let Some(at) = self.nodes.iter().position(|n| n.id == id && n.open) else {
            return false;
        };
        self.nodes[at].open = false;
        self.resize_card(at, (CARD_WIDTH, CARD_HEIGHT));
        true
    }

    /// Gives the card at `at` the size `size` from its top left corner. Each frame around it
    /// grows or shrinks by as much as the card, though never so far that it lets go of what it
    /// holds, and makes way for it: what lies right of the card in its frame moves across by
    /// the change in width, what lies below it moves down by the change in height, and so, one
    /// frame out, does what lies right of or below that frame, up to the nodes outside all of
    /// them. What lies in a frame moves with it. Only what lies wholly above or wholly left
    /// stays, so the move is undone exactly by the opposite one. A card in no frame only
    /// changes size.
    fn resize_card(&mut self, at: usize, size: (f64, f64)) {
        let card = self.nodes[at].rect;
        let (dw, dh) = (size.0 - card.width, size.1 - card.height);
        let index = self.rect_index();
        let mut around: Vec<usize> = index
            .query(&card)
            .into_iter()
            .filter(|&n| self.nodes[n].is_frame() && contains(&self.nodes[n].rect, &card))
            .collect();
        let area = |n: usize| self.nodes[n].rect.width * self.nodes[n].rect.height;
        around.sort_by(|&a, &b| area(a).total_cmp(&area(b)));
        if around.is_empty() {
            self.nodes[at].rect.width = size.0;
            self.nodes[at].rect.height = size.1;
            return;
        }
        let held: Vec<Vec<usize>> = around
            .iter()
            .map(|&f| index.inside(&self.nodes[f].rect))
            .collect();

        // Frame by frame from the card out, what lies between the frame and the one inside it,
        // or the card, moves as a whole: a node with everything inside it, unless it lies in
        // another such frame and moves with that.
        let fixed: HashSet<usize> = around.iter().copied().chain([at]).collect();
        let mut moves: HashMap<usize, (f64, f64)> = HashMap::new();
        let mut inner = card;
        for level in 0..=around.len() {
            let outer = around.get(level).map(|&f| self.nodes[f].rect);
            let loose: Vec<usize> = match outer {
                Some(outer) => index.inside(&outer),
                None => (0..self.nodes.len()).collect(),
            }
            .into_iter()
            .filter(|n| !fixed.contains(n) && !contains(&inner, &self.nodes[*n].rect))
            .collect();
            let is_loose: HashSet<usize> = loose.iter().copied().collect();
            for &n in &loose {
                let rect = self.nodes[n].rect;
                let in_frame = index.query(&rect).into_iter().any(|f| {
                    f != n
                        && is_loose.contains(&f)
                        && self.nodes[f].is_frame()
                        && contains(&self.nodes[f].rect, &rect)
                });
                let by = (
                    if rect.x >= inner.x + inner.width && rect.y + rect.height > inner.y {
                        dw
                    } else {
                        0.0
                    },
                    if rect.y >= inner.y + inner.height && rect.x + rect.width > inner.x {
                        dh
                    } else {
                        0.0
                    },
                );
                if in_frame || by == (0.0, 0.0) {
                    continue;
                }
                let whole = if self.nodes[n].is_frame() {
                    index.inside(&rect)
                } else {
                    vec![n]
                };
                for m in whole {
                    moves.entry(m).or_insert(by);
                }
            }
            if let Some(outer) = outer {
                inner = outer;
            }
        }

        for (n, (dx, dy)) in moves {
            self.nodes[n].rect.x += dx;
            self.nodes[n].rect.y += dy;
        }
        self.nodes[at].rect.width = size.0;
        self.nodes[at].rect.height = size.1;
        for (&f, held) in around.iter().zip(&held) {
            let (right, bottom) = held
                .iter()
                .filter(|&&n| n != f)
                .map(|&n| self.nodes[n].rect)
                .fold((f64::NEG_INFINITY, f64::NEG_INFINITY), |(r, b), n| {
                    (r.max(n.x + n.width), b.max(n.y + n.height))
                });
            let frame = &mut self.nodes[f].rect;
            frame.width = (frame.width + dw).max(right - frame.x);
            frame.height = (frame.height + dh).max(bottom - frame.y);
        }
    }

    /// Draws an arrow from node `from` to node `to` and returns its id; `None` when one of them
    /// isn't on the board, they are the same node or the arrow is already there.
    pub fn connect(&mut self, from: &str, to: &str) -> Option<String> {
        let exists = |id: &str| self.nodes.iter().any(|n| n.id == id);
        if from == to || !exists(from) || !exists(to) {
            return None;
        }
        if self.edges.iter().any(|e| e.from == from && e.to == to) {
            return None;
        }
        let id = self.new_id(&format!("{from}>{to}"));
        self.edges.push(BoardEdge {
            id: id.clone(),
            from: from.to_owned(),
            from_side: String::new(),
            to: to.to_owned(),
            to_side: String::new(),
            from_arrow: false,
            to_arrow: true,
            label: String::new(),
            color: String::new(),
            extra: String::new(),
        });
        Some(id)
    }

    /// What is shown: the nodes not inside a folded frame and the arrows between them.
    pub fn layout(&self) -> BoardLayout {
        let hidden = self.hidden();
        let nodes: Vec<LaidNode> = self
            .nodes
            .iter()
            .filter(|n| !hidden.contains(n.id.as_str()))
            .map(|n| LaidNode {
                id: n.id.clone(),
                rect: n.shown(),
                frame: n.is_frame(),
            })
            .collect();
        let shown: HashMap<&str, Rect> = nodes.iter().map(|n| (n.id.as_str(), n.rect)).collect();
        let rect = |id: &str| shown.get(id).copied();
        let edges = self
            .edges
            .iter()
            .filter_map(|edge| {
                let (from, to) = (rect(&edge.from)?, rect(&edge.to)?);
                let from_side = side_toward(&edge.from_side, &from, &to);
                let to_side = side_toward(&edge.to_side, &to, &from);
                Some(LaidBoardEdge {
                    id: edge.id.clone(),
                    from: middle_of(&from, from_side),
                    from_side: from_side.to_owned(),
                    to: middle_of(&to, to_side),
                    to_side: to_side.to_owned(),
                })
            })
            .collect();
        let bounds = nodes.iter().map(|n| n.rect).reduce(|a, b| union(a, &b));
        BoardLayout {
            nodes,
            edges,
            bounds,
        }
    }

    /// For each frame, the tests whose cards lie inside it, shown or folded away, each test
    /// once and in board order: what a frame sums up.
    pub fn frame_tests(&self) -> Vec<FrameTests> {
        let mut frames = self.nodes.iter().filter(|n| n.is_frame()).peekable();
        if frames.peek().is_none() {
            return Vec::new();
        }
        let index = self.rect_index();
        frames
            .map(|frame| {
                let mut seen = HashSet::new();
                let tests = index
                    .inside(&frame.rect)
                    .into_iter()
                    .filter_map(|at| self.nodes[at].test())
                    .filter(|test| seen.insert(*test))
                    .map(str::to_owned)
                    .collect();
                FrameTests {
                    frame: frame.id.clone(),
                    tests,
                }
            })
            .collect()
    }

    /// The ids of the nodes inside a folded frame, which aren't shown.
    fn hidden(&self) -> HashSet<&str> {
        let mut hidden = HashSet::new();
        let mut folded = self.nodes.iter().filter(|n| n.is_folded()).peekable();
        if folded.peek().is_none() {
            return hidden;
        }
        let index = self.rect_index();
        for frame in folded {
            for at in index.query(&frame.rect) {
                let node = &self.nodes[at];
                if node.id != frame.id && contains(&frame.rect, &node.rect) {
                    hidden.insert(node.id.as_str());
                }
            }
        }
        hidden
    }

    /// An index of where the nodes lie, by position in `nodes`.
    pub(crate) fn rect_index(&self) -> RectIndex {
        RectIndex::new(&self.nodes.iter().map(|n| n.rect).collect::<Vec<_>>())
    }

    /// On the board of all tests, puts the tests `tests` (relative to the project, in sidebar
    /// order) that aren't on it yet into the frame of their folder, below its cards; a folder
    /// without a frame gets one right of everything, titled with the folder, or `root_title`
    /// for the project folder. Cards of tests no longer in `tests` go. `false` when nothing
    /// changed.
    pub fn sync_tests(&mut self, tests: &[String], root_title: &str) -> bool {
        if !self.all_tests {
            return false;
        }
        let wanted: HashSet<&str> = tests.iter().map(String::as_str).collect();
        let before = self.nodes.len();
        let gone: Vec<String> = self
            .nodes
            .iter()
            .filter(|n| n.test().is_some_and(|t| !wanted.contains(t)))
            .map(|n| n.id.clone())
            .collect();
        self.remove(&gone);
        let mut changed = self.nodes.len() != before;

        let placed: HashSet<&str> = self.nodes.iter().filter_map(BoardNode::test).collect();
        let missing: Vec<&str> = tests
            .iter()
            .map(String::as_str)
            .filter(|t| !placed.contains(t))
            .collect();
        if !missing.is_empty() {
            self.place_tests(&missing, root_title);
            changed = true;
        }
        changed
    }

    /// Puts a card for each of `tests` into the frame of its folder, in one go: the places are
    /// worked out first, so a board of thousands of tests fills in at once.
    fn place_tests(&mut self, tests: &[&str], root_title: &str) {
        /// A folder's frame while cards are added to it.
        struct Area {
            /// Where the frame already is among the nodes; `None` for one made now.
            at: Option<usize>,
            id: String,
            label: String,
            folder: String,
            rect: Rect,
            /// The lowest edge of what lies in the frame.
            below: f64,
        }

        let mut taken = self.ids();
        let rects: Vec<Rect> = self.nodes.iter().map(|n| n.rect).collect();
        let index = RectIndex::new(&rects);
        let mut frames: HashMap<&str, usize> = HashMap::new();
        for (at, node) in self.nodes.iter().enumerate() {
            if let BoardNodeKind::Group {
                folder: Some(folder),
                ..
            } = &node.kind
            {
                frames.entry(folder).or_insert(at);
            }
        }
        let bounds = self.layout().bounds;
        let top = bounds.map_or(0.0, |b| snap(b.y));
        let mut right = bounds.map(|b| b.x + b.width);

        let mut areas: Vec<Area> = Vec::new();
        let mut area_of: HashMap<&str, usize> = HashMap::new();
        let mut cards: Vec<BoardNode> = Vec::with_capacity(tests.len());
        for &test in tests {
            let folder = folder_of(test);
            let area = *area_of.entry(folder).or_insert_with(|| {
                let area = match frames.get(folder) {
                    Some(&at) => {
                        let rect = rects[at];
                        let below = index
                            .query(&rect)
                            .into_iter()
                            .filter(|&n| n != at && contains(&rect, &rects[n]))
                            .map(|n| rects[n].y + rects[n].height)
                            .fold(rect.y + FRAME_HEADER - PAD, f64::max);
                        let BoardNodeKind::Group { label, .. } = &self.nodes[at].kind else {
                            unreachable!("frames are groups");
                        };
                        Area {
                            at: Some(at),
                            id: self.nodes[at].id.clone(),
                            label: label.clone(),
                            folder: folder.to_owned(),
                            rect,
                            below,
                        }
                    }
                    None => {
                        let x = right.map_or(0.0, |right| snap(right + COLUMN_GAP));
                        let rect = Rect {
                            x,
                            y: top,
                            width: CARD_WIDTH + 2.0 * PAD,
                            height: FRAME_HEADER,
                        };
                        right = Some(x + rect.width);
                        Area {
                            at: None,
                            id: fresh_id(&mut taken, &format!("folder:{folder}")),
                            label: if folder.is_empty() {
                                root_title.to_owned()
                            } else {
                                folder.to_owned()
                            },
                            folder: folder.to_owned(),
                            rect,
                            below: rect.y + FRAME_HEADER - PAD,
                        }
                    }
                };
                areas.push(area);
                areas.len() - 1
            });
            let area = &mut areas[area];
            let card = Rect {
                x: area.rect.x + PAD,
                y: area.below + PAD,
                width: CARD_WIDTH,
                height: CARD_HEIGHT,
            };
            area.rect.height = area
                .rect
                .height
                .max(card.y + card.height + PAD - area.rect.y);
            area.below = card.y + card.height;
            cards.push(BoardNode {
                id: fresh_id(&mut taken, test),
                kind: BoardNodeKind::File {
                    file: test.to_owned(),
                },
                rect: card,
                color: String::new(),
                open: false,
                extra: String::new(),
            });
        }

        // New frames go behind everything, the latest first; the cards on top of everything.
        let mut front = Vec::new();
        for area in areas {
            match area.at {
                Some(at) => self.nodes[at].rect.height = area.rect.height,
                None => front.push(BoardNode {
                    id: area.id,
                    kind: BoardNodeKind::Group {
                        label: area.label,
                        collapsed: false,
                        folder: Some(area.folder),
                    },
                    rect: area.rect,
                    color: String::new(),
                    open: false,
                    extra: String::new(),
                }),
            }
        }
        front.reverse();
        front.append(&mut self.nodes);
        self.nodes = front;
        self.nodes.extend(cards);
    }

    /// Points the nodes that show the file or folder `old` (relative to the project) at `new`
    /// instead, after it was renamed or moved. `false` when no node showed it.
    pub fn rename_file(&mut self, old: &str, new: &str) -> bool {
        let mut changed = false;
        for node in &mut self.nodes {
            match &mut node.kind {
                BoardNodeKind::File { file } => {
                    if let Some(path) = renamed(file, old, new) {
                        *file = path;
                        changed = true;
                    }
                }
                BoardNodeKind::Group {
                    folder: Some(folder),
                    label,
                    ..
                } => {
                    if let Some(path) = renamed(folder, old, new) {
                        if label == folder {
                            label.clone_from(&path);
                        }
                        *folder = path;
                        changed = true;
                    }
                }
                _ => {}
            }
        }
        changed
    }

    /// The ids of the nodes and edges.
    fn ids(&self) -> HashSet<String> {
        self.nodes
            .iter()
            .map(|n| n.id.clone())
            .chain(self.edges.iter().map(|e| e.id.clone()))
            .collect()
    }

    fn new_id(&self, seed: &str) -> String {
        fresh_id(&mut self.ids(), seed)
    }
}

/// An id not in `taken`, which it joins: sixteen hex digits, like the ones Obsidian makes.
fn fresh_id(taken: &mut HashSet<String>, seed: &str) -> String {
    let id = (0u64..)
        .map(|n| {
            let mut hasher = DefaultHasher::new();
            (seed, taken.len(), n).hash(&mut hasher);
            format!("{:016x}", hasher.finish())
        })
        .find(|id| !taken.contains(id))
        .expect("an unused id always exists");
    taken.insert(id.clone());
    id
}

/// `side` when the file names one, otherwise the side of `rect` that faces `other`.
fn side_toward<'a>(side: &'a str, rect: &Rect, other: &Rect) -> &'a str {
    if !side.is_empty() {
        return side;
    }
    let dx = (other.x + other.width / 2.0) - (rect.x + rect.width / 2.0);
    let dy = (other.y + other.height / 2.0) - (rect.y + rect.height / 2.0);
    // Compare in units of each box's size, so wide cards side by side join left to right.
    let (wide, tall) = (
        dx.abs() / (rect.width + other.width),
        dy.abs() / (rect.height + other.height),
    );
    match (wide >= tall, dx >= 0.0, dy >= 0.0) {
        (true, true, _) => "right",
        (true, false, _) => "left",
        (false, _, true) => "bottom",
        (false, _, false) => "top",
    }
}

fn middle_of(rect: &Rect, side: &str) -> Point {
    let (x, y) = match side {
        "top" => (rect.x + rect.width / 2.0, rect.y),
        "bottom" => (rect.x + rect.width / 2.0, rect.y + rect.height),
        "left" => (rect.x, rect.y + rect.height / 2.0),
        _ => (rect.x + rect.width, rect.y + rect.height / 2.0),
    };
    Point { x, y }
}

// MARK: Reading

fn take_list(top: &mut Map<String, Value>, key: &str) -> Result<Vec<Value>, String> {
    match top.remove(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(list)) => Ok(list),
        Some(_) => Err(format!("\"{key}\" must be a list")),
    }
}

fn take_ours(object: &mut Map<String, Value>, what: &str) -> Result<Map<String, Value>, String> {
    match object.remove(OURS) {
        None => Ok(Map::new()),
        Some(Value::Object(ours)) => Ok(ours),
        Some(_) => Err(format!("\"{OURS}\" of {what} must be an object")),
    }
}

fn take_flag(ours: &mut Map<String, Value>, key: &str, what: &str) -> Result<bool, String> {
    match ours.remove(key) {
        None => Ok(false),
        Some(Value::Bool(flag)) => Ok(flag),
        Some(_) => Err(format!("\"{key}\" of {what} must be true or false")),
    }
}

fn no_more(ours: &Map<String, Value>, what: &str) -> Result<(), String> {
    match ours.keys().next() {
        Some(key) => Err(format!(
            "\"{key}\" in \"{OURS}\" of {what} is unknown; it may come from a newer version \
             of JEVision"
        )),
        None => Ok(()),
    }
}

fn take_text(
    object: &mut Map<String, Value>,
    key: &str,
    what: &str,
    required: bool,
) -> Result<String, String> {
    match object.remove(key) {
        Some(Value::String(text)) => Ok(text),
        None if !required => Ok(String::new()),
        None => Err(format!("{what} has no \"{key}\"")),
        Some(_) => Err(format!("\"{key}\" of {what} must be text")),
    }
}

fn take_number(object: &mut Map<String, Value>, key: &str, what: &str) -> Result<f64, String> {
    match object.remove(key).as_ref().and_then(Value::as_f64) {
        Some(n) if n.is_finite() => Ok(n.round()),
        _ => Err(format!("\"{key}\" of {what} must be a number")),
    }
}

fn check_color(color: &str, what: &str) -> Result<(), String> {
    let preset = matches!(color, "" | "1" | "2" | "3" | "4" | "5" | "6");
    let hex = color.len() == 7
        && color.starts_with('#')
        && color[1..].chars().all(|c| c.is_ascii_hexdigit());
    if preset || hex {
        Ok(())
    } else {
        Err(format!(
            "the color \"{color}\" of {what} must be 1 to 6 or a color such as #ff8800"
        ))
    }
}

fn extra(rest: Map<String, Value>) -> String {
    if rest.is_empty() {
        String::new()
    } else {
        Value::Object(rest).to_string()
    }
}

fn extra_map(extra: &str) -> BTreeMap<String, Value> {
    serde_json::from_str::<BTreeMap<String, Value>>(extra).unwrap_or_default()
}

fn parse_node(value: Value, n: usize) -> Result<BoardNode, String> {
    let Value::Object(mut object) = value else {
        return Err(format!("node {} must be an object", n + 1));
    };
    let id = match object.remove("id") {
        Some(Value::String(id)) if !id.trim().is_empty() => id,
        _ => return Err(format!("node {} has no \"id\"", n + 1)),
    };
    let what = format!("node \"{id}\"");
    let kind = take_text(&mut object, "type", &what, true)?;
    let mut ours = take_ours(&mut object, &what)?;
    let mut open = false;
    let kind = match kind.as_str() {
        "text" => BoardNodeKind::Text {
            text: take_text(&mut object, "text", &what, true)?,
        },
        "file" => {
            let file = take_text(&mut object, "file", &what, true)?;
            if file.trim().is_empty() {
                return Err(format!("\"file\" of {what} is empty"));
            }
            open = take_flag(&mut ours, "open", &what)?;
            BoardNodeKind::File { file }
        }
        "link" => BoardNodeKind::Link {
            url: take_text(&mut object, "url", &what, true)?,
        },
        "group" => {
            let label = take_text(&mut object, "label", &what, false)?;
            let collapsed = take_flag(&mut ours, "collapsed", &what)?;
            let folder = match ours.remove("folder") {
                None => None,
                Some(Value::String(folder)) => Some(folder),
                Some(_) => return Err(format!("\"folder\" of {what} must be text")),
            };
            BoardNodeKind::Group {
                label,
                collapsed,
                folder,
            }
        }
        other => {
            return Err(format!(
                "{what} has the type \"{other}\"; a node is text, file, link or group"
            ));
        }
    };
    no_more(&ours, &what)?;
    let rect = Rect {
        x: take_number(&mut object, "x", &what)?,
        y: take_number(&mut object, "y", &what)?,
        width: take_number(&mut object, "width", &what)?,
        height: take_number(&mut object, "height", &what)?,
    };
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return Err(format!("{what} must have a width and a height above 0"));
    }
    let color = take_text(&mut object, "color", &what, false)?;
    check_color(&color, &what)?;
    Ok(BoardNode {
        id,
        kind,
        rect,
        color,
        open,
        extra: extra(object),
    })
}

fn parse_edge(value: Value, n: usize) -> Result<BoardEdge, String> {
    let Value::Object(mut object) = value else {
        return Err(format!("arrow {} must be an object", n + 1));
    };
    let id = match object.remove("id") {
        Some(Value::String(id)) if !id.trim().is_empty() => id,
        _ => return Err(format!("arrow {} has no \"id\"", n + 1)),
    };
    let what = format!("arrow \"{id}\"");
    let mut side = |key: &str| -> Result<String, String> {
        let side = take_text(&mut object, key, &what, false)?;
        if side.is_empty() || SIDES.contains(&side.as_str()) {
            Ok(side)
        } else {
            Err(format!(
                "\"{key}\" of {what} is \"{side}\"; it must be top, right, bottom or left"
            ))
        }
    };
    let (from_side, to_side) = (side("fromSide")?, side("toSide")?);
    let mut end = |key: &str, default: bool| -> Result<bool, String> {
        let end = take_text(&mut object, key, &what, false)?;
        match end.as_str() {
            "" => Ok(default),
            _ if ENDS.contains(&end.as_str()) => Ok(end == "arrow"),
            _ => Err(format!(
                "\"{key}\" of {what} is \"{end}\"; it must be none or arrow"
            )),
        }
    };
    let (from_arrow, to_arrow) = (end("fromEnd", false)?, end("toEnd", true)?);
    let from = take_text(&mut object, "fromNode", &what, true)?;
    let to = take_text(&mut object, "toNode", &what, true)?;
    let label = take_text(&mut object, "label", &what, false)?;
    let color = take_text(&mut object, "color", &what, false)?;
    check_color(&color, &what)?;
    Ok(BoardEdge {
        id,
        from,
        from_side,
        to,
        to_side,
        from_arrow,
        to_arrow,
        label,
        color,
        extra: extra(object),
    })
}

// MARK: Writing

fn json(text: &str) -> String {
    Value::String(text.to_owned()).to_string()
}

/// `"key":value` pairs in the order given, then the kept keys in name order, as one object.
fn line(fields: Vec<(&str, String)>, extra: &str) -> String {
    let pairs = fields
        .into_iter()
        .map(|(key, value)| (json(key), value))
        .chain(
            extra_map(extra)
                .into_iter()
                .map(|(key, value)| (json(&key), value.to_string())),
        )
        .map(|(key, value)| format!("{key}:{value}"))
        .collect::<Vec<_>>();
    format!("{{{}}}", pairs.join(","))
}

fn write_node(node: &BoardNode) -> String {
    let mut fields = vec![("id", json(&node.id))];
    let mut ours = Vec::new();
    match &node.kind {
        BoardNodeKind::Text { text } => {
            fields.push(("type", json("text")));
            fields.push(("text", json(text)));
        }
        BoardNodeKind::File { file } => {
            fields.push(("type", json("file")));
            fields.push(("file", json(file)));
            if node.open {
                ours.push("\"open\":true".to_owned());
            }
        }
        BoardNodeKind::Link { url } => {
            fields.push(("type", json("link")));
            fields.push(("url", json(url)));
        }
        BoardNodeKind::Group {
            label,
            collapsed,
            folder,
        } => {
            fields.push(("type", json("group")));
            if !label.is_empty() {
                fields.push(("label", json(label)));
            }
            if *collapsed {
                ours.push("\"collapsed\":true".to_owned());
            }
            if let Some(folder) = folder {
                ours.push(format!("\"folder\":{}", json(folder)));
            }
        }
    }
    let number = |v: f64| format!("{}", v.round() as i64);
    fields.extend([
        ("x", number(node.rect.x)),
        ("y", number(node.rect.y)),
        ("width", number(node.rect.width)),
        ("height", number(node.rect.height)),
    ]);
    if !node.color.is_empty() {
        fields.push(("color", json(&node.color)));
    }
    if !ours.is_empty() {
        fields.push((OURS, format!("{{{}}}", ours.join(","))));
    }
    line(fields, &node.extra)
}

fn write_edge(edge: &BoardEdge) -> String {
    let mut fields = vec![("id", json(&edge.id)), ("fromNode", json(&edge.from))];
    if !edge.from_side.is_empty() {
        fields.push(("fromSide", json(&edge.from_side)));
    }
    if edge.from_arrow {
        fields.push(("fromEnd", json("arrow")));
    }
    fields.push(("toNode", json(&edge.to)));
    if !edge.to_side.is_empty() {
        fields.push(("toSide", json(&edge.to_side)));
    }
    if !edge.to_arrow {
        fields.push(("toEnd", json("none")));
    }
    if !edge.color.is_empty() {
        fields.push(("color", json(&edge.color)));
    }
    if !edge.label.is_empty() {
        fields.push(("label", json(&edge.label)));
    }
    line(fields, &edge.extra)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A canvas as Obsidian saves one, with keys in its own order, its own fields and float
    /// coordinates.
    const OBSIDIAN: &str = r##"{
	"nodes":[
		{"id":"8a9fe0b1c2d3e4f5","type":"group","x":-420,"y":-300,"width":720,"height":520,"label":"Checkout","background":"assets/bg.png","backgroundStyle":"cover"},
		{"id":"0123456789abcdef","x":-380,"y":-220,"width":250,"height":60,"type":"text","text":"# Plan\n\n- pay **twice**","color":"4"},
		{"id":"fedcba9876543210","type":"file","file":"smoke/login.flow.json","subpath":"#Start","x":-80.4,"y":-220,"width":300,"height":120},
		{"id":"1111222233334444","type":"link","url":"https://jsoncanvas.org","x":-380,"y":-60,"width":400,"height":200,"color":"#ff8800","styleAttributes":{}}
	],
	"edges":[
		{"id":"aaaabbbbccccdddd","fromNode":"0123456789abcdef","fromSide":"right","toNode":"fedcba9876543210","toSide":"left","label":"then"},
		{"id":"eeeeffff00001111","fromNode":"fedcba9876543210","fromEnd":"arrow","toNode":"1111222233334444","toEnd":"none","color":"2"}
	],
	"metadata":{"version":"1.0-1.0"}
}"##;

    #[test]
    fn reads_a_canvas_from_obsidian_and_keeps_what_it_doesnt_use() {
        let board = Board::parse(OBSIDIAN).unwrap();
        assert!(!board.all_tests);
        assert_eq!(board.nodes.len(), 4);
        assert_eq!(
            board.nodes[0].kind,
            BoardNodeKind::Group {
                label: "Checkout".into(),
                collapsed: false,
                folder: None
            }
        );
        assert!(board.nodes[0].extra.contains("backgroundStyle"));
        assert_eq!(
            board.nodes[1].kind,
            BoardNodeKind::Text {
                text: "# Plan\n\n- pay **twice**".into()
            }
        );
        assert_eq!(board.nodes[1].color, "4");
        assert_eq!(board.nodes[2].test(), Some("smoke/login.flow.json"));
        assert_eq!(board.nodes[2].rect.x, -80.0, "whole points");
        assert_eq!(board.edges[0].from_side, "right");
        assert_eq!(board.edges[0].label, "then");
        assert!(!board.edges[0].from_arrow && board.edges[0].to_arrow);
        assert!(board.edges[1].from_arrow && !board.edges[1].to_arrow);

        // Written back, every key survives, and the canonical text reads back to the same board
        // and the same bytes.
        let text = board.to_json();
        for kept in [
            "\"background\":\"assets/bg.png\"",
            "\"subpath\":\"#Start\"",
            "\"styleAttributes\":{}",
            "\"metadata\":{\"version\":\"1.0-1.0\"}",
            "\"fromEnd\":\"arrow\"",
            "\"toEnd\":\"none\"",
        ] {
            assert!(text.contains(kept), "{kept} in {text}");
        }
        let again = Board::parse(&text).unwrap();
        assert_eq!(again, board);
        assert_eq!(again.to_json(), text);
    }

    #[test]
    fn writes_one_line_per_node_in_spec_order() {
        let mut board = Board {
            all_tests: true,
            ..Board::default()
        };
        assert_eq!(
            board.to_json(),
            "{\n\t\"nodes\":[],\n\t\"edges\":[],\n\t\"jevision\":{\"allTests\":true}\n}\n"
        );
        let note = board.add(
            BoardNodeKind::Text {
                text: "Hi \"there\"".into(),
            },
            Point { x: 130.0, y: 85.0 },
        );
        let frame = board.add(
            BoardNodeKind::Group {
                label: "Pay".into(),
                collapsed: true,
                folder: Some("pay".into()),
            },
            Point { x: 0.0, y: 0.0 },
        );
        board.connect(&frame, &note).unwrap();
        let text = board.to_json();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 10, "{text}");
        assert_eq!(
            lines[2],
            format!(
                "\t\t{{\"id\":\"{frame}\",\"type\":\"group\",\"label\":\"Pay\",\"x\":-240,\"y\":-160,\"width\":480,\"height\":320,\"jevision\":{{\"collapsed\":true,\"folder\":\"pay\"}}}},"
            )
        );
        assert_eq!(
            lines[3],
            format!(
                "\t\t{{\"id\":\"{note}\",\"type\":\"text\",\"text\":\"Hi \\\"there\\\"\",\"x\":0,\"y\":0,\"width\":256,\"height\":160}}"
            )
        );
        assert!(lines[6].starts_with("\t\t{\"id\":\""), "{text}");
        assert!(lines[6].ends_with(&format!("\"fromNode\":\"{frame}\",\"toNode\":\"{note}\"}}")));
        assert_eq!(Board::parse(&text).unwrap(), board);
    }

    #[test]
    fn rejects_broken_boards_with_a_reason() {
        let bad = [
            ("[]", "one JSON object"),
            (r#"{"nodes":{}}"#, "\"nodes\" must be a list"),
            (r#"{"nodes":[{"type":"text"}]}"#, "node 1 has no \"id\""),
            (
                r#"{"nodes":[{"id":"a","type":"shape","x":0,"y":0,"width":1,"height":1}]}"#,
                "the type \"shape\"",
            ),
            (
                r#"{"nodes":[{"id":"a","type":"text","x":0,"y":0,"width":1,"height":1}]}"#,
                "has no \"text\"",
            ),
            (
                r#"{"nodes":[{"id":"a","type":"text","text":"","x":"0","y":0,"width":1,"height":1}]}"#,
                "\"x\" of node \"a\" must be a number",
            ),
            (
                r#"{"nodes":[{"id":"a","type":"text","text":"","x":0,"y":0,"width":0,"height":1}]}"#,
                "above 0",
            ),
            (
                r#"{"nodes":[{"id":"a","type":"file","file":" ","x":0,"y":0,"width":1,"height":1}]}"#,
                "is empty",
            ),
            (
                r#"{"nodes":[{"id":"a","type":"text","text":"","x":0,"y":0,"width":1,"height":1,"color":"red"}]}"#,
                "the color \"red\"",
            ),
            (
                r#"{"nodes":[{"id":"a","type":"text","text":"","x":0,"y":0,"width":1,"height":1},{"id":"a","type":"text","text":"","x":0,"y":0,"width":1,"height":1}]}"#,
                "used more than once",
            ),
            (
                r#"{"nodes":[{"id":"a","type":"text","text":"","x":0,"y":0,"width":1,"height":1}],"edges":[{"id":"e","fromNode":"a","toNode":"b"}]}"#,
                "isn't a node of the board",
            ),
            (
                r#"{"nodes":[{"id":"a","type":"text","text":"","x":0,"y":0,"width":1,"height":1}],"edges":[{"id":"e","fromNode":"a","toNode":"a","fromSide":"up"}]}"#,
                "must be top, right, bottom or left",
            ),
            (
                r#"{"nodes":[{"id":"a","type":"text","text":"","x":0,"y":0,"width":1,"height":1}],"edges":[{"id":"e","fromNode":"a","toNode":"a","toEnd":"dot"}]}"#,
                "must be none or arrow",
            ),
            (
                r#"{"nodes":[{"id":"a","type":"text","text":"","x":0,"y":0,"width":1,"height":1,"jevision":{"collapsed":true}}]}"#,
                "\"collapsed\" in \"jevision\" of node \"a\" is unknown",
            ),
            (r#"{"nodes":[],"jevision":{"zoom":2}}"#, "newer version"),
        ];
        for (text, reason) in bad {
            let error = Board::parse(text).unwrap_err();
            assert!(error.contains(reason), "{text}: {error}");
        }
        assert_eq!(Board::parse("{}").unwrap(), Board::default());
    }

    fn tests(files: &[&str]) -> Vec<String> {
        files.iter().map(|f| (*f).to_owned()).collect()
    }

    fn card<'a>(board: &'a Board, test: &str) -> &'a BoardNode {
        board.nodes.iter().find(|n| n.test() == Some(test)).unwrap()
    }

    fn frame<'a>(board: &'a Board, folder: &str) -> &'a BoardNode {
        board
            .nodes
            .iter()
            .find(
                |n| matches!(&n.kind, BoardNodeKind::Group { folder: Some(f), .. } if f == folder),
            )
            .unwrap()
    }

    #[test]
    fn the_board_of_all_tests_keeps_up_with_the_project() {
        let mut board = Board {
            all_tests: true,
            ..Board::default()
        };
        let files = tests(&["a.flow.json", "smoke/b.flow.json", "smoke/c.flow.json"]);
        assert!(board.sync_tests(&files, "Acme"));
        assert!(!board.sync_tests(&files, "Acme"), "nothing new");
        // A frame per folder, side by side, each around its cards, behind them.
        let (root, smoke) = (frame(&board, ""), frame(&board, "smoke"));
        assert!(matches!(&root.kind, BoardNodeKind::Group { label, .. } if label == "Acme"));
        assert!(matches!(&smoke.kind, BoardNodeKind::Group { label, .. } if label == "smoke"));
        assert_eq!(root.rect.x, 0.0);
        assert_eq!(smoke.rect.x, root.rect.width + COLUMN_GAP);
        assert!(contains(&root.rect, &card(&board, "a.flow.json").rect));
        let (b, c) = (
            card(&board, "smoke/b.flow.json"),
            card(&board, "smoke/c.flow.json"),
        );
        assert!(contains(&smoke.rect, &b.rect) && contains(&smoke.rect, &c.rect));
        assert_eq!(c.rect.y, b.rect.y + CARD_HEIGHT + PAD);
        assert_eq!(
            smoke.rect.y + smoke.rect.height,
            c.rect.y + c.rect.height + PAD
        );
        let last_frame = board
            .nodes
            .iter()
            .rposition(|n| matches!(n.kind, BoardNodeKind::Group { .. }))
            .unwrap();
        assert!(last_frame < board.nodes.iter().position(|n| n.test().is_some()).unwrap());

        // A test moved by hand stays, even out of its frame; a new one goes below the cards
        // left in the frame of its folder; a deleted one goes away with its arrows.
        let a = card(&board, "a.flow.json").id.clone();
        let b = card(&board, "smoke/b.flow.json").id.clone();
        board.connect(&a, &b).unwrap();
        board.move_nodes(std::slice::from_ref(&a), Point { x: 0.0, y: 200.0 }, true);
        let moved = card(&board, "a.flow.json").rect;
        let files = tests(&["a.flow.json", "d.flow.json", "smoke/c.flow.json"]);
        assert!(board.sync_tests(&files, "Acme"));
        assert_eq!(card(&board, "a.flow.json").rect, moved);
        let d = card(&board, "d.flow.json").rect;
        assert!(contains(&frame(&board, "").rect, &d));
        assert_eq!(d.y, FRAME_HEADER);
        assert!(
            board
                .nodes
                .iter()
                .all(|n| n.test() != Some("smoke/b.flow.json"))
        );
        assert!(board.edges.is_empty());

        // Other boards hold only what was put on them.
        let mut other = Board::default();
        assert!(!other.sync_tests(&files, "Acme"));
        assert!(other.nodes.is_empty());
    }

    #[test]
    fn frames_take_along_what_lies_inside_and_fold_it_away() {
        let mut board = Board::default();
        let frame = board.add(
            BoardNodeKind::Group {
                label: String::new(),
                collapsed: false,
                folder: None,
            },
            Point { x: 240.0, y: 160.0 },
        );
        let inside = board.add(
            BoardNodeKind::File {
                file: "a.flow.json".into(),
            },
            Point { x: 200.0, y: 120.0 },
        );
        let outside = board.add(
            BoardNodeKind::Link {
                url: "https://example.com".into(),
            },
            Point {
                x: 1000.0,
                y: 120.0,
            },
        );
        assert_eq!(board.contents(&frame), [inside.as_str()]);
        assert!(
            board.contents(&inside).is_empty(),
            "only frames hold things"
        );
        let at = |board: &Board, id: &str| board.nodes.iter().find(|n| n.id == id).unwrap().rect;
        let (before, far) = (at(&board, &inside), at(&board, &outside));
        board.move_nodes(
            std::slice::from_ref(&frame),
            Point { x: 37.0, y: -9.0 },
            true,
        );
        assert_eq!(at(&board, &frame).x, 32.0, "on the grid");
        assert_eq!(at(&board, &inside).x, before.x + 32.0);
        assert_eq!(at(&board, &inside).y, before.y - 16.0);
        assert_eq!(at(&board, &outside), far);
        board.move_nodes(
            std::slice::from_ref(&outside),
            Point { x: 3.4, y: 0.0 },
            false,
        );
        assert_eq!(at(&board, &outside).x, far.x + 3.0, "off the grid");

        board.connect(&inside, &outside).unwrap();
        assert_eq!(board.connect(&inside, &outside), None, "already there");
        assert_eq!(board.connect(&inside, &inside), None);
        let layout = board.layout();
        assert_eq!(layout.nodes.len(), 3);
        assert_eq!(layout.edges.len(), 1);
        assert_eq!(layout.edges[0].from_side, "right");
        assert_eq!(layout.edges[0].to_side, "left");

        let BoardNodeKind::Group { collapsed, .. } = &mut board.nodes[0].kind else {
            panic!("the frame is drawn first");
        };
        *collapsed = true;
        let folded = board.layout();
        assert_eq!(
            folded
                .nodes
                .iter()
                .map(|n| n.id.as_str())
                .collect::<Vec<_>>(),
            [frame.as_str(), outside.as_str()]
        );
        assert_eq!(folded.nodes[0].rect.height, FRAME_HEADER);
        assert!(folded.edges.is_empty(), "the arrow's start is folded away");
        let bounds = folded.bounds.unwrap();
        assert_eq!(
            bounds.y + bounds.height,
            at(&board, &outside).y + at(&board, &outside).height
        );

        board.remove(std::slice::from_ref(&frame));
        assert_eq!(board.nodes.len(), 2, "what was inside stays");
        board.remove(&[inside]);
        assert!(board.edges.is_empty());
        assert_eq!(Board::default().layout().bounds, None);
    }

    #[test]
    fn renamed_tests_and_folders_stay_on_the_board() {
        let mut board = Board {
            all_tests: true,
            ..Board::default()
        };
        board.sync_tests(
            &tests(&["smoke/a.flow.json", "smoke/deep/b.flow.json"]),
            "P",
        );
        let other = board.add(
            BoardNodeKind::File {
                file: "boards/Other.canvas".into(),
            },
            Point { x: 0.0, y: 0.0 },
        );
        assert!(board.rename_file("smoke/a.flow.json", "smoke/z.flow.json"));
        assert!(board.rename_file("smoke", "checks"));
        assert!(board.rename_file("boards/Other.canvas", "boards/Team.canvas"));
        assert!(!board.rename_file("smok", "x"), "only whole names");
        let files: Vec<&str> = board
            .nodes
            .iter()
            .filter_map(|n| match &n.kind {
                BoardNodeKind::File { file } => Some(file.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            files,
            [
                "checks/z.flow.json",
                "checks/deep/b.flow.json",
                "boards/Team.canvas"
            ]
        );
        assert!(
            matches!(&frame(&board, "checks/deep").kind, BoardNodeKind::Group { label, .. } if label == "checks/deep")
        );
        assert!(board.nodes.iter().any(|n| n.id == other));
        assert!(!board.sync_tests(
            &tests(&["checks/z.flow.json", "checks/deep/b.flow.json"]),
            "P"
        ));
    }

    #[test]
    fn resizing_keeps_to_the_grid_and_a_least_size() {
        let mut board = Board::default();
        let note = board.add(
            BoardNodeKind::Text {
                text: String::new(),
            },
            Point { x: 0.0, y: 0.0 },
        );
        board.resize(
            &note,
            Rect {
                x: 5.0,
                y: 9.0,
                width: 3.0,
                height: 101.0,
            },
        );
        assert_eq!(
            board.nodes[0].rect,
            Rect {
                x: 0.0,
                y: 16.0,
                width: MIN_SIZE,
                height: 96.0
            }
        );
    }

    #[test]
    fn a_frame_sums_up_the_tests_inside_it_folded_or_not() {
        let card = |id: &str, file: &str, x: i64, y: i64| {
            format!(
                r#"{{"id":"{id}","type":"file","file":"{file}","x":{x},"y":{y},"width":224,"height":96}}"#
            )
        };
        let text = format!(
            r#"{{"nodes":[
                {{"id":"outer","type":"group","label":"Outer","x":0,"y":0,"width":1000,"height":600}},
                {{"id":"inner","type":"group","label":"Inner","x":16,"y":64,"width":400,"height":300,
                  "jevision":{{"collapsed":true}}}},
                {a},{b},{again},{c},
                {{"id":"note","type":"text","text":"n","x":500,"y":200,"width":200,"height":80}}
            ],"edges":[]}}"#,
            a = card("a", "x/a.flow.json", 32, 100),
            b = card("b", "x/b.flow.json", 520, 300),
            again = card("again", "x/b.flow.json", 520, 450),
            c = card("c", "y/c.flow.json", 2000, 0),
        );
        let board = Board::parse(&text).unwrap();
        // The folded frame hides its card, yet the frames still count it.
        assert!(board.layout().nodes.iter().all(|n| n.id != "a"));
        assert_eq!(
            board.frame_tests(),
            [
                FrameTests {
                    frame: "outer".into(),
                    tests: vec!["x/a.flow.json".into(), "x/b.flow.json".into()],
                },
                FrameTests {
                    frame: "inner".into(),
                    tests: vec!["x/a.flow.json".into()],
                },
            ]
        );
        assert!(Board::default().frame_tests().is_empty());
    }

    fn texts(board: &Board) -> Vec<(String, Rect)> {
        board
            .nodes
            .iter()
            .map(|n| (format!("{:?}", n.kind), n.rect))
            .collect()
    }

    #[test]
    fn many_tests_land_where_one_by_one_would_put_them() {
        let files: Vec<String> = (0..40)
            .map(|i| format!("{}/t{i}.flow.json", ["", "smoke", "a/b"][i % 3]))
            .collect();
        let empty = || Board {
            all_tests: true,
            ..Board::default()
        };
        let mut together = empty();
        assert!(together.sync_tests(&files, "Acme"));
        let mut one_by_one = empty();
        for n in 1..=files.len() {
            one_by_one.sync_tests(&files[..n], "Acme");
        }
        assert_eq!(texts(&together), texts(&one_by_one));
        let ids: HashSet<&str> = together.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids.len(), together.nodes.len(), "ids are unique");
    }

    #[test]
    fn pictures_are_files_that_keep_their_proportions() {
        assert!(is_image_file("assets/shot.PNG") && is_image_file("a.b/c.heic"));
        assert!(!is_image_file("smoke/a.flow.json") && !is_image_file("png"));
        let mut board = Board::default();
        board.add_image("assets/wide.png", 1920.0, 960.0, Point { x: 0.0, y: 0.0 });
        board.add_image("assets/tiny.png", 40.0, 30.0, Point { x: 0.0, y: 0.0 });
        let (wide, tiny) = (&board.nodes[0], &board.nodes[1]);
        assert_eq!(wide.image(), Some("assets/wide.png"));
        assert_eq!(
            (wide.rect.width, wide.rect.height),
            (IMAGE_SIDE, IMAGE_SIDE / 2.0)
        );
        assert_eq!(
            (tiny.rect.width, tiny.rect.height),
            (MIN_SIZE, 36.0),
            "small ones grow"
        );
        assert!(wide.test().is_none(), "a picture isn't a test");

        // Obsidian writes pictures as file nodes too, so the board still reads in both.
        let again = Board::parse(&board.to_json()).unwrap();
        assert_eq!(again, board);
        assert!(
            board
                .to_json()
                .contains("\"type\":\"file\",\"file\":\"assets/wide.png\"")
        );
    }

    #[test]
    fn an_open_test_grows_to_show_its_blocks_and_folds_back() {
        let mut board = Board {
            all_tests: true,
            ..Board::default()
        };
        board.sync_tests(&tests(&["a.flow.json", "b.flow.json"]), "Acme");
        let note = board.add(
            BoardNodeKind::Text { text: "n".into() },
            Point { x: 0.0, y: 0.0 },
        );
        let a = card(&board, "a.flow.json").id.clone();
        let card_rect = card(&board, "a.flow.json").rect;

        assert!(!board.open_test(&note, (400.0, 300.0)), "only tests open");
        assert!(!board.open_test("nope", (400.0, 300.0)));
        assert!(board.open_test(&a, (400.0, 300.0)));
        assert!(!board.open_test(&a, (400.0, 300.0)), "already open");
        let open = board.nodes.last().unwrap();
        assert!(open.open && open.id == a, "an open test comes to the front");
        assert_eq!((open.rect.x, open.rect.y), (card_rect.x, card_rect.y));
        assert_eq!(
            (open.rect.width, open.rect.height),
            (400.0, OPEN_HEADER + 304.0)
        );

        // It stays open in the file, in our own part of the node, and reads back so.
        let text = board.to_json();
        assert!(text.contains("\"jevision\":{\"open\":true}"), "{text}");
        assert_eq!(Board::parse(&text).unwrap(), board);
        assert!(!card(&board, "b.flow.json").open);

        assert!(board.close_test(&a));
        assert!(!board.close_test(&a));
        let closed = card(&board, "a.flow.json");
        assert_eq!(
            (closed.rect.width, closed.rect.height),
            (CARD_WIDTH, CARD_HEIGHT)
        );
        assert!(!board.to_json().contains("open"));

        // A test too big for the board's limit shows its blocks smaller, in the same shape.
        assert!(board.open_test(&a, (10_000.0, 5_000.0)));
        let huge = card(&board, "a.flow.json").rect;
        assert_eq!(huge.width, OPEN_LIMIT);
        assert_eq!(huge.height, OPEN_HEADER + OPEN_LIMIT / 2.0);
    }

    #[test]
    fn an_open_test_makes_room_in_its_frames_and_gives_it_back() {
        let overlaps = |p: &Rect, q: &Rect| {
            p.x < q.x + q.width
                && q.x < p.x + p.width
                && p.y < q.y + q.height
                && q.y < p.y + p.height
        };
        let place = |board: &mut Board, kind: BoardNodeKind, rect: Rect| {
            let id = board.add(kind, Point { x: 0.0, y: 0.0 });
            board.resize(&id, rect);
            id
        };
        let note = || BoardNodeKind::Text { text: "n".into() };
        let rect = |x: f64, y: f64, width: f64, height: f64| Rect {
            x,
            y,
            width,
            height,
        };
        let mut board = Board {
            all_tests: true,
            ..Board::default()
        };
        let files = ["mail/a.flow.json", "mail/b.flow.json", "pay/c.flow.json"];
        board.sync_tests(&tests(&files), "Acme");
        let mail = frame(&board, "mail").id.clone();
        let team = BoardNodeKind::Group {
            label: "Team".into(),
            collapsed: false,
            folder: None,
        };
        // A frame drawn by hand around the folder's frame, things right of it, below it, wholly
        // above it and wholly left of it.
        let team = place(&mut board, team, rect(-32.0, -48.0, 304.0, 352.0));
        let below = place(&mut board, note(), rect(0.0, 320.0, 256.0, 160.0));
        let left = place(&mut board, note(), rect(-320.0, 320.0, 256.0, 160.0));
        let above = place(&mut board, note(), rect(288.0, -320.0, 256.0, 160.0));
        let a = card(&board, "mail/a.flow.json").id.clone();
        // How the board is to look once the test `id` is folded back: as now, but with the test
        // in front, where opening it brings it.
        let folded = |board: &Board, id: &str| {
            let mut folded = board.clone();
            let at = folded.nodes.iter().position(|n| n.id == id).unwrap();
            let front = folded.nodes.remove(at);
            folded.nodes.push(front);
            folded
        };
        let closed = folded(&board, &a);
        let was = |id: &str| closed.nodes.iter().find(|n| n.id == id).unwrap().rect;
        let now = |board: &Board, id: &str| board.nodes.iter().find(|n| n.id == id).unwrap().rect;

        assert!(board.open_test(&a, (400.0, 300.0)));
        let open = now(&board, &a);
        let (dw, dh) = (400.0 - CARD_WIDTH, OPEN_HEADER + 304.0 - CARD_HEIGHT);
        // Both frames still hold the test, with room to spare, and count it.
        for frame in [&mail, &team] {
            let rect = now(&board, frame);
            assert!(contains(&rect, &open), "{frame}");
            assert_eq!(
                (rect.width, rect.height),
                (was(frame).width + dw, was(frame).height + dh)
            );
            let counted = board.frame_tests().into_iter().find(|f| &f.frame == frame);
            assert!(counted.unwrap().tests.contains(&files[0].to_owned()));
        }
        assert_eq!(
            now(&board, &mail).x + now(&board, &mail).width,
            open.x + open.width + PAD
        );
        // The card below it in its frame, the folder's frame to the right with its card and the
        // note below the hand-drawn frame make way; what lies wholly above or left stays.
        let b = card(&board, "mail/b.flow.json");
        assert_eq!(
            b.rect,
            Rect {
                y: was(&b.id).y + dh,
                ..was(&b.id)
            }
        );
        let pay = frame(&board, "pay");
        let c = card(&board, "pay/c.flow.json");
        for moved in [pay, c] {
            assert_eq!(
                moved.rect,
                Rect {
                    x: was(&moved.id).x + dw,
                    ..was(&moved.id)
                }
            );
        }
        assert_eq!(
            now(&board, &below),
            Rect {
                y: was(&below).y + dh,
                ..was(&below)
            }
        );
        assert_eq!(now(&board, &left), was(&left));
        assert_eq!(now(&board, &above), was(&above));
        for node in board
            .nodes
            .iter()
            .filter(|n| ![&a, &mail, &team].contains(&&n.id))
        {
            assert!(
                !overlaps(&node.rect, &open),
                "{} is under the open test",
                node.id
            );
        }

        assert!(board.close_test(&a));
        assert_eq!(board, closed, "everything back where it was");

        // A frame made bigger by hand, or given more to hold, while the test is open keeps it.
        assert!(board.open_test(&a, (400.0, 300.0)));
        let wider = Rect {
            width: now(&board, &team).width + 160.0,
            ..now(&board, &team)
        };
        board.resize(&team, wider);
        let extra = place(&mut board, note(), rect(256.0, 416.0, 160.0, 96.0));
        assert!(board.close_test(&a));
        assert_eq!(now(&board, &team).width, was(&team).width + 160.0);
        assert!(contains(&now(&board, &mail), &now(&board, &extra)));

        // A test in no frame only grows, as nothing moves out of its way.
        let mut board = Board::default();
        let loose = place(
            &mut board,
            BoardNodeKind::File {
                file: "loose.flow.json".into(),
            },
            rect(0.0, 0.0, CARD_WIDTH, CARD_HEIGHT),
        );
        let near = place(&mut board, note(), rect(240.0, 0.0, 256.0, 160.0));
        let closed = folded(&board, &loose);
        assert!(board.open_test(&loose, (400.0, 300.0)));
        assert_eq!(now(&board, &near), now(&closed, &near));
        assert_eq!(
            now(&board, &loose),
            rect(0.0, 0.0, 400.0, OPEN_HEADER + 304.0)
        );
        assert!(board.close_test(&loose));
        assert_eq!(board, closed);
    }

    #[test]
    fn open_marks_only_belong_to_tests() {
        let group = r#"{"nodes":[{"id":"g","type":"group","x":0,"y":0,"width":160,"height":160,"jevision":{"open":true}}],"edges":[]}"#;
        assert!(Board::parse(group).unwrap_err().contains("\"open\""));
        let card = r#"{"nodes":[{"id":"c","type":"file","file":"a.flow.json","x":0,"y":0,"width":400,"height":300,"jevision":{"open":"yes"}}],"edges":[]}"#;
        assert!(Board::parse(card).unwrap_err().contains("true or false"));
    }

    #[test]
    fn the_blocks_of_an_open_test_fit_under_its_title_bar() {
        let card = Rect {
            x: 100.0,
            y: 200.0,
            width: 400.0,
            height: OPEN_HEADER + 300.0,
        };
        // As big as the card, in the same proportions, centered.
        assert_eq!(
            blocks_area(&card, (400.0, 300.0)),
            Rect {
                x: 100.0,
                y: 200.0 + OPEN_HEADER,
                width: 400.0,
                height: 300.0
            }
        );
        let narrow = blocks_area(&card, (100.0, 300.0));
        assert_eq!((narrow.width, narrow.height), (100.0, 300.0));
        assert_eq!(narrow.x, 100.0 + 150.0, "centered across the card");
        let small = blocks_area(&card, (4000.0, 3000.0));
        assert_eq!((small.width, small.height), (400.0, 300.0), "shown smaller");
    }

    #[test]
    fn the_layout_tells_frames_from_the_rest() {
        let board = Board::parse(OBSIDIAN).unwrap();
        let frames: Vec<bool> = board.layout().nodes.iter().map(|n| n.frame).collect();
        assert_eq!(frames, [true, false, false, false]);
    }
}
