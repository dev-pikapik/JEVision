//! Interface elements and the arguments and environment of `openApp`, as a test file writes them.

use jevision_core::*;

fn minimal(blocks: &str) -> String {
    format!(
        r#"{{"format": 1, "name": "t", "app": "com.example.App", "start": "a", "blocks": {blocks}}}"#
    )
}

/// `(code, field)` of every issue but the warning that a test checks nothing.
fn issues_of(blocks: &str) -> Vec<(String, String)> {
    validate(&parse(&minimal(blocks)).unwrap())
        .into_iter()
        .filter(|i| i.code != "flow.noCheck")
        .map(|i| (i.code.clone(), i.param("field").unwrap_or("").to_owned()))
        .collect()
}

fn parse_codes(blocks: &str) -> Vec<String> {
    parse(&minimal(blocks))
        .unwrap_err()
        .issues
        .into_iter()
        .map(|i| i.code)
        .collect()
}

fn kind(blocks: &str) -> BlockKind {
    parse(&minimal(blocks)).unwrap().blocks.remove(0).kind
}

fn query(id: &str, role: &str, title: &str, index: Option<u32>) -> ElementQuery {
    ElementQuery {
        id: id.into(),
        role: role.into(),
        title: title.into(),
        index,
    }
}

#[test]
fn every_block_that_points_or_checks_reads_an_element() {
    let save = || query("", "button", "Save", None);
    assert!(matches!(
        kind(r#"{"a": {"type": "click", "target": {"element": {"role": "button", "title": "Save"}}}}"#),
        BlockKind::Click { target: Target::Element(e), .. } if e == save()
    ));
    assert!(matches!(
        kind(r#"{"a": {"type": "type", "text": "x", "into": {"element": {"id": "name", "index": 2}}}}"#),
        BlockKind::Type { into: Some(Target::Element(e)), .. } if e == query("name", "", "", Some(2))
    ));
    assert!(matches!(
        kind(r#"{"a": {"type": "check", "element": {"role": "button", "title": "Save", "is": "enabled"}}}"#),
        BlockKind::Check { of: CheckOf::Element(c), .. } if c.element == save() && c.is == ElementState::Enabled
    ));
    assert!(matches!(
        kind(r#"{"a": {"type": "waitFor", "until": {"element": {"id": "name", "is": "valueEquals", "to": "Ann"}}}}"#),
        BlockKind::WaitFor { until: WaitUntil::Element(c), .. } if c.is == ElementState::ValueEquals("Ann".into())
    ));
    let BlockKind::If {
        condition, else_if, ..
    } = kind(
        r#"{"a": {"type": "if", "element": {"role": "checkBox", "is": "selected"},
            "elseIf": [{"element": {"title": "Later", "is": "absent"}}]}}"#,
    )
    else {
        panic!("not an if")
    };
    assert!(matches!(condition, Condition::Element(c) if c.is == ElementState::Selected));
    assert!(
        matches!(&else_if[0].condition, Condition::Element(c) if c.element.title == "Later" && c.is == ElementState::Absent)
    );
}

#[test]
fn an_element_is_written_flat_in_canonical_form_and_reads_back_the_same() {
    let text = minimal(
        r#"{"a": {"type": "check", "element": {"to": "Ann", "is": "valueContains", "index": 0, "title": "Name", "role": "textField", "id": "name"}, "next": "b"},
            "b": {"type": "openApp", "environment": {"B": "2", "A": "${env.API}"}, "arguments": ["--demo", "${data.user}"], "next": "c"},
            "c": {"type": "click", "target": {"element": {"title": "Save"}}}}"#,
    );
    let flow = parse(&text).unwrap();
    let once = to_canonical_json(&flow);
    assert!(once.contains(
        r#""element": {
        "id": "name",
        "role": "textField",
        "title": "Name",
        "index": 0,
        "is": "valueContains",
        "to": "Ann"
      }"#
    ));
    assert!(once.contains(
        r#""arguments": [
        "--demo",
        "${data.user}"
      ],
      "environment": {
        "A": "${env.API}",
        "B": "2"
      }"#
    ));
    assert!(once.contains(
        r#""target": {
        "element": {
          "title": "Save"
        }
      }"#
    ));
    assert_eq!(parse(&once).unwrap(), flow);
    assert_eq!(canonicalize(&once).unwrap(), once);
}

#[test]
fn an_open_app_block_without_arguments_or_environment_is_written_as_before() {
    let old = minimal(r#"{"a": {"type": "openApp"}}"#);
    let once = canonicalize(&old).unwrap();
    assert!(!once.contains("arguments") && !once.contains("environment"));
    assert!(matches!(
        kind(r#"{"a": {"type": "openApp"}}"#),
        BlockKind::OpenApp { arguments, environment, .. } if arguments.is_empty() && environment.is_empty()
    ));
    assert!(once.contains("\"format\": 1,"));
}

#[test]
fn an_element_needs_something_to_find_it_by_and_a_known_kind() {
    assert_eq!(
        issues_of(r#"{"a": {"type": "click", "target": {"element": {"index": 1}}}}"#),
        [("flow.element.empty".into(), "target.element".into())]
    );
    assert_eq!(
        issues_of(r#"{"a": {"type": "click", "target": {"element": {"role": "pushButton"}}}}"#),
        [("flow.field.value".into(), "target.element.role".into())]
    );
    assert_eq!(
        issues_of(r#"{"a": {"type": "waitFor", "until": {"element": {"is": "present"}}}}"#),
        [("flow.element.empty".into(), "until.element".into())]
    );
    assert_eq!(
        issues_of(
            r#"{"a": {"type": "if", "reference": {"image": "x.png"}, "elseIf": [{"element": {"role": "x", "is": "present"}}]}}"#
        )
        .into_iter()
        .filter(|(code, _)| code != "flow.reference.notSaved")
        .collect::<Vec<_>>(),
        [("flow.field.value".into(), "elseIf.0.element.role".into())]
    );
    assert_eq!(
        issues_of(
            r#"{"a": {"type": "check", "element": {"role": "button", "is": "titleContains", "to": ""}}}"#
        ),
        [("flow.field.empty".into(), "element.to".into())]
    );
    assert!(
        issues_of(r#"{"a": {"type": "check", "element": {"role": "switch", "is": "selected"}}}"#)
            .is_empty()
    );
}

#[test]
fn a_state_is_one_the_format_knows_with_text_only_where_it_compares_text() {
    let codes = |element: &str| {
        parse_codes(&format!(
            r#"{{"a": {{"type": "check", "element": {element}}}}}"#
        ))
    };
    assert_eq!(
        codes(r#"{"role": "button", "is": "visible"}"#),
        ["flow.field.value"]
    );
    assert_eq!(
        codes(r#"{"role": "button", "is": "valueEquals"}"#),
        ["flow.field.missing"]
    );
    assert_eq!(
        codes(r#"{"role": "button", "is": "enabled", "to": "x"}"#),
        ["flow.element.toNeedsText"]
    );
    assert_eq!(codes(r#"{"role": "button"}"#), ["flow.field.missing"]);
    assert_eq!(
        codes(r#"{"role": "button", "is": "present", "label": "x"}"#),
        ["flow.field.unknown"]
    );
    assert_eq!(
        parse_codes(
            r#"{"a": {"type": "check", "reference": {"image": "x.png"}, "element": {"role": "button", "is": "present"}}}"#
        ),
        ["flow.field.oneOf"]
    );
}

#[test]
fn placeholders_in_arguments_environment_and_elements_are_checked() {
    let issues = issues_of(
        r#"{"a": {"type": "openApp", "arguments": ["--user=${data.user}", "${nope.x}"],
                  "environment": {"API": "${env.API}", "1BAD": "x", "TOKEN": "${secret}"}, "next": "b"},
            "b": {"type": "click", "target": {"element": {"title": "${var.missing}", "id": "${bad"}}}}"#,
    );
    assert!(issues.contains(&("flow.placeholder.invalid".into(), "arguments.1".into())));
    assert!(issues.contains(&(
        "flow.placeholder.invalid".into(),
        "environment.TOKEN".into()
    )));
    assert!(issues.contains(&("flow.name.invalid".into(), "environment.1BAD".into())));
    assert!(issues.contains(&("flow.var.unknown".into(), "target.element.title".into())));
    assert!(issues.contains(&(
        "flow.placeholder.invalid".into(),
        "target.element.id".into()
    )));
    assert!(
        !issues
            .iter()
            .any(|(_, f)| f == "arguments.0" || f == "environment.API")
    );
}

#[test]
fn a_check_or_wait_on_an_element_counts_as_checking_the_app() {
    let warned = |blocks: &str| {
        validate(&parse(&minimal(blocks)).unwrap())
            .iter()
            .any(|i| i.code == "flow.noCheck")
    };
    assert!(!warned(
        r#"{"a": {"type": "check", "element": {"role": "button", "is": "present"}}}"#
    ));
    assert!(!warned(
        r#"{"a": {"type": "waitFor", "until": {"element": {"role": "button", "is": "present"}}}}"#
    ));
    assert!(warned(
        r#"{"a": {"type": "click", "target": {"element": {"role": "button"}}}}"#
    ));
}
