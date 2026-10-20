//! Which rectangles touch a given one, without looking at all of them: what a board of tens of
//! thousands of nodes needs to draw only the part in view.

use crate::board::{BoardLayout, LaidBoardEdge, contains};
use crate::model::Rect;

/// Children per tree node; a handful of cache lines, so a step down the tree is one scan.
const FANOUT: usize = 16;

/// How far an arrow's curve may swing out past its two ends; the app never bends one further.
pub const ARROW_REACH: f64 = 160.0;

/// One tree node: the box of everything below it and where its children are, as a range of the
/// level below (or of the rectangles, for the lowest level).
#[derive(Debug, Clone, Copy)]
struct Cell {
    bounds: Rect,
    start: usize,
    end: usize,
}

/// A packed R-tree over a fixed list of rectangles (sort-tile-recursive): built in one go in
/// `O(n log n)`, answers "what touches this rectangle" in about `O(log n + hits)`.
#[derive(Debug, Clone, Default)]
pub struct RectIndex {
    rects: Vec<Rect>,
    /// The rectangles in tree order, as their positions in the list given to [`RectIndex::new`].
    items: Vec<usize>,
    /// The levels from the one over the rectangles up to the root; the last one is one cell.
    levels: Vec<Vec<Cell>>,
}

impl RectIndex {
    /// Indexes `rects`; answers name them by their position in the slice.
    pub fn new(rects: &[Rect]) -> Self {
        let mut index = RectIndex {
            rects: rects.to_vec(),
            items: Vec::new(),
            levels: Vec::new(),
        };
        if rects.is_empty() {
            return index;
        }
        let (order, mut cells) = pack(rects);
        index.items = order;
        for cell in &mut cells {
            cell.bounds = cover(index.items[cell.start..cell.end].iter().map(|&i| &rects[i]));
        }
        loop {
            let done = cells.len() == 1;
            index.levels.push(cells);
            if done {
                break;
            }
            let below = index.levels.last_mut().expect("just pushed");
            let boxes: Vec<Rect> = below.iter().map(|c| c.bounds).collect();
            let (order, mut parents) = pack(&boxes);
            *below = order.iter().map(|&i| below[i]).collect();
            for cell in &mut parents {
                cell.bounds = cover(below[cell.start..cell.end].iter().map(|c| &c.bounds));
            }
            cells = parents;
        }
        index
    }

    pub fn len(&self) -> usize {
        self.rects.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rects.is_empty()
    }

    /// The positions of the rectangles that overlap or touch `area`, in ascending order.
    pub fn query(&self, area: &Rect) -> Vec<usize> {
        let mut hits = Vec::new();
        let Some(root) = self.levels.last().and_then(|l| l.first()) else {
            return hits;
        };
        if !touches(&root.bounds, area) {
            return hits;
        }
        // (level, cell): the root is the only cell of the top level.
        let mut stack = vec![(self.levels.len() - 1, 0usize)];
        while let Some((level, at)) = stack.pop() {
            let cell = self.levels[level][at];
            if level == 0 {
                hits.extend(
                    self.items[cell.start..cell.end]
                        .iter()
                        .copied()
                        .filter(|&i| touches(&self.rects[i], area)),
                );
            } else {
                stack.extend(
                    (cell.start..cell.end)
                        .filter(|&child| touches(&self.levels[level - 1][child].bounds, area))
                        .map(|child| (level - 1, child)),
                );
            }
        }
        hits.sort_unstable();
        hits
    }

    /// The positions of the rectangles that lie entirely inside `area`, in ascending order.
    pub fn inside(&self, area: &Rect) -> Vec<usize> {
        let mut hits = self.query(area);
        hits.retain(|&i| contains(area, &self.rects[i]));
        hits
    }
}

/// What is in view of a board: positions in the lists of its [`BoardLayout`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BoardVisible {
    /// Nodes, in drawing order.
    pub nodes: Vec<u32>,
    pub edges: Vec<u32>,
}

/// A direction on the board, for moving from node to node with the arrow keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardHeading {
    Up,
    Down,
    Left,
    Right,
}

/// An index over a board's laid-out nodes and arrows, for drawing and hit-testing only what a
/// window shows. Valid for the layout it was made from.
#[derive(Debug, Clone, Default)]
pub struct BoardIndex {
    nodes: RectIndex,
    edges: RectIndex,
    frames: Vec<bool>,
}

impl BoardIndex {
    pub fn new(layout: &BoardLayout) -> Self {
        let nodes: Vec<Rect> = layout.nodes.iter().map(|n| n.rect).collect();
        let edges: Vec<Rect> = layout.edges.iter().map(arrow_area).collect();
        BoardIndex {
            nodes: RectIndex::new(&nodes),
            edges: RectIndex::new(&edges),
            frames: layout.nodes.iter().map(|n| n.frame).collect(),
        }
    }

    /// The nodes and arrows that touch `area`, in drawing order.
    pub fn visible(&self, area: &Rect) -> BoardVisible {
        BoardVisible {
            nodes: positions(self.nodes.query(area)),
            edges: positions(self.edges.query(area)),
        }
    }

    /// The node to go to from node `from` when moving `heading`: the nearest one that lies that
    /// way, with sideways distance counting double so the move stays in its row or column. A
    /// frame around `from` is never the answer. `None` when nothing lies that way.
    pub fn neighbour(&self, from: u32, heading: BoardHeading) -> Option<u32> {
        let origin = self.nodes.rects.get(from as usize)?;
        let (ox, oy) = (
            origin.x + origin.width / 2.0,
            origin.y + origin.height / 2.0,
        );
        self.nodes
            .rects
            .iter()
            .enumerate()
            .filter(|&(at, rect)| {
                at != from as usize && !(self.frames[at] && contains(rect, origin))
            })
            .filter_map(|(at, rect)| {
                let (dx, dy) = (
                    rect.x + rect.width / 2.0 - ox,
                    rect.y + rect.height / 2.0 - oy,
                );
                let (along, across) = match heading {
                    BoardHeading::Right => (dx, dy),
                    BoardHeading::Left => (-dx, dy),
                    BoardHeading::Down => (dy, dx),
                    BoardHeading::Up => (-dy, dx),
                };
                (along > 0.0).then(|| (along + 2.0 * across.abs(), at))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
            .map(|(_, at)| at as u32)
    }
}

fn positions(hits: Vec<usize>) -> Vec<u32> {
    hits.into_iter().map(|i| i as u32).collect()
}

/// Where an arrow can be drawn: between its ends, widened by how far the curve swings out.
fn arrow_area(edge: &LaidBoardEdge) -> Rect {
    let (x, y) = (edge.from.x.min(edge.to.x), edge.from.y.min(edge.to.y));
    Rect {
        x: x - ARROW_REACH,
        y: y - ARROW_REACH,
        width: (edge.from.x - edge.to.x).abs() + 2.0 * ARROW_REACH,
        height: (edge.from.y - edge.to.y).abs() + 2.0 * ARROW_REACH,
    }
}

fn touches(a: &Rect, b: &Rect) -> bool {
    a.x <= b.x + b.width && b.x <= a.x + a.width && a.y <= b.y + b.height && b.y <= a.y + a.height
}

fn cover<'a>(mut rects: impl Iterator<Item = &'a Rect>) -> Rect {
    let first = *rects.next().expect("a cell has children");
    rects.fold(first, |a, b| {
        let (x, y) = (a.x.min(b.x), a.y.min(b.y));
        Rect {
            x,
            y,
            width: (a.x + a.width).max(b.x + b.width) - x,
            height: (a.y + a.height).max(b.y + b.height) - y,
        }
    })
}

/// Sort-tile-recursive packing of `boxes` into cells of up to [`FANOUT`]: the boxes ordered by
/// column and then by row within it, and the cells over runs of that order. The cells' bounds
/// are left for the caller to fill in.
fn pack(boxes: &[Rect]) -> (Vec<usize>, Vec<Cell>) {
    let centre_x = |i: usize| boxes[i].x + boxes[i].width / 2.0;
    let centre_y = |i: usize| boxes[i].y + boxes[i].height / 2.0;
    let mut order: Vec<usize> = (0..boxes.len()).collect();
    let cells = boxes.len().div_ceil(FANOUT);
    let columns = (cells as f64).sqrt().ceil() as usize;
    let per_column = columns.max(1) * FANOUT;
    order.sort_unstable_by(|&a, &b| centre_x(a).total_cmp(&centre_x(b)).then(a.cmp(&b)));
    for column in order.chunks_mut(per_column) {
        column.sort_unstable_by(|&a, &b| centre_y(a).total_cmp(&centre_y(b)).then(a.cmp(&b)));
    }
    let cells = (0..cells)
        .map(|n| Cell {
            bounds: boxes[order[n * FANOUT]],
            start: n * FANOUT,
            end: ((n + 1) * FANOUT).min(boxes.len()),
        })
        .collect();
    (order, cells)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixed pseudo-random sequence, so a failure can be repeated.
    fn random(seed: &mut u64) -> f64 {
        *seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (*seed >> 11) as f64 / (1u64 << 53) as f64
    }

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    fn scatter(count: usize, seed: u64) -> Vec<Rect> {
        let mut seed = seed;
        (0..count)
            .map(|_| {
                rect(
                    random(&mut seed) * 5000.0,
                    random(&mut seed) * 5000.0,
                    10.0 + random(&mut seed) * 300.0,
                    10.0 + random(&mut seed) * 200.0,
                )
            })
            .collect()
    }

    fn brute(rects: &[Rect], area: &Rect) -> Vec<usize> {
        (0..rects.len())
            .filter(|&i| touches(&rects[i], area))
            .collect()
    }

    #[test]
    fn finds_what_a_scan_finds() {
        for count in [0, 1, 2, 15, 16, 17, 255, 256, 257, 1000, 4097] {
            let rects = scatter(count, count as u64 + 1);
            let index = RectIndex::new(&rects);
            assert_eq!(index.len(), count);
            let mut seed = 99;
            for _ in 0..60 {
                let area = rect(
                    random(&mut seed) * 5000.0 - 200.0,
                    random(&mut seed) * 5000.0 - 200.0,
                    random(&mut seed) * 1500.0,
                    random(&mut seed) * 1500.0,
                );
                assert_eq!(index.query(&area), brute(&rects, &area), "{count} rects");
            }
            let all = rect(-1e9, -1e9, 2e9, 2e9);
            assert_eq!(index.query(&all).len(), count);
        }
    }

    #[test]
    fn touching_edges_count_and_far_areas_find_nothing() {
        let index = RectIndex::new(&[rect(0.0, 0.0, 10.0, 10.0)]);
        assert_eq!(index.query(&rect(10.0, 0.0, 5.0, 5.0)), [0]);
        assert_eq!(index.query(&rect(5.0, 5.0, 0.0, 0.0)), [0]);
        assert!(index.query(&rect(10.5, 0.0, 5.0, 5.0)).is_empty());
        assert!(
            RectIndex::default()
                .query(&rect(0.0, 0.0, 9.0, 9.0))
                .is_empty()
        );
    }

    #[test]
    fn inside_keeps_only_what_fits_entirely() {
        let rects = [
            rect(0.0, 0.0, 10.0, 10.0),
            rect(5.0, 5.0, 10.0, 10.0),
            rect(2.0, 2.0, 3.0, 3.0),
        ];
        let index = RectIndex::new(&rects);
        assert_eq!(index.inside(&rect(0.0, 0.0, 12.0, 12.0)), [0, 2]);
        assert_eq!(index.inside(&rect(0.0, 0.0, 20.0, 20.0)), [0, 1, 2]);
    }

    #[test]
    fn arrow_keys_walk_to_the_node_in_that_direction() {
        use crate::board::LaidNode;
        let at = |id: &str, x: f64, y: f64, frame: bool| LaidNode {
            id: id.into(),
            rect: rect(x, y, 100.0, 50.0),
            frame,
        };
        let layout = BoardLayout {
            nodes: vec![
                LaidNode {
                    id: "frame".into(),
                    rect: rect(-20.0, -20.0, 150.0, 200.0),
                    frame: true,
                },
                at("a", 0.0, 0.0, false),
                at("b", 200.0, 0.0, false),
                at("c", 200.0, 300.0, false),
                at("d", 0.0, 100.0, false),
                at("far", 10_000.0, 10.0, false),
            ],
            edges: Vec::new(),
            bounds: None,
        };
        let index = BoardIndex::new(&layout);
        let go = |from: u32, heading| index.neighbour(from, heading);
        assert_eq!(go(1, BoardHeading::Right), Some(2));
        assert_eq!(
            go(1, BoardHeading::Down),
            Some(4),
            "the one below, not the farther one"
        );
        assert_eq!(go(2, BoardHeading::Down), Some(3));
        assert_eq!(go(4, BoardHeading::Up), Some(1));
        assert_eq!(go(2, BoardHeading::Left), Some(1));
        assert_eq!(
            go(1, BoardHeading::Left),
            None,
            "the frame around it doesn't count"
        );
        assert_eq!(go(1, BoardHeading::Up), None);
        assert_eq!(
            go(2, BoardHeading::Right),
            Some(5),
            "even a far one, if it is the only one"
        );
        assert_eq!(go(99, BoardHeading::Up), None, "no such node");
    }

    #[test]
    fn a_board_index_tells_what_is_in_view() {
        use crate::board::{Board, BoardNodeKind};
        use crate::model::Point;
        let mut board = Board::default();
        let near = board.add(
            BoardNodeKind::Text { text: "a".into() },
            Point { x: 100.0, y: 100.0 },
        );
        let far = board.add(
            BoardNodeKind::Text { text: "b".into() },
            Point {
                x: 90_000.0,
                y: 100.0,
            },
        );
        board.connect(&near, &far).expect("arrow");
        let layout = board.layout();
        let index = BoardIndex::new(&layout);
        let near_view = index.visible(&rect(0.0, 0.0, 400.0, 400.0));
        assert_eq!(near_view.nodes, [0]);
        assert_eq!(
            near_view.edges,
            [0],
            "the arrow leaves the view but starts in it"
        );
        let along = index.visible(&rect(40_000.0, 0.0, 400.0, 400.0));
        assert!(along.nodes.is_empty());
        assert_eq!(along.edges, [0], "the arrow runs through this view");
        let beside = index.visible(&rect(40_000.0, 5_000.0, 400.0, 400.0));
        assert!(beside.nodes.is_empty() && beside.edges.is_empty());
        assert_eq!(index.visible(&rect(-1e9, -1e9, 2e9, 2e9)).nodes, [0, 1]);
    }
}
