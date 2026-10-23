//! `jevision check`: a project must carry every file its tests read, under the same names, so it
//! runs the same after a copy, a clone or on CI.

use std::fs;
use std::path::{Path, PathBuf};

use jevision_core::*;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("jevision-check-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn put(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn test_with(blocks: &str) -> String {
    format!(
        r#"{{"format": 1, "name": "t", "app": "com.example.App", "start": "a", "blocks": {{{blocks}}}}}"#
    )
}

fn click(id: &str, image: &str) -> String {
    format!(r#""{id}": {{"type": "click", "target": {{"image": "{image}"}}}}"#)
}

/// `(code, block, error)` of each problem found in `file`.
fn found(checked: &[CheckedTest], file: &str) -> Vec<(String, String, bool)> {
    let test = checked
        .iter()
        .find(|t| t.file == file)
        .unwrap_or_else(|| panic!("{file} wasn't checked"));
    let mut all: Vec<_> = test
        .issues
        .iter()
        .map(|i| {
            let block = i.param("block").unwrap_or_default().to_owned();
            (i.code.clone(), block, i.severity == Severity::Error)
        })
        .collect();
    all.sort();
    all
}

/// A project whose test `login/sign-in.flow.json` reads files in every way that breaks on
/// another computer, and one that's fine.
fn project(name: &str) -> (PathBuf, PathBuf) {
    let project = temp(name);
    let elsewhere = temp(&format!("{name}-elsewhere"));
    fs::create_dir_all(project.join(".jevision")).unwrap();
    put(&project.join("Data/users.csv"), "name\nAnn\n");
    put(&project.join("images/ok.png"), "png");
    put(&project.join("images/ok.dark.png"), "png");
    put(&elsewhere.join("button.png"), "png");
    std::os::unix::fs::symlink(
        elsewhere.join("button.png"),
        project.join("images/linked.png"),
    )
    .unwrap();
    fs::File::create(project.join("images/huge.png"))
        .unwrap()
        .set_len(LARGE_FILE_BYTES)
        .unwrap();
    let blocks = [
        r#""a": {"type": "useData", "file": "../data/users.csv", "body": "b"}"#.to_owned(),
        click("absolute", "/Users/ann/button.png"),
        click("home", "~/button.png"),
        click("backslash", r"..\\images\\ok.png"),
        click("outside", "../../elsewhere/button.png"),
        click("link", "../images/linked.png"),
        click("large", "../images/huge.png"),
        click("missing", "../images/gone.png"),
        click("variable", "${var.folder}/button.png"),
        click("named", "../images/${var.name}.png"),
        click("absoluteVariable", "/tmp/${var.name}.png"),
        click("fine", "../images/ok.png"),
        r#""web": {"type": "openFile", "files": ["https://example.com"]}"#.to_owned(),
        r#""script": {"type": "command", "command": "make", "folder": "../scripts"}"#.to_owned(),
        r#""here": {"type": "command", "command": "make", "folder": "."}"#.to_owned(),
        r#""reference": {"type": "check", "reference": {"image": "../refs/new.png"}}"#.to_owned(),
        r#""sub": {"type": "runTest", "test": "../Fine.flow.json"}"#.to_owned(),
    ];
    put(
        &project.join("login/sign-in.flow.json"),
        &test_with(&blocks.join(", ")),
    );
    put(
        &project.join("fine.flow.json"),
        &test_with(&click("a", "images/ok.png")),
    );
    put(&project.join("broken.flow.json"), "{ not a test");
    put(
        &project.join(".hidden/skipped.flow.json"),
        &test_with(&click("a", "/nowhere.png")),
    );
    (project, elsewhere)
}

fn issue(code: &str, block: &str, error: bool) -> (String, String, bool) {
    (code.to_owned(), block.to_owned(), error)
}

#[test]
fn every_way_a_path_breaks_elsewhere_is_found() {
    let (project, elsewhere) = project("all");
    let checked = check_project(&project);

    let files: Vec<&str> = checked.iter().map(|t| t.file.as_str()).collect();
    assert_eq!(
        files,
        [
            "broken.flow.json",
            "fine.flow.json",
            "login/sign-in.flow.json"
        ]
    );
    assert_eq!(found(&checked, "fine.flow.json"), []);
    assert!(
        found(&checked, "broken.flow.json")
            .iter()
            .all(|(_, _, error)| *error)
    );
    assert!(!found(&checked, "broken.flow.json").is_empty());

    let mut want = vec![
        issue("check.path.case", "a", true),
        issue("check.path.absolute", "absolute", true),
        issue("check.path.absolute", "home", true),
        issue("check.path.backslash", "backslash", true),
        issue("check.path.outside", "outside", true),
        issue("check.path.link", "link", true),
        issue("check.file.large", "large", false),
        issue("check.file.missing", "missing", true),
        issue("check.path.absolute", "absoluteVariable", true),
        issue("check.folder.missing", "script", true),
        issue("flow.reference.notSaved", "reference", false),
        issue("check.path.case", "sub", true),
    ];
    want.sort();
    assert_eq!(found(&checked, "login/sign-in.flow.json"), want);

    let test = checked
        .iter()
        .find(|t| t.file == "login/sign-in.flow.json")
        .unwrap();
    let case = test.issues.iter().find(|i| i.code == "check.path.case");
    assert_eq!(
        case.and_then(|i| i.param("actual")),
        Some("Data/users.csv"),
        "the name on disk, from the project folder"
    );
    let missing = test.issues.iter().find(|i| i.code == "check.file.missing");
    assert_eq!(
        describe(missing.unwrap()),
        "Block “missing”, field “target.image”: Missing file “images/gone.png” in the project."
    );
    fs::remove_dir_all(project).unwrap();
    fs::remove_dir_all(elsewhere).unwrap();
}

#[test]
fn only_the_home_folder_and_a_drive_count_as_elsewhere() {
    let project = temp("names");
    fs::create_dir_all(project.join(".jevision")).unwrap();
    put(&project.join("~notes.png"), "png");
    put(&project.join("a:b.png"), "png");
    let blocks = [
        click("tilde", "~notes.png"),
        click("colon", "a:b.png"),
        // The runner reads `~user/…` as a folder of the project: only `~/…` is the home folder.
        click("user", "~ann/button.png"),
        click("homeFile", "~/button.png"),
        r#""home": {"type": "command", "command": "make", "folder": "~"}"#.to_owned(),
        click("drive", "C:/button.png"),
        click("driveBackslash", r"c:\\button.png"),
    ];
    put(&project.join("t.flow.json"), &test_with(&blocks.join(", ")));
    let mut want = vec![
        issue("check.file.missing", "user", true),
        issue("check.path.absolute", "homeFile", true),
        issue("check.path.absolute", "home", true),
        issue("check.path.absolute", "drive", true),
        issue("check.path.absolute", "driveBackslash", true),
    ];
    want.sort();
    assert_eq!(found(&check_project(&project), "t.flow.json"), want);
    fs::remove_dir_all(project).unwrap();
}

#[test]
fn validate_warns_about_what_check_fails() {
    let (project, elsewhere) = project("validate");
    let dir = project.join("login");
    let flow = parse(&fs::read_to_string(dir.join("sign-in.flow.json")).unwrap()).unwrap();
    let issues = validate_in(&flow, &dir);
    for code in [
        "check.path.case",
        "check.path.outside",
        "check.path.link",
        "check.file.large",
    ] {
        let issue = issues.iter().find(|i| i.code == code);
        assert_eq!(
            issue.map(|i| i.severity),
            Some(Severity::Warning),
            "{code}: {issues:?}"
        );
    }
    fs::remove_dir_all(project).unwrap();
    fs::remove_dir_all(elsewhere).unwrap();
}

#[test]
fn linked_tests_are_checked_without_reading_tests_outside_the_project() {
    let root = temp("linked-tests");
    let outside = temp("linked-tests-outside");
    put(
        &root.join("original.flow.json"),
        &test_with(&click("a", "missing.png")),
    );
    put(&outside.join("outside.flow.json"), "not valid JSON");
    std::os::unix::fs::symlink(
        root.join("original.flow.json"),
        root.join("linked.flow.json"),
    )
    .unwrap();
    std::os::unix::fs::symlink(
        outside.join("outside.flow.json"),
        root.join("outside.flow.json"),
    )
    .unwrap();
    std::os::unix::fs::symlink(root.join("gone.flow.json"), root.join("broken.flow.json")).unwrap();
    let checked = check_project(&root);
    assert!(
        found(&checked, "linked.flow.json")
            .iter()
            .any(|(code, _, error)| code == "check.file.missing" && *error)
    );
    assert!(
        found(&checked, "outside.flow.json")
            .iter()
            .any(|(code, _, error)| code == "check.path.link" && *error)
    );
    assert!(
        found(&checked, "broken.flow.json")
            .iter()
            .any(|(code, _, error)| code == "check.file.missing" && *error)
    );
    assert_eq!(
        fs::read_to_string(outside.join("outside.flow.json")).unwrap(),
        "not valid JSON"
    );
}
