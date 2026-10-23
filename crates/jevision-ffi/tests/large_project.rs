//! A generated project as large as real ones get, and an empty one: the paths the app and the
//! command line take through the core must stay fast on both. Bounds are generous so a slow CI
//! machine running a debug build passes; a quadratic slip blows far past them.
//!
//! How long a step takes is for a quiet machine to say: with other programs on the cores the same
//! step takes twice as long. So the bounds are held only by `a_large_project_stays_fast`, which
//! `cargo test` leaves out and CI runs alone (`cargo test -p jevision-ffi --test large_project --
//! --ignored a_large_project_stays_fast`); the test every run does goes through the same steps and
//! checks what they answer, with no limit on the time.

use std::cell::Cell;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use jevision_core::{Board, BoardNodeKind, Point};
use jevision_core::{Flow, Rect, SearchCandidate, Slot};
use jevision_ffi::{
    SearchIndex, add_to_board, board_with_contents, flaky_tests, lay_out_board, layout_flow,
    list_runs, load_board, load_flow, map_link_route, move_on_board, project_map, rank_search,
    remove_block, save_board, save_flow, sync_board_tests, validate_flow_in,
};
use serde_json::{Map, Value, json};

const TESTS: usize = 500;
/// Every this many tests, one has hundreds of blocks.
const LARGE_EVERY: usize = 25;
const RUNS_PER_TEST: usize = 2;
const FOLDERS: usize = 10;

/// A fresh folder for one test of this file.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("jevision-large-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Writes a project of [`TESTS`] tests in [`FOLDERS`] folders plus a shared one, with data
/// tables and saved runs, and returns the test files relative to `root`.
fn write_project(root: &Path) -> Vec<String> {
    fs::create_dir_all(root.join("shared")).unwrap();
    let login = test_json("Log in", 0, 2, false);
    fs::write(root.join("shared/login.flow.json"), login).unwrap();
    let mut files = vec!["shared/login.flow.json".to_owned()];
    for folder in 0..FOLDERS {
        fs::create_dir_all(root.join(format!("area-{folder}/data"))).unwrap();
        fs::write(
            root.join(format!("area-{folder}/data/users.csv")),
            "name,password\nanna,one\nboris,two\n",
        )
        .unwrap();
    }
    for n in 1..TESTS {
        let large = n % LARGE_EVERY == 0;
        let segments = if large { 80 } else { 3 + n % 8 };
        let file = format!("area-{}/test-{n:03}.flow.json", n % FOLDERS);
        let text = test_json(
            &format!("Test number {n}"),
            n,
            segments,
            large && n % 2 == 0,
        );
        fs::write(root.join(&file), text).unwrap();
        files.push(file);
    }
    for (n, file) in files.iter().enumerate() {
        for run in 0..RUNS_PER_TEST {
            write_run(root, file, n, run);
        }
    }
    files
}

/// A test of `segments` pieces in a row: plain steps, an `if` with several exits, a loop, a
/// data table loop, a sub-test and a condition inside a loop. With `pinned`, every block has
/// a place on the canvas, as after arranging by hand.
fn test_json(name: &str, seed: usize, segments: usize, pinned: bool) -> String {
    let mut blocks = Map::new();
    let mut next: Option<String> = None;
    for s in (0..segments).rev() {
        next = Some(segment(&mut blocks, s, (seed + s) % 6, next));
    }
    let mut flow = json!({
        "format": 1,
        "name": name,
        "app": "com.example.App",
        "tags": ["generated"],
        "start": next.unwrap_or_default(),
        "blocks": blocks,
    });
    if pinned {
        let positions: Map<String, Value> = flow["blocks"]
            .as_object()
            .unwrap()
            .keys()
            .enumerate()
            .map(|(i, id)| {
                (
                    id.clone(),
                    json!({"x": (i % 7) as f64 * 260.0, "y": (i / 7) as f64 * 120.0}),
                )
            })
            .collect();
        flow["canvas"] = json!({ "positions": positions });
    }
    jevision_core::canonicalize(&flow.to_string()).unwrap()
}

/// Adds segment `s` of kind `kind` leading to `next` and returns its first block.
fn segment(blocks: &mut Map<String, Value>, s: usize, kind: usize, next: Option<String>) -> String {
    let id = |part: &str| format!("s{s}-{part}");
    let mut add = |part: &str, mut block: Value, to: Option<&str>| {
        if let Some(to) = to {
            block["next"] = json!(to);
        }
        blocks.insert(id(part), block);
        id(part)
    };
    let click =
        || json!({"type": "click", "target": {"point": {"x": 0.5, "y": 0.5}}, "button": "left"});
    let typing = |text: &str| json!({"type": "type", "text": text});
    let wait = || json!({"type": "waitFor", "until": {"appears": {"image": "refs/ready.png"}}, "timeout": "5s"});
    let next = next.as_deref();
    match kind {
        0 => {
            let keys = add("keys", json!({"type": "pressKeys", "keys": "cmd+s"}), next);
            let text = add("type", typing("hello"), Some(&keys));
            add("click", click(), Some(&text))
        }
        1 => {
            let mut branch = |name: &str| {
                let w = add(&format!("{name}-wait"), wait(), next);
                add(&format!("{name}-click"), click(), Some(&w))
            };
            let then = branch("then");
            let cases: Vec<Value> = (0..3)
                .map(|c| json!({"reference": {"image": format!("refs/case-{c}.png")}, "then": branch(&format!("case{c}"))}))
                .collect();
            let otherwise = branch("else");
            let block = json!({"type": "if", "reference": {"image": "refs/then.png"}, "then": then, "elseIf": cases, "else": otherwise});
            add("if", block, None)
        }
        2 => {
            let text = add("body-type", typing("again"), None);
            let body = add("body-click", click(), Some(&text));
            add(
                "repeat",
                json!({"type": "repeat", "times": 3, "body": body}),
                next,
            )
        }
        3 => {
            let tap = add("row-click", click(), None);
            let body = add("row-type", typing("${data.name}"), Some(&tap));
            add(
                "data",
                json!({"type": "useData", "file": "data/users.csv", "body": body}),
                next,
            )
        }
        4 => add(
            "login",
            json!({"type": "runTest", "test": "../shared/login.flow.json"}),
            next,
        ),
        _ => {
            let yes = add("inner-click", click(), None);
            let no = add("inner-type", typing("no"), None);
            let check = add(
                "inner-if",
                json!({"type": "if", "reference": {"image": "refs/inner.png"}, "then": yes, "else": no}),
                None,
            );
            add(
                "loop",
                json!({"type": "repeat", "times": 2, "body": check}),
                next,
            )
        }
    }
}

/// Saves run `run` of test `file`, the `n`th test, as the runner would.
fn write_run(root: &Path, file: &str, n: usize, run: usize) {
    let dir = root.join(format!(
        ".jevision/runs/2026-10-{:02}T10-00-{n:03}-{run}",
        1 + run
    ));
    fs::create_dir_all(&dir).unwrap();
    let steps: Vec<Value> = (0..20)
        .map(|seq| json!({"seq": seq, "test": file, "block": format!("s{seq}-click"), "type": "click", "status": "passed", "durationMs": 40}))
        .collect();
    let verdict = if (n + run) % 7 == 0 {
        "failed"
    } else {
        "passed"
    };
    let report = json!({
        "format": 1, "test": format!("Test number {n}"), "file": file, "app": "com.example.App",
        "startedAt": format!("2026-10-{:02}T10:00:{:02}.000Z", 1 + run, n % 60), "durationMs": 1200,
        "verdict": verdict, "flaky": false, "fileHash": "abc",
        "attempts": [{"number": 1, "verdict": verdict, "durationMs": 1200, "steps": steps}],
    });
    fs::write(dir.join("report.json"), report.to_string()).unwrap();
}

thread_local! {
    /// Whether the test running on this thread holds its steps to their limits.
    static BOUNDED: Cell<bool> = const { Cell::new(false) };
}

/// Runs `work`, prints how long it took and, in the test that holds the limits, fails when that
/// is over `limit`.
fn timed<T>(what: &str, limit: Duration, work: impl FnOnce() -> T) -> T {
    let clock = Instant::now();
    let out = work();
    let took = clock.elapsed();
    println!("{what}: {took:?}");
    assert!(
        !BOUNDED.get() || took < limit,
        "{what} took {took:?}, more than {limit:?}"
    );
    out
}

fn read_flow(root: &Path, file: &str) -> Flow {
    load_flow(fs::read_to_string(root.join(file)).unwrap()).unwrap()
}

fn folder_of(root: &Path, file: &str) -> String {
    root.join(file)
        .parent()
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

/// What every step of a large project answers, with no limit on the time.
#[test]
fn a_large_project_gives_the_right_answers() {
    a_large_project(false);
}

/// The same steps held to their limits. Time is for a quiet machine to say, so this one is left
/// out of `cargo test` and run alone.
#[test]
#[ignore = "measures time, which a busy machine decides: run it alone, as CI does"]
fn a_large_project_stays_fast() {
    a_large_project(true);
}

fn a_large_project(bounded: bool) {
    BOUNDED.set(bounded);
    let root = scratch(if bounded { "bounded" } else { "project" });
    let files = timed("generate", Duration::from_secs(20), || write_project(&root));
    let path = root.to_string_lossy().into_owned();

    let flows: Vec<Flow> = timed("load every test", Duration::from_secs(2), || {
        files.iter().map(|f| read_flow(&root, f)).collect()
    });
    let largest = flows.iter().map(|f| f.blocks.len()).max().unwrap();
    assert!(largest >= 300, "the largest test has {largest} blocks");

    timed("validate every test", Duration::from_secs(2), || {
        for (file, flow) in files.iter().zip(&flows) {
            validate_flow_in(flow.clone(), folder_of(&root, file));
        }
    });
    timed("format every test", Duration::from_secs(2), || {
        for (file, flow) in files.iter().zip(&flows) {
            assert_eq!(
                save_flow(flow.clone()),
                fs::read_to_string(root.join(file)).unwrap(),
                "{file}"
            );
        }
    });
    timed("lay out every test", Duration::from_secs(2), || {
        for flow in &flows {
            layout_flow(flow.clone());
        }
    });

    let map = timed("project map", Duration::from_secs(3), || {
        project_map(path.clone(), files.clone())
    });
    assert_eq!(map.len(), TESTS);
    assert!(map.iter().all(|t| t.readable));

    // The map's cards: one column per folder, an arrow from every test that runs another.
    let cards: Vec<Rect> = (0..files.len())
        .map(|i| Rect {
            x: (i % (FOLDERS + 1)) as f64 * 260.0,
            y: (i / (FOLDERS + 1)) as f64 * 110.0,
            width: 208.0,
            height: 90.0,
        })
        .collect();
    let links: Vec<(usize, usize)> = map
        .iter()
        .enumerate()
        .filter(|(_, t)| !t.uses.is_empty())
        .map(|(i, _)| (i, 0))
        .collect();
    assert!(links.len() > 100, "{} links", links.len());
    timed("route every map arrow", Duration::from_secs(2), || {
        for &(from, to) in &links {
            let others: Vec<Rect> = (0..cards.len())
                .filter(|&i| i != from && i != to)
                .map(|i| cards[i])
                .collect();
            map_link_route(cards[from], cards[to], others);
        }
    });

    let board = timed(
        "put every test on the board",
        Duration::from_secs(2),
        || {
            let board = Board {
                all_tests: true,
                ..Board::default()
            };
            sync_board_tests(board, files.clone(), "Project".into()).unwrap()
        },
    );
    assert_eq!(
        board.nodes.len(),
        TESTS + FOLDERS + 1,
        "a card per test, a frame per folder"
    );
    let mut board = board;
    for n in 0..200 {
        let kind = BoardNodeKind::Text {
            text: format!("Note **{n}**"),
        };
        let at = Point {
            x: -400.0 * f64::from(n % 10),
            y: 300.0 * f64::from(n / 10),
        };
        board = add_to_board(board, kind, at).board;
    }
    timed("edit the board", Duration::from_secs(2), || {
        let frames: Vec<String> = board
            .nodes
            .iter()
            .filter(|n| matches!(n.kind, BoardNodeKind::Group { .. }))
            .map(|n| n.id.clone())
            .collect();
        // A drag: what moves is worked out once, then every step of it lays the board out.
        let moved = board_with_contents(board.clone(), frames.clone());
        assert_eq!(moved.len(), TESTS + FOLDERS + 1);
        for step in 0..30 {
            let by = Point {
                x: f64::from(step),
                y: 0.0,
            };
            lay_out_board(move_on_board(board.clone(), frames.clone(), by, true));
        }
    });
    let text = timed("save and read the board", Duration::from_secs(1), || {
        let text = save_board(board.clone());
        assert_eq!(load_board(text.clone()).unwrap(), board);
        text
    });
    assert_eq!(text.lines().count(), TESTS + FOLDERS + 1 + 200 + 6);

    let runs = timed("list runs", Duration::from_secs(3), || {
        list_runs(path.clone()).unwrap()
    });
    assert_eq!(runs.len(), TESTS * RUNS_PER_TEST);
    timed("flaky tests", Duration::from_secs(1), || {
        flaky_tests(
            runs.iter().map(|r| r.report.clone()).collect(),
            1_792_000_000_000,
        )
    });

    let candidates: Vec<SearchCandidate> = files
        .iter()
        .zip(&flows)
        .map(|(file, flow)| SearchCandidate {
            title: flow.name.clone(),
            keywords: file.clone(),
        })
        .collect();
    timed("search as you type", Duration::from_secs(1), || {
        let index = SearchIndex::new(candidates.clone());
        for query in [
            "t", "te", "tes", "test", "test n", "test nu", "test 42", "area-3",
        ] {
            let ranked = rank_search(query.into(), candidates.clone());
            assert_eq!(index.rank(query.into()), ranked);
        }
    });

    let big = flows.iter().max_by_key(|f| f.blocks.len()).unwrap();
    timed("edit the largest test", Duration::from_secs(2), || {
        let mut flow = big.clone();
        for id in big.blocks.iter().step_by(10).map(|b| b.id.clone()) {
            flow = remove_block(flow, id).flow;
            layout_flow(flow.clone());
        }
        for _ in 0..30 {
            flow = jevision_ffi::insert_block(flow, "click".into(), None)
                .unwrap()
                .flow;
        }
        let slot = Slot {
            from: String::new(),
            edge: jevision_core::START.into(),
        };
        jevision_ffi::insert_block(flow, "if".into(), Some(slot)).unwrap();
    });

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn an_empty_project_has_nothing_and_does_not_fail() {
    let root = scratch("empty");
    let path = root.to_string_lossy().into_owned();
    assert!(project_map(path.clone(), Vec::new()).is_empty());
    assert!(list_runs(path.clone()).unwrap().is_empty());
    assert!(flaky_tests(Vec::new(), 0).is_empty());
    assert!(rank_search("anything".into(), Vec::new()).is_empty());
    assert!(rank_search(String::new(), Vec::new()).is_empty());
    assert!(
        SearchIndex::new(Vec::new())
            .rank("anything".into())
            .is_empty()
    );
    let card = Rect {
        x: 0.0,
        y: 0.0,
        width: 208.0,
        height: 90.0,
    };
    assert!(map_link_route(card, card, Vec::new()).len() >= 2);
    let _ = fs::remove_dir_all(&root);
}

/// Writes the large project to `$JEVISION_LARGE_PROJECT` for timing the app and the command
/// line by hand: `JEVISION_LARGE_PROJECT=/tmp/big cargo test -p jevision-ffi --test
/// large_project -- --ignored`.
#[test]
#[ignore = "writes a project for timing by hand"]
fn write_the_large_project() {
    let root =
        PathBuf::from(std::env::var("JEVISION_LARGE_PROJECT").expect("JEVISION_LARGE_PROJECT"));
    let _ = fs::remove_dir_all(&root);
    write_project(&root);
}
