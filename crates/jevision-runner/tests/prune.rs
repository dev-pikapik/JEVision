//! Reading saved runs as summaries, and deleting the old ones a project's settings ask for.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, SystemTime};

use jevision_core::RunRetention;
use jevision_runner::{PruneSummary, Run, StepStatus, preview_prune_runs, prune_runs, rfc3339};

const DAY: Duration = Duration::from_secs(24 * 60 * 60);
const NONE: RunRetention = RunRetention {
    max_age_days: None,
    max_runs_per_test: None,
    media_max_age_days: None,
    max_total_mb: None,
};

/// A project folder that removes itself.
struct Project(PathBuf);

impl Project {
    fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "jevision-prune-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.0.join(".jevision/runs").join(rel)
    }

    fn write(&self, rel: &str, bytes: impl AsRef<[u8]>) -> &Self {
        let path = self.path(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
        self
    }

    /// Saves the run `name` of the test `file`, started `days` before `now`.
    fn run(&self, name: &str, file: &str, now: SystemTime, days: u32) -> &Self {
        self.write(
            &format!("{name}/report.json"),
            report(file, now - DAY * days),
        )
    }

    /// Every file under the runs folder.
    fn files(&self) -> BTreeSet<PathBuf> {
        fn walk(dir: &Path, out: &mut BTreeSet<PathBuf>) {
            for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
                if entry.file_type().unwrap().is_dir() {
                    walk(&entry.path(), out);
                } else {
                    out.insert(entry.path());
                }
            }
        }
        let mut out = BTreeSet::new();
        walk(&self.0.join(".jevision/runs"), &mut out);
        out
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn report(file: &str, started: SystemTime) -> String {
    let step = |seq: u32, test: &str, status: &str| {
        format!(
            r#"{{"seq":{seq},"test":"{test}","block":"b{seq}","type":"pressKeys","status":"{status}","durationMs":1}}"#
        )
    };
    let steps = [
        step(1, file, "passed"),
        step(2, file, "passed"),
        step(3, "sub.flow.json", "passed"),
        step(4, "sub.flow.json", "failed"),
        step(5, file, "passed"),
    ]
    .join(",");
    format!(
        r#"{{"format":1,"test":"T","file":"{file}","app":"com.example.App","startedAt":"{}","durationMs":5,"verdict":"failed","flaky":false,"attempts":[{{"number":1,"verdict":"failed","durationMs":5,"steps":[{steps}]}}]}}"#,
        rfc3339(started)
    )
}

fn set_modified(path: &Path, time: SystemTime) {
    File::open(path).unwrap().set_modified(time).unwrap();
}

fn names(p: &Project) -> BTreeSet<String> {
    fs::read_dir(p.0.join(".jevision/runs"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect()
}

fn set(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|&n| n.to_owned()).collect()
}

#[test]
fn old_runs_go_and_the_newest_of_each_test_stays() {
    let p = Project::new();
    let now = SystemTime::now();
    p.run("a200", "a.flow.json", now, 200)
        .run("a190", "a.flow.json", now, 190)
        .run("a10", "a.flow.json", now, 10)
        .run("b400", "b.flow.json", now, 400);
    let keep = RunRetention {
        max_age_days: Some(180),
        ..NONE
    };

    let summary = prune_runs(&p.0, &keep, now).unwrap();
    assert_eq!(summary.removed_runs, 2);
    assert_eq!(summary.removed_media_files, 0);
    assert!(summary.freed_bytes > 0);
    assert_eq!(names(&p), set(&["a10", "b400"]));
}

#[test]
fn only_so_many_runs_of_each_test_are_kept() {
    let p = Project::new();
    let now = SystemTime::now();
    for day in 1..=5 {
        p.run(&format!("a{day}"), "a.flow.json", now, day);
    }
    p.run("b1", "b.flow.json", now, 1)
        .run("b9", "b.flow.json", now, 9);
    let keep = RunRetention {
        max_runs_per_test: Some(2),
        ..NONE
    };

    assert_eq!(prune_runs(&p.0, &keep, now).unwrap().removed_runs, 3);
    assert_eq!(names(&p), set(&["a1", "a2", "b1", "b9"]));
}

#[test]
fn the_oldest_runs_go_while_all_take_more_than_the_cap() {
    let p = Project::new();
    let now = SystemTime::now();
    let shot = vec![0u8; 400_000];
    for day in 1..=6 {
        p.run(&format!("a{day}"), "a.flow.json", now, day)
            .write(&format!("a{day}/shots/1.png"), &shot);
    }
    p.run("b10", "b.flow.json", now, 10)
        .write("b10/shots/1.png", &shot);
    let keep = RunRetention {
        max_total_mb: Some(1),
        ..NONE
    };

    // 2.8 MB in all: the oldest runs of `a` go until less than 1 MB is left; `b` has one run.
    let summary = prune_runs(&p.0, &keep, now).unwrap();
    assert_eq!(summary.removed_runs, 5);
    assert!(summary.freed_bytes > 5 * 400_000);
    assert_eq!(names(&p), set(&["a1", "b10"]));
}

#[test]
fn videos_and_screenshots_go_and_the_report_stays() {
    let p = Project::new();
    let now = SystemTime::now();
    p.run("old", "a.flow.json", now, 40)
        .write("old/video.mp4", "movie")
        .write("old/shots/1-001-a-after.png", "png1")
        .write("old/shots/1-002-b-after.png", "png2")
        .write("old/junit.xml", "<x/>")
        .write("old/report.html", "<html>")
        .run("new", "a.flow.json", now, 10)
        .write("new/video.mp4", "movie")
        .write("new/shots/1-001-a-after.png", "png")
        // Not saved by a run that ended: what is in them is as old as the files.
        .write("crashed/video.mp4", "movie")
        .write("going/video.mp4", "movie")
        .write("going/shots/1-001-a-after.png", "png");
    set_modified(&p.path("crashed/video.mp4"), now - DAY * 31);
    let keep = RunRetention {
        media_max_age_days: Some(30),
        ..NONE
    };

    let summary = prune_runs(&p.0, &keep, now).unwrap();
    assert_eq!(
        summary,
        PruneSummary {
            removed_runs: 0,
            removed_media_files: 4,
            freed_bytes: 5 + 4 + 4 + 5,
        }
    );
    for gone in ["old/video.mp4", "old/shots", "crashed/video.mp4"] {
        assert!(!p.path(gone).exists(), "{gone}");
    }
    for kept in [
        "old/report.json",
        "old/junit.xml",
        "old/report.html",
        "new/video.mp4",
        "new/shots/1-001-a-after.png",
        "going/video.mp4",
        "going/shots/1-001-a-after.png",
    ] {
        assert!(p.path(kept).exists(), "{kept}");
    }
    assert_eq!(
        prune_runs(&p.0, &keep, now).unwrap(),
        PruneSummary::default()
    );
}

#[test]
fn a_run_still_being_written_is_left_alone() {
    let p = Project::new();
    let now = SystemTime::now();
    let shot = vec![0u8; 2_000_000];
    p.run("a5", "a.flow.json", now, 5)
        .run("a3", "a.flow.json", now, 3)
        .write("going/shots/1-001-a-after.png", &shot)
        .write("going/video.mp4", "movie")
        .write("left/shots/1-001-a-after.png", "png");
    // Left behind by a run that stopped long ago without a report.
    for rel in ["left/shots/1-001-a-after.png", "left/shots", "left"] {
        set_modified(&p.path(rel), now - DAY * 3);
    }
    let keep = RunRetention {
        max_age_days: Some(2),
        max_runs_per_test: Some(1),
        media_max_age_days: Some(1),
        max_total_mb: Some(1),
    };

    let summary = prune_runs(&p.0, &keep, now).unwrap();
    assert_eq!(summary.removed_runs, 2, "{summary:?}");
    assert_eq!(names(&p), set(&["a3", "going"]));
    assert!(p.path("going/shots/1-001-a-after.png").exists());
    assert!(p.path("going/video.mp4").exists());
}

#[test]
fn a_preview_deletes_nothing_and_tells_what_pruning_does() {
    let p = Project::new();
    let now = SystemTime::now();
    for day in [1, 20, 45, 200, 400] {
        p.run(&format!("a{day}"), "a.flow.json", now, day)
            .write(&format!("a{day}/video.mp4"), "movie")
            .write(&format!("a{day}/shots/1.png"), vec![0u8; 300_000]);
    }
    p.run("b1", "b.flow.json", now, 1)
        .write("b1/shots/1.png", vec![0u8; 900_000]);
    let keep = RunRetention {
        max_total_mb: Some(1),
        ..RunRetention::default()
    };
    let before = p.files();

    let preview = preview_prune_runs(&p.0, &keep, now).unwrap();
    assert_eq!(p.files(), before);
    assert_ne!(preview, PruneSummary::default());
    assert_eq!(prune_runs(&p.0, &keep, now).unwrap(), preview);
    assert_eq!(
        prune_runs(&p.0, &keep, now).unwrap(),
        PruneSummary::default()
    );
    assert_eq!(
        preview_prune_runs(&p.0, &keep, now).unwrap(),
        PruneSummary::default()
    );
}

#[test]
fn a_project_without_runs_prunes_nothing() {
    let p = Project::new();
    let keep = RunRetention::default();
    assert_eq!(
        prune_runs(&p.0, &keep, SystemTime::now()).unwrap(),
        PruneSummary::default()
    );
    assert_eq!(
        preview_prune_runs(&p.0.join("elsewhere"), &keep, SystemTime::now()).unwrap(),
        PruneSummary::default()
    );
}

#[test]
fn summaries_keep_what_lists_need_and_follow_the_files() {
    let p = Project::new();
    let now = SystemTime::now();
    p.run("one", "a.flow.json", now, 2)
        .run("two", "b.flow.json", now, 1)
        .write("going/video.mp4", "movie")
        .write("broken/report.json", "{");

    let full = Run::list(&p.0).unwrap();
    let summaries = Run::summaries(&p.0).unwrap();
    assert_eq!(summaries.len(), 2);
    for (summary, full) in summaries.iter().zip(&full) {
        assert_eq!(summary.dir, full.dir);
        let steps: Vec<(u32, StepStatus)> = summary.report.attempts[0]
            .steps
            .iter()
            .map(|s| (s.seq, s.status))
            .collect();
        // The failed step, and the first step of each test file the attempt ran.
        assert_eq!(
            steps,
            [
                (1, StepStatus::Passed),
                (3, StepStatus::Passed),
                (4, StepStatus::Failed)
            ]
        );
        let mut whole = summary.report.clone();
        whole.attempts = full.report.attempts.clone();
        assert_eq!(&whole, &full.report);
    }
    assert_eq!(
        Run::load(&p.path("one")).unwrap().report.attempts[0]
            .steps
            .len(),
        5
    );

    p.write("two/report.json", report("cc.flow.json", now - DAY));
    fs::remove_dir_all(p.path("one")).unwrap();
    let files: Vec<String> = Run::summaries(&p.0)
        .unwrap()
        .into_iter()
        .map(|r| r.report.file)
        .collect();
    assert_eq!(files, ["cc.flow.json"]);
}
