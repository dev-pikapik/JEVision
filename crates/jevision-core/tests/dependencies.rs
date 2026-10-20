use std::fs;
use std::path::{Path, PathBuf};

use jevision_core::*;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("jevision-deps-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn codes(issues: &[Issue]) -> Vec<&str> {
    issues.iter().map(|i| i.code.as_str()).collect()
}

/// `block.json` of the package `id`; `more` goes after the required fields.
fn manifest_text(id: &str, version: &str, more: &str) -> String {
    format!(
        r#"{{"format": 1, "id": "{id}", "name": "{id}", "version": "{version}",
            "author": "Acme", "license": "MIT"{more}}}"#
    )
}

/// A package `id` that needs the packages `needs`, as `(id, range)`.
fn manifest(id: &str, version: &str, needs: &[(&str, &str)]) -> BlockManifest {
    let list: Vec<String> = needs
        .iter()
        .map(|(id, range)| format!(r#"{{"id": "{id}", "version": "{range}"}}"#))
        .collect();
    let more = format!(r#", "dependencies": [{}]"#, list.join(", "));
    parse_manifest(&manifest_text(id, version, &more)).unwrap()
}

fn steps(blocks: &str) -> String {
    format!(
        r#"{{"format": 1, "name": "t", "app": "com.example.App", "start": "a", "blocks": {blocks}}}"#
    )
}

/// Puts the package `id` in the blocks of `project` as if it were installed.
fn put(project: &Path, id: &str, version: &str, more: &str, blocks: &str) {
    put_package(project, id, &manifest_text(id, version, more), blocks);
}

fn put_package(project: &Path, id: &str, manifest: &str, blocks: &str) {
    let dir = package_folder(project, id);
    fs::create_dir_all(dir.join("images")).unwrap();
    fs::write(dir.join(MANIFEST_FILE), manifest).unwrap();
    fs::write(dir.join(PACKAGE_FLOW_FILE), steps(blocks)).unwrap();
}

const NOTHING: &str = r#"{"a": {"type": "pressKeys", "keys": "cmd+s"}}"#;

fn run(test: &str) -> String {
    format!(r#"{{"a": {{"type": "runTest", "test": "{test}"}}}}"#)
}

#[test]
fn version_ranges_follow_the_cargo_rules() {
    let fits = |range: &str, version: &str| {
        let range: VersionReq = range.parse().unwrap();
        range.matches(&version.parse().unwrap())
    };
    let cases: &[(&str, &[&str], &[&str])] = &[
        ("*", &["0.0.1", "1.2.3", "99.0.0"], &["1.0.0-beta"]),
        ("1.2.3", &["1.2.3"], &["1.2.4", "1.2.2", "1.2.3-rc.1"]),
        ("=1.2.3", &["1.2.3"], &["1.3.0"]),
        (
            "^1.2.3",
            &["1.2.3", "1.9.0"],
            &["1.2.2", "2.0.0", "2.0.0-alpha"],
        ),
        ("^0.2.3", &["0.2.3", "0.2.9"], &["0.2.2", "0.3.0"]),
        ("^0.0.3", &["0.0.3"], &["0.0.4", "0.0.2"]),
        ("^1", &["1.0.0", "1.9.9"], &["2.0.0", "0.9.0"]),
        ("^0", &["0.0.0", "0.9.9"], &["1.0.0"]),
        ("^0.0", &["0.0.9"], &["0.1.0"]),
        ("~1.2.3", &["1.2.3", "1.2.9"], &["1.2.2", "1.3.0"]),
        ("~1", &["1.0.0", "1.9.0"], &["2.0.0"]),
        (">=1.2.0 <2.0.0", &["1.2.0", "1.99.0"], &["1.1.9", "2.0.0"]),
        (">=1.2.0, <2.0.0", &["1.5.0"], &["2.0.0"]),
        (">= 1.2.0 , < 2.0.0", &["1.5.0"], &["2.0.0"]),
        (">1.2.0", &["1.2.1"], &["1.2.0"]),
        (">=1.2.0", &["1.2.0", "7.0.0"], &["1.1.9"]),
        ("<1.2.0", &["1.1.9"], &["1.2.0", "1.2.0-beta"]),
        ("<=1.2.0", &["1.2.0"], &["1.2.1"]),
        (">1.2", &["1.3.0"], &["1.2.9"]),
        ("<=1.2", &["1.2.9"], &["1.3.0"]),
        ("<1", &["0.9.9"], &["1.0.0"]),
        ("1.x", &["1.0.0", "1.9.9"], &["0.9.0", "2.0.0"]),
        ("1.2.x", &["1.2.0", "1.2.7"], &["1.3.0"]),
        ("1.2.*", &["1.2.7"], &["1.3.0"]),
        ("1.X", &["1.4.0"], &["2.0.0"]),
        (
            ">=1.2.0-beta.1",
            &["1.2.0-beta.2", "1.2.0", "1.3.0"],
            &["1.2.0-alpha", "1.3.0-alpha"],
        ),
    ];
    for (range, yes, no) in cases {
        for version in *yes {
            assert!(fits(range, version), "{range} should take {version}");
        }
        for version in *no {
            assert!(!fits(range, version), "{range} should refuse {version}");
        }
    }

    for text in ["^1.2.0", ">= 1.2.0, <2.0.0", "1.x", "*"] {
        assert_eq!(text.parse::<VersionReq>().unwrap().to_string(), text);
    }
    for bad in [
        "1.2.3.4",
        "abc",
        "^",
        ">=",
        "01.2.3",
        "1.x.2",
        "^*",
        ">*",
        "~1.2.3 latest",
    ] {
        let error = bad.parse::<VersionReq>().unwrap_err();
        assert!(error.contains("isn't a version range"), "{bad}: {error}");
    }
    assert!(
        "".parse::<VersionReq>()
            .unwrap_err()
            .contains("write * for any version")
    );
}

#[test]
fn a_manifest_lists_the_packages_it_needs() {
    let text = manifest_text(
        "com.acme.app",
        "1.0.0",
        r#", "dependencies": [
            {"id": "com.acme.helper", "version": "^1.2.0", "source": "https://github.com/acme/helper"},
            {"id": "com.acme.other"}
        ]"#,
    );
    let read = parse_manifest(&text).unwrap();
    assert_eq!(read.dependencies.len(), 2);
    assert_eq!(read.dependencies[0].version.to_string(), "^1.2.0");
    assert_eq!(
        read.dependencies[0].source.as_deref(),
        Some("https://github.com/acme/helper")
    );
    assert_eq!(read.dependencies[1].version.to_string(), "*");
    assert_eq!(read.dependencies[1].source, None);
    assert_eq!(parse_manifest(&read.to_json()).unwrap(), read);
    assert!(read.to_json().contains(r#""version": "*""#));
    assert!(
        !manifest("com.acme.app", "1.0.0", &[])
            .to_json()
            .contains("dependencies")
    );

    let issues = parse_manifest(&manifest_text(
        "com.acme.app",
        "1.0.0",
        r#", "dependencies": [
            {"id": "Helper"},
            {"id": "com.acme.a", "version": "^x"},
            {"id": "com.acme.b", "source": "http://acme.test/b"},
            {"id": "com.acme.app"},
            {"id": "com.acme.c"},
            {"id": "com.acme.c", "version": "1.x"},
            {"id": "com.acme.d", "url": "https://acme.test/d"},
            "com.acme.e",
            {"version": "1.0.0"}
        ]"#,
    ))
    .unwrap_err()
    .issues;
    let found: Vec<(&str, &str)> = issues
        .iter()
        .map(|i| (i.code.as_str(), i.param("field").unwrap_or("")))
        .collect();
    assert_eq!(
        found,
        [
            ("block.dependency.invalid", "dependencies.0"),
            ("block.dependency.invalid", "dependencies.1"),
            ("block.dependency.invalid", "dependencies.2"),
            ("block.dependency.self", "dependencies.3"),
            ("block.dependency.duplicate", "dependencies.5"),
            ("block.field.unknown", "dependencies.6"),
            ("block.field.type", "dependencies.7"),
            ("block.field.missing", "dependencies.8"),
        ]
    );
    let said: Vec<String> = issues.iter().filter_map(describe_package_issue).collect();
    assert!(said[1].contains("isn't a version range"), "{}", said[1]);
    assert!(said[2].contains("https"), "{}", said[2]);
    assert!(said[4].contains("listed twice"), "{}", said[4]);
}

#[test]
fn a_package_runs_only_the_packages_it_declares() {
    let manifest = manifest("com.acme.app", "1.0.0", &[("com.acme.helper", "^1.0.0")]);
    let dir = Path::new("/nonexistent/blocks/com.acme.app.jevblock");
    // The folder isn't there, so the files the steps name are missing as well.
    let check = |test: &str| -> Vec<Issue> {
        let flow = parse(&steps(&run(test))).unwrap();
        (check_package(&manifest, &flow, dir).into_iter())
            .filter(|i| i.code != "flow.file.missing")
            .collect()
    };
    assert!(check("../com.acme.helper.jevblock/flow.json").is_empty());
    let undeclared = check("../com.acme.other.jevblock/flow.json");
    assert_eq!(codes(&undeclared), ["block.dependency.undeclared"]);
    assert_eq!(undeclared[0].param("id"), Some("com.acme.other"));
    assert_eq!(undeclared[0].param("block"), Some("a"));
    assert!(
        describe_package_issue(&undeclared[0])
            .unwrap()
            .starts_with("Block \"a\": runs ../com.acme.other.jevblock/flow.json")
    );
    assert_eq!(
        codes(&check("../shared/close.flow.json")),
        ["block.path.outside"]
    );
    assert_eq!(
        codes(&check("../com.acme.helper.jevblock/parts/inner.flow.json")),
        ["block.path.outside"]
    );
}

#[test]
fn a_set_installs_dependencies_first_and_says_what_stops_it() {
    let base = manifest("com.acme.base", "1.0.0", &[]);
    let helper = manifest("com.acme.helper", "1.2.0", &[("com.acme.base", "*")]);
    let app = manifest("com.acme.app", "1.0.0", &[("com.acme.helper", "^1.0.0")]);
    let set = [app.clone(), helper.clone(), base.clone()];
    assert_eq!(
        install_order(&[], &set, false).unwrap(),
        ["com.acme.base", "com.acme.helper", "com.acme.app"]
    );
    assert_eq!(
        install_order(
            std::slice::from_ref(&base),
            std::slice::from_ref(&helper),
            false
        )
        .unwrap(),
        ["com.acme.helper"]
    );

    let error = |installed: &[BlockManifest], set: &[BlockManifest], force: bool| {
        install_order(installed, set, force).unwrap_err().issues
    };
    let gone = parse_manifest(&manifest_text(
        "com.acme.app",
        "1.0.0",
        r#", "dependencies": [
            {"id": "com.acme.gone", "version": "^2.0.0", "source": "https://github.com/acme/gone"},
            {"id": "com.acme.lost"}
        ]"#,
    ))
    .unwrap();
    let missing = error(&[], &[gone], false);
    assert_eq!(
        codes(&missing),
        ["block.dependency.missing", "block.dependency.missing"]
    );
    assert_eq!(missing[0].param("id"), Some("com.acme.gone"));
    assert_eq!(missing[0].param("range"), Some("^2.0.0"));
    assert_eq!(
        missing[0].param("source"),
        Some("https://github.com/acme/gone")
    );
    assert_eq!(missing[0].param("by"), Some("com.acme.app"));
    assert_eq!(missing[1].param("source"), None);
    let said = describe_package_issue(&missing[0]).unwrap();
    assert!(
        said.contains("com.acme.app needs the block com.acme.gone ^2.0.0")
            && said.contains("`jevision blocks install https://github.com/acme/gone`"),
        "{said}"
    );

    let old_helper = manifest("com.acme.helper", "0.9.0", &[]);
    let unmet = error(&[old_helper], std::slice::from_ref(&app), false);
    assert_eq!(codes(&unmet), ["block.dependency.unmet"]);
    assert_eq!(unmet[0].param("installed"), Some("0.9.0"));

    let a = manifest("com.acme.a", "1.0.0", &[("com.acme.b", "*")]);
    let b = manifest("com.acme.b", "1.0.0", &[("com.acme.a", "*")]);
    let two = error(&[], &[a.clone(), b], false);
    assert_eq!(codes(&two), ["block.dependency.cycle"]);
    assert_eq!(
        two[0].param("chain"),
        Some("com.acme.a → com.acme.b → com.acme.a")
    );
    let b = manifest("com.acme.b", "1.0.0", &[("com.acme.c", "*")]);
    let c = manifest("com.acme.c", "1.0.0", &[("com.acme.a", "*")]);
    let three = error(&[], &[c, b, a], false);
    assert_eq!(codes(&three), ["block.dependency.cycle"]);
    assert_eq!(
        three[0].param("chain"),
        Some("com.acme.a → com.acme.b → com.acme.c → com.acme.a")
    );
    assert!(
        describe_package_issue(&three[0])
            .unwrap()
            .contains("in a circle: com.acme.a → com.acme.b")
    );

    let twice = error(&[], &[base.clone(), base.clone()], false);
    assert_eq!(codes(&twice), ["block.set.duplicate"]);
    assert_eq!(twice[0].param("id"), Some("com.acme.base"));

    // A new helper that the installed app can't use.
    let new_helper = manifest("com.acme.helper", "2.0.0", &[]);
    let installed = [app.clone(), helper.clone(), base.clone()];
    let breaks = error(&installed, std::slice::from_ref(&new_helper), false);
    assert_eq!(codes(&breaks), ["block.dependency.breaks"]);
    assert_eq!(breaks[0].param("id"), Some("com.acme.app"));
    assert_eq!(breaks[0].param("dependency"), Some("com.acme.helper"));
    assert_eq!(breaks[0].param("range"), Some("^1.0.0"));
    assert_eq!(breaks[0].param("found"), Some("2.0.0"));
    assert_eq!(
        install_order(&installed, std::slice::from_ref(&new_helper), true).unwrap(),
        ["com.acme.helper"]
    );
    let new_app = manifest("com.acme.app", "2.0.0", &[("com.acme.helper", "^2.0.0")]);
    assert_eq!(
        install_order(&installed, &[new_app, new_helper], false).unwrap(),
        ["com.acme.helper", "com.acme.app"]
    );
}

#[test]
fn an_installed_package_says_what_it_still_needs_and_what_needs_it() {
    let project = temp("installed");
    let helper_step = run("../com.acme.helper.jevblock/flow.json");
    put(
        &project,
        "com.acme.app",
        "1.0.0",
        r#", "dependencies": [
            {"id": "com.acme.helper", "version": "^1.0.0"},
            {"id": "com.acme.base", "source": "https://github.com/acme/base"}
        ]"#,
        &helper_step,
    );
    put(
        &project,
        "com.acme.helper",
        "1.1.0",
        r#", "dependencies": [{"id": "com.acme.deep"}, {"id": "com.acme.app"}]"#,
        NOTHING,
    );
    let found = unmet_dependencies(&project, "com.acme.app");
    let mut seen: Vec<(&str, &str, &str)> = found
        .iter()
        .map(|i| {
            (
                i.code.as_str(),
                i.param("id").unwrap(),
                i.param("by").unwrap(),
            )
        })
        .collect();
    seen.sort_unstable();
    assert_eq!(
        seen,
        [
            ("block.dependency.missing", "com.acme.base", "com.acme.app"),
            (
                "block.dependency.missing",
                "com.acme.deep",
                "com.acme.helper"
            ),
        ]
    );
    assert_eq!(
        block_dependents(&project, "com.acme.helper"),
        ["com.acme.app"]
    );
    assert_eq!(
        block_dependents(&project, "com.acme.app"),
        ["com.acme.helper"]
    );
    assert_eq!(
        block_dependents(&project, "com.acme.deep"),
        ["com.acme.helper"]
    );
    assert!(block_dependents(&project, "com.acme.nobody").is_empty());

    put(&project, "com.acme.base", "1.0.0", "", NOTHING);
    put(&project, "com.acme.deep", "1.0.0", "", NOTHING);
    assert!(unmet_dependencies(&project, "com.acme.app").is_empty());
    put(&project, "com.acme.helper", "2.0.0", "", NOTHING);
    let unmet = unmet_dependencies(&project, "com.acme.app");
    assert_eq!(codes(&unmet), ["block.dependency.unmet"]);
    assert_eq!(unmet[0].param("installed"), Some("2.0.0"));
    fs::remove_dir_all(&project).unwrap();
}

#[test]
fn a_test_says_when_a_package_it_runs_lacks_what_it_needs() {
    let project = temp("validate");
    put(
        &project,
        "com.acme.app",
        "1.0.0",
        r#", "dependencies": [{"id": "com.acme.helper", "version": "^1.0.0",
            "source": "https://github.com/acme/helper"}]"#,
        &run("../com.acme.helper.jevblock/flow.json"),
    );
    let at_root = parse(&steps(&run("blocks/com.acme.app.jevblock/flow.json"))).unwrap();
    let in_folder = parse(&steps(&run("../blocks/com.acme.app.jevblock/flow.json"))).unwrap();
    let missing = validate_in(&at_root, &project);
    fs::create_dir_all(project.join("tests")).unwrap();
    let deeper = validate_in(&in_folder, &project.join("tests"));
    put(&project, "com.acme.helper", "0.5.0", "", NOTHING);
    let old = validate_in(&at_root, &project);
    put(&project, "com.acme.helper", "1.5.0", "", NOTHING);
    let fine = validate_in(&at_root, &project);
    fs::remove_dir_all(&project).unwrap();

    let unmet = |issues: &[Issue]| -> Vec<Issue> {
        (issues.iter())
            .filter(|i| i.code == "flow.block.dependencyUnmet")
            .cloned()
            .collect()
    };
    let missing = unmet(&missing);
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].severity, Severity::Error);
    for (name, value) in [
        ("block", "a"),
        ("field", "test"),
        ("id", "com.acme.app"),
        ("by", "com.acme.app"),
        ("dependency", "com.acme.helper"),
        ("range", "^1.0.0"),
        ("source", "https://github.com/acme/helper"),
    ] {
        assert_eq!(missing[0].param(name), Some(value), "{name}");
    }
    let said = describe_package_issue(&missing[0]).unwrap();
    assert!(
        said.starts_with("Block \"a\", field \"test\": the block com.acme.app needs the block com.acme.helper ^1.0.0, which isn't installed")
            && said.contains("`jevision blocks install https://github.com/acme/helper`"),
        "{said}"
    );
    assert_eq!(unmet(&deeper).len(), 1);
    let old = unmet(&old);
    assert_eq!(old[0].param("installed"), Some("0.5.0"));
    assert!(
        describe_package_issue(&old[0])
            .unwrap()
            .contains("but com.acme.helper 0.5.0 is installed")
    );
    assert!(unmet(&fine).is_empty());
}

#[test]
fn the_lock_keeps_the_installed_versions_to_restore() {
    let root = temp("lock");
    let project = root.join("project");
    let source = root.join("com.acme.helper.jevblock");
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(&source).unwrap();
    fs::write(
        source.join(MANIFEST_FILE),
        manifest_text("com.acme.helper", "1.0.0", ""),
    )
    .unwrap();
    fs::write(source.join(PACKAGE_FLOW_FILE), steps(NOTHING)).unwrap();
    let address = "https://github.com/acme/helper";
    install_folder(&project, &source, Some(address), false).unwrap();
    let lock_file = project.join(PACKAGE_LOCK_FILE);
    let lock = PackageLock::parse(&fs::read_to_string(&lock_file).unwrap()).unwrap();
    assert_eq!(lock.versions["com.acme.helper"], Version::new(1, 0, 0));
    assert_eq!(PackageLock::parse(&lock.to_json()).unwrap(), lock);
    assert!(restore_entries(&project).unwrap().is_empty());

    let entry = |version: Option<Version>| RestoreEntry {
        id: "com.acme.helper".to_owned(),
        source: address.to_owned(),
        locked_version: version,
    };
    let folder = package_folder(&project, "com.acme.helper");
    fs::write(
        folder.join(MANIFEST_FILE),
        manifest_text("com.acme.helper", "1.1.0", ""),
    )
    .unwrap();
    assert_eq!(
        restore_entries(&project).unwrap(),
        [entry(Some(Version::new(1, 0, 0)))]
    );
    fs::remove_dir_all(&folder).unwrap();
    assert_eq!(
        restore_entries(&project).unwrap(),
        [entry(Some(Version::new(1, 0, 0)))]
    );

    // A lock written before versions were kept still reads.
    fs::write(
        &lock_file,
        format!(r#"{{"format": 1, "sources": {{"com.acme.helper": "{address}"}}}}"#),
    )
    .unwrap();
    assert_eq!(restore_entries(&project).unwrap(), [entry(None)]);
    install_folder(&project, &source, Some(address), false).unwrap();
    assert!(restore_entries(&project).unwrap().is_empty());
    remove_package(&project, "com.acme.helper").unwrap();
    assert!(!lock_file.exists());

    fs::write(
        &lock_file,
        r#"{"format": 1, "sources": {}, "versions": {"com.acme.helper": "new"}}"#,
    )
    .unwrap();
    assert_eq!(
        restore_entries(&project).unwrap_err().issues[0].code,
        "block.lock.invalid"
    );
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_package_becomes_an_own_block_and_the_tests_follow_it() {
    let project = temp("own");
    let id = "com.acme.sign-in";
    put_package(
        &project,
        id,
        r#"{"format": 1, "id": "com.acme.sign-in", "name": "Sign In", "version": "1.0.0",
            "author": "Acme", "license": "MIT", "description": "Signs in.",
            "icon": "person.badge.key",
            "inputs": [{"name": "email", "default": "a@b.c"}, {"name": "password"}],
            "dependencies": [{"id": "com.acme.helper"}]}"#,
        r#"{
            "a": {"type": "type", "text": "${input.email} ${input.password}", "next": "b"},
            "b": {"type": "click", "target": {"image": "images/go.png"}, "next": "c"},
            "c": {"type": "openFile", "files": ["files/data.txt", "https://acme.test"], "next": "d"},
            "d": {"type": "runTest", "test": "../com.acme.helper.jevblock/flow.json"}
        }"#,
    );
    put(&project, "com.acme.helper", "1.0.0", "", NOTHING);
    let package = package_folder(&project, id);
    fs::create_dir_all(package.join("files")).unwrap();
    fs::write(package.join("images/go.png"), b"go").unwrap();
    fs::write(package.join("images/go.dark.png"), b"go dark").unwrap();
    fs::write(package.join("files/data.txt"), b"data").unwrap();
    let blocks = project.join(BLOCKS_FOLDER);
    fs::create_dir_all(blocks.join("images")).unwrap();
    fs::write(blocks.join("images/go.png"), b"another").unwrap();
    fs::write(blocks.join("Sign In.block.json"), steps(NOTHING)).unwrap();

    let own = make_own_block(&project, id).unwrap();
    assert_eq!(
        own,
        OwnBlock {
            file: "blocks/Sign In 2.block.json".to_owned(),
            name: "Sign In".to_owned()
        }
    );
    let flow = parse(&fs::read_to_string(project.join(&own.file)).unwrap()).unwrap();
    assert_eq!(flow.description.as_deref(), Some("Signs in."));
    assert_eq!(flow.icon.as_deref(), Some("person.badge.key"));
    let step = |id: &str| &flow.blocks.iter().find(|b| b.id == id).unwrap().kind;
    assert!(
        matches!(step("b"), BlockKind::Click { target: Target::Image(path), .. } if path == "images/go-2.png")
    );
    assert!(
        matches!(step("c"), BlockKind::OpenFile { files, .. } if files == &["files/data.txt", "https://acme.test"])
    );
    assert!(
        matches!(step("d"), BlockKind::RunTest { test, .. } if test == "com.acme.helper.jevblock/flow.json")
    );
    assert_eq!(fs::read(blocks.join("images/go-2.png")).unwrap(), b"go");
    assert_eq!(
        fs::read(blocks.join("images/go-2.dark.png")).unwrap(),
        b"go dark"
    );
    assert_eq!(fs::read(blocks.join("images/go.png")).unwrap(), b"another");
    assert_eq!(fs::read(blocks.join("files/data.txt")).unwrap(), b"data");
    assert!(installed(&project, id).is_some());
    assert_eq!(usage(&project, "com.acme.helper"), [own.file.as_str()]);

    // A second copy reuses the files that are the same.
    let again = make_own_block(&project, id).unwrap();
    assert_eq!(again.file, "blocks/Sign In 3.block.json");
    assert!(!blocks.join("images/go-3.png").exists());

    let package_step = |test: &str| {
        steps(&format!(
            r#"{{"a": {{"type": "runTest", "test": "{test}",
                "package": {{"version": "1.0.0"}}, "inputs": {{"password": "x"}}}}}}"#
        ))
    };
    fs::write(
        project.join("t.flow.json"),
        package_step("blocks/com.acme.sign-in.jevblock/flow.json"),
    )
    .unwrap();
    fs::create_dir_all(project.join("tests")).unwrap();
    fs::write(
        project.join("tests/u.flow.json"),
        package_step("../blocks/com.acme.sign-in.jevblock/flow.json"),
    )
    .unwrap();
    fs::write(project.join("other.flow.json"), steps(NOTHING)).unwrap();
    assert_eq!(usage(&project, id), ["t.flow.json", "tests/u.flow.json"]);

    for bad in [
        "blocks/com.acme.sign-in.jevblock/flow.json",
        "blocks/Nothing.block.json",
        "t.flow.json",
        "../Sign In 2.block.json",
    ] {
        assert_eq!(
            switch_tests_to_copy(&project, id, bad).unwrap_err().issues[0].code,
            "block.export.notBlock",
            "{bad}"
        );
    }
    let changed = switch_tests_to_copy(&project, id, &own.file).unwrap();
    assert_eq!(changed, ["t.flow.json", "tests/u.flow.json"]);
    assert!(usage(&project, id).is_empty());
    for (file, expected) in [
        ("t.flow.json", "blocks/Sign In 2.block.json"),
        ("tests/u.flow.json", "../blocks/Sign In 2.block.json"),
    ] {
        let flow = parse(&fs::read_to_string(project.join(file)).unwrap()).unwrap();
        let BlockKind::RunTest {
            test,
            package,
            inputs,
            ..
        } = &flow.blocks[0].kind
        else {
            panic!("{file}");
        };
        assert_eq!(test, expected);
        assert_eq!(*package, None);
        assert_eq!(inputs["email"], "a@b.c");
        assert_eq!(inputs["password"], "x");
    }
    assert_eq!(
        fs::read_to_string(project.join("other.flow.json")).unwrap(),
        steps(NOTHING)
    );

    put(
        &project,
        "com.acme.nested",
        "1.0.0",
        "",
        &run("parts/inner.flow.json"),
    );
    let nested = make_own_block(&project, "com.acme.nested").unwrap_err();
    assert_eq!(nested.issues[0].code, "block.copy.nested");
    assert!(!blocks.join("com.acme.nested.block.json").exists());
    assert_eq!(
        make_own_block(&project, "com.acme.gone")
            .unwrap_err()
            .issues[0]
            .code,
        "block.notInstalled"
    );
    fs::remove_dir_all(&project).unwrap();
}

#[test]
fn a_shared_block_needs_the_installed_packages_it_runs() {
    let root = temp("export");
    let project = root.join("project");
    let source = root.join("src.jevblock");
    fs::create_dir_all(&source).unwrap();
    fs::write(
        source.join(MANIFEST_FILE),
        manifest_text(
            "com.acme.helper",
            "1.4.0",
            r#", "inputs": [{"name": "who"}], "dependencies": [{"id": "com.acme.gone"}]"#,
        ),
    )
    .unwrap();
    fs::write(
        source.join(PACKAGE_FLOW_FILE),
        steps(r#"{"a": {"type": "type", "text": "${input.who}", "next": "b"}, "b": {"type": "runTest", "test": "../com.acme.gone.jevblock/flow.json"}}"#),
    )
    .unwrap();
    fs::create_dir_all(&project).unwrap();
    install_folder(
        &project,
        &source,
        Some("https://github.com/acme/helper"),
        false,
    )
    .unwrap();
    // Two calls of one package, from a block in a folder of its own, passing an input on.
    fs::create_dir_all(project.join("blocks/team")).unwrap();
    fs::write(
        project.join("blocks/team/Greet.block.json"),
        steps(
            r#"{"a": {"type": "runTest", "test": "../com.acme.helper.jevblock/flow.json",
                      "inputs": {"who": "${input.name}"}, "next": "b"},
                "b": {"type": "runTest", "test": "../com.acme.helper.jevblock/flow.json",
                      "inputs": {"who": "everyone"}}}"#,
        ),
    )
    .unwrap();
    let meta = ExportMeta {
        id: "com.acme.greet".to_owned(),
        name: None,
        description: None,
        author: "Acme".to_owned(),
        version: "1.0.0".to_owned(),
        license: "MIT".to_owned(),
        inputs: Vec::new(),
    };
    let out = root.join("out");
    let done = export_package(&project, "blocks/team/Greet.block.json", &meta, &out).unwrap();
    let needs = &done.manifest.dependencies;
    assert_eq!(needs.len(), 1);
    assert_eq!(needs[0].id, "com.acme.helper");
    assert_eq!(needs[0].version.to_string(), "^1.4.0");
    assert_eq!(
        needs[0].source.as_deref(),
        Some("https://github.com/acme/helper")
    );
    assert_eq!(
        done.manifest
            .inputs
            .iter()
            .map(|i| i.name.as_str())
            .collect::<Vec<_>>(),
        ["name"]
    );
    let flow = parse(&fs::read_to_string(done.folder.join(PACKAGE_FLOW_FILE)).unwrap()).unwrap();
    for b in &flow.blocks {
        assert!(
            matches!(&b.kind, BlockKind::RunTest { test, .. } if test == "../com.acme.helper.jevblock/flow.json"),
            "{b:?}"
        );
    }
    let read = read_package(&done.folder).unwrap();
    assert!(!read.has_errors(), "{:?}", read.issues);
    assert_eq!(read.manifest, done.manifest);
    let other = root.join("other");
    fs::create_dir_all(&other).unwrap();
    let again = install_folder(&other, &done.folder, None, false).unwrap();
    assert_eq!(again.manifest, done.manifest);

    // A call of the project's own block can't go into a package.
    fs::write(project.join("blocks/Inner.block.json"), steps(NOTHING)).unwrap();
    fs::write(
        project.join("blocks/Outer.block.json"),
        steps(&run("Inner.block.json")),
    )
    .unwrap();
    let refused = export_package(&project, "blocks/Outer.block.json", &meta, &out).unwrap_err();
    assert_eq!(codes(&refused.issues), ["block.export.nested"]);
    let said = describe_package_issue(&refused.issues[0]).unwrap();
    assert!(
        said.contains("runs Inner.block.json") && said.contains("installed community block"),
        "{said}"
    );
    fs::write(
        project.join("blocks/Gone.block.json"),
        steps(&run("com.acme.gone.jevblock/flow.json")),
    )
    .unwrap();
    assert_eq!(
        codes(
            &export_package(&project, "blocks/Gone.block.json", &meta, &out)
                .unwrap_err()
                .issues
        ),
        ["block.export.nested"]
    );
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn an_input_given_to_another_block_is_read() {
    let manifest = parse_manifest(&manifest_text(
        "com.acme.greet",
        "1.0.0",
        r#", "inputs": [{"name": "name"}], "dependencies": [{"id": "com.acme.helper"}]"#,
    ))
    .unwrap();
    let flow = parse(&steps(
        r#"{"a": {"type": "runTest", "test": "../com.acme.helper.jevblock/flow.json",
                  "inputs": {"who": "Dear ${input.name}"}}}"#,
    ))
    .unwrap();
    assert_eq!(required_inputs(&flow), ["name"]);
    let found = check_package(&manifest, &flow, Path::new("/nonexistent/x.jevblock"));
    assert!(found.is_empty(), "{found:?}");
}
