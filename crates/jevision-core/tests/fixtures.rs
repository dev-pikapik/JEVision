//! Saved compatibility anchors for tests, resource labels and release-critical project settings.
//! Supported files must keep opening, and edits must retain their optional metadata.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use jevision_core::*;
use serde_json::Value;
use sha2::{Digest, Sha256};

fn folder(kind: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(kind)
}

/// The fixtures of `kind`, named `format-<n><ending>`: their format and text, oldest first.
fn fixtures(kind: &str, ending: &str) -> Vec<(u32, String)> {
    let mut found: Vec<(u32, String)> = fs::read_dir(folder(kind))
        .unwrap()
        .filter_map(|entry| {
            let name = entry.unwrap().file_name().into_string().unwrap();
            let format = name
                .strip_prefix("format-")?
                .strip_suffix(ending)?
                .parse()
                .ok()?;
            Some((
                format,
                fs::read_to_string(folder(kind).join(&name)).unwrap(),
            ))
        })
        .collect();
    found.sort();
    found
}

fn formats(found: &[(u32, String)]) -> Vec<u32> {
    found.iter().map(|(format, _)| *format).collect()
}

#[test]
fn every_test_format_has_a_fixture_that_opens_and_saves_without_loss() {
    let found = fixtures("test", ".flow.json");
    assert_eq!(formats(&found), (1..=FORMAT_VERSION).collect::<Vec<_>>());
    for (format, text) in &found {
        let flow = parse(text).unwrap_or_else(|e| panic!("format {format}: {e:?}"));
        assert_eq!(Requirement::of(&flow).format, *format, "format {format}");
        let written = to_canonical_json(&flow);
        assert_eq!(parse(&written).unwrap(), flow, "format {format}");
        assert!(written.contains(&format!("\n  \"format\": {format},\n")));
    }
    let (_, newest) = found.last().unwrap();
    assert_eq!(&to_canonical_json(&parse(newest).unwrap()), newest);
}

#[test]
fn the_labels_in_the_format_2_fixture_name_the_files_of_the_format_1_one() {
    let project = folder("test");
    let read = |format: u32| {
        let text = fs::read_to_string(project.join(format!("format-{format}.flow.json")));
        parse(&text.unwrap()).unwrap()
    };
    let mut labeled = read(2);
    let labels = resolve_labels(&mut labeled, &project, Some(&project));
    assert_eq!(labels.issues, []);
    assert_eq!(labels.paths.len(), 9);
    assert_eq!(labeled, read(1));
    let issues = validate_in(&read(2), &project);
    assert!(
        issues.iter().all(|i| !i.code.starts_with("resource.")),
        "{issues:?}"
    );
}

#[test]
fn every_label_format_has_a_fixture_that_opens_and_saves_without_loss() {
    let found = fixtures("jevmeta", ".jevmeta");
    let (_, newest) = found.last().unwrap();
    let current = parse_meta(newest).unwrap().to_json();
    let current: serde_json::Value = serde_json::from_str(&current).unwrap();
    let current = u32::try_from(current["format"].as_u64().unwrap()).unwrap();
    assert_eq!(formats(&found), (1..=current).collect::<Vec<_>>());
    for (format, text) in &found {
        let meta = parse_meta(text).unwrap_or_else(|e| panic!("format {format}: {e:?}"));
        assert_eq!(
            parse_meta(&meta.to_json()).unwrap(),
            meta,
            "format {format}"
        );
    }
    assert_eq!(&parse_meta(newest).unwrap().to_json(), newest);
}

struct Project(PathBuf);

impl Project {
    fn new(kind: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "jevision-settings-fixture-{kind}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn write(&self, name: &str, text: &str) -> PathBuf {
        let file = self.0.join(name);
        fs::write(&file, text).unwrap();
        file
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn json(file: &Path) -> Value {
    serde_json::from_slice(&fs::read(file).unwrap()).unwrap()
}

#[test]
fn saved_environment_formats_keep_values_and_metadata_when_a_value_changes() {
    let found = fixtures("environments", ".json");
    assert_eq!(
        formats(&found),
        (1..=ENVIRONMENTS_FORMAT).collect::<Vec<_>>()
    );
    for (_, text) in found {
        let project = Project::new("environments");
        let file = project.write(ENVIRONMENTS_FILE, &text);
        let read = Environments::load(&project.0).unwrap();
        assert_eq!(read.default.as_deref(), Some("staging"));
        assert_eq!(read.all.len(), 2);
        assert_eq!(
            read.all["production"]["BASE_URL"],
            "https://production.example.test"
        );
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            text,
            "reading is read-only"
        );

        Environments::set_value(
            &project.0,
            "staging",
            "BASE_URL",
            "https://preview.example.test",
        )
        .unwrap();
        let mut expected: Value = serde_json::from_str(&text).unwrap();
        expected["format"] = ENVIRONMENTS_FORMAT.into();
        expected["environments"]["staging"]["BASE_URL"] = "https://preview.example.test".into();
        assert_eq!(
            json(&file),
            expected,
            "the edit retains all other values and optional fields"
        );
        let saved = Environments::load(&project.0).unwrap();
        assert_eq!(
            saved.all["staging"]["BASE_URL"],
            "https://preview.example.test"
        );
        assert_eq!(saved.default, read.default);
        assert_eq!(saved.metadata, read.metadata);
    }
}

#[test]
fn saved_project_formats_keep_owned_metadata_when_quarantine_is_edited() {
    let found = fixtures("project", ".json");
    assert_eq!(formats(&found), (1..=PROJECT_FORMAT).collect::<Vec<_>>());
    for (_, text) in found {
        let project = Project::new("project");
        let file = project.write(PROJECT_FILE, &text);
        let mut settings = ProjectSettings::load(&project.0).unwrap();
        assert!(settings.local_only && settings.clean_home);
        assert_eq!(settings.retries, Some(1));
        assert_eq!(settings.run_retention().max_age_days, Some(90));
        assert_eq!(settings.ai(AiTask::Screen).unwrap().provider, "ollama");
        assert_eq!(
            settings.run_mode("smoke/cart"),
            RunMode::Parallel { limit: 2 }
        );
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            text,
            "reading is read-only"
        );

        settings
            .quarantine(
                "smoke/checkout.flow.json",
                Some("Updated triage"),
                Some("2099-12-31"),
            )
            .unwrap();
        settings.save(&project.0).unwrap();
        let mut expected: Value = serde_json::from_str(&text).unwrap();
        expected["format"] = PROJECT_FORMAT.into();
        expected["quarantine"][0]["reason"] = "Updated triage".into();
        assert_eq!(
            json(&file),
            expected,
            "root, retention, privacy, AI, test and folder metadata survive"
        );
        let saved = ProjectSettings::load(&project.0).unwrap();
        assert_eq!(
            saved
                .quarantined("smoke/checkout.flow.json", "2099-01-01")
                .unwrap()
                .reason
                .as_deref(),
            Some("Updated triage")
        );
        assert_eq!(saved.metadata, settings.metadata);
        assert_eq!(saved.hide, settings.hide);
    }
}

#[test]
fn saved_schedule_formats_keep_targets_and_metadata_when_the_time_changes() {
    let found = fixtures("schedule", ".json");
    assert_eq!(formats(&found), (1..=SCHEDULE_FORMAT).collect::<Vec<_>>());
    for (_, text) in found {
        let project = Project::new("schedule");
        let file = project.write(SCHEDULE_FILE, &text);
        let mut schedules = load_schedules(&project.0).unwrap();
        assert_eq!(schedules.len(), 1);
        assert_eq!(
            schedules[0].tests,
            ScheduleTests::Files(vec![
                "smoke/checkout.flow.json".into(),
                "smoke/profile.flow.json".into()
            ])
        );
        assert_eq!(
            schedules[0].repeat,
            Repeat::Days(vec![Weekday::Mon, Weekday::Fri])
        );
        assert_eq!(schedules[0].environment.as_deref(), Some("staging"));
        assert_eq!(schedules[0].video.as_deref(), Some("on-failure"));
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            text,
            "reading is read-only"
        );

        schedules[0].at = TimeOfDay::new(10, 45).unwrap();
        save_schedules(&project.0, &schedules).unwrap();
        let mut expected: Value = serde_json::from_str(&text).unwrap();
        expected["format"] = SCHEDULE_FORMAT.into();
        expected["schedules"][0]["at"] = "10:45".into();
        assert_eq!(
            json(&file),
            expected,
            "the schedule keeps its owner metadata and all execution choices"
        );
        assert_eq!(load_schedules(&project.0).unwrap(), schedules);
    }
}

#[test]
fn saved_package_and_lock_keep_provenance_when_an_installed_package_is_updated() {
    let found = fixtures("package-lock", ".json");
    assert_eq!(formats(&found), (1..=PACKAGE_FORMAT).collect::<Vec<_>>());
    for (format, text) in found {
        let fixture = folder("package").join(format!("format-{format}.jevblock"));
        let manifest_text = fs::read_to_string(fixture.join(MANIFEST_FILE)).unwrap();
        let flow_bytes = fs::read(fixture.join(PACKAGE_FLOW_FILE)).unwrap();
        let package = read_package(&fixture).unwrap();
        assert!(!package.has_errors(), "{:?}", package.issues);
        assert_eq!(package.manifest.id, "org.example.greeting");
        assert_eq!(package.manifest.version, Version::new(1, 2, 0));
        assert_eq!(
            package.manifest.inputs[0].default_value.as_deref(),
            Some("Hello fixture")
        );
        assert_eq!(
            serde_json::from_str::<Value>(&package.manifest.to_json()).unwrap(),
            serde_json::from_str::<Value>(&manifest_text).unwrap()
        );
        assert_eq!(
            fs::read_to_string(fixture.join(MANIFEST_FILE)).unwrap(),
            manifest_text
        );
        assert_eq!(
            fs::read(fixture.join(PACKAGE_FLOW_FILE)).unwrap(),
            flow_bytes
        );

        let project = Project::new("package");
        let lock_file = project.write(PACKAGE_LOCK_FILE, &text);
        let lock = PackageLock::parse(&text).unwrap();
        let id = &package.manifest.id;
        let source = &lock.sources[id];
        assert_eq!(lock.versions[id], package.manifest.version);
        assert_eq!(
            BlockOrigin::of_installed(Some(source)).author_matches(&package.manifest.author),
            Some(true)
        );
        assert_eq!(
            fs::read_to_string(&lock_file).unwrap(),
            text,
            "reading is read-only"
        );
        let mut expected: Value = serde_json::from_str(&text).unwrap();
        // The checksum is opaque lock metadata, not a new executable manifest field.
        assert_eq!(
            expected["provenance"][id]["files"][PACKAGE_FLOW_FILE]["sha256"],
            Sha256::digest(&flow_bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        );

        let original = package_folder(&project.0, id);
        let update_source = project.0.join("update.jevblock");
        for destination in [&original, &update_source] {
            fs::create_dir_all(destination).unwrap();
            fs::write(destination.join(MANIFEST_FILE), &manifest_text).unwrap();
            fs::write(destination.join(PACKAGE_FLOW_FILE), &flow_bytes).unwrap();
        }
        let mut manifest = package.manifest.clone();
        manifest.version = Version::new(1, 2, 1);
        fs::write(update_source.join(MANIFEST_FILE), manifest.to_json()).unwrap();
        let update = install_folder(&project.0, &update_source, Some(source), true).unwrap();
        assert_eq!(update.manifest, manifest);
        expected["format"] = PACKAGE_FORMAT.into();
        expected["versions"][id] = "1.2.1".into();
        assert_eq!(
            json(&lock_file),
            expected,
            "the updated id retains its source, publisher and checksum metadata"
        );
        let saved = PackageLock::parse(&fs::read_to_string(&lock_file).unwrap()).unwrap();
        assert_eq!(saved.versions[id], Version::new(1, 2, 1));
        assert_eq!(saved.sources, lock.sources);
        assert_eq!(saved.metadata, lock.metadata);
        assert_eq!(read_package(&original).unwrap().flow, package.flow);
        assert_eq!(installed(&project.0, id).unwrap().manifest, manifest);
    }
}
