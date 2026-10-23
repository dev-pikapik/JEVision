//! One saved file for every format of every kind of file JEVision writes, kept as it was
//! written. Each must keep opening for as long as JEVision reads its format, so a change to a
//! format that would break files people already have fails here first.

use std::fs;
use std::path::{Path, PathBuf};

use jevision_core::*;

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
