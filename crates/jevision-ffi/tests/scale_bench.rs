//! How long the core takes on a made-up project of a thousand tests and thousands of saved runs,
//! along the paths the app takes. Each measurement prints `BENCH\t<name>\t<ms>`, the fastest of
//! three; `scripts/bench-scale.sh` collects them. Heavy, so left out of the usual test run:
//!
//! `cargo test --release -p jevision-ffi --test scale_bench -- --ignored --nocapture --test-threads=1`

mod common;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use common::{bench, bench_with, forget_runs_since, project, report, rss_kb, run_test, saved_runs};
use jevision_core::{BoardNodeKind, Rect, SearchCandidate, TestOutcome};
use jevision_ffi::{
    BoardIndex, RunOutcome, SyntheticSpec, TestBatch, TestFileSummary, data_issues,
    default_environment, dependency_issues, environment_issues, flaky_tests, folder_run_mode,
    lay_out_board, layout_flow, list_community_blocks, list_runs, load_board, load_flow,
    map_link_route, project_environments, project_map, project_schedules, project_settings,
    project_statistics, quarantine_suggestions, rank_search, required_inputs, scheduled_runs,
    sync_board_tests, test_health, test_statistics, test_summary, unrestored_blocks, validate_flow,
    validate_flow_in,
};

/// What the app's history ends at, so the saved runs are as recent as they look.
fn now_ms() -> i64 {
    SyntheticSpec::default().end_ms as i64
}

fn path(root: &Path) -> String {
    root.display().to_string()
}

fn folder_of(root: &Path, file: &str) -> String {
    path(root.join(file).parent().unwrap())
}

/// What the sidebar reads at open: every test file in the project's folders, hidden ones
/// skipped, with its summary, by file relative to the project.
fn scan(dir: &Path, prefix: &str, out: &mut Vec<(String, TestFileSummary)>) {
    let mut entries: Vec<_> = fs::read_dir(dir).unwrap().flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let id = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        if entry.file_type().unwrap().is_dir() {
            scan(&entry.path(), &id, out);
        } else if name.ends_with(".flow.json") {
            let text = fs::read_to_string(entry.path()).unwrap();
            out.push((id, test_summary(text)));
        }
    }
}

fn summaries(root: &Path) -> Vec<(String, TestFileSummary)> {
    let mut out = Vec::new();
    scan(root, "", &mut out);
    out
}

/// The files of `folder` of the project that end in `suffix`, read.
fn files_in(root: &Path, folder: &str, suffix: &str) -> Vec<(PathBuf, String)> {
    fs::read_dir(root.join(folder))
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.to_string_lossy().ends_with(suffix))
                .map(|p| {
                    let text = fs::read_to_string(&p).unwrap();
                    (p, text)
                })
                .collect()
        })
        .unwrap_or_default()
}

const ALL_TESTS_BOARD: &str = "boards/All Tests.canvas";

#[test]
#[ignore = "heavy: a measurement, see the top of the file"]
fn project_open_summaries() {
    let (root, summary) = project();
    bench("project.open.summaries", || {
        let found = summaries(root);
        assert_eq!(found.len(), summary.tests.len());
        found
    });
}

/// Everything `ProjectStore.open` and `rescan` ask the core for, in their order.
#[test]
#[ignore = "heavy: a measurement, see the top of the file"]
fn project_open_all() {
    let (root, summary) = project();
    let project = path(root);
    let title = root.file_name().unwrap().to_string_lossy().into_owned();
    bench("project.open.all", || {
        project_environments(project.clone()).unwrap();
        default_environment(project.clone()).unwrap();
        let boards = files_in(root, "boards", ".canvas");
        let all = boards
            .iter()
            .map(|(_, text)| load_board(text.clone()).unwrap())
            .find(|b| b.all_tests)
            .unwrap();
        project_schedules(project.clone()).unwrap();
        scheduled_runs(project.clone()).unwrap();
        for folder in ["templates", "blocks"] {
            for (_, text) in files_in(root, folder, ".json") {
                if let Ok(flow) = load_flow(text) {
                    required_inputs(flow);
                }
            }
        }
        list_community_blocks(project.clone());
        unrestored_blocks(project.clone()).unwrap();
        project_settings(project.clone()).unwrap();
        let found = summaries(root);
        let runs = list_runs(project.clone()).unwrap();
        let reports: Vec<_> = runs.iter().map(|r| r.report.clone()).collect();
        flaky_tests(reports.clone(), now_ms());
        quarantine_suggestions(reports, now_ms(), 1);
        sync_board_tests(all, summary.tests.clone(), title.clone());
        (found, runs)
    });
}

#[test]
#[ignore = "heavy: a measurement, see the top of the file"]
fn search_rank() {
    let (root, _) = project();
    let candidates: Vec<SearchCandidate> = summaries(root)
        .into_iter()
        .map(|(file, s)| {
            let (folder, name) = file.rsplit_once('/').unwrap_or(("", &file));
            SearchCandidate {
                title: name.trim_end_matches(".flow.json").to_owned(),
                keywords: format!("{folder} {}", s.keywords),
            }
        })
        .collect();
    let queries = [
        "add",
        "coupon",
        "checkout coupon",
        "dark mode",
        "rename tag",
        "zzz",
    ];
    let keystrokes: usize = queries.iter().map(|q| q.len()).sum();
    let total = bench("search.rank.all_keystrokes", || {
        for query in queries {
            for end in 1..=query.len() {
                rank_search(query[..end].to_owned(), candidates.clone());
            }
        }
    });
    report("search.rank", total / keystrokes as f64);
}

#[test]
#[ignore = "heavy: a measurement, see the top of the file"]
fn map_project() {
    let (root, summary) = project();
    bench("map.project", || {
        project_map(path(root), summary.tests.clone())
    });
}

/// The arrows of the map between the cards of the board of all tests, each around the cards
/// near it, as `ProjectMap.links` asks for them.
#[test]
#[ignore = "heavy: a measurement, see the top of the file"]
fn map_routes() {
    let (root, summary) = project();
    let board = load_board(fs::read_to_string(root.join(ALL_TESTS_BOARD)).unwrap()).unwrap();
    let layout = lay_out_board(board.clone());
    let rect_of: HashMap<&str, Rect> = board
        .nodes
        .iter()
        .zip(&layout.nodes)
        .filter_map(|(node, laid)| match &node.kind {
            BoardNodeKind::File { file } => Some((file.as_str(), laid.rect)),
            _ => None,
        })
        .collect();
    const REACH: f64 = 600.0;
    let mut links = Vec::new();
    for test in project_map(path(root), summary.tests.clone()) {
        for target in test.uses.iter().chain(&test.depends_on) {
            let (Some(from), Some(to)) = (
                rect_of.get(test.file.as_str()),
                rect_of.get(target.as_str()),
            ) else {
                continue;
            };
            let x = from.x.min(to.x) - REACH;
            let y = from.y.min(to.y) - REACH;
            let right = (from.x + from.width).max(to.x + to.width) + REACH;
            let bottom = (from.y + from.height).max(to.y + to.height) + REACH;
            let others: Vec<Rect> = rect_of
                .iter()
                .filter(|(file, r)| {
                    **file != test.file
                        && *file != target
                        && r.x < right
                        && r.x + r.width > x
                        && r.y < bottom
                        && r.y + r.height > y
                })
                .map(|(_, r)| *r)
                .collect();
            links.push((*from, *to, others));
        }
    }
    assert!(!links.is_empty());
    report("map.routes.links", links.len() as f64);
    bench("map.routes", || {
        for (from, to, others) in &links {
            map_link_route(*from, *to, others.clone());
        }
    });
}

#[test]
#[ignore = "heavy: a measurement, see the top of the file"]
fn board_layout_and_visible() {
    let (root, _) = project();
    let text = fs::read_to_string(root.join(ALL_TESTS_BOARD)).unwrap();
    bench("board.layout", || {
        lay_out_board(load_board(text.clone()).unwrap())
    });
    let layout = lay_out_board(load_board(text).unwrap());
    let bounds = layout.bounds.unwrap();
    bench("board.index", || BoardIndex::new(layout.clone()));
    let index = BoardIndex::new(layout);
    // A window's worth at full size and at a quarter, panned in steps over the whole board.
    let viewports: Vec<Rect> = [(1440.0, 900.0), (5760.0, 3600.0)]
        .into_iter()
        .flat_map(|(width, height)| {
            (0..100).map(move |step| Rect {
                x: bounds.x + bounds.width * f64::from(step % 10) / 10.0,
                y: bounds.y + bounds.height * f64::from(step / 10) / 10.0,
                width,
                height,
            })
        })
        .collect();
    let total = bench("board.visible.all_viewports", || {
        for area in &viewports {
            index.visible(*area);
        }
    });
    report("board.visible", total / viewports.len() as f64);
}

/// What `FlowDocument` does on opening the largest test: read, check every way the editor
/// does, and lay out.
#[test]
#[ignore = "heavy: a measurement, see the top of the file"]
fn flow_open_200() {
    let (root, summary) = project();
    assert_eq!(summary.largest_blocks, 200);
    let file = summary.largest.clone();
    let folder = folder_of(root, &file);
    bench("flow.open.200", || {
        let flow = load_flow(fs::read_to_string(root.join(&file)).unwrap()).unwrap();
        validate_flow(flow.clone());
        validate_flow_in(flow.clone(), folder.clone());
        environment_issues(flow.clone(), folder.clone());
        data_issues(flow.clone(), folder.clone());
        dependency_issues(&flow, path(root), file.clone());
        layout_flow(flow)
    });
}

#[test]
#[ignore = "heavy: a measurement, see the top of the file"]
fn project_validate() {
    let (root, summary) = project();
    bench("project.validate", || {
        for file in &summary.tests {
            let flow = load_flow(fs::read_to_string(root.join(file)).unwrap()).unwrap();
            let issues = validate_flow_in(flow.clone(), folder_of(root, file));
            assert!(issues.is_empty(), "{file}: {issues:?}");
            dependency_issues(&flow, path(root), file.clone());
        }
    });
}

/// Run All as `RunCenter.runAll` plans it, every test run one after another.
#[test]
#[ignore = "heavy: a measurement, see the top of the file"]
fn runall_screenless() {
    let (root, summary) = project();
    let project = path(root);
    let mode = folder_run_mode(&project_settings(project.clone()).unwrap(), String::new());
    let before = saved_runs(root);
    let total = bench_with(
        "runall.screenless",
        || forget_runs_since(root, &before),
        |()| {
            let batch = TestBatch::new(project.clone(), summary.screenless.clone(), mode);
            let entries = batch.entries();
            let mut passed = 0;
            while let Some(next) = batch.start_next() {
                let outcome = match run_test(root, &entries[next as usize].file) {
                    RunOutcome::Finished { report, .. }
                        if report.verdict == jevision_runner::Verdict::Passed =>
                    {
                        passed += 1;
                        TestOutcome::Passed
                    }
                    RunOutcome::Finished { .. } => TestOutcome::Failed,
                    RunOutcome::Failed { .. } => TestOutcome::NotRun,
                };
                batch.finish(next, outcome);
            }
            assert!(batch.is_over());
            assert_eq!(passed, entries.len(), "every test needing no screen passes");
        },
    );
    forget_runs_since(root, &before);
    report(
        "runall.screenless.per_test",
        total / summary.screenless.len() as f64,
    );
}

#[test]
#[ignore = "heavy: a measurement, see the top of the file"]
fn runs_history() {
    let (root, _) = project();
    let project = path(root);
    bench("runs.list", || list_runs(project.clone()).unwrap());
    let reports: Vec<_> = list_runs(project)
        .unwrap()
        .into_iter()
        .map(|r| r.report)
        .collect();
    bench_with(
        "runs.flaky",
        || reports.clone(),
        |r| flaky_tests(r, now_ms()),
    );
    bench_with(
        "runs.suggestions",
        || reports.clone(),
        |r| quarantine_suggestions(r, now_ms(), 1),
    );
    bench_with(
        "runs.health",
        || reports.clone(),
        |r| test_health(r, now_ms()),
    );
    let one = reports[0].file.clone();
    let of_one: Vec<_> = reports.iter().filter(|r| r.file == one).cloned().collect();
    bench_with(
        "runs.stats",
        || (reports.clone(), of_one.clone()),
        |(all, one)| (project_statistics(all, now_ms()), test_statistics(one, 0)),
    );
}

/// Memory a run of a test leaves behind: after ten runs to warm up, a hundred more.
#[test]
#[ignore = "heavy: a measurement, see the top of the file"]
fn memory_repeat_run100() {
    let (root, summary) = project();
    let file = &summary.screenless[0];
    let before = saved_runs(root);
    let run = || {
        let outcome = run_test(root, file);
        assert!(
            matches!(outcome, RunOutcome::Finished { .. }),
            "{outcome:?}"
        );
    };
    for _ in 0..10 {
        run();
    }
    forget_runs_since(root, &before);
    let start = rss_kb();
    for _ in 0..100 {
        run();
    }
    let growth = rss_kb().saturating_sub(start);
    forget_runs_since(root, &before);
    report("memory.repeat.run100.rss_growth_kb", growth as f64);
}
