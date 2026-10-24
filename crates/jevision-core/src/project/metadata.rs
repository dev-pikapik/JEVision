//! Optional fields of project settings, attached to their owning objects and named entries.

use super::PROJECT_FILE;
use crate::{Issue, SettingsMetadata};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

type Fields = BTreeMap<String, Value>;

/// Compatibility headers and unfamiliar fields retained by the project settings editor.
/// Removing an owning entry removes its fields too. Array entries with names follow their
/// names; screenshot areas follow their geometry, so removing an area cannot move its fields
/// onto another area.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectMetadata {
    header: SettingsMetadata,
    nested: BTreeMap<String, Fields>,
}

const TOP: &[&str] = &[
    "format",
    "retries",
    "quarantine",
    "suggestQuarantineAfter",
    "localOnly",
    "hide",
    "ai",
    "runModes",
    "cleanHome",
    "retention",
];
const RECT: &[&str] = &["x", "y", "width", "height"];

fn take_unknown(object: &mut Map<String, Value>, known: &[&str]) -> Fields {
    let keys: Vec<_> = object
        .keys()
        .filter(|key| !known.contains(&key.as_str()))
        .cloned()
        .collect();
    keys.into_iter()
        .map(|key| {
            let value = object.remove(&key).expect("a collected key exists");
            (key, value)
        })
        .collect()
}

fn geometry(object: &Map<String, Value>) -> String {
    let known: Map<_, _> = object
        .iter()
        .filter(|(key, _)| RECT.contains(&key.as_str()))
        .map(|(key, value)| {
            (
                key.clone(),
                value
                    .as_f64()
                    .map(Value::from)
                    .unwrap_or_else(|| value.clone()),
            )
        })
        .collect();
    Value::Object(known).to_string()
}

/// Visits only the objects this version understands. Malformed known values are left for
/// the typed parser, and unfamiliar fields never become executable settings.
fn objects(value: &mut Value, mut visit: impl FnMut(String, &mut Map<String, Value>, &[&str])) {
    if let Some(object) = value.get_mut("retention").and_then(Value::as_object_mut) {
        visit(
            "/retention".into(),
            object,
            &[
                "maxAgeDays",
                "maxRunsPerTest",
                "mediaMaxAgeDays",
                "maxTotalMb",
            ],
        );
    }
    if let Some(object) = value.get_mut("ai").and_then(Value::as_object_mut) {
        visit("/ai".into(), object, &["screen", "text"]);
        for task in ["screen", "text"] {
            if let Some(choice) = object.get_mut(task).and_then(Value::as_object_mut) {
                visit(format!("/ai/{task}"), choice, &["provider", "model"]);
            }
        }
    }
    for (array, identity, known) in [
        ("quarantine", "test", &["test", "reason", "until"][..]),
        ("runModes", "folder", &["folder", "run", "limit"][..]),
        ("hide", "", RECT),
    ] {
        if let Some(entries) = value.get_mut(array).and_then(Value::as_array_mut) {
            let mut totals = BTreeMap::<String, usize>::new();
            if identity.is_empty() {
                for object in entries.iter().filter_map(Value::as_object) {
                    *totals.entry(geometry(object)).or_default() += 1;
                }
            }
            let mut seen = BTreeMap::<String, usize>::new();
            for entry in entries {
                let Some(object) = entry.as_object_mut() else {
                    continue;
                };
                let key = if identity.is_empty() {
                    let geometry = geometry(object);
                    let total = totals[&geometry];
                    let occurrence = seen.entry(geometry.clone()).or_default();
                    let key = if total > 1 {
                        format!("{geometry}#{total}#{occurrence}")
                    } else {
                        geometry
                    };
                    *occurrence += 1;
                    key
                } else {
                    let Some(name) = object.get(identity).and_then(Value::as_str) else {
                        continue;
                    };
                    name.to_owned()
                };
                let escaped = key.replace('~', "~0").replace('/', "~1");
                visit(format!("/{array}/{escaped}"), object, known);
            }
        }
    }
}

impl ProjectMetadata {
    pub(super) fn read(mut value: Value) -> (Value, Self) {
        let mut metadata = Self::default();
        let root = value
            .as_object_mut()
            .expect("the checked header is an object");
        metadata.header.schema = root
            .remove("$schema")
            .and_then(|v| v.as_str().map(str::to_owned));
        metadata.header.min_version = root
            .remove("minVersion")
            .and_then(|v| v.as_str().map(str::to_owned));
        metadata.header.extra = take_unknown(root, TOP);
        objects(&mut value, |path, object, known| {
            let extra = take_unknown(object, known);
            if !extra.is_empty() {
                metadata.nested.insert(path, extra);
            }
        });
        (value, metadata)
    }

    pub(super) fn restore(&self, mut value: Value) -> Value {
        // The ai object can contain only unfamiliar task choices. Preserve that container
        // even when this reader has no screen/text choice of its own.
        if self.nested.contains_key("/ai") && value.get("ai").is_none() {
            value["ai"] = Value::Object(Map::new());
        }
        objects(&mut value, |path, object, _| {
            if let Some(extra) = self.nested.get(&path) {
                object.extend(extra.iter().map(|(k, v)| (k.clone(), v.clone())));
            }
        });
        let root = value
            .as_object_mut()
            .expect("settings serialize as an object");
        root.extend(
            self.header
                .extra
                .iter()
                .map(|(k, v)| (k.clone(), v.clone())),
        );
        if let Some(schema) = &self.header.schema {
            root.insert("$schema".into(), schema.clone().into());
        }
        if let Some(version) = &self.header.min_version {
            root.insert("minVersion".into(), version.clone().into());
        }
        value
    }

    pub(super) fn is_empty(&self) -> bool {
        self == &Self::default()
    }

    pub(super) fn warnings(&self) -> Vec<Issue> {
        let mut issues = self.header.warnings(PROJECT_FILE);
        for (path, fields) in &self.nested {
            for key in fields.keys() {
                issues.push(
                    Issue::warning("settings.field.unknown")
                        .with("file", PROJECT_FILE)
                        .with("name", format!("{path}/{key}")),
                );
            }
        }
        issues
    }
}
