//! Boards: an endless canvas with the project's tests, frames, notes, links and other boards on
//! it. A board is a `*.canvas` file in the project's `boards` folder, in the open JSON Canvas
//! 1.0 format (jsoncanvas.org), so Obsidian opens it too: a test is a `file` node that points at
//! its `*.flow.json`, another board a `file` node that points at its `*.canvas`, a note a `text`
//! node in Markdown, a link a `link` node and a frame a `group` node. What only JEVision uses
//! sits under a `jevision` key; keys this build doesn't know are kept as they were. Files are
//! written in one canonical form, a line per node and edge, so a change shows as a small diff.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};

use serde_json::{Map, Value};

use crate::layout::GRID;
use crate::model::{Point, Rect};

/// Folder of the project that holds its boards.
pub const BOARDS_FOLDER: &str = "boards";
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

fn contains(outer: &Rect, inner: &Rect) -> bool {
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
        for (n, edge) in edges.into_iter().enumerate() {
            let edge = parse_edge(edge, n)?;
            for end in [&edge.from, &edge.to] {
                if !board.nodes.iter().any(|node| &node.id == end) {
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
        let (width, height) = BoardNode::size(&kind);
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
            extra: String::new(),
        };
        if matches!(node.kind, BoardNodeKind::Group { .. }) {
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
        let mut moved: Vec<String> = Vec::new();
        for id in ids {
            for id in std::iter::once(id.clone()).chain(self.contents(id)) {
                if !moved.contains(&id) {
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
        self.nodes.retain(|n| !ids.contains(&n.id));
        self.edges
            .retain(|e| !ids.contains(&e.from) && !ids.contains(&e.to));
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
        let mut hidden: HashSet<&str> = HashSet::new();
        for frame in self.nodes.iter().filter(|n| n.is_folded()) {
            for node in &self.nodes {
                if node.id != frame.id && contains(&frame.rect, &node.rect) {
                    hidden.insert(&node.id);
                }
            }
        }
        let nodes: Vec<LaidNode> = self
            .nodes
            .iter()
            .filter(|n| !hidden.contains(n.id.as_str()))
            .map(|n| LaidNode {
                id: n.id.clone(),
                rect: n.shown(),
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

        let placed: HashSet<String> = self
            .nodes
            .iter()
            .filter_map(|n| n.test().map(str::to_owned))
            .collect();
        for test in tests.iter().filter(|t| !placed.contains(t.as_str())) {
            self.place_test(test, root_title);
            changed = true;
        }
        changed
    }

    fn place_test(&mut self, test: &str, root_title: &str) {
        let folder = folder_of(test);
        let frame = self.nodes.iter().position(
            |n| matches!(&n.kind, BoardNodeKind::Group { folder: Some(f), .. } if f == folder),
        );
        let frame = match frame {
            Some(i) => i,
            None => {
                let (x, y) = match self.layout().bounds {
                    Some(b) => (snap(b.x + b.width + COLUMN_GAP), snap(b.y)),
                    None => (0.0, 0.0),
                };
                let label = if folder.is_empty() {
                    root_title.to_owned()
                } else {
                    folder.to_owned()
                };
                let id = self.new_id(&format!("folder:{folder}"));
                self.nodes.insert(
                    0,
                    BoardNode {
                        id,
                        kind: BoardNodeKind::Group {
                            label,
                            collapsed: false,
                            folder: Some(folder.to_owned()),
                        },
                        rect: Rect {
                            x,
                            y,
                            width: CARD_WIDTH + 2.0 * PAD,
                            height: FRAME_HEADER,
                        },
                        color: String::new(),
                        extra: String::new(),
                    },
                );
                0
            }
        };
        let area = self.nodes[frame].rect;
        let below = self
            .nodes
            .iter()
            .filter(|n| n.id != self.nodes[frame].id && contains(&area, &n.rect))
            .map(|n| n.rect.y + n.rect.height)
            .fold(area.y + FRAME_HEADER - PAD, f64::max);
        let card = Rect {
            x: area.x + PAD,
            y: below + PAD,
            width: CARD_WIDTH,
            height: CARD_HEIGHT,
        };
        let rect = &mut self.nodes[frame].rect;
        rect.height = rect.height.max(card.y + card.height + PAD - rect.y);
        let id = self.new_id(test);
        self.nodes.push(BoardNode {
            id,
            kind: BoardNodeKind::File {
                file: test.to_owned(),
            },
            rect: card,
            color: String::new(),
            extra: String::new(),
        });
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

    /// An id no node or edge has: sixteen hex digits, like the ones Obsidian makes.
    fn new_id(&self, seed: &str) -> String {
        let taken: HashSet<&str> = self
            .nodes
            .iter()
            .map(|n| n.id.as_str())
            .chain(self.edges.iter().map(|e| e.id.as_str()))
            .collect();
        (0u64..)
            .map(|n| {
                let mut hasher = DefaultHasher::new();
                (seed, taken.len(), n).hash(&mut hasher);
                format!("{:016x}", hasher.finish())
            })
            .find(|id| !taken.contains(id.as_str()))
            .expect("an unused id always exists")
    }
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
    let kind = match kind.as_str() {
        "text" => BoardNodeKind::Text {
            text: take_text(&mut object, "text", &what, true)?,
        },
        "file" => {
            let file = take_text(&mut object, "file", &what, true)?;
            if file.trim().is_empty() {
                return Err(format!("\"file\" of {what} is empty"));
            }
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
}
