//! Saved schedule checkpoints remain readable before any new scheduled work is dispatched.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use jevision_core::{
    SCHEDULE_HISTORY_FILE, ScheduleHistory, ScheduledOutcome, ScheduledRun, SkipReason,
    record_scheduled_run,
};
use serde_json::Value;

struct Project(PathBuf);

impl Project {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "jevision-history-fixture-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn saved_history_formats_keep_outcomes_and_optional_fields_during_updates() {
    for text in [
        include_str!("fixtures/history/legacy.json"),
        include_str!("fixtures/history/format-1.json"),
    ] {
        let project = Project::new();
        let file = project.0.join(SCHEDULE_HISTORY_FILE);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, text).unwrap();
        let original: Value = serde_json::from_str(text).unwrap();
        let mut history = ScheduleHistory::load(&project.0).unwrap();
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            text,
            "reading is read-only"
        );
        assert_eq!(history.checked_at, Some(29_000_000));
        assert_eq!(
            history.runs[0].outcome,
            ScheduledOutcome::Finished {
                passed: 2,
                failed: 1
            }
        );
        if original.get("format").is_some() {
            assert_eq!(history.runs.len(), 3);
            assert_eq!(
                history.runs[1].outcome,
                ScheduledOutcome::Skipped {
                    reason: SkipReason::ScreenLocked
                }
            );
            assert_eq!(
                history.runs[2].outcome,
                ScheduledOutcome::Error {
                    message: "Environment is unavailable.".into()
                }
            );
        }
        history.checked_at = Some(29_000_001);
        history.save(&project.0).unwrap();
        let added = ScheduledRun {
            schedule: "Next day".into(),
            started_at_ms: 1_740_000_180_000,
            duration_ms: 400,
            outcome: ScheduledOutcome::Finished {
                passed: 3,
                failed: 0,
            },
        };
        record_scheduled_run(&project.0, added.clone()).unwrap();
        let saved: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
        assert_eq!(saved["format"], 1);
        assert_eq!(saved["checkedAt"], 29_000_001);
        let original_runs = original["runs"].as_array().unwrap();
        let saved_runs = saved["runs"].as_array().unwrap();
        assert_eq!(&saved_runs[..original_runs.len()], original_runs);
        assert_eq!(saved_runs.len(), original_runs.len() + 1);
        for (key, value) in original.as_object().unwrap() {
            if !matches!(key.as_str(), "checkedAt" | "runs") {
                assert_eq!(&saved[key], value, "root field {key}");
            }
        }
        let reloaded = ScheduleHistory::load(&project.0).unwrap();
        assert_eq!(reloaded.checked_at, Some(29_000_001));
        assert_eq!(reloaded.runs.last(), Some(&added));
    }
}
