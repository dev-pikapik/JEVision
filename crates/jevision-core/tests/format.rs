use std::fs;
use std::path::Path;

use jevision_core::*;

/// Every block type, written loosely: keys out of order, defaults left out, odd durations.
const ALL_BLOCKS: &str = r#"{
  "blocks": {
    "users": { "type": "useData", "file": "data/users.csv", "body": "launch", "next": "done" },
    "launch": { "next": "doc", "type": "openApp", "app": "com.apple.TextEdit" },
    "doc": { "type": "openFile", "files": ["fixtures/a.txt", "https://example.com"], "newInstance": true, "next": "name" },
    "name": { "type": "type", "text": "${data.name} ${secret.TOKEN}", "into": { "point": { "x": 0.5, "y": 0.25 } }, "next": "save" },
    "save": { "type": "pressKeys", "keys": "cmd+s", "next": "tip" },
    "tip": { "type": "hover", "target": { "image": "refs/help.png" }, "waitForChange": true, "next": "move" },
    "move": { "next": "list", "to": { "point": { "x": 0.75, "y": 0.5 } }, "type": "drag", "from": { "image": "refs/file.png" } },
    "list": { "type": "scroll", "at": { "point": { "x": 0.5, "y": 0.6 } }, "amount": 240, "unit": "pixels", "direction": "down", "next": "menu" },
    "menu": { "type": "click", "target": { "image": "refs/menu.png" }, "button": "right", "double": true, "waitForChange": true, "next": "spinner" },
    "spinner": { "type": "waitFor", "until": { "disappears": { "image": "refs/spinner.png", "compare": "exact" } }, "timeout": "120000ms", "next": "settle" },
    "settle": { "type": "waitFor", "until": "screenStable", "next": "branch" },
    "branch": { "type": "if", "reference": { "expect": "A dialog asks whether to save", "compare": "meaning", "area": { "x": 0, "y": 0, "width": 1, "height": 0.5 } }, "then": "again", "else": "users" },
    "again": { "type": "repeat", "times": 3, "body": "shared", "next": "users" },
    "shared": { "type": "runTest", "test": "../shared/close-dialog.flow.json", "next": "again" },
    "done": { "type": "check", "reference": { "image": "refs/done.png", "threshold": 0.8 } }
  },
  "start": "users",
  "app": "com.apple.TextEdit",
  "name": "Every block",
  "format": 1
}"#;

fn codes(issues: &[Issue]) -> Vec<&str> {
    issues.iter().map(|i| i.code.as_str()).collect()
}

fn parse_err(json: &str) -> Vec<Issue> {
    parse(json).unwrap_err().issues
}

fn minimal(blocks: &str) -> String {
    format!(
        r#"{{"format": 1, "name": "t", "app": "com.example.App", "start": "a", "blocks": {blocks}}}"#
    )
}

/// The issues of a test made of `blocks`, without the warning that it checks nothing, which
/// most of these small tests would get.
fn issues_of(blocks: &str) -> Vec<Issue> {
    validate(&parse(&minimal(blocks)).unwrap())
        .into_iter()
        .filter(|i| i.code != "flow.noCheck")
        .collect()
}

#[test]
fn a_test_that_never_checks_the_screen_gets_a_warning() {
    let warned = |blocks: &str| {
        validate(&parse(&minimal(blocks)).unwrap())
            .iter()
            .any(|i| i.code == "flow.noCheck" && i.severity == Severity::Warning)
    };
    assert!(warned(r#"{"a": {"type": "pressKeys", "keys": "a"}}"#));
    assert!(warned(
        r#"{"a": {"type": "waitFor", "until": "screenStable"}}"#
    ));
    assert!(warned(
        r#"{"a": {"type": "if", "reference": {"image": "x.png"}}}"#
    ));
    assert!(!warned(
        r#"{"a": {"type": "check", "reference": {"image": "x.png"}}}"#
    ));
    assert!(!warned(
        r#"{"a": {"type": "waitFor", "until": {"appears": {"image": "x.png"}}}}"#
    ));
    assert!(!warned(
        r#"{"a": {"type": "waitFor", "until": {"disappears": {"image": "x.png"}}}}"#
    ));
    // The test it runs may check the screen.
    assert!(!warned(
        r#"{"a": {"type": "runTest", "test": "other.flow.json"}}"#
    ));
    // Checks that look past the screen count too; a query that only saves a value does not.
    assert!(!warned(
        r#"{"a": {"type": "http", "url": "https://x.test"}}"#
    ));
    assert!(!warned(
        r#"{"a": {"type": "checkFile", "path": "out.txt", "until": "exists"}}"#
    ));
    assert!(!warned(
        r#"{"a": {"type": "database", "file": "a.sqlite", "query": "select 1", "expect": {"rows": 1}}}"#
    ));
    assert!(warned(
        r#"{"a": {"type": "database", "file": "a.sqlite", "query": "select 1", "saveAs": "x"}}"#
    ));
    assert!(warned(r#"{"a": {"type": "command", "command": "true"}}"#));
}

#[test]
fn examples_are_valid_and_canonical() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut seen = 0;
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if !path.to_string_lossy().ends_with(".flow.json") {
            continue;
        }
        seen += 1;
        let text = fs::read_to_string(&path).unwrap();
        let flow = parse(&text).unwrap();
        assert_eq!(validate(&flow), vec![], "{}", path.display());
        assert_eq!(to_canonical_json(&flow), text, "{}", path.display());
    }
    assert!(seen > 0);
}

#[test]
fn example_packages_are_valid_and_canonical() {
    let blocks = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/textedit/blocks");
    let mut seen = 0;
    for entry in fs::read_dir(blocks).unwrap() {
        let path = entry.unwrap().path();
        if !path.to_string_lossy().ends_with(PACKAGE_SUFFIX) {
            continue;
        }
        seen += 1;
        let package = read_package(&path).unwrap();
        assert_eq!(package.issues, vec![], "{}", path.display());
        assert_eq!(
            path.file_name().unwrap().to_string_lossy(),
            format!("{}{PACKAGE_SUFFIX}", package.manifest.id)
        );
        assert_eq!(
            package.manifest.to_json(),
            fs::read_to_string(path.join(MANIFEST_FILE)).unwrap()
        );
        assert_eq!(
            to_canonical_json(&package.flow),
            fs::read_to_string(path.join(PACKAGE_FLOW_FILE)).unwrap()
        );
    }
    assert!(seen > 0);

    let project = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/textedit");
    let text = fs::read_to_string(project.join("add-a-line-with-a-block.flow.json")).unwrap();
    let flow = parse(&text).unwrap();
    assert_eq!(to_canonical_json(&flow), text);
    assert_eq!(
        codes(&validate_in(&flow, &project)),
        ["flow.reference.notSaved"],
        "The block is installed; only the picture of the first run is missing"
    );
}

#[test]
fn canonical_form_is_idempotent_and_lossless() {
    let flow = parse(ALL_BLOCKS).unwrap();
    assert_eq!(validate(&flow), vec![]);
    let once = to_canonical_json(&flow);
    assert_eq!(canonicalize(&once).unwrap(), once);
    assert_eq!(parse(&once).unwrap(), flow);
    assert!(once.ends_with("}\n") && !once.ends_with("\n\n"));
    assert!(!once.contains('\t') && !once.contains("\r"));
    assert!(once.contains("\n  \"format\": 1,\n"));
}

#[test]
fn canonical_form_fixes_order_and_defaults() {
    let once = canonicalize(ALL_BLOCKS).unwrap();
    let keys: Vec<&str> = once
        .lines()
        .filter(|l| l.starts_with("  \"") || l.starts_with("    \""))
        .map(|l| l.trim().split('"').nth(1).unwrap())
        .collect();
    assert_eq!(
        keys,
        [
            "format", "name", "app", "start", "blocks", "users", "launch", "doc", "name", "save",
            "tip", "move", "list", "menu", "spinner", "settle", "branch", "again", "shared",
            "done"
        ]
    );
    assert!(once.contains("\"timeout\": \"2m\""));
    assert!(once.contains("\"timeout\": \"10s\""));
    assert!(once.contains("\"compare\": \"similar\",\n        \"threshold\": 0.8"));
    assert!(once.contains(
        "\"reference\": {\n        \"compare\": \"meaning\",\n        \"expect\": \"A dialog asks whether to save\","
    ));
}

#[test]
fn reading_order_keeps_unreachable_blocks_last() {
    let flow = parse(&minimal(
        r#"{"z": {"type": "pressKeys", "keys": "a"}, "b": {"type": "pressKeys", "keys": "b"},
            "a": {"type": "pressKeys", "keys": "c", "next": "z"}}"#,
    ))
    .unwrap();
    let ids: Vec<&str> = flow
        .blocks_in_flow_order()
        .iter()
        .map(|b| b.id.as_str())
        .collect();
    assert_eq!(ids, ["a", "z", "b"]);
}

#[test]
fn format_versions() {
    assert_eq!(FORMAT_VERSION, 1);
    let with_format =
        |n: &str| minimal(r#"{"a": {"type": "openApp"}}"#).replace("\"format\": 1", n);
    assert!(parse(&with_format("\"format\": 1")).is_ok());
    let too_new = parse_err(&with_format("\"format\": 2"));
    assert_eq!(codes(&too_new), ["flow.format.tooNew"]);
    assert_eq!(too_new[0].param("supported"), Some("1"));
    assert_eq!(
        codes(&parse_err(&with_format("\"format\": 0"))),
        ["flow.format.unsupported"]
    );
    assert_eq!(
        codes(&parse_err(&with_format("\"v\": 1"))),
        ["flow.format.missing"]
    );
}

#[test]
fn unreadable_files_report_codes_with_params() {
    let syntax = parse_err("{\n  \"format\": 1,\n  oops\n}");
    assert_eq!(codes(&syntax), ["flow.json.syntax"]);
    assert_eq!(syntax[0].param("line"), Some("3"));
    assert_eq!(codes(&parse_err("[]")), ["flow.json.notObject"]);

    let unknown = parse_err(&minimal(r#"{"a": {"type": "teleport"}}"#));
    assert_eq!(codes(&unknown), ["flow.block.unknownType"]);
    assert_eq!(unknown[0].param("block"), Some("a"));
    assert_eq!(unknown[0].param("type"), Some("teleport"));

    let issues = parse_err(&minimal(
        r#"{"a": {"type": "click", "target": {"image": 5}, "colour": "red", "button": "middle"},
            "b": {"type": "waitFor", "until": {"appears": {}}, "timeout": "soon"},
            "c": {"type": "check", "reference": {"image": "x.png", "compare": "exact", "threshold": 0.5}},
            "d": {"type": "click", "target": {"image": "a.png", "point": {"x": 0, "y": 0}}, "next": "e"},
            "e": {"type": "check", "reference": {"image": "x.png", "expect": "A cart"}},
            "f": {"type": "check", "reference": {"image": "x.png", "model": "gpt-5"}},
            "g": {"type": "check", "reference": {"image": "x.png", "provider": "openai"}}}"#,
    ));
    let found: Vec<(&str, Option<&str>, Option<&str>)> = issues
        .iter()
        .map(|i| (i.code.as_str(), i.param("block"), i.param("field")))
        .collect();
    assert_eq!(
        found,
        [
            ("flow.field.type", Some("a"), Some("target.image")),
            ("flow.field.value", Some("a"), Some("button")),
            ("flow.field.unknown", Some("a"), Some("colour")),
            ("flow.field.missing", Some("b"), Some("until.appears.image")),
            ("flow.field.duration", Some("b"), Some("timeout")),
            (
                "flow.reference.thresholdNeedsSimilar",
                Some("c"),
                Some("reference.threshold")
            ),
            ("flow.field.oneOf", Some("d"), Some("target")),
            (
                "flow.reference.expectNeedsMeaning",
                Some("e"),
                Some("reference.expect")
            ),
            (
                "flow.reference.modelNeedsMeaning",
                Some("f"),
                Some("reference.model")
            ),
            (
                "flow.reference.providerNeedsMeaning",
                Some("g"),
                Some("reference.provider")
            ),
        ]
    );
    assert_eq!(issues[1].param("allowed"), Some("left, right"));
}

#[test]
fn broken_edges_are_errors() {
    let issues =
        issues_of(r#"{"a": {"type": "if", "reference": {"image": "x.png"}, "then": "nowhere"}}"#);
    assert_eq!(codes(&issues), ["flow.edge.unknownTarget"]);
    assert_eq!(issues[0].param("edge"), Some("then"));
    assert_eq!(issues[0].param("target"), Some("nowhere"));

    let start = validate(
        &parse(&minimal(
            r#"{"b": {"type": "check", "reference": {"image": "b.png"}}}"#,
        ))
        .unwrap(),
    );
    assert_eq!(codes(&start), ["flow.start.unknownBlock"]);
}

const THREE_WAYS: &str = r#"{
  "format": 1,
  "name": "t",
  "app": "com.example.App",
  "start": "a",
  "blocks": {
    "a": {
      "type": "if",
      "reference": {
        "image": "refs/one.png",
        "compare": "similar",
        "threshold": 0.95
      },
      "then": "one",
      "elseIf": [
        {
          "reference": {
            "compare": "meaning",
            "expect": "An update dialog is open",
            "provider": "openai",
            "model": "gpt-5"
          },
          "then": "two"
        },
        {
          "reference": {
            "image": "refs/three.png",
            "compare": "exact"
          }
        }
      ],
      "else": "one"
    },
    "one": {
      "type": "pressKeys",
      "keys": "a"
    },
    "two": {
      "type": "pressKeys",
      "keys": "b"
    }
  }
}
"#;

#[test]
fn else_if_cases_round_trip_and_plain_ifs_stay_unchanged() {
    let flow = parse(THREE_WAYS).unwrap();
    assert_eq!(codes(&validate(&flow)), ["flow.noCheck"]);
    assert_eq!(to_canonical_json(&flow), THREE_WAYS);
    let edges: Vec<(String, &str)> = flow.blocks[0]
        .kind
        .edges()
        .into_iter()
        .map(|(n, t)| (n.into_owned(), t))
        .collect();
    assert_eq!(
        edges,
        [
            ("then".to_owned(), "one"),
            ("elseIf1".to_owned(), "two"),
            ("else".to_owned(), "one")
        ]
    );
    assert_eq!(
        flow.blocks[0].kind.edge_names(),
        ["then", "elseIf1", "elseIf2", "else"]
    );

    // A file without cases is written back byte for byte, with no trace of the new key.
    let plain = minimal(
        r#"{"a": {"type": "if", "reference": {"image": "x.png"}, "then": "b", "else": "b"},
            "b": {"type": "pressKeys", "keys": "b"}}"#,
    );
    let once = canonicalize(&plain).unwrap();
    assert!(!once.contains("elseIf"));
    assert!(once.contains("\"then\": \"b\",\n      \"else\": \"b\"\n"));
    assert_eq!(canonicalize(&once).unwrap(), once);
}

#[test]
fn else_if_edge_names_map_to_cases() {
    assert_eq!(else_if_edge(0), "elseIf1");
    assert_eq!(else_if_index("elseIf1"), Some(0));
    assert_eq!(else_if_index("elseIf12"), Some(11));
    for wrong in [
        "elseIf", "elseIf0", "elseIf01", "elseIf-1", "elseIfx", "then",
    ] {
        assert_eq!(else_if_index(wrong), None, "{wrong}");
    }
}

#[test]
fn malformed_else_if_cases_are_reported() {
    let issues = parse_err(&minimal(
        r#"{"a": {"type": "if", "reference": {"image": "x.png"}, "elseIf": [{"then": "a", "extra": 1}, 3]}}"#,
    ));
    let found: Vec<(&str, Option<&str>)> = issues
        .iter()
        .map(|i| (i.code.as_str(), i.param("field")))
        .collect();
    assert_eq!(
        found,
        [
            ("flow.field.oneOf", Some("elseIf.0")),
            ("flow.field.unknown", Some("elseIf.0.extra")),
            ("flow.field.type", Some("elseIf.1")),
        ]
    );
    let not_a_list = parse_err(&minimal(
        r#"{"a": {"type": "if", "reference": {"image": "x.png"}, "elseIf": {}}}"#,
    ));
    assert_eq!(codes(&not_a_list), ["flow.field.type"]);
}

#[test]
fn else_if_cases_are_validated() {
    let issues = issues_of(
        r#"{"a": {"type": "if", "reference": {"image": "x.png"}, "then": "b",
                  "elseIf": [{"reference": {"image": "/abs.png"}, "then": "nowhere"},
                             {"reference": {"image": "x.png"}, "then": "b"},
                             {"reference": {"image": ""}, "then": "b"},
                             {"reference": {"image": ""}, "then": "b"}]},
            "b": {"type": "pressKeys", "keys": "b"}}"#,
    );
    let found: Vec<(&str, Option<&str>, Option<&str>)> = issues
        .iter()
        .map(|i| (i.code.as_str(), i.param("field"), i.param("edge")))
        .collect();
    assert_eq!(
        found,
        [
            ("flow.edge.unknownTarget", None, Some("elseIf1")),
            ("flow.path.invalid", Some("elseIf.0.reference.image"), None),
            ("flow.if.sameCondition", Some("elseIf.1.reference"), None),
            ("flow.field.empty", Some("elseIf.2.reference.image"), None),
            ("flow.field.empty", Some("elseIf.3.reference.image"), None),
        ]
    );
    assert_eq!(issues[2].severity, Severity::Warning);
}

#[test]
fn cycles_are_allowed_only_through_a_loop_body() {
    let plain = issues_of(
        r#"{"a": {"type": "pressKeys", "keys": "a", "next": "b"},
            "b": {"type": "pressKeys", "keys": "b", "next": "a"}}"#,
    );
    assert_eq!(codes(&plain), ["flow.cycle"]);
    assert_eq!(plain[0].param("blocks"), Some("a, b"));

    let through_body = issues_of(
        r#"{"a": {"type": "repeat", "times": 2, "body": "b"},
            "b": {"type": "pressKeys", "keys": "b", "next": "a"}}"#,
    );
    assert_eq!(through_body, vec![]);

    let through_exit = issues_of(
        r#"{"a": {"type": "repeat", "times": 2, "body": "b", "next": "c"},
            "b": {"type": "pressKeys", "keys": "b", "next": "a"},
            "c": {"type": "pressKeys", "keys": "c", "next": "a"}}"#,
    );
    assert_eq!(codes(&through_exit), ["flow.cycle"]);

    let self_loop = issues_of(r#"{"a": {"type": "repeat", "times": 2, "body": "a"}}"#);
    assert_eq!(codes(&self_loop), ["flow.loop.emptyBody"]);
}

#[test]
fn suspicious_graphs_are_warnings() {
    let issues = issues_of(
        r#"{"a": {"type": "repeat", "times": 2, "body": "b"},
            "b": {"type": "pressKeys", "keys": "b"},
            "orphan": {"type": "pressKeys", "keys": "c"}}"#,
    );
    assert_eq!(
        codes(&issues),
        ["flow.block.unreachable", "flow.loop.noReturn"]
    );
    assert!(issues.iter().all(|i| i.severity == Severity::Warning));
}

#[test]
fn parameters_are_checked() {
    let issues = issues_of(
        r#"{"a": {"type": "type", "text": "${secret.ok} ${env.HOME} ${home.X}", "next": "b"},
            "b": {"type": "openFile", "files": ["/etc/hosts"], "app": "Safari", "next": "c"},
            "c": {"type": "repeat", "times": 0, "body": "d", "next": "e"},
            "d": {"type": "click", "target": {"point": {"x": 1.5, "y": 0}}, "next": "c"},
            "e": {"type": "runTest", "test": "other.json", "next": "f"},
            "f": {"type": "waitFor", "until": {"appears": {"image": "x.png", "threshold": 0,
                  "ignore": [{"x": 0.5, "y": 0, "width": 0.6, "height": 1}]}}, "timeout": "0s", "next": "g"},
            "g": {"type": "pressKeys", "keys": "command+s", "next": "h"},
            "h": {"type": "check", "reference": {"compare": "meaning", "expect": "Saved", "provider": "skynet"}}}"#,
    );
    let found: Vec<(&str, &str)> = issues
        .iter()
        .map(|i| (i.code.as_str(), i.param("field").unwrap_or("")))
        .collect();
    assert_eq!(
        found,
        [
            ("flow.placeholder.invalid", "text"),
            ("flow.path.invalid", "files.0"),
            ("flow.app.invalid", "app"),
            ("flow.field.range", "times"),
            ("flow.point.invalid", "target.point"),
            ("flow.path.extension", "test"),
            ("flow.field.range", "until.appears.threshold"),
            ("flow.area.invalid", "until.appears.ignore.0"),
            ("flow.field.range", "timeout"),
            ("flow.keys.invalid", "keys"),
            ("flow.reference.unknownProvider", "reference.provider"),
        ]
    );
    assert_eq!(issues[0].param("placeholder"), Some("${home.X}"));
    assert_eq!(issues[10].param("provider"), Some("skynet"));
    assert_eq!(
        describe(&issues[10]),
        "Block \"h\", field \"reference.provider\": \"skynet\" isn't an AI provider; use one of: \
         anthropic, openai, gemini, jev, ollama, lmstudio, openai-compatible."
    );
}

#[test]
fn pointer_blocks_are_written_in_canonical_form() {
    let once = canonicalize(ALL_BLOCKS).unwrap();
    assert!(once.contains(
        r#""move": {
      "type": "drag",
      "from": {
        "image": "refs/file.png"
      },
      "to": {
        "point": {
          "x": 0.75,
          "y": 0.5
        }
      },
      "next": "list"
    }"#
    ));
    assert!(once.contains(
        r#""type": "scroll",
      "direction": "down",
      "amount": 240,
      "unit": "pixels",
      "at": {"#
    ));
    // The unit is always written; a scroll without a place scrolls the middle of the window.
    let plain = canonicalize(&minimal(
        r#"{"a": {"type": "scroll", "direction": "up", "amount": 3}}"#,
    ))
    .unwrap();
    assert!(plain.contains(
        r#""type": "scroll",
      "direction": "up",
      "amount": 3,
      "unit": "lines"
    }"#
    ));
    let flow = parse(&plain).unwrap();
    assert_eq!(
        flow.blocks[0].kind,
        BlockKind::Scroll {
            direction: ScrollDirection::Up,
            amount: 3,
            unit: ScrollUnit::Lines,
            at: None,
            wait_for_change: false,
            next: None,
        }
    );
}

#[test]
fn malformed_pointer_blocks_are_reported() {
    let issues = parse_err(&minimal(
        r#"{"a": {"type": "drag", "from": {"image": "a.png"}, "next": "b"},
            "b": {"type": "scroll", "direction": "sideways", "unit": "pages"},
            "c": {"type": "hover", "target": {"image": "a.png", "point": {"x": 0, "y": 0}}}}"#,
    ));
    let found: Vec<(&str, &str)> = issues
        .iter()
        .map(|i| (i.code.as_str(), i.param("field").unwrap_or("")))
        .collect();
    assert_eq!(
        found,
        [
            ("flow.field.missing", "to"),
            ("flow.field.value", "direction"),
            ("flow.field.missing", "amount"),
            ("flow.field.value", "unit"),
            ("flow.field.oneOf", "target"),
        ]
    );
}

#[test]
fn pointer_blocks_are_validated() {
    let issues = issues_of(
        r#"{"a": {"type": "hover", "target": {"image": ""}, "next": "b"},
            "b": {"type": "drag", "from": {"image": "refs/x.png"}, "to": {"image": "refs/x.png"}, "next": "c"},
            "c": {"type": "drag", "from": {"point": {"x": 0.5, "y": 0.5}}, "to": {"point": {"x": 0.5, "y": 0.5}}, "next": "d"},
            "d": {"type": "scroll", "direction": "left", "amount": 0, "next": "e"},
            "e": {"type": "scroll", "direction": "right", "amount": 10001, "at": {"point": {"x": 2, "y": 0}}, "next": "f"},
            "f": {"type": "drag", "from": {"description": "the inbox"}, "to": {"description": " the inbox "}, "next": "g"},
            "g": {"type": "scroll", "direction": "down", "amount": 3, "at": {"description": " "}}}"#,
    );
    let found: Vec<(&str, &str, &str)> = issues
        .iter()
        .map(|i| {
            (
                i.code.as_str(),
                i.param("block").unwrap_or(""),
                i.param("field").unwrap_or(""),
            )
        })
        .collect();
    assert_eq!(
        found,
        [
            ("flow.field.empty", "a", "target.image"),
            ("flow.drag.samePlace", "b", "to"),
            ("flow.drag.samePlace", "c", "to"),
            ("flow.field.range", "d", "amount"),
            ("flow.field.range", "e", "amount"),
            ("flow.point.invalid", "e", "at.point"),
            ("flow.drag.samePlace", "f", "to"),
            ("flow.field.empty", "g", "at.description"),
        ]
    );
    assert_eq!(issues[4].param("max"), Some("10000"));
    // A new block starts empty but complete: only its pictures are left to choose.
    for (type_name, empty) in [("hover", 1), ("drag", 2), ("scroll", 0)] {
        let flow = Flow {
            blocks: vec![Block {
                id: "a".into(),
                kind: BlockKind::new(type_name, "a").unwrap(),
            }],
            ..parse(&minimal(r#"{"a": {"type": "openApp"}}"#)).unwrap()
        };
        assert_eq!(
            codes(&validate(&flow)),
            [vec!["flow.field.empty"; empty], vec!["flow.noCheck"]].concat(),
            "{type_name}"
        );
    }
}

#[test]
fn model_edits_are_validated_too() {
    let mut flow = parse(ALL_BLOCKS).unwrap();
    flow.name = " ".into();
    flow.retries = Some(MAX_RETRIES + 1);
    flow.tags = vec!["a".into(), "a".into()];
    let dup = flow.blocks[0].clone();
    flow.blocks.push(dup);
    flow.blocks[1].id = "has space".into();
    let issues = validate(&flow);
    for code in [
        "flow.field.empty",
        "flow.tags.duplicate",
        "flow.field.range",
        "flow.block.duplicateId",
        "flow.block.invalidId",
    ] {
        assert!(
            codes(&issues).contains(&code),
            "{code} missing in {issues:?}"
        );
    }
}

#[test]
fn secrets_are_listed_by_name() {
    let flow = parse(ALL_BLOCKS).unwrap();
    assert_eq!(required_secrets(&flow), ["TOKEN"]);
}

#[test]
fn a_target_described_in_words_round_trips_and_must_say_something() {
    let flow = parse(&minimal(
        r#"{"a": {"type": "click", "target": {"description": "the blue Save button"}, "next": "b"},
            "b": {"type": "type", "text": "hi", "into": {"description": "the search field"}}}"#,
    ))
    .unwrap();
    assert_eq!(
        flow.blocks[0].kind,
        BlockKind::Click {
            target: Target::Description("the blue Save button".to_owned()),
            button: MouseButton::Left,
            double: false,
            modifiers: Modifiers::default(),
            wait_for_change: false,
            next: Some("b".to_owned()),
        }
    );
    let json = to_canonical_json(&flow);
    assert!(json.contains(r#""description": "the blue Save button""#));
    assert_eq!(parse(&json).unwrap(), flow);
    assert_eq!(codes(&validate(&flow)), ["flow.noCheck"]);

    let issues = issues_of(r#"{"a": {"type": "click", "target": {"description": "  "}}}"#);
    let found: Vec<(&str, Option<&str>)> = issues
        .iter()
        .map(|i| (i.code.as_str(), i.param("field")))
        .collect();
    assert_eq!(found, [("flow.field.empty", Some("target.description"))]);
    assert_eq!(
        describe(&issues[0]),
        "Block \"a\", field \"target.description\": can't be empty."
    );
}

#[test]
fn catalog_matches_the_file_format() {
    let specs = catalog();
    assert_eq!(specs.len(), 22);
    for spec in specs {
        let issues = parse_err(&minimal(&format!(
            r#"{{"a": {{"type": "{}", "bogus": 1}}}}"#,
            spec.type_name
        )));
        assert!(
            !codes(&issues).contains(&"flow.block.unknownType"),
            "{} is not a block type",
            spec.type_name
        );
    }
    let flow = parse(ALL_BLOCKS).unwrap();
    for b in &flow.blocks {
        let spec = catalog()
            .into_iter()
            .find(|s| s.type_name == b.kind.type_name())
            .unwrap();
        for (edge, _) in b.kind.edges() {
            assert!(
                spec.edges.iter().any(|e| *e == edge)
                    || (spec.type_name == "if" && else_if_index(&edge).is_some()),
                "{} has no {edge}",
                spec.type_name
            );
        }
    }
}

#[test]
fn pauses_closing_files_and_custom_blocks_round_trip() {
    let text = minimal(
        r#"{"a": {"type": "pause", "duration": "1500ms", "next": "b"},
            "b": {"type": "openFile", "files": ["f.txt"], "closeAfter": true, "next": "c"},
            "c": {"type": "runTest", "test": "blocks/Sign in.block.json",
                  "inputs": {"email": "${env.EMAIL}", "Bad name": "x"}}}"#,
    );
    let flow = parse(&text).unwrap();
    let saved = to_canonical_json(&flow);
    assert!(saved.contains(r#""duration": "1500ms""#), "{saved}");
    assert!(saved.contains(r#""closeAfter": true"#), "{saved}");
    assert!(
        saved.contains("\"inputs\": {\n        \"Bad name\": \"x\",\n        \"email\": \"${env.EMAIL}\"\n      }"),
        "{saved}"
    );
    assert_eq!(parse(&saved).unwrap(), flow);
    let issues = validate(&flow);
    let found: Vec<(&str, &str)> = issues
        .iter()
        .map(|i| (i.code.as_str(), i.param("field").unwrap_or("")))
        .collect();
    assert_eq!(found, [("flow.input.invalid", "inputs.Bad name")]);

    let issues = issues_of(
        r#"{"a": {"type": "pause", "duration": "0s", "next": "b"},
            "b": {"type": "runTest", "test": "x.block.json", "inputs": {"n": "${input.n}"}}}"#,
    );
    assert_eq!(codes(&issues), ["flow.field.range"]);
    let missing = parse_err(&minimal(r#"{"a": {"type": "pause"}}"#));
    assert_eq!(codes(&missing), ["flow.field.missing"]);
}

#[test]
fn a_custom_block_asks_for_its_inputs() {
    let dir = std::env::temp_dir().join(format!("jevision-inputs-{}", std::process::id()));
    fs::create_dir_all(dir.join("blocks")).unwrap();
    fs::write(
        dir.join("blocks/login.block.json"),
        minimal(
            r#"{"a": {"type": "type", "text": "${input.email} ${input.name}", "next": "b"},
                "b": {"type": "runTest", "test": "inner.block.json", "inputs": {"x": "${input.name}"}}}"#,
        ),
    )
    .unwrap();
    let block = parse(&fs::read_to_string(dir.join("blocks/login.block.json")).unwrap()).unwrap();
    assert_eq!(required_inputs(&block), ["email", "name"]);
    let flow = parse(&minimal(
        r#"{"a": {"type": "runTest", "test": "blocks/login.block.json", "inputs": {"email": "a@b.c"}}}"#,
    ))
    .unwrap();
    let issues = validate_in(&flow, &dir);
    fs::remove_dir_all(&dir).unwrap();
    let found: Vec<(&str, &str)> = issues
        .iter()
        .map(|i| (i.code.as_str(), i.param("field").unwrap_or("")))
        .collect();
    assert_eq!(found, [("flow.input.missing", "inputs.name")]);
}

#[test]
fn a_test_that_runs_a_package_needs_it_installed() {
    let dir = std::env::temp_dir().join(format!("jevision-package-use-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let package = dir.join("blocks/com.acme.sign-in.jevblock");
    let path = "blocks/com.acme.sign-in.jevblock/flow.json";
    fs::create_dir_all(&package).unwrap();
    let flow = parse(&minimal(&format!(
        r#"{{"a": {{"type": "runTest", "test": "{path}", "inputs": {{"email": "a@b.c"}}}}}}"#
    )))
    .unwrap();
    let missing = validate_in(&flow, &dir);
    fs::write(
        package.join("flow.json"),
        minimal(r#"{"a": {"type": "type", "text": "${input.email} ${input.password}"}}"#),
    )
    .unwrap();
    let installed = validate_in(&flow, &dir);
    fs::remove_dir_all(&dir).unwrap();
    assert_eq!(codes(&missing), ["flow.block.notInstalled"]);
    assert_eq!(missing[0].severity, Severity::Error);
    assert_eq!(missing[0].param("id"), Some("com.acme.sign-in"));
    assert_eq!(missing[0].param("block"), Some("a"));
    assert_eq!(codes(&installed), ["flow.input.missing"]);
    assert_eq!(installed[0].param("name"), Some("password"));
    assert!(is_block_path(path) && !is_block_path("blocks/com.acme.sign-in.jevblock/block.json"));
    assert_eq!(package_of_path(path), Some("com.acme.sign-in"));
}

#[test]
fn an_input_with_a_default_may_be_left_out() {
    let dir = std::env::temp_dir().join(format!("jevision-input-default-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let package = dir.join("blocks/com.acme.sign-in.jevblock");
    fs::create_dir_all(&package).unwrap();
    fs::write(
        package.join("block.json"),
        r#"{"format": 1, "id": "com.acme.sign-in", "name": "Sign in", "version": "1.0.0",
            "author": "Anna", "license": "MIT",
            "inputs": [{"name": "email", "default": "a@b.c"}, {"name": "password"}]}"#,
    )
    .unwrap();
    fs::write(
        package.join("flow.json"),
        minimal(r#"{"a": {"type": "type", "text": "${input.email} ${input.password}"}}"#),
    )
    .unwrap();
    let flow = parse(&minimal(
        r#"{"a": {"type": "runTest", "test": "blocks/com.acme.sign-in.jevblock/flow.json"}}"#,
    ))
    .unwrap();
    let found = validate_in(&flow, &dir);
    let defaults = input_defaults(&package.join("flow.json"));
    let elsewhere = input_defaults(&dir.join("t.flow.json"));
    fs::remove_dir_all(&dir).unwrap();
    assert_eq!(codes(&found), ["flow.input.missing"]);
    assert_eq!(found[0].param("name"), Some("password"));
    assert_eq!(defaults.len(), 1);
    assert_eq!(defaults["email"], "a@b.c");
    assert!(elsewhere.is_empty());
}

#[test]
fn missing_files_are_warnings_next_to_the_test() {
    let dir = std::env::temp_dir().join(format!("jevision-files-{}", std::process::id()));
    fs::create_dir_all(dir.join("refs")).unwrap();
    fs::create_dir_all(dir.join("data")).unwrap();
    fs::write(dir.join("refs/menu.png"), b"png").unwrap();
    fs::write(dir.join("data/users.csv"), b"name\n").unwrap();
    let flow = parse(ALL_BLOCKS).unwrap();
    let issues = validate_in(&flow, &dir);
    fs::remove_dir_all(&dir).unwrap();

    assert!(issues.iter().all(|i| i.severity == Severity::Warning));
    let found: Vec<(&str, &str, &str)> = issues
        .iter()
        .map(|i| {
            let p = |n| i.param(n).unwrap();
            (i.code.as_str(), p("block"), p("path"))
        })
        .collect();
    assert_eq!(
        found,
        [
            ("flow.file.missing", "doc", "fixtures/a.txt"),
            ("flow.reference.notSaved", "done", "refs/done.png"),
            ("flow.file.missing", "move", "refs/file.png"),
            (
                "flow.file.missing",
                "shared",
                "../shared/close-dialog.flow.json"
            ),
            ("flow.file.missing", "spinner", "refs/spinner.png"),
            ("flow.file.missing", "tip", "refs/help.png"),
        ]
    );
    assert_eq!(
        describe(&issues[1]),
        "Block \"done\", field \"reference.image\": refs/done.png doesn't exist yet; the first run will save the screen there as the reference."
    );
    assert_eq!(
        describe(&issues[4]),
        "Block \"spinner\", field \"until.disappears.image\": refs/spinner.png doesn't exist; the run will stop at this block until it's added."
    );
}

#[test]
fn paths_take_placeholders_like_text() {
    let dir = std::env::temp_dir().join(format!("jevision-path-vars-{}", std::process::id()));
    fs::create_dir_all(dir.join(".jevision")).unwrap();
    fs::write(
        dir.join(ENVIRONMENTS_FILE),
        r#"{"format":1,"environments":{"dev":{"SET":"a"},"prod":{}}}"#,
    )
    .unwrap();
    let flow = parse(&minimal(
        r#"{"a": {"type": "useData", "file": "data/${env.SET}.csv", "body": "b", "next": "c"},
            "b": {"type": "openFile", "files": ["docs/${data.name}.txt", "${env.NOPE}/x", "${home.X}"], "next": "a"},
            "c": {"type": "runTest", "test": "subs/${secret.WHO}.flow.json"}}"#,
    ))
    .unwrap();
    let issues = validate_in(&flow, &dir);
    fs::remove_dir_all(&dir).unwrap();
    let found: Vec<(&str, &str, &str)> = issues
        .iter()
        .map(|i| {
            let p = |n| i.param(n).unwrap_or("");
            (i.code.as_str(), p("block"), p("field"))
        })
        .collect();
    // No file is reported missing: a path with placeholders names one only during a run.
    assert_eq!(
        found,
        [
            ("flow.placeholder.invalid", "b", "files.2"),
            ("flow.env.partial", "a", "file"),
            ("flow.env.unknown", "b", "files.1"),
        ]
    );
    assert_eq!(required_secrets(&flow), ["WHO"]);
    assert_eq!(required_env_values(&flow), ["NOPE", "SET"]);
}

#[test]
fn a_file_a_check_block_saves_is_not_reported_twice() {
    let flow = parse(&minimal(
        r#"{"a": {"type": "check", "reference": {"image": "refs/home.png"}, "next": "b"},
            "b": {"type": "click", "target": {"image": "refs/home.png"}}}"#,
    ))
    .unwrap();
    let issues = validate_in(&flow, Path::new("/nonexistent"));
    assert_eq!(codes(&issues), ["flow.reference.notSaved"]);
}

#[test]
fn parse_errors_read_as_sentences() {
    let err = parse(r#"{"format": 1, "name": "t", "start": "a", "blocks": {"a": {"type": "openApp", "colour": 1}}}"#)
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "Field \"app\": required, but missing. Block \"a\", field \"colour\": not a known field."
    );
}

const CANVAS: &str = r#"{
  "format": 1,
  "name": "Drawn",
  "app": "com.apple.TextEdit",
  "start": "a",
  "blocks": {
    "a": { "type": "pressKeys", "keys": "a", "next": "b" },
    "b": { "type": "pressKeys", "keys": "b" }
  },
  "canvas": {
    "positions": { "gone": { "x": 1, "y": 1 }, "b": { "x": 32, "y": 160.5 }, "a": { "y": 32, "x": 32 } },
    "sections": { "verify": { "frame": { "x": 0, "y": 128, "width": 400, "height": 96 }, "title": "Verify" },
                  "prepare": { "title": "Prepare", "frame": { "x": 0, "y": 0, "width": 400, "height": 112 } } },
    "notes": { "n": { "text": "Flaky on CI", "time": "2026-10-18T09:30:00Z", "author": "Anna",
                      "frame": { "x": 448, "y": 32, "width": 224, "height": 144 } },
               "bare": { "text": "", "frame": { "x": 448, "y": 192, "width": 224, "height": 144 } } }
  }
}"#;

#[test]
fn the_canvas_round_trips_in_canonical_form() {
    let flow = parse(CANVAS).unwrap();
    assert_eq!(codes(&validate(&flow)), ["flow.noCheck"]);
    assert_eq!(flow.canvas.sections.len(), 2);
    assert_eq!(
        flow.canvas
            .notes
            .iter()
            .find(|n| n.id == "n")
            .unwrap()
            .author,
        "Anna"
    );
    let text = to_canonical_json(&flow);
    assert!(
        text.ends_with(
            r#"  "canvas": {
    "positions": {
      "a": {
        "x": 32,
        "y": 32
      },
      "b": {
        "x": 32,
        "y": 160.5
      }
    },
    "sections": {
      "prepare": {
        "title": "Prepare",
        "frame": {
          "x": 0,
          "y": 0,
          "width": 400,
          "height": 112
        }
      },
      "verify": {
        "title": "Verify",
        "frame": {
          "x": 0,
          "y": 128,
          "width": 400,
          "height": 96
        }
      }
    },
    "notes": {
      "bare": {
        "text": "",
        "frame": {
          "x": 448,
          "y": 192,
          "width": 224,
          "height": 144
        }
      },
      "n": {
        "text": "Flaky on CI",
        "author": "Anna",
        "time": "2026-10-18T09:30:00Z",
        "frame": {
          "x": 448,
          "y": 32,
          "width": 224,
          "height": 144
        }
      }
    }
  }
}
"#
        ),
        "{text}"
    );
    let again = parse(&text).unwrap();
    assert_eq!(to_canonical_json(&again), text);
    // A position of a block that isn't there is not written.
    assert!(!text.contains("gone"));

    let plain = parse(&minimal(r#"{"a": {"type": "pressKeys", "keys": "a"}}"#)).unwrap();
    assert!(!to_canonical_json(&plain).contains("canvas"));
}

#[test]
fn a_malformed_canvas_is_reported() {
    let issues =
        parse_err(&CANVAS.replace(r#""title": "Verify""#, r#""title": 3, "colour": "red""#));
    assert_eq!(codes(&issues), ["flow.field.type", "flow.field.unknown"]);
    assert_eq!(
        issues[0].param("field"),
        Some("canvas.sections.verify.title")
    );
    let issues = parse_err(&CANVAS.replace(r#""text": "Flaky on CI","#, ""));
    assert_eq!(codes(&issues), ["flow.field.missing"]);
    assert_eq!(issues[0].param("field"), Some("canvas.notes.n.text"));
    let issues = parse_err(&CANVAS.replace(r#""a": { "y": 32, "x": 32 }"#, r#""a": [32, 32]"#));
    assert_eq!(codes(&issues), ["flow.field.type"]);
    let issues = parse_err(&CANVAS.replace(r#""canvas": {"#, r#""canvas": { "zoom": 2,"#));
    assert_eq!(codes(&issues), ["flow.field.unknown"]);
    assert_eq!(issues[0].param("field"), Some("canvas.zoom"));
}

/// The blocks that work without the screen, plus an `if` that branches on a saved value.
const SYSTEM_BLOCKS: &str = r#"{
  "format": 1, "name": "System", "app": "com.apple.TextEdit", "start": "ask",
  "blocks": {
    "ask": { "type": "http", "method": "POST", "url": "http://127.0.0.1:8080/items", "headers": [{ "name": "Authorization", "value": "Bearer ${secret.TOKEN}" }], "body": "{}", "expectStatus": 201, "save": [{ "name": "id", "from": "body.items[0].id" }], "next": "count" },
    "count": { "type": "database", "file": "~/Library/app.sqlite", "query": "select count(*) from items", "saveAs": "rows", "expect": { "rows": 1 }, "next": "list" },
    "list": { "type": "command", "command": "ls ${var.id}", "folder": "fixtures", "exitCode": 1, "saveOutput": "listing", "timeout": "5s", "next": "calc" },
    "calc": { "type": "script", "language": "python3", "code": "print(1)", "inputs": [{ "name": "ID", "value": "${var.id}" }], "outputs": [{ "name": "total", "type": "number" }], "next": "wait" },
    "wait": { "type": "checkFile", "path": "out/${var.id}.txt", "until": { "contains": "done" }, "next": "branch" },
    "branch": { "type": "if", "value": { "of": "${var.total}", "is": "greaterThan", "to": "3" }, "then": "seen", "elseIf": [{ "reference": { "image": "refs/a.png" }, "then": "seen" }] },
    "seen": { "type": "check", "reference": { "image": "refs/a.png" } }
  }
}"#;

#[test]
fn system_blocks_round_trip_in_canonical_form() {
    let flow = parse(SYSTEM_BLOCKS).unwrap();
    let all = validate(&flow);
    let issues: Vec<&str> = all
        .iter()
        .filter(|i| i.code != "flow.file.missing")
        .map(|i| i.code.as_str())
        .collect();
    assert_eq!(issues, Vec::<&str>::new());
    let once = to_canonical_json(&flow);
    assert_eq!(canonicalize(&once).unwrap(), once);
    assert_eq!(parse(&once).unwrap(), flow);
    assert!(once.contains("\"expectStatus\": 201"));
    assert!(once.contains("\"until\": {\n        \"contains\": \"done\"\n      }"));
    assert!(once.contains("\"is\": \"greaterThan\""));
    assert_eq!(saved_variables(&flow), ["id", "listing", "rows", "total"]);
    assert_eq!(required_secrets(&flow), ["TOKEN"]);

    let defaults = canonicalize(&minimal(
        r#"{"a": {"type": "http", "url": "https://x.test", "expectStatus": "success"}, "b": {"type": "command", "command": "true", "exitCode": 0}}"#,
    ))
    .unwrap();
    assert!(!defaults.contains("expectStatus") && !defaults.contains("exitCode"));
    assert!(defaults.contains("\"method\": \"GET\"") && defaults.contains("\"timeout\": \"30s\""));
}

#[test]
fn system_blocks_are_validated() {
    let issues = issues_of(
        r#"{
        "a": {"type": "script", "language": "sh", "code": " ", "inputs": [{"name": "1x", "value": ""}, {"name": "A", "value": ""}, {"name": "A", "value": ""}], "outputs": [{"name": "out", "type": "text"}], "next": "b"},
        "b": {"type": "http", "url": "ftp://x", "headers": [{"name": "bad name", "value": "1"}], "expectStatus": 99, "save": [{"name": "out", "from": "body.["}], "next": "c"},
        "c": {"type": "command", "command": "", "exitCode": 300, "next": "d"},
        "d": {"type": "checkFile", "path": "", "until": {"contains": ""}, "next": "e"},
        "e": {"type": "database", "file": "", "query": "", "next": "f"},
        "f": {"type": "if", "value": {"of": "", "is": "lessThan", "to": "many"}, "then": "g"},
        "g": {"type": "type", "text": "${var.nobody}"}
    }"#,
    );
    let found: Vec<(&str, Option<&str>, Option<&str>)> = issues
        .iter()
        .map(|i| (i.code.as_str(), i.param("block"), i.param("field")))
        .collect();
    for expected in [
        ("flow.field.empty", Some("a"), Some("code")),
        ("flow.name.invalid", Some("a"), Some("inputs.0.name")),
        ("flow.name.duplicate", Some("a"), Some("inputs.2.name")),
        ("flow.http.url", Some("b"), Some("url")),
        ("flow.http.header", Some("b"), Some("headers.0.name")),
        ("flow.field.range", Some("b"), Some("expectStatus")),
        ("flow.http.saveFrom", Some("b"), Some("save.0.from")),
        ("flow.field.empty", Some("c"), Some("command")),
        ("flow.field.range", Some("c"), Some("exitCode")),
        ("flow.field.empty", Some("d"), Some("path")),
        ("flow.field.empty", Some("d"), Some("until.contains")),
        ("flow.field.empty", Some("e"), Some("file")),
        ("flow.field.empty", Some("e"), Some("query")),
        ("flow.field.empty", Some("f"), Some("value.of")),
        ("flow.value.notNumber", Some("f"), Some("value.to")),
        ("flow.var.unknown", Some("g"), Some("text")),
    ] {
        assert!(
            found.contains(&expected),
            "{expected:?} missing in {found:?}"
        );
    }
}

#[test]
fn json_paths_name_fields_and_items() {
    assert_eq!(
        json_path("items[0].id"),
        Some(vec![
            JsonStep::Field("items"),
            JsonStep::Index(0),
            JsonStep::Field("id")
        ])
    );
    assert_eq!(
        json_path("[2].name"),
        Some(vec![JsonStep::Index(2), JsonStep::Field("name")])
    );
    for wrong in ["", "a..b", "a[", "a[x]", "a]", "a.[0]"] {
        assert_eq!(json_path(wrong), None, "{wrong}");
    }
}

#[test]
fn dependencies_round_trip_and_are_validated() {
    let text = format!(
        r#"{{"format": {FORMAT_VERSION}, "name": "Pay", "app": "com.example.App", "retries": 1,
            "dependsOn": ["../login.flow.json", "cart.flow.json"], "start": "a",
            "blocks": {{"a": {{"type": "pressKeys", "keys": "return"}}}}}}"#
    );
    let flow = parse(&text).unwrap();
    assert_eq!(flow.depends_on, ["../login.flow.json", "cart.flow.json"]);
    let saved = to_canonical_json(&flow);
    assert!(saved.contains("\"retries\": 1,\n  \"dependsOn\": [\n    \"../login.flow.json\""));
    assert_eq!(parse(&saved).unwrap(), flow);
    assert!(
        validate(&flow)
            .iter()
            .all(|i| i.severity == Severity::Warning)
    );

    let mut bad = flow;
    bad.depends_on = vec![
        "/abs.flow.json".into(),
        "cart.csv".into(),
        "cart.flow.json".into(),
        "cart.flow.json".into(),
    ];
    let issues = validate(&bad);
    assert_eq!(
        codes(&issues),
        [
            "flow.dependency.invalid",
            "flow.dependency.invalid",
            "flow.dependency.duplicate",
            "flow.noCheck"
        ]
    );
    assert_eq!(
        describe(&issues[2]),
        "Field \"dependsOn\": cart.flow.json is listed twice."
    );
}

#[test]
fn the_test_card_is_saved_in_canonical_form() {
    let text = r#"{"flaky": true, "link": "https://example.com/issues/12", "platform": "macOS 26",
        "author": "Anna", "preconditions": "Signed in as a teacher", "tags": ["smoke"],
        "format": 1, "name": "t", "app": "com.example.App", "start": "a",
        "blocks": {"a": {"type": "check", "reference": {"image": "refs/a.png"}}}}"#;
    let flow = parse(text).unwrap();
    assert_eq!(
        flow.preconditions.as_deref(),
        Some("Signed in as a teacher")
    );
    assert_eq!(flow.author.as_deref(), Some("Anna"));
    assert_eq!(flow.platform.as_deref(), Some("macOS 26"));
    assert_eq!(flow.link.as_deref(), Some("https://example.com/issues/12"));
    assert!(flow.flaky);
    assert_eq!(validate(&flow), vec![]);
    let once = to_canonical_json(&flow);
    assert!(
        once.contains(
            "\"tags\": [\n    \"smoke\"\n  ],\n  \"preconditions\": \"Signed in as a teacher\",\n  \
             \"author\": \"Anna\",\n  \"platform\": \"macOS 26\",\n  \
             \"link\": \"https://example.com/issues/12\",\n  \"flaky\": true,\n  \"start\""
        ),
        "{once}"
    );
    assert_eq!(parse(&once).unwrap(), flow);

    // Left out when not set; `"flaky": false` is the default and isn't written.
    let plain = canonicalize(
        &minimal(r#"{"a": {"type": "openApp"}}"#)
            .replace(r#""name": "t","#, r#""name": "t", "flaky": false,"#),
    )
    .unwrap();
    for key in ["preconditions", "author", "platform", "link", "flaky"] {
        assert!(!plain.contains(key), "{key} in {plain}");
    }

    let mut flow = flow;
    flow.link = Some("JIRA-12".into());
    let issues = validate(&flow);
    assert_eq!(codes(&issues), ["flow.link.invalid"]);
    assert_eq!(issues[0].param("field"), Some("link"));
    assert_eq!(
        codes(
            &parse(&minimal("{}").replace("\"t\",", "\"t\", \"flaky\": 1,"))
                .unwrap_err()
                .issues
        ),
        ["flow.field.type"]
    );
}

#[test]
fn clicks_and_drags_hold_keys_and_a_button() {
    let text = minimal(
        r#"{"a": {"type": "click", "target": {"description": "the third file"}, "modifiers": ["ctrl", "cmd", "cmd"], "next": "b"},
            "b": {"type": "drag", "from": {"point": {"x": 0.1, "y": 0.1}}, "to": {"point": {"x": 0.9, "y": 0.9}},
                  "button": "right", "modifiers": ["option", "shift"], "next": "c"},
            "c": {"type": "drag", "from": {"point": {"x": 0.1, "y": 0.1}}, "to": {"point": {"x": 0.9, "y": 0.9}}, "button": "left", "modifiers": []}}"#,
    );
    let flow = parse(&text).unwrap();
    let BlockKind::Click { modifiers, .. } = &flow.blocks[0].kind else {
        panic!("not a click");
    };
    assert_eq!(
        *modifiers,
        Modifiers {
            command: true,
            control: true,
            ..Modifiers::default()
        }
    );
    let BlockKind::Drag {
        button, modifiers, ..
    } = &flow.blocks[1].kind
    else {
        panic!("not a drag");
    };
    assert_eq!(*button, MouseButton::Right);
    assert_eq!(modifiers.names(), ["shift", "option"]);

    let once = to_canonical_json(&flow);
    assert!(once.contains("\"modifiers\": [\n        \"cmd\",\n        \"ctrl\"\n      ]"));
    assert!(once.contains("\"button\": \"right\""));
    // The left button and no keys are the defaults and are left out.
    let last = once.split("\"c\"").last().unwrap_or_default();
    assert!(!last.contains("button") && !last.contains("modifiers"));
    assert_eq!(parse(&once).unwrap(), flow);
    assert_eq!(canonicalize(&once).unwrap(), once);

    let bad = parse_err(&minimal(
        r#"{"a": {"type": "click", "target": {"description": "x"}, "modifiers": ["fn"]},
            "b": {"type": "drag", "from": {"description": "x"}, "to": {"description": "y"}, "button": "middle", "modifiers": "cmd"}}"#,
    ));
    let found: Vec<(&str, Option<&str>)> = bad
        .iter()
        .map(|i| (i.code.as_str(), i.param("field")))
        .collect();
    assert!(
        found.contains(&("flow.field.value", Some("modifiers"))),
        "{found:?}"
    );
    assert!(
        found.contains(&("flow.field.value", Some("button"))),
        "{found:?}"
    );
    assert!(
        found.contains(&("flow.field.type", Some("modifiers"))),
        "{found:?}"
    );
}

const CLIPBOARD_AND_TIME: &str = r#"{
  "format": 1, "name": "t", "app": "com.example.App", "start": "put",
  "blocks": {
    "put": {"type": "clipboard", "action": "put", "text": "order ${data.id}", "next": "read"},
    "read": {"type": "clipboard", "action": "read", "saveAs": "copied", "next": "check"},
    "check": {"type": "clipboard", "action": "check", "is": "contains", "to": "${var.copied}", "timeout": "5s", "next": "now"},
    "now": {"type": "dateTime", "saveAs": "today", "format": "%d.%m.%Y", "shift": {"amount": -2, "unit": "days"}, "next": "stamp"},
    "stamp": {"type": "dateTime", "saveAs": "stamp"}
  }
}"#;

#[test]
fn clipboard_and_date_blocks_round_trip_in_canonical_form() {
    let flow = parse(CLIPBOARD_AND_TIME).unwrap();
    assert_eq!(validate(&flow), vec![]);
    assert_eq!(saved_variables(&flow), ["copied", "stamp", "today"]);
    let once = to_canonical_json(&flow);
    assert_eq!(canonicalize(&once).unwrap(), once);
    assert_eq!(parse(&once).unwrap(), flow);
    assert!(once.contains("\"action\": \"check\",\n      \"is\": \"contains\""));
    assert!(once.contains("\"timeout\": \"5s\""));
    assert!(once.contains("\"shift\": {\n        \"amount\": -2,\n        \"unit\": \"days\""));
    // The usual format is left out.
    let stamp = once.split("\"stamp\": {").nth(1).unwrap();
    assert!(!stamp.contains("format") && !stamp.contains("shift"));
    let kind = |id: &str| &flow.blocks.iter().find(|b| b.id == id).unwrap().kind;
    assert_eq!(
        *kind("put"),
        BlockKind::Clipboard {
            action: ClipboardAction::Put {
                text: "order ${data.id}".to_owned()
            },
            next: Some("read".to_owned()),
        }
    );
    // Waiting for the clipboard to hold something is a check; a date and a put are not.
    let ids = ["put", "read", "check", "now", "stamp"];
    assert_eq!(
        ids.map(|id| kind(id).checks()),
        [false, false, true, false, false]
    );
    // Only the check waits, and for as long as its timeout says.
    assert_eq!(
        ids.map(|id| kind(id).wait_ms()),
        [None, None, Some(5000), None, None]
    );
    for name in ["clipboard", "dateTime"] {
        let block = BlockKind::new(name, "x").unwrap();
        assert_eq!(block.type_name(), name);
    }
}

#[test]
fn open_app_can_fix_the_window_and_reset_the_state() {
    let text = minimal(
        r#"{"a": {"type": "openApp", "resetState": true,
                  "windowSize": {"width": 900, "height": 600},
                  "windowPosition": {"x": -40, "y": 60}, "next": "b"},
            "b": {"type": "openApp"}}"#,
    );
    let flow = parse(&text).unwrap();
    match &flow.blocks[0].kind {
        BlockKind::OpenApp {
            reset_state,
            window_size,
            window_position,
            ..
        } => {
            assert!(reset_state);
            assert_eq!(
                *window_size,
                Some(WindowSize {
                    width: 900,
                    height: 600
                })
            );
            assert_eq!(*window_position, Some(WindowPosition { x: -40, y: 60 }));
        }
        other => panic!("{other:?}"),
    }
    let saved = to_canonical_json(&flow);
    assert_eq!(parse(&saved).unwrap(), flow);
    assert!(saved.contains(r#""resetState": true"#), "{saved}");
    // A block that asks for nothing writes none of the new fields: old files stay as they were.
    let plain = saved.split(r#""b": "#).nth(1).unwrap();
    for field in ["resetState", "windowSize", "windowPosition"] {
        assert!(!plain.contains(field), "{plain}");
    }
    assert!(validate(&flow).iter().all(|i| i.code == "flow.noCheck"));
}

#[test]
fn open_app_window_fields_are_checked() {
    let parsed = parse_err(&minimal(
        r#"{"a": {"type": "openApp", "windowSize": {"width": 900}, "next": "b"},
            "b": {"type": "openApp", "windowPosition": {"x": 1.5, "y": 2}, "next": "c"},
            "c": {"type": "openApp", "windowSize": {"width": 900, "height": 600, "depth": 1}, "resetState": "yes"}}"#,
    ));
    let found: Vec<(&str, Option<&str>, Option<&str>)> = parsed
        .iter()
        .map(|i| (i.code.as_str(), i.param("block"), i.param("field")))
        .collect();
    for expected in [
        ("flow.field.missing", Some("a"), Some("windowSize.height")),
        ("flow.field.type", Some("b"), Some("windowPosition.x")),
        ("flow.field.unknown", Some("c"), Some("windowSize.depth")),
        ("flow.field.type", Some("c"), Some("resetState")),
    ] {
        assert!(
            found.contains(&expected),
            "{expected:?} missing in {found:?}"
        );
    }

    let issues = issues_of(
        r#"{"a": {"type": "openApp", "windowSize": {"width": 50, "height": 30000},
                  "windowPosition": {"x": 20001, "y": -20000}}}"#,
    );
    let found: Vec<(&str, Option<&str>)> = issues
        .iter()
        .map(|i| (i.code.as_str(), i.param("field")))
        .collect();
    assert_eq!(
        found,
        [
            ("flow.field.range", Some("windowSize.width")),
            ("flow.field.range", Some("windowSize.height")),
            ("flow.field.range", Some("windowPosition.x")),
        ]
    );
}

#[test]
fn clipboard_and_date_blocks_are_validated() {
    let parsed = parse_err(&minimal(
        r#"{"a": {"type": "clipboard", "action": "paste", "text": "x"},
            "b": {"type": "clipboard", "action": "put"},
            "c": {"type": "clipboard", "action": "read", "text": "x", "saveAs": "v"},
            "d": {"type": "clipboard", "action": "check", "to": "x"},
            "e": {"type": "dateTime", "shift": {"amount": 1, "unit": "fortnights"}}}"#,
    ));
    let found: Vec<(&str, Option<&str>, Option<&str>)> = parsed
        .iter()
        .map(|i| (i.code.as_str(), i.param("block"), i.param("field")))
        .collect();
    for expected in [
        ("flow.field.value", Some("a"), Some("action")),
        ("flow.field.missing", Some("b"), Some("text")),
        ("flow.field.unknown", Some("c"), Some("text")),
        ("flow.field.missing", Some("d"), Some("is")),
        ("flow.field.missing", Some("e"), Some("saveAs")),
        ("flow.field.value", Some("e"), Some("shift.unit")),
    ] {
        assert!(
            found.contains(&expected),
            "{expected:?} missing in {found:?}"
        );
    }

    let issues = issues_of(
        r#"{"a": {"type": "clipboard", "action": "read", "saveAs": "1st", "next": "b"},
            "b": {"type": "clipboard", "action": "check", "is": "greaterThan", "to": "many", "timeout": "0s", "next": "c"},
            "c": {"type": "dateTime", "saveAs": "when", "format": "%Q", "shift": {"amount": 100001, "unit": "days"}, "next": "d"},
            "d": {"type": "dateTime", "saveAs": "when2", "format": ""}}"#,
    );
    let found: Vec<(&str, Option<&str>, Option<&str>)> = issues
        .iter()
        .map(|i| (i.code.as_str(), i.param("block"), i.param("field")))
        .collect();
    for expected in [
        ("flow.name.invalid", Some("a"), Some("saveAs")),
        ("flow.value.notNumber", Some("b"), Some("to")),
        ("flow.field.range", Some("b"), Some("timeout")),
        ("flow.dateTime.format", Some("c"), Some("format")),
        ("flow.field.range", Some("c"), Some("shift.amount")),
        ("flow.field.empty", Some("d"), Some("format")),
    ] {
        assert!(
            found.contains(&expected),
            "{expected:?} missing in {found:?}"
        );
    }
}

#[test]
fn script_values_have_types() {
    let text = minimal(
        r#"{"a": {"type": "script", "language": "python3", "code": "print(1)",
            "inputs": [{"name": "LABEL", "value": "hi"},
                       {"name": "COUNT", "type": "number", "value": "3"},
                       {"name": "FLAG", "type": "boolean", "value": "yes"},
                       {"name": "SHOT", "type": "image", "value": "refs/a.png"},
                       {"name": "WHERE", "type": "area", "value": "0.1,0.2,0.5,0.3"}],
            "outputs": [{"name": "shot", "type": "image"}, {"name": "box", "type": "area"}, {"name": "n", "type": "number"}]}}"#,
    );
    let flow = parse(&text).unwrap();
    let once = to_canonical_json(&flow);
    assert_eq!(parse(&once).unwrap(), flow);
    assert!(
        once.contains("\"name\": \"LABEL\",\n          \"value\": \"hi\""),
        "text is the default and is left out"
    );
    assert!(once.contains(
        "\"name\": \"SHOT\",\n          \"type\": \"image\",\n          \"value\": \"refs/a.png\""
    ));
    assert_eq!(saved_variables(&flow), ["box", "n", "shot"]);

    // Before this, inputs had no type and outputs knew only three.
    let old = parse(&minimal(
        r#"{"a": {"type": "script", "language": "sh", "code": "true", "inputs": [{"name": "A", "value": "1"}], "outputs": [{"name": "o", "type": "boolean"}]}}"#,
    ))
    .unwrap();
    assert!(matches!(
        &old.blocks[0].kind,
        BlockKind::Script { inputs, .. } if inputs[0].kind == ValueKind::Text
    ));

    let bad = parse_err(&minimal(
        r#"{"a": {"type": "script", "language": "sh", "code": "true",
            "inputs": [{"name": "A", "type": "colour", "value": "1"}],
            "outputs": [{"name": "o", "type": "picture"}, {"name": "p"}]}}"#,
    ));
    let found: Vec<(&str, Option<&str>)> = bad
        .iter()
        .map(|i| (i.code.as_str(), i.param("field")))
        .collect();
    assert!(
        found.contains(&("flow.field.value", Some("inputs.0.type"))),
        "{found:?}"
    );
    assert!(
        found.contains(&("flow.field.value", Some("outputs.0.type"))),
        "{found:?}"
    );
    assert!(
        found.contains(&("flow.field.missing", Some("outputs.1.type"))),
        "{found:?}"
    );
}

#[test]
fn script_values_must_fit_their_type() {
    let issues = issues_of(
        r#"{"a": {"type": "script", "language": "sh", "code": "true",
            "inputs": [{"name": "N", "type": "number", "value": "three"},
                       {"name": "B", "type": "boolean", "value": "maybe"},
                       {"name": "I", "type": "image", "value": "refs/a.jpg"},
                       {"name": "R", "type": "area", "value": "0.5,0.5,0.8,0.1"},
                       {"name": "E", "type": "number", "value": " "},
                       {"name": "OK", "type": "number", "value": "${data.count}"},
                       {"name": "T", "value": "${var.box}"}],
            "outputs": [{"name": "box", "type": "area"}], "next": "b"},
            "b": {"type": "script", "language": "sh", "code": "true",
            "inputs": [{"name": "S", "type": "image", "value": "${var.box}"},
                       {"name": "AREA", "type": "area", "value": "${var.box}"},
                       {"name": "TXT", "type": "text", "value": "${var.box}"},
                       {"name": "FROMTEXT", "type": "number", "value": "${var.word}"}],
            "next": "c"},
            "c": {"type": "command", "command": "echo hi", "saveOutput": "word"}}"#,
    );
    let found: Vec<(&str, Option<&str>, Option<&str>)> = issues
        .iter()
        .map(|i| (i.code.as_str(), i.param("block"), i.param("field")))
        .collect();
    for expected in [
        ("flow.script.inputType", Some("a"), Some("inputs.0.value")),
        ("flow.script.inputType", Some("a"), Some("inputs.1.value")),
        ("flow.script.inputType", Some("a"), Some("inputs.2.value")),
        ("flow.script.inputType", Some("a"), Some("inputs.3.value")),
        ("flow.field.empty", Some("a"), Some("inputs.4.value")),
        ("flow.script.inputKind", Some("b"), Some("inputs.0.value")),
    ] {
        assert!(
            found.contains(&expected),
            "{expected:?} missing in {found:?}"
        );
    }
    let wrong = found
        .iter()
        .filter(|f| f.0.starts_with("flow.script"))
        .count();
    assert_eq!(
        wrong, 5,
        "a placeholder or a variable that holds text may fit: {found:?}"
    );
    let kind = issues
        .iter()
        .find(|i| i.code == "flow.script.inputKind")
        .unwrap();
    assert_eq!(kind.param("variable"), Some("box"));
    assert_eq!(kind.param("from"), Some("area"));

    assert_eq!(
        ValueKind::Area.parse(" 0.1, 0.2,0.5 ,0.3").as_deref(),
        Some("0.1,0.2,0.5,0.3")
    );
    assert_eq!(ValueKind::Area.parse("0.1,0.2,0.5"), None);
    assert_eq!(ValueKind::Area.parse("0.1,0.2,0.5,0.3,0.1"), None);
    assert_eq!(ValueKind::Area.parse("0,0,1,1").as_deref(), Some("0,0,1,1"));
    assert_eq!(ValueKind::Area.parse("0,0,0,1"), None);
    assert_eq!(ValueKind::Number.parse(" 3.5\n").as_deref(), Some("3.5"));
    assert_eq!(ValueKind::Number.parse("NaN"), None);
    assert_eq!(ValueKind::Boolean.parse("Yes").as_deref(), Some("true"));
    assert_eq!(ValueKind::Boolean.parse("0").as_deref(), Some("false"));
    assert_eq!(ValueKind::Text.parse(" a ").as_deref(), Some(" a "));
    assert_eq!(ValueKind::Image.parse("  "), None);
}
