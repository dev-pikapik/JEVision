//! Headers and optional fields shared by forward-compatible settings files.

use std::collections::BTreeMap;
use std::fmt;

use jevision_l10n::tr;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::issue::{Issue, json_problem};
use crate::{Version, app_version};

/// Optional headers and fields a settings reader must keep even when it does not use them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub struct SettingsMetadata {
    #[serde(default, rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    #[serde(
        default,
        rename = "minVersion",
        skip_serializing_if = "Option::is_none"
    )]
    pub min_version: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl SettingsMetadata {
    pub(crate) fn warnings(&self, file: &str) -> Vec<Issue> {
        self.extra
            .keys()
            .map(|name| {
                Issue::warning("settings.field.unknown")
                    .with("file", file)
                    .with("name", name)
            })
            .collect()
    }
}

/// An unreadable settings file, distinguished from one that needs a newer reader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsFileError {
    Invalid(String),
    TooNew { found: u64, supported: u32 },
    NeedsNewer { version: String },
    InvalidVersion { value: String },
}

impl fmt::Display for SettingsFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(reason) => f.write_str(reason),
            Self::TooNew { found, supported } => {
                f.write_str(&tr!("project.format.tooNew", *found, *supported))
            }
            Self::NeedsNewer { version } => f.write_str(&tr!("settings.needsNewer", version)),
            Self::InvalidVersion { value } => f.write_str(&tr!("flow.minVersion.invalid", value)),
        }
    }
}

impl std::error::Error for SettingsFileError {}

impl SettingsFileError {
    pub fn requires_newer(&self) -> bool {
        matches!(self, Self::TooNew { .. } | Self::NeedsNewer { .. })
    }

    /// A structured diagnostic for validation, reports and command-line exit statuses.
    pub fn issue(&self, file: &str, invalid_code: &str) -> Issue {
        let issue = match self {
            Self::Invalid(reason) => Issue::error(invalid_code).with("reason", reason),
            Self::TooNew { found, supported } => Issue::error("settings.format.tooNew")
                .with("found", found)
                .with("supported", supported),
            Self::NeedsNewer { version } => {
                Issue::error("settings.needsNewer").with("version", version)
            }
            Self::InvalidVersion { value } => {
                Issue::error("settings.minVersion.invalid").with("value", value)
            }
        };
        issue.with("file", file)
    }
}

/// Check requirements before decoding the payload: newer files may have unfamiliar fields
/// or values. The typed reader still checks every field it understands afterwards.
pub(crate) fn check_settings_header(
    text: &str,
    supported: u32,
) -> Result<Value, SettingsFileError> {
    check_settings_value(read_settings_json(text)?, supported, None)
}

pub(crate) fn read_settings_json(text: &str) -> Result<Value, SettingsFileError> {
    serde_json::from_str::<UniqueValue>(text)
        .map(|value| value.0)
        .map_err(|error| SettingsFileError::Invalid(json_problem(&error)))
}

/// A missing format is accepted only by explicitly legacy readers.
pub(crate) fn check_settings_value(
    mut value: Value,
    supported: u32,
    legacy: Option<u32>,
) -> Result<Value, SettingsFileError> {
    if let (Some(object), Some(format)) = (value.as_object_mut(), legacy) {
        object.entry("format").or_insert_with(|| format.into());
    }
    let object = value
        .as_object()
        .ok_or_else(|| SettingsFileError::Invalid(tr!("flow.json.notObject")))?;
    let format = object
        .get("format")
        .and_then(Value::as_u64)
        .filter(|format| *format > 0)
        .ok_or_else(|| SettingsFileError::Invalid(tr!("project.format.invalid", supported)))?;
    if format > u64::from(supported) {
        return Err(SettingsFileError::TooNew {
            found: format,
            supported,
        });
    }
    if let Some(value) = object.get("minVersion") {
        let version = value
            .as_str()
            .and_then(|version| version.parse::<Version>().ok())
            .ok_or_else(|| SettingsFileError::InvalidVersion {
                value: value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| value.to_string()),
            })?;
        if version > app_version() {
            return Err(SettingsFileError::NeedsNewer {
                version: version.to_string(),
            });
        }
    }
    if object
        .get("$schema")
        .is_some_and(|value| !value.is_string())
    {
        return Err(SettingsFileError::Invalid(tr!(
            "flow.field.type",
            "$schema"
        )));
    }
    Ok(value)
}

/// Reject duplicate keys before reducing the JSON to known fields. Otherwise two conflicting
/// values could collapse into one before the typed settings reader sees them.
pub(crate) struct UniqueValue(pub(crate) Value);
impl<'de> Deserialize<'de> for UniqueValue {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::{Error, MapAccess, SeqAccess, Visitor};
        struct Json;
        impl<'de> Visitor<'de> for Json {
            type Value = UniqueValue;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("JSON with unique object keys")
            }
            fn visit_bool<E: Error>(self, v: bool) -> Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }
            fn visit_i64<E: Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }
            fn visit_u64<E: Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }
            fn visit_f64<E: Error>(self, v: f64) -> Result<Self::Value, E> {
                serde_json::Number::from_f64(v)
                    .map(|v| UniqueValue(Value::Number(v)))
                    .ok_or_else(|| E::custom("non-finite number"))
            }
            fn visit_str<E: Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }
            fn visit_unit<E: Error>(self) -> Result<Self::Value, E> {
                Ok(UniqueValue(Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(UniqueValue(value)) = seq.next_element()? {
                    values.push(value);
                }
                Ok(UniqueValue(Value::Array(values)))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut values = serde_json::Map::new();
                while let Some((key, UniqueValue(value))) =
                    map.next_entry::<String, UniqueValue>()?
                {
                    if values.insert(key.clone(), value).is_some() {
                        return Err(A::Error::custom(format!("duplicate field `{key}`")));
                    }
                }
                Ok(UniqueValue(Value::Object(values)))
            }
        }
        deserializer.deserialize_any(Json)
    }
}
