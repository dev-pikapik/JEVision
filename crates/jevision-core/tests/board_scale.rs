//! A board of tens of thousands of nodes must still open, lay out, fill in and answer "what is in
//! view" fast enough to scroll: the time is compared with a smaller board on the same machine and
//! build, never with a fixed budget.

use std::time::{Duration, Instant};

use jevision_core::{Board, BoardIndex, BoardLayout, Rect};

/// How much bigger the large board is than the small one.
const FACTOR: usize = 10;

/// A board of about `n` nodes: frames of ten by ten test cards, every fifth one folded, with an
/// arrow from each card to the next one in its row.
fn big_board(n: usize) -> String {
    let frames = n.div_ceil(101);
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    for f in 0..frames {
        let (fx, fy) = ((f % 20) as i64 * 2700, (f / 20) as i64 * 1500);
        let folded = if f % 5 == 4 {
            r#","jevision":{"collapsed":true}"#
        } else {
            ""
        };
        nodes.push(format!(
            r#"{{"id":"g{f}","type":"group","label":"Frame {f}","x":{fx},"y":{fy},"width":2600,"height":1400{folded}}}"#
        ));
        for c in 0..100 {
            let (x, y) = (
                fx + 16 + (c % 10) as i64 * 256,
                fy + 64 + (c / 10) as i64 * 128,
            );
            nodes.push(format!(
                r#"{{"id":"n{f}-{c}","type":"file","file":"f{f}/t{c}.flow.json","x":{x},"y":{y},"width":224,"height":96}}"#
            ));
            if c % 10 != 9 {
                edges.push(format!(
                    r#"{{"id":"e{f}-{c}","fromNode":"n{f}-{c}","toNode":"n{f}-{}"}}"#,
                    c + 1
                ));
            }
        }
    }
    format!(
        "{{\"nodes\":[\n{}\n],\"edges\":[\n{}\n]}}",
        nodes.join(",\n"),
        edges.join(",\n")
    )
}

/// The fastest of a few runs of `work`, so a busy machine skews the time as little as possible.
fn fastest<T>(mut work: impl FnMut() -> T) -> Duration {
    (0..3)
        .map(|_| {
            let clock = Instant::now();
            std::hint::black_box(work());
            clock.elapsed()
        })
        .min()
        .unwrap()
}

/// What `work` takes on the small and the large board, which must differ by well under the
/// square of `FACTOR`.
fn compare(what: &str, limit: u32, small: Duration, large: Duration) {
    println!("{what}: {small:?} -> {large:?}");
    assert!(
        large < small.max(Duration::from_micros(200)) * limit,
        "{what} took {large:?}, more than {limit} times the {small:?} of the board {FACTOR} times \
         smaller"
    );
}

fn viewports(layout: &BoardLayout) -> Vec<Rect> {
    let bounds = layout.bounds.expect("a board with nodes");
    (0..200)
        .map(|i| {
            let (a, b) = (
                (i * 7919 % 1000) as f64 / 1000.0,
                (i * 104729 % 1000) as f64 / 1000.0,
            );
            Rect {
                x: bounds.x + a * bounds.width,
                y: bounds.y + b * bounds.height,
                width: 1600.0,
                height: 1000.0,
            }
        })
        .collect()
}

#[test]
fn a_board_of_many_thousands_of_nodes_stays_fast() {
    let (small_text, large_text) = (big_board(3_000), big_board(3_000 * FACTOR));
    let (small, large) = (
        Board::parse(&small_text).unwrap(),
        Board::parse(&large_text).unwrap(),
    );
    assert!(large.nodes.len() >= 30_000, "{} nodes", large.nodes.len());
    println!("{} and {} nodes", small.nodes.len(), large.nodes.len());

    let limit = 40;
    compare(
        "parse",
        limit,
        fastest(|| Board::parse(&small_text).unwrap()),
        fastest(|| Board::parse(&large_text).unwrap()),
    );
    compare(
        "lay out",
        limit,
        fastest(|| small.layout()),
        fastest(|| large.layout()),
    );
    compare(
        "write",
        limit,
        fastest(|| small.to_json()),
        fastest(|| large.to_json()),
    );

    compare(
        "frame sums",
        limit,
        fastest(|| small.frame_tests()),
        fastest(|| large.frame_tests()),
    );
    // Folded frames still count the cards they hide: ten by ten in each of the 30000 / 101.
    let sums = large.frame_tests();
    assert!(
        sums.iter().all(|f| f.tests.len() == 100),
        "a frame holds its hundred tests"
    );

    let (small_laid, large_laid) = (small.layout(), large.layout());
    compare(
        "index",
        limit,
        fastest(|| BoardIndex::new(&small_laid)),
        fastest(|| BoardIndex::new(&large_laid)),
    );
    let (small_index, large_index) = (BoardIndex::new(&small_laid), BoardIndex::new(&large_laid));
    let (small_views, large_views) = (viewports(&small_laid), viewports(&large_laid));
    // A view's cost follows what is in it, not how much the board holds.
    let ask = |index: &BoardIndex, views: &[Rect]| {
        views
            .iter()
            .map(|view| index.visible(view).nodes.len())
            .sum::<usize>()
    };
    compare(
        "200 views",
        8,
        fastest(|| ask(&small_index, &small_views)),
        fastest(|| ask(&large_index, &large_views)),
    );

    // The index finds exactly what looking at every node finds.
    for view in large_views.iter().take(20) {
        let seen = large_index.visible(view);
        let touching: Vec<u32> = (0..large_laid.nodes.len() as u32)
            .filter(|&i| {
                let r = large_laid.nodes[i as usize].rect;
                r.x <= view.x + view.width
                    && view.x <= r.x + r.width
                    && r.y <= view.y + view.height
                    && view.y <= r.y + r.height
            })
            .collect();
        assert_eq!(seen.nodes, touching);
        assert!(
            seen.nodes.len() < 400,
            "a view holds a few hundred nodes at most"
        );
    }

    // Folded frames hide their cards: the large board shows fewer nodes than it holds.
    assert!(large_laid.nodes.len() < large.nodes.len());
}

#[test]
fn filling_in_the_board_of_all_tests_stays_fast() {
    let tests = |n: usize| -> Vec<String> {
        (0..n)
            .map(|i| format!("folder{}/test{i}.flow.json", i % 60))
            .collect()
    };
    let empty = || Board::parse(r#"{"nodes":[],"edges":[],"jevision":{"allTests":true}}"#).unwrap();
    let (small_tests, large_tests) = (tests(2_000), tests(2_000 * FACTOR));
    compare(
        "add every test",
        40,
        fastest(|| {
            let mut board = empty();
            assert!(board.sync_tests(&small_tests, "Project"));
            board
        }),
        fastest(|| {
            let mut board = empty();
            assert!(board.sync_tests(&large_tests, "Project"));
            board
        }),
    );

    let filled = |tests: &[String]| {
        let mut board = empty();
        board.sync_tests(tests, "Project");
        board
    };
    let (small_board, large_board) = (filled(&small_tests), filled(&large_tests));
    assert_eq!(large_board.nodes.len(), 20_000 + 60);
    // Looked at again, nothing is missing; and tests that are gone leave their cards at once.
    compare(
        "look again",
        40,
        fastest(|| small_board.clone().sync_tests(&small_tests, "Project")),
        fastest(|| large_board.clone().sync_tests(&large_tests, "Project")),
    );
    assert!(!large_board.clone().sync_tests(&large_tests, "Project"));
    compare(
        "drop half",
        40,
        fastest(|| {
            small_board
                .clone()
                .sync_tests(&small_tests[..1_000], "Project")
        }),
        fastest(|| {
            large_board
                .clone()
                .sync_tests(&large_tests[..10_000], "Project")
        }),
    );
}
