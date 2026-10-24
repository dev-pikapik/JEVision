//! Retained notification settings; transport types stay strict about the values they use.

use super::*;
use jevision_core::{
    Issue, SettingsFileError, SettingsMetadata, parse_settings_document, write_file_atomically,
};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NotificationMetadata {
    pub root: SettingsMetadata,
    channels: BTreeMap<String, BTreeMap<String, Value>>,
}

impl NotificationSettings {
    pub fn parse_compatible(text: &str) -> Result<Self, SettingsFileError> {
        let mut value = parse_settings_document(text, FORMAT, None)?;
        reject_secrets(&value)?;
        migrate(&mut value).map_err(SettingsFileError::Invalid)?;
        let root = value.as_object_mut().expect("the header checked an object");
        // Format 2 has no global policy. Ignoring this known obsolete field could send
        // messages at times the author explicitly did not request.
        if root.contains_key("when") {
            return Err(SettingsFileError::Invalid(tr!(
                "notify.settings.globalWhen"
            )));
        }
        let known = ["format", "unattendedOnly", "slack", "telegram", "email"];
        let mut extra = root.clone();
        extra.retain(|key, _| !known.contains(&key.as_str()));
        root.retain(|key, _| known.contains(&key.as_str()));
        let mut channels = BTreeMap::new();
        for (name, fields) in [
            ("slack", &["when"][..]),
            ("telegram", &["chat", "when"][..]),
            (
                "email",
                &["server", "port", "security", "user", "from", "to", "when"][..],
            ),
        ] {
            if let Some(channel) = root.get_mut(name).and_then(Value::as_object_mut) {
                let extra: BTreeMap<_, _> = channel
                    .iter()
                    .filter(|(key, _)| !fields.contains(&key.as_str()))
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect();
                channel.retain(|key, _| fields.contains(&key.as_str()));
                if !extra.is_empty() {
                    channels.insert(name.to_owned(), extra);
                }
            }
        }
        let invalid = |e: serde_json::Error| SettingsFileError::Invalid(e.to_string());
        let file: File = serde_json::from_value(value).map_err(invalid)?;
        let settings = Self {
            unattended_only: file.unattended_only,
            slack: file.slack,
            telegram: file.telegram,
            email: file.email,
            metadata: NotificationMetadata {
                root: serde_json::from_value(extra.into()).map_err(invalid)?,
                channels,
            },
        };
        settings.check().map_err(SettingsFileError::Invalid)?;
        Ok(settings)
    }

    pub fn warnings(&self) -> Vec<Issue> {
        let mut warnings = self.metadata.root.warnings(FILE);
        for (channel, fields) in &self.metadata.channels {
            for name in fields.keys() {
                warnings.push(
                    Issue::warning("settings.field.unknown")
                        .with("file", FILE)
                        .with("name", format!("{channel}.{name}")),
                );
            }
        }
        warnings
    }

    /// Known executable choices and the optional fields belonging to channels still enabled.
    pub fn to_json(&self) -> Result<String, String> {
        self.check()?;
        let file = File {
            format: FORMAT,
            unattended_only: self.unattended_only,
            slack: self.slack,
            telegram: self.telegram.clone(),
            email: self.email.clone(),
        };
        let mut value = serde_json::to_value(file).expect("settings serialize");
        let root = value.as_object_mut().expect("settings are an object");
        let metadata = serde_json::to_value(&self.metadata.root).expect("metadata serializes");
        for (key, value) in metadata.as_object().expect("metadata is an object") {
            if root.contains_key(key) {
                return Err(format!("duplicate field `{key}`"));
            }
            root.insert(key.clone(), value.clone());
        }
        for (name, fields) in &self.metadata.channels {
            if let Some(channel) = root.get_mut(name).and_then(Value::as_object_mut) {
                for (key, value) in fields {
                    if channel.contains_key(key) {
                        return Err(format!("duplicate field `{key}`"));
                    }
                    channel.insert(key.clone(), value.clone());
                }
            }
        }
        // Catch invalid proposed headers and reserved fields before creating a file.
        let compact = value.to_string();
        Self::parse_compatible(&compact).map_err(|error| error.to_string())?;
        // Keep the version first, as in the other project settings files.
        #[derive(Serialize)]
        struct Ordered {
            format: u32,
            #[serde(flatten)]
            fields: serde_json::Map<String, Value>,
        }
        let mut fields = value.as_object().expect("settings are an object").clone();
        fields.remove("format");
        Ok(serde_json::to_string_pretty(&Ordered {
            format: FORMAT,
            fields,
        })
        .expect("settings serialize")
            + "\n")
    }

    /// Preserve the current file's compatible metadata and refuse unreadable files even
    /// when an editor tries to switch every channel off from a stale draft.
    pub fn save(&self, project: &Path) -> Result<(), NotifyError> {
        self.check()
            .map_err(|reason| NotifyError::Rejected { reason })?;
        let path = project.join(FILE);
        let mut saved = self.clone();
        match fs::read_to_string(&path) {
            Ok(text) => {
                saved.metadata = Self::parse_compatible(&text)
                    .map_err(|e| unreadable(e.to_string()))?
                    .metadata
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(unreadable(e.to_string())),
        }
        saved
            .metadata
            .channels
            .retain(|channel, _| match channel.as_str() {
                "slack" => saved.slack.is_some(),
                "telegram" => saved.telegram.is_some(),
                "email" => saved.email.is_some(),
                _ => false,
            });
        let text = saved
            .to_json()
            .map_err(|reason| NotifyError::Rejected { reason })?;
        if saved.channels().is_empty()
            && !saved.unattended_only
            && saved.metadata == NotificationMetadata::default()
        {
            return match fs::remove_file(&path) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => Err(unwritable(e.to_string())),
                _ => Ok(()),
            };
        }
        write_file_atomically(&path, text).map_err(|e| unwritable(e.to_string()))
    }
}

/// Explicit credential fields remain invalid, including inside future optional metadata.
/// The diagnostic names the field without ever repeating its value.
fn reject_secrets(value: &Value) -> Result<(), SettingsFileError> {
    match value {
        Value::Object(fields) => {
            for (key, value) in fields {
                let normalized = key.replace(['_', '-'], "").to_ascii_lowercase();
                if [
                    "webhook",
                    "webhookurl",
                    "slackwebhook",
                    "slackwebhookurl",
                    "token",
                    "bottoken",
                    "telegramtoken",
                    "password",
                    "smtppassword",
                    "secret",
                    "apikey",
                    "authorization",
                    "credentials",
                ]
                .contains(&normalized.as_str())
                {
                    return Err(SettingsFileError::Invalid(tr!(
                        "notify.settings.secret",
                        key
                    )));
                }
                reject_secrets(value)?;
            }
        }
        Value::Array(values) => {
            for value in values {
                reject_secrets(value)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Diagnostics are data, so clients can distinguish required updates from invalid settings.
pub fn settings_issues(project: &Path) -> Vec<Issue> {
    match fs::read_to_string(project.join(FILE)) {
        Ok(text) => match NotificationSettings::parse_compatible(&text) {
            Ok(settings) => settings.warnings(),
            Err(error) => vec![error.issue(FILE, "settings.notification.invalid")],
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(e) => vec![
            Issue::error("project.file.unreadable")
                .with("file", FILE)
                .with("reason", e),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "jevision-notify-compat-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn original() -> Value {
        json!({"format":2,"$schema":"https://example.test/notify.json","minVersion":"0.1.0+saved",
            "cursor":{"keep":[null,42]},"slack":{"when":"changed","extra":{"label":"qa"}},
            "telegram":{"chat":"-123","when":"every","extra":[1,2]},
            "email":{"server":"smtp.example.test","port":465,"from":"a@example.test","to":["b@example.test"],"extra":true}})
    }

    #[test]
    fn compatible_metadata_and_format_one_migration_round_trip() {
        let value = original();
        let settings = NotificationSettings::parse_compatible(&value.to_string()).unwrap();
        let saved: Value = serde_json::from_str(&settings.to_json().unwrap()).unwrap();
        for key in ["$schema", "minVersion", "cursor"] {
            assert_eq!(saved[key], value[key]);
        }
        for key in ["slack", "telegram", "email"] {
            assert_eq!(saved[key]["extra"], value[key]["extra"]);
        }
        assert_eq!(settings.warnings().len(), 4);
        let legacy = r#"{"format":1,"when":"every","slack":true,"telegram":{"chat":"-123","extra":{"keep":42}},"optional":true,"minVersion":"0.1.0"}"#;
        let migrated = NotificationSettings::parse_compatible(legacy).unwrap();
        let saved: Value = serde_json::from_str(&migrated.to_json().unwrap()).unwrap();
        assert_eq!(saved["format"], 2);
        assert!(saved.get("when").is_none());
        assert_eq!(saved["slack"]["when"], "every");
        assert_eq!(saved["telegram"]["when"], "every");
        assert_eq!(saved["telegram"]["extra"]["keep"], 42);
        assert_eq!(saved["optional"], true);
    }

    #[test]
    fn stale_saves_take_current_metadata_including_external_removals() {
        let project = project("latest");
        let file = project.join(FILE);
        let mut value = original();
        fs::write(&file, value.to_string()).unwrap();
        let mut stale = NotificationSettings::load(&project).unwrap();
        stale.slack.as_mut().unwrap().when = NotifyWhen::Failed;
        value.as_object_mut().unwrap().remove("$schema");
        value.as_object_mut().unwrap().remove("cursor");
        value["slack"].as_object_mut().unwrap().remove("extra");
        value["telegram"]["extra"] = json!({"changed":true});
        fs::write(&file, value.to_string()).unwrap();
        stale.save(&project).unwrap();
        let saved: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        assert!(saved.get("$schema").is_none() && saved.get("cursor").is_none());
        assert!(saved["slack"].get("extra").is_none());
        assert_eq!(saved["slack"]["when"], "failed");
        assert_eq!(saved["telegram"]["extra"], json!({"changed":true}));
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn channel_metadata_is_removed_with_its_channel_but_root_settings_remain() {
        let project = project("disable");
        let file = project.join(FILE);
        fs::write(&file, original().to_string()).unwrap();
        NotificationSettings::default().save(&project).unwrap();
        let saved = NotificationSettings::load(&project).unwrap();
        assert!(saved.channels().is_empty());
        assert!(saved.metadata.channels.is_empty());
        assert_eq!(
            saved.metadata.root.extra["cursor"]["keep"],
            json!([null, 42])
        );
        let mut enabled = saved;
        enabled.slack = Some(SlackChannel::default());
        enabled.save(&project).unwrap();
        let value: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        assert!(value["slack"].get("extra").is_none());
        fs::write(&file, r#"{"format":2}"#).unwrap();
        NotificationSettings {
            unattended_only: true,
            ..Default::default()
        }
        .save(&project)
        .unwrap();
        assert!(file.exists());
        NotificationSettings::default().save(&project).unwrap();
        assert!(!file.exists());
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn incompatible_or_malformed_files_refuse_stale_save_clear_and_notification_loading() {
        let project = project("refusals");
        let file = project.join(FILE);
        let stale = NotificationSettings::parse_compatible(&original().to_string()).unwrap();
        for text in [
            r#"{"format":99,"slack":true}"#,
            r#"{"format":1,"minVersion":"9999.0.0","email":false}"#,
            r#"{"format":2,"minVersion":null}"#,
            r#"{"format":2,"$schema":null}"#,
            r#"{"format":2,"slack":{"when":"future"}}"#,
            r#"{"format":2,"slack":{},"slack":{}}"#,
            r#"{"format":2,"optional":{"key":1,"key":2}}"#,
            r#"{"format":2,"when":"every"}"#,
            "{",
        ] {
            fs::write(&file, text).unwrap();
            assert!(stale.save(&project).is_err());
            assert!(NotificationSettings::default().save(&project).is_err());
            // No channel, secret or endpoint can be reached before this load is accepted.
            assert!(notify(&project, &[]).is_err());
            assert_eq!(fs::read_to_string(&file).unwrap(), text);
            assert_eq!(settings_issues(&project).len(), 1);
            if text.contains("9999") {
                assert_eq!(settings_issues(&project)[0].code, "settings.needsNewer");
            }
        }
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn credentials_are_rejected_even_inside_unrecognized_fields_without_echoing_values() {
        for text in [
            r#"{"format":2,"webhook":"private-value"}"#,
            r#"{"format":2,"telegram":{"chat":"1","botToken":"private-value"}}"#,
            r#"{"format":2,"email":{"password":"private-value"}}"#,
            r#"{"format":2,"optional":[{"Authorization":"private-value"}]}"#,
        ] {
            let error = NotificationSettings::parse_compatible(text)
                .unwrap_err()
                .to_string();
            assert!(!error.contains("private-value"));
        }
    }
}
