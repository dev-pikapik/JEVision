//! Arrows between boxes laid out by someone else, such as the cards of the project map: a path
//! of straight runs that goes around every other box instead of through it.

use crate::model::{Point, Rect};

/// How far a turn sits from a box's side when nothing is close by.
const LANE: f64 = 30.0;
/// How far a run along a box's top or bottom keeps from it.
const ALONG: f64 = 6.0;
/// How close a run may pass by a box other than the two it joins.
const CLEARANCE: f64 = 4.0;
/// What a turn costs, in points of length, when choosing between paths.
const BEND: f64 = 24.0;

/// A path from a side of `from` to a side of `to` (y grows downwards) that keeps clear of
/// `obstacles`: from the right side to the left one when `to` is to the right, the other way
/// round when it's to the left, and from right side to right side when the two share columns.
/// Turns sit halfway between neighbouring boxes; the path with the fewest turns and the
/// shortest length wins. Without a clear path it goes over every box.
pub fn route_around(from: &Rect, to: &Rect, obstacles: &[Rect]) -> Vec<Point> {
    let right = |r: &Rect| r.x + r.width;
    let bottom = |r: &Rect| r.y + r.height;
    let (from_y, to_y) = (from.y + from.height / 2.0, to.y + to.height / 2.0);
    // Free room right of `edge` up to the nearest box, and left of it.
    let room_right = |edge: f64| {
        obstacles
            .iter()
            .chain([from, to])
            .map(|r| r.x)
            .filter(|&x| x >= edge)
            .fold(edge + 2.0 * LANE, f64::min)
    };
    let room_left = |edge: f64| {
        obstacles
            .iter()
            .chain([from, to])
            .map(right)
            .filter(|&x| x <= edge)
            .fold(edge - 2.0 * LANE, f64::max)
    };
    let (start, end, lanes) = if to.x >= right(from) {
        let out = (right(from) + room_right(right(from)).min(to.x)) / 2.0;
        let into = (to.x + room_left(to.x).max(right(from))) / 2.0;
        (pt(right(from), from_y), pt(to.x, to_y), (out, into))
    } else if right(to) <= from.x {
        let out = (from.x + room_left(from.x).max(right(to))) / 2.0;
        let into = (right(to) + room_right(right(to)).min(from.x)) / 2.0;
        (pt(from.x, from_y), pt(right(to), to_y), (out, into))
    } else {
        let edge = right(from).max(right(to));
        let lane = (edge + room_right(edge)) / 2.0;
        (pt(right(from), from_y), pt(right(to), to_y), (lane, lane))
    };
    let (out, into) = lanes;
    let through = |y: f64| {
        vec![
            start,
            pt(out, start.y),
            pt(out, y),
            pt(into, y),
            pt(into, end.y),
            end,
        ]
    };
    let mut candidates = vec![
        vec![start, pt(out, start.y), pt(out, end.y), end],
        vec![start, pt(into, start.y), pt(into, end.y), end],
    ];
    for r in obstacles.iter().chain([from, to]) {
        candidates.push(through(r.y - ALONG));
        candidates.push(through(bottom(r) + ALONG));
    }
    let clear = |path: &[Point]| {
        path.windows(2).all(|run| {
            obstacles
                .iter()
                .all(|r| !crosses(run[0], run[1], r, CLEARANCE))
                && [from, to].iter().all(|r| !crosses(run[0], run[1], r, 0.0))
        })
    };
    let over = obstacles
        .iter()
        .chain([from, to])
        .map(|r| r.y)
        .fold(f64::INFINITY, f64::min)
        - LANE;
    // Checking a path against every box is the costly part, so paths are checked cheapest
    // first and the first clear one wins; the sort is stable, so ties keep their order.
    let mut candidates: Vec<(f64, Vec<Point>)> = candidates
        .into_iter()
        .map(|path| {
            let path = straighten(path);
            (cost(&path), path)
        })
        .collect();
    candidates.sort_by(|a, b| a.0.total_cmp(&b.0));
    candidates
        .into_iter()
        .map(|(_, path)| path)
        .find(|path| clear(path))
        .unwrap_or_else(|| straighten(through(over)))
}

fn pt(x: f64, y: f64) -> Point {
    Point { x, y }
}

/// Length plus a price for every turn.
fn cost(path: &[Point]) -> f64 {
    let length: f64 = path
        .windows(2)
        .map(|w| (w[1].x - w[0].x).abs() + (w[1].y - w[0].y).abs())
        .sum();
    length + BEND * path.len().saturating_sub(2) as f64
}

/// Whether the straight run from `a` to `b` goes into `r` grown by `margin` on every side.
/// Touching the edge doesn't count.
fn crosses(a: Point, b: Point, r: &Rect, margin: f64) -> bool {
    let (left, top) = (r.x - margin, r.y - margin);
    let (right, bottom) = (r.x + r.width + margin, r.y + r.height + margin);
    a.x.max(b.x) > left && a.x.min(b.x) < right && a.y.max(b.y) > top && a.y.min(b.y) < bottom
}

/// `points` without repeats and without the middle one of three in a line.
fn straighten(points: Vec<Point>) -> Vec<Point> {
    let mut clean: Vec<Point> = Vec::with_capacity(points.len());
    for p in points {
        if clean.last() == Some(&p) {
            continue;
        }
        if let [.., a, b] = clean[..]
            && ((a.x == b.x && b.x == p.x) || (a.y == b.y && b.y == p.y))
        {
            clean.pop();
        }
        clean.push(p);
    }
    clean
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(x: f64, y: f64) -> Rect {
        Rect {
            x,
            y,
            width: 200.0,
            height: 80.0,
        }
    }

    fn clear_of(path: &[Point], boxes: &[Rect]) -> bool {
        path.windows(2)
            .all(|run| boxes.iter().all(|r| !crosses(run[0], run[1], r, 0.0)))
    }

    #[test]
    fn neighbours_join_side_to_side_with_one_jog() {
        let (a, b) = (card(0.0, 0.0), card(260.0, 100.0));
        let path = route_around(&a, &b, &[]);
        assert_eq!(
            path,
            [
                pt(200.0, 40.0),
                pt(230.0, 40.0),
                pt(230.0, 140.0),
                pt(260.0, 140.0)
            ]
        );
        let level = route_around(&a, &card(260.0, 0.0), &[]);
        assert_eq!(level, [pt(200.0, 40.0), pt(260.0, 40.0)]);
    }

    #[test]
    fn a_card_in_the_column_between_is_passed_around() {
        // `smoke` in the first column runs `login` in the third; `greeting` sits in the middle
        // column at the same height.
        let smoke = card(0.0, 0.0);
        let greeting = card(260.0, 0.0);
        let below = card(260.0, 92.0);
        let login = card(520.0, 0.0);
        let path = route_around(&smoke, &login, &[greeting, below]);
        assert!(clear_of(&path, &[greeting, below]), "{path:?}");
        assert_eq!(path.first(), Some(&pt(200.0, 40.0)));
        assert_eq!(path.last(), Some(&pt(520.0, 40.0)));
    }

    #[test]
    fn a_gap_between_cards_is_used_when_it_is_shorter() {
        let middle = [
            card(260.0, 0.0),
            card(260.0, 92.0),
            card(260.0, 184.0),
            card(260.0, 400.0),
        ];
        let path = route_around(&card(0.0, 380.0), &card(520.0, 380.0), &middle);
        assert_eq!(
            path,
            [
                pt(200.0, 420.0),
                pt(230.0, 420.0),
                pt(230.0, 394.0),
                pt(490.0, 394.0),
                pt(490.0, 420.0),
                pt(520.0, 420.0),
            ]
        );
    }

    #[test]
    fn cards_in_one_column_join_around_the_right() {
        let (a, between, b) = (card(0.0, 0.0), card(0.0, 92.0), card(0.0, 184.0));
        let path = route_around(&a, &b, &[between, card(260.0, 0.0)]);
        assert_eq!(
            path,
            [
                pt(200.0, 40.0),
                pt(230.0, 40.0),
                pt(230.0, 224.0),
                pt(200.0, 224.0)
            ]
        );
        let back = route_around(&b, &a, &[between]);
        assert_eq!(back.first(), Some(&pt(200.0, 224.0)));
        assert!(clear_of(&back, &[between]));
    }

    #[test]
    fn leftward_links_leave_from_the_left() {
        let (a, b) = (card(520.0, 0.0), card(0.0, 0.0));
        let middle = card(260.0, 0.0);
        let path = route_around(&a, &b, &[middle]);
        assert_eq!(path.first(), Some(&pt(520.0, 40.0)));
        assert_eq!(path.last(), Some(&pt(200.0, 40.0)));
        assert!(clear_of(&path, &[middle]), "{path:?}");
    }
}
