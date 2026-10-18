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
    "save": { "type": "pressKeys", "keys": "cmd+s", "next": "menu" },
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

fn issues_of(blocks: &str) -> Vec<Issue> {
    validate(&parse(&minimal(blocks)).unwrap())
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
            "menu", "spinner", "settle", "branch", "again", "shared", "done"
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
            "f": {"type": "check", "reference": {"image": "x.png", "model": "gpt-5"}}}"#,
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

    let start = validate(&parse(&minimal(r#"{"b": {"type": "openApp"}}"#)).unwrap());
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
    assert_eq!(validate(&flow), vec![]);
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
            ("flow.field.missing", Some("elseIf.0.reference")),
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
            "g": {"type": "pressKeys", "keys": "command+s"}}"#,
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
        ]
    );
    assert_eq!(issues[0].param("placeholder"), Some("${home.X}"));
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
fn catalog_matches_the_file_format() {
    let specs = catalog();
    assert_eq!(specs.len(), 11);
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
            (
                "flow.file.missing",
                "shared",
                "../shared/close-dialog.flow.json"
            ),
            ("flow.file.missing", "spinner", "refs/spinner.png"),
        ]
    );
    assert_eq!(
        describe(&issues[1]),
        "Block \"done\", field \"reference.image\": refs/done.png doesn't exist yet; the first run will save the screen there as the reference."
    );
    assert_eq!(
        describe(&issues[3]),
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
    assert_eq!(validate(&flow), vec![]);
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
