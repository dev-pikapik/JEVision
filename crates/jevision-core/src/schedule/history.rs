//! A checkpoint must remain readable before scheduled input can be dispatched.

use super::*;
use crate::SettingsMetadata;
use crate::compat::{UniqueValue, check_settings_value, read_settings_json};

/// Scheduled runs on this computer, oldest first, and the last checked local minute.
#[derive(Debug, Clone)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[cfg_attr(test, schemars(rename_all = "camelCase"))]
pub struct ScheduleHistory {
    pub(crate) format: u32,
    /// Local minute last checked; earlier runs are complete or missed.
    pub checked_at: Option<i64>,
    /// Newest retained runs, oldest first.
    #[cfg_attr(test, schemars(default))]
    pub runs: Vec<ScheduledRun>,
    #[cfg_attr(test, schemars(flatten))]
    pub metadata: SettingsMetadata,
    #[cfg_attr(test, schemars(skip))]
    retained: Vec<(ScheduledRun, Value)>,
    #[cfg_attr(test, schemars(skip))]
    baseline: Option<(Option<i64>, Vec<ScheduledRun>)>,
}

impl Default for ScheduleHistory {
    fn default() -> Self {
        Self {
            format: SCHEDULE_HISTORY_FORMAT,
            checked_at: None,
            runs: Vec::new(),
            metadata: SettingsMetadata::default(),
            retained: Vec::new(),
            baseline: None,
        }
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct Document {
    format: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    checked_at: Option<i64>,
    #[serde(default)]
    runs: Vec<Value>,
    #[serde(flatten)]
    metadata: SettingsMetadata,
}

impl ScheduleHistory {
    fn from_value(value: Value) -> Result<Self, crate::SettingsFileError> {
        let mut value =
            check_settings_value(value, SCHEDULE_HISTORY_FORMAT, Some(OLDEST_HISTORY_FORMAT))?;
        let object = value.as_object_mut().expect("the header checked an object");
        let from = (object["format"].as_u64().expect("a checked format")
            - u64::from(OLDEST_HISTORY_FORMAT)) as usize;
        for step in &HISTORY_MIGRATIONS[from..] {
            step(object);
        }
        object.insert("format".into(), SCHEDULE_HISTORY_FORMAT.into());
        let document: Document = serde_json::from_value(value)
            .map_err(|e| crate::SettingsFileError::Invalid(e.to_string()))?;
        let retained = document
            .runs
            .into_iter()
            .map(|raw| {
                serde_json::from_value::<ScheduledRun>(raw.clone())
                    .map(|run| (run, raw))
                    .map_err(|e| crate::SettingsFileError::Invalid(e.to_string()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let runs: Vec<_> = retained.iter().map(|(run, _)| run.clone()).collect();
        Ok(Self {
            format: SCHEDULE_HISTORY_FORMAT,
            checked_at: document.checked_at,
            baseline: Some((document.checked_at, runs.clone())),
            runs,
            metadata: document.metadata,
            retained,
        })
    }

    /// Read legacy format 1 or a supported file without discarding optional data. Broken
    /// checkpoints and unknown outcome variants refuse scheduling rather than starting over.
    pub fn parse(text: &str) -> Result<Self, ScheduleProblem> {
        read_settings_json(text)
            .and_then(Self::from_value)
            .map_err(problem)
    }

    pub fn load(project: &Path) -> Result<Self, ScheduleProblem> {
        match fs::read_to_string(project.join(SCHEDULE_HISTORY_FILE)) {
            Ok(text) => Self::parse(&text),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(ScheduleProblem::io(
                "project.file.unreadable",
                SCHEDULE_HISTORY_FILE,
                &e,
            )),
        }
    }

    /// Recheck the current file, retaining its optional fields. A stale checkpoint or run
    /// list cannot overwrite a different writer's progress.
    pub fn save(&self, project: &Path) -> Result<(), ScheduleProblem> {
        with_lock(project, || self.save_locked(project))
    }

    pub(super) fn save_locked(&self, project: &Path) -> Result<(), ScheduleProblem> {
        let file = project.join(SCHEDULE_HISTORY_FILE);
        let current = Self::load(project)?;
        let expected = self.baseline.as_ref();
        let unchanged = match expected {
            Some((checked, runs)) => {
                file.exists() && current.checked_at == *checked && current.runs == *runs
            }
            None => current.checked_at.is_none() && current.runs.is_empty(),
        };
        if !unchanged {
            return Err(ScheduleProblem::new("schedule.history.changed")
                .with("file", SCHEDULE_HISTORY_FILE));
        }
        let mut saved = self.clone();
        if file.exists() {
            saved.metadata = current.metadata;
            saved.retained = current.retained;
        }
        // Removing only some identical rows would make their optional fields ambiguous.
        for (run, _) in &saved.retained {
            let before = saved.retained.iter().filter(|(old, _)| old == run).count();
            let after = saved.runs.iter().filter(|new| *new == run).count();
            if before > 1 && after > 0 && after < before {
                return Err(ScheduleProblem::new("schedule.history.changed")
                    .with("file", SCHEDULE_HISTORY_FILE));
            }
        }
        saved.write_locked(project)
    }

    pub(super) fn append_locked(
        &mut self,
        project: &Path,
        run: ScheduledRun,
    ) -> Result<(), ScheduleProblem> {
        self.runs.push(run);
        let extra = self.runs.len().saturating_sub(SCHEDULE_HISTORY_KEPT);
        self.runs.drain(..extra);
        self.retained.drain(..extra.min(self.retained.len()));
        self.write_locked(project)
    }

    fn write_locked(&self, project: &Path) -> Result<(), ScheduleProblem> {
        let file = project.join(SCHEDULE_HISTORY_FILE);
        let text = serde_json::to_string_pretty(self).expect("a history serializes");
        Self::parse(&text)?;
        file.parent()
            .map_or(Ok(()), fs::create_dir_all)
            .and_then(|()| crate::atomic::write(&file, text))
            .map_err(|e| ScheduleProblem::io("project.file.unwritable", SCHEDULE_HISTORY_FILE, &e))
    }

    fn document(&self) -> Document {
        let mut used = vec![false; self.retained.len()];
        let runs = self
            .runs
            .iter()
            .map(|run| {
                if let Some((i, (_, raw))) = self
                    .retained
                    .iter()
                    .enumerate()
                    .find(|(i, (known, _))| !used[*i] && known == run)
                {
                    used[i] = true;
                    raw.clone()
                } else {
                    serde_json::to_value(run).expect("a run serializes")
                }
            })
            .collect();
        Document {
            format: self.format,
            checked_at: self.checked_at,
            runs,
            metadata: self.metadata.clone(),
        }
    }
}

/// The lock has a stable inode beside the atomically replaced history. Closing releases it.
pub(super) fn with_lock<T>(
    project: &Path,
    work: impl FnOnce() -> Result<T, ScheduleProblem>,
) -> Result<T, ScheduleProblem> {
    let path = project
        .join(SCHEDULE_HISTORY_FILE)
        .with_extension("json.lock");
    let acquire = || -> io::Result<fs::File> {
        fs::create_dir_all(path.parent().expect("a project-relative path"))?;
        let file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)?;
        file.lock()?;
        Ok(file)
    };
    let _lock = acquire()
        .map_err(|e| ScheduleProblem::io("project.file.unwritable", SCHEDULE_HISTORY_FILE, &e))?;
    work()
}

fn problem(error: crate::SettingsFileError) -> ScheduleProblem {
    let issue = error.issue(SCHEDULE_HISTORY_FILE, "project.file.unreadable");
    ScheduleProblem {
        code: issue.code,
        params: issue
            .params
            .into_iter()
            .map(|p| (p.name, p.value))
            .collect(),
    }
}

impl Serialize for ScheduleHistory {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.document().serialize(serializer)
    }
}

impl PartialEq for ScheduleHistory {
    fn eq(&self, other: &Self) -> bool {
        self.document() == other.document()
    }
}

impl Eq for ScheduleHistory {}

impl<'de> Deserialize<'de> for ScheduleHistory {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let UniqueValue(value) = UniqueValue::deserialize(deserializer)?;
        Self::from_value(value).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{finished_run, history_in};
    use super::*;
    use serde_json::json;

    fn document() -> Value {
        let mut run = serde_json::to_value(finished_run()).unwrap();
        run["host"] = json!({"id": [1, null, "mac"]});
        run["outcome"]["report"] = json!({"url": "local"});
        json!({"format": 1, "$schema": "https://example.test/history.json", "minVersion": "0.1.0+archive", "checkedAt": 7, "runs": [run], "cursor": {"generation": 2}})
    }

    #[test]
    fn preserves_headers_and_each_runs_optional_fields() {
        let (project, file) = history_in("retained");
        let original = document();
        fs::write(&file, original.to_string()).unwrap();
        begin_schedule_check(&project, 8).unwrap();
        record_scheduled_run(&project, finished_run()).unwrap();
        let mut saved: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(saved["checkedAt"], 8);
        assert_eq!(saved["runs"][0], original["runs"][0]);
        assert!(saved["runs"][1].get("host").is_none());
        saved["checkedAt"] = 7.into();
        saved["runs"].as_array_mut().unwrap().pop();
        assert_eq!(saved, original);
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn equality_is_document_content_not_the_write_checkpoint() {
        let original = ScheduleHistory::default();
        let loaded: ScheduleHistory =
            serde_json::from_str(&serde_json::to_string(&original).unwrap()).unwrap();
        assert_eq!(original, loaded);
        let retained = ScheduleHistory::parse(&document().to_string()).unwrap();
        let reloaded = ScheduleHistory::parse(&serde_json::to_string(&retained).unwrap()).unwrap();
        assert_eq!(retained, reloaded);
    }

    #[test]
    fn stale_saves_keep_current_metadata_and_do_not_restore_removed_fields() {
        let (project, file) = history_in("latest-fields");
        let mut original = document();
        fs::write(&file, original.to_string()).unwrap();
        let mut stale = ScheduleHistory::load(&project).unwrap();
        original.as_object_mut().unwrap().remove("$schema");
        original.as_object_mut().unwrap().remove("cursor");
        original["runs"][0].as_object_mut().unwrap().remove("host");
        original["runs"][0]["outcome"]["report"] = "updated".into();
        fs::write(&file, original.to_string()).unwrap();
        stale.checked_at = Some(9);
        stale.save(&project).unwrap();
        original["checkedAt"] = 9.into();
        let saved: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(saved, original);
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn refuses_future_or_broken_files_before_writing_even_with_stale_edits() {
        let (project, file) = history_in("refuse-writes");
        let stale = ScheduleHistory::default();
        let mut unknown = document();
        unknown["runs"][0]["outcome"]["result"] = "deferred".into();
        for text in [
            "{".to_owned(),
            r#"{"format":2,"runs":"new-layout"}"#.to_owned(),
            r#"{"format":1,"minVersion":"9999.0.0","runs":"new-layout"}"#.to_owned(),
            r#"{"format":1,"minVersion":"no"}"#.to_owned(),
            r#"{"format":1,"minVersion":null}"#.to_owned(),
            r#"{"format":1,"$schema":null}"#.to_owned(),
            r#"{"format":1,"checkedAt":"7","runs":[]}"#.to_owned(),
            r#"{"format":1,"runs":[],"runs":[]}"#.to_owned(),
            r#"{"format":1,"extra":{"duplicate":1,"duplicate":2}}"#.to_owned(),
            unknown.to_string(),
        ] {
            fs::write(&file, &text).unwrap();
            let error = ScheduleHistory::load(&project).unwrap_err();
            if text.contains("new-layout") {
                assert!(matches!(
                    error.code.as_str(),
                    "settings.format.tooNew" | "settings.needsNewer"
                ));
            }
            assert!(stale.save(&project).is_err());
            assert!(begin_schedule_check(&project, 8).is_err());
            assert!(record_scheduled_run(&project, finished_run()).is_err());
            assert_eq!(fs::read_to_string(&file).unwrap(), text);
            assert!(serde_json::from_str::<ScheduleHistory>(&text).is_err());
        }
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn stale_checkpoints_and_deleted_histories_do_not_erase_progress() {
        let (project, file) = history_in("stale-checkpoint");
        fs::write(&file, document().to_string()).unwrap();
        let stale = ScheduleHistory::load(&project).unwrap();
        record_scheduled_run(&project, finished_run()).unwrap();
        let latest = fs::read_to_string(&file).unwrap();
        assert_eq!(
            stale.save(&project).unwrap_err().code,
            "schedule.history.changed"
        );
        assert_eq!(fs::read_to_string(&file).unwrap(), latest);
        fs::remove_file(&file).unwrap();
        assert!(stale.save(&project).is_err());
        assert!(!file.exists());
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn retention_keeps_the_correct_metadata_for_identical_rows() {
        let (project, file) = history_in("duplicate-retention");
        let mut original = document();
        let row = original["runs"][0].clone();
        original["runs"] = (0..SCHEDULE_HISTORY_KEPT)
            .map(|i| {
                let mut row = row.clone();
                row["ordinal"] = i.into();
                row
            })
            .collect();
        fs::write(&file, original.to_string()).unwrap();
        record_scheduled_run(&project, finished_run()).unwrap();
        let saved: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(
            saved["runs"].as_array().unwrap().len(),
            SCHEDULE_HISTORY_KEPT
        );
        assert_eq!(saved["runs"][0]["ordinal"], 1);
        assert_eq!(
            saved["runs"][SCHEDULE_HISTORY_KEPT - 2]["ordinal"],
            SCHEDULE_HISTORY_KEPT - 1
        );
        assert!(
            saved["runs"][SCHEDULE_HISTORY_KEPT - 1]
                .get("ordinal")
                .is_none()
        );
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn simultaneous_writers_append_without_losing_runs_or_metadata() {
        let (project, file) = history_in("concurrent");
        fs::write(&file, document().to_string()).unwrap();
        let barrier = std::sync::Barrier::new(12);
        std::thread::scope(|scope| {
            for i in 0..12 {
                let project = &project;
                let barrier = &barrier;
                scope.spawn(move || {
                    let mut run = finished_run();
                    run.started_at_ms = i + 1;
                    barrier.wait();
                    record_scheduled_run(project, run).unwrap();
                });
            }
        });
        let history = ScheduleHistory::load(&project).unwrap();
        let ids: BTreeSet<_> = history.runs.iter().map(|run| run.started_at_ms).collect();
        assert_eq!(ids, (0..=12).collect());
        assert_eq!(history.runs.len(), 13);
        let saved: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(saved["runs"][0], document()["runs"][0]);
        fs::remove_dir_all(project).unwrap();
    }
}
