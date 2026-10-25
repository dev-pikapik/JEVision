//! Notification files retained as compatibility examples for the 1.0 reader and editor.

use std::fs;
use std::path::PathBuf;

use jevision_notify::{FILE, FORMAT, MailSecurity, NotificationSettings, NotifyWhen};
use serde_json::{Value, json};

struct Project(PathBuf);

impl Project {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "jevision-notify-fixture-{name}-{}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn saved_notification_versions_keep_channel_choices_and_metadata_after_an_edit() {
    for (name, text, expected_when) in [
        (
            "format-1",
            include_str!("fixtures/release-1.0/notifications-1.json"),
            [
                NotifyWhen::Changed,
                NotifyWhen::Changed,
                NotifyWhen::Changed,
            ],
        ),
        (
            "format-2",
            include_str!("fixtures/release-1.0/notifications-2.json"),
            [NotifyWhen::Failed, NotifyWhen::Every, NotifyWhen::Changed],
        ),
    ] {
        let project = Project::new(name);
        let file = project.0.join(FILE);
        fs::write(&file, text).unwrap();
        let original: Value = serde_json::from_str(text).unwrap();
        let mut settings = NotificationSettings::load(&project.0).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), text, "reading {name}");
        assert!(settings.unattended_only);
        assert_eq!(settings.slack.unwrap().when, expected_when[0]);
        let telegram = settings.telegram.as_ref().unwrap();
        assert_eq!(telegram.chat, "-1001234567890");
        assert_eq!(telegram.when, expected_when[1]);
        let email = settings.email.as_ref().unwrap();
        assert_eq!(email.server, "smtp.example.test");
        assert_eq!(email.port, 587);
        assert_eq!(email.security, MailSecurity::StartTls);
        assert_eq!(email.user, "tests@example.test");
        assert_eq!(email.from, "tests@example.test");
        assert_eq!(email.to, ["qa@example.test"]);
        assert_eq!(email.when, expected_when[2]);

        settings.email.as_mut().unwrap().to = vec!["release@example.test".into()];
        settings.save(&project.0).unwrap();
        assert_eq!(NotificationSettings::load(&project.0).unwrap(), settings);
        let saved: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(saved["format"], FORMAT);
        assert!(saved.get("when").is_none(), "global policy is migrated");
        assert_eq!(saved["email"]["to"], json!(["release@example.test"]));
        for key in ["$schema", "minVersion", "review"] {
            assert_eq!(saved[key], original[key], "{name}: {key}");
        }
        assert_eq!(saved["telegram"]["label"], original["telegram"]["label"]);
        if name == "format-2" {
            assert_eq!(saved["slack"]["label"], original["slack"]["label"]);
        }
    }
}
