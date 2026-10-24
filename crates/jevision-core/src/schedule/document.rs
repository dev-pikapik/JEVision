//! The schedule document keeps optional fields separate from the schedules it executes.

use super::*;
use crate::compat::check_settings_header;
use crate::{Issue, SettingsMetadata};

type Fields = BTreeMap<String, Value>;

/// Schedules with their compatibility headers and retained optional fields.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScheduleFile {
    pub schedules: Vec<Schedule>,
    pub metadata: SettingsMetadata,
    entries: BTreeMap<String, Fields>,
}

impl ScheduleFile {
    /// A new document without metadata from an existing file.
    pub fn new(schedules: Vec<Schedule>) -> Self {
        Self {
            schedules,
            ..Self::default()
        }
    }

    pub fn parse(text: &str) -> Result<Self, ScheduleProblem> {
        // Preserve the established syntax diagnostic, including its line and column.
        serde_json::from_str::<Value>(text).map_err(|e| {
            ScheduleProblem::new("flow.json.syntax")
                .with("file", SCHEDULE_FILE)
                .with("line", e.line())
                .with("column", e.column())
        })?;
        let mut value = check_settings_header(text, SCHEDULE_FORMAT).map_err(|error| {
            let issue = error.issue(SCHEDULE_FILE, "project.file.unreadable");
            ScheduleProblem {
                code: issue.code,
                params: issue
                    .params
                    .into_iter()
                    .map(|p| (p.name, p.value))
                    .collect(),
            }
        })?;
        let root = value.as_object_mut().expect("the header checked an object");
        let mut header = root.clone();
        header.remove("format");
        header.remove("schedules");
        root.retain(|key, _| key == "format" || key == "schedules");
        let unreadable = |reason: String| {
            ScheduleProblem::new("project.file.unreadable")
                .with("file", SCHEDULE_FILE)
                .with("reason", reason)
        };
        let metadata =
            serde_json::from_value(header.into()).map_err(|e| unreadable(e.to_string()))?;
        let mut entries = BTreeMap::new();
        if let Some(schedules) = root.get_mut("schedules").and_then(Value::as_array_mut) {
            for schedule in schedules {
                if let Some(time) = schedule.get("at").and_then(Value::as_str)
                    && TimeOfDay::parse(time).is_none()
                {
                    return Err(ScheduleProblem::new("schedule.time.invalid")
                        .with("file", SCHEDULE_FILE)
                        .with("time", time));
                }
                let Some(object) = schedule.as_object_mut() else {
                    continue;
                };
                let name = object
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::to_lowercase);
                let keys: Vec<_> = object
                    .keys()
                    .filter(|key| {
                        !["name", "tests", "repeat", "at", "environment", "video"]
                            .contains(&key.as_str())
                    })
                    .cloned()
                    .collect();
                let fields: Fields = keys
                    .into_iter()
                    .map(|key| {
                        let value = object.remove(&key).expect("a collected key exists");
                        (key, value)
                    })
                    .collect();
                if let Some(name) = name.filter(|_| !fields.is_empty()) {
                    entries.insert(name, fields);
                }
            }
        }
        let file: File = serde_json::from_value(value).map_err(|e| unreadable(e.to_string()))?;
        check(&file.schedules)?;
        Ok(Self {
            schedules: file.schedules,
            metadata,
            entries,
        })
    }

    pub fn load(project: &Path) -> Result<Self, ScheduleProblem> {
        match fs::read_to_string(project.join(SCHEDULE_FILE)) {
            Ok(text) => Self::parse(&text),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(ScheduleProblem::io(
                "project.file.unreadable",
                SCHEDULE_FILE,
                &e,
            )),
        }
    }

    pub fn warnings(&self) -> Vec<Issue> {
        let mut issues = self.metadata.warnings(SCHEDULE_FILE);
        for schedule in &self.schedules {
            if let Some(fields) = self.entries.get(&schedule.name.to_lowercase()) {
                for key in fields.keys() {
                    issues.push(
                        Issue::warning("settings.field.unknown")
                            .with("file", SCHEDULE_FILE)
                            .with("name", format!("schedules[{}].{key}", schedule.name)),
                    );
                }
            }
        }
        issues
    }

    pub fn to_json(&self) -> Result<String, ScheduleProblem> {
        check(&self.schedules)?;
        let mut schedules = self.schedules.clone();
        for schedule in &mut schedules {
            if let Repeat::Days(days) = &mut schedule.repeat {
                days.sort();
            }
        }
        #[derive(Serialize)]
        struct Entry<'a> {
            #[serde(flatten)]
            schedule: &'a Schedule,
            #[serde(flatten)]
            extra: &'a Fields,
        }
        #[derive(Serialize)]
        struct Output<'a> {
            format: u32,
            schedules: Vec<Entry<'a>>,
            #[serde(flatten)]
            metadata: &'a SettingsMetadata,
        }
        let empty = Fields::new();
        let output = Output {
            format: SCHEDULE_FORMAT,
            metadata: &self.metadata,
            schedules: schedules
                .iter()
                .map(|s| Entry {
                    schedule: s,
                    extra: self.entries.get(&s.name.to_lowercase()).unwrap_or(&empty),
                })
                .collect(),
        };
        let mut text = serde_json::to_string_pretty(&output).expect("schedules always serialize");
        text.push('\n');
        Ok(text)
    }

    /// Recheck the current file and retain its latest opaque fields, including deletions.
    pub fn save(&self, project: &Path) -> Result<(), ScheduleProblem> {
        self.save_renaming(project, None)
    }

    pub(super) fn save_renaming(
        &self,
        project: &Path,
        rename: Option<(&str, &str)>,
    ) -> Result<(), ScheduleProblem> {
        Self::parse(&self.to_json()?)?;
        let file = project.join(SCHEDULE_FILE);
        let current = match fs::read_to_string(&file) {
            Ok(text) => Some(Self::parse(&text)?),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => {
                return Err(ScheduleProblem::io(
                    "project.file.unreadable",
                    SCHEDULE_FILE,
                    &e,
                ));
            }
        };
        let mut edited = self.clone();
        if let Some(current) = &current {
            edited.metadata = current.metadata.clone();
            edited.entries = current.entries.clone();
        }
        if let Some((from, to)) = rename {
            let from_key = from.to_lowercase();
            let to_key = to.to_lowercase();
            if !current.as_ref().is_some_and(|c| {
                c.schedules
                    .iter()
                    .any(|s| s.name.to_lowercase() == from_key)
            }) {
                return Err(ScheduleProblem::new("schedule.name.notFound").with("name", from));
            }
            if !edited
                .schedules
                .iter()
                .any(|s| s.name.to_lowercase() == to_key)
            {
                return Err(ScheduleProblem::new("schedule.name.notFound").with("name", to));
            }
            if from_key != to_key {
                if edited
                    .schedules
                    .iter()
                    .any(|s| s.name.to_lowercase() == from_key)
                {
                    return Err(ScheduleProblem::new("schedule.rename.invalid"));
                }
                let fields = edited.entries.remove(&from_key);
                edited.entries.remove(&to_key);
                if let Some(fields) = fields {
                    edited.entries.insert(to_key, fields);
                }
            }
        }
        if edited.schedules.is_empty() && edited.metadata == SettingsMetadata::default() {
            return match fs::remove_file(&file) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => Err(ScheduleProblem::io(
                    "project.file.unwritable",
                    SCHEDULE_FILE,
                    &e,
                )),
                _ => Ok(()),
            };
        }
        let text = edited.to_json()?;
        Self::parse(&text)?;
        crate::atomic::write(&file, &text)
            .map_err(|e| ScheduleProblem::io("project.file.unwritable", SCHEDULE_FILE, &e))
    }
}
