use super::*;
use crate::render;

fn fact(label: &str, value: &str) -> (String, String) {
    (label.into(), value.into())
}

/// A message with every part, and markup where text from a test could carry it.
fn full() -> Message {
    Message::new(
        "Failed: 1 of 3 tests".into(),
        Details {
            tone: Tone::Bad,
            lead: Some("Wait for <Inbox> & \"more\"".into()),
            facts: vec![fact("Took", "5.0 s"), fact("Project", "A & B")],
            failures: vec![
                FailedTest {
                    test: "Sign <in>".into(),
                    reason: "no & such".into(),
                },
                FailedTest {
                    test: "Sign up".into(),
                    reason: String::new(),
                },
            ],
            unlisted: 3,
            fixed: vec!["Profile".into()],
            report: Some(ReportLink {
                label: "Report".into(),
                target: "https://github.com/acme/shop/actions/runs/7".into(),
                is_link: true,
                system: "GitHub Actions".into(),
            }),
            picture: None,
        },
    )
}

fn png() -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.extend((0..64).map(|n| n as u8));
    bytes
}

#[test]
fn slack_gets_blocks_for_each_part_with_the_markup_escaped() {
    let body = render::slack(&full());
    let blocks = body["blocks"].as_array().unwrap();
    let types: Vec<&str> = blocks.iter().map(|b| b["type"].as_str().unwrap()).collect();
    assert_eq!(
        types,
        [
            "header", "section", "section", "section", "section", "context"
        ]
    );
    assert_eq!(
        blocks[0]["text"],
        json!({"type": "plain_text", "text": "Failed: 1 of 3 tests"})
    );
    assert_eq!(
        blocks[1]["text"]["text"],
        "Wait for &lt;Inbox&gt; &amp; \"more\""
    );
    assert_eq!(
        blocks[2]["fields"],
        json!([
            {"type": "mrkdwn", "text": "*Took*\n5.0 s"},
            {"type": "mrkdwn", "text": "*Project*\nA &amp; B"},
        ])
    );
    assert_eq!(
        blocks[3]["text"]["text"],
        "• *Sign &lt;in&gt;*: no &amp; such\n• *Sign up*\nand 3 more"
    );
    assert_eq!(blocks[4]["text"]["text"], "*Fixed:* Profile");
    assert_eq!(
        blocks[5]["elements"][0]["text"],
        "Report: <https://github.com/acme/shop/actions/runs/7|Open the run on GitHub>"
    );
    // What notifications and screen readers show.
    assert!(
        body["text"]
            .as_str()
            .unwrap()
            .starts_with("Failed: 1 of 3 tests\nWait for &lt;Inbox&gt;"),
        "{body}"
    );

    let mut on_the_mac = full();
    on_the_mac.details.report = Some(ReportLink {
        label: "Report on the Mac that ran it".into(),
        target: "/runs/a`b".into(),
        is_link: false,
        system: String::new(),
    });
    let body = render::slack(&on_the_mac);
    assert_eq!(
        body["blocks"][5]["elements"][0]["text"],
        "Report on the Mac that ran it: `/runs/a`b`"
    );
}

#[test]
fn slack_blocks_stay_inside_slacks_limits() {
    let mut message = full();
    message.title = "T".repeat(400);
    message.details.facts = (0..14)
        .map(|n| fact(&format!("L{n}"), &"v".repeat(3000)))
        .collect();
    message.details.failures = vec![FailedTest {
        test: "t".into(),
        reason: "r".repeat(5000),
    }];
    let body = render::slack(&message);
    let blocks = body["blocks"].as_array().unwrap();
    let length = |value: &serde_json::Value| value.as_str().unwrap().chars().count();
    assert_eq!(length(&blocks[0]["text"]["text"]), 151);
    let fields = blocks[2]["fields"].as_array().unwrap();
    assert_eq!(fields.len(), 10);
    assert!(fields.iter().all(|f| length(&f["text"]) <= 2001));
    assert!(length(&blocks[3]["text"]["text"]) <= 3001);
}

#[test]
fn slack_gets_the_plain_text_when_it_refuses_the_blocks() {
    let server = Http::serve(vec![
        (400, "invalid_blocks"),
        (200, "ok"),
        (400, "invalid_payload"),
    ]);
    let webhook = format!("{}/services/T/B/x", server.address);
    let settings = slack_only(NotifyWhen::Every);
    let secret = |_| Some(webhook.clone());
    let endpoints = Endpoints::default();
    let sent = || {
        send(
            &settings,
            NotifyChannel::Slack,
            &full(),
            &secret,
            &endpoints,
        )
    };
    sent().unwrap();
    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert!(json_of(&requests[0])["blocks"].is_array());
    let fallback = json_of(&requests[1]);
    assert_eq!(fallback.as_object().unwrap().len(), 1, "{fallback}");
    assert!(
        fallback["text"]
            .as_str()
            .unwrap()
            .starts_with("Failed: 1 of 3 tests\n")
    );

    // Any other refusal is the answer.
    let Err(NotifyError::Failed { reason }) = sent() else {
        panic!("a refused post is an error");
    };
    assert_eq!(reason, "Slack answered with error 400: invalid_payload");
    assert_eq!(server.requests().len(), 3);
}

#[test]
fn the_html_has_each_part_with_the_markup_escaped() {
    let html = render::html(&full());
    assert!(
        html.contains("<title>Failed: 1 of 3 tests</title>"),
        "{html}"
    );
    assert!(html.contains("background:#b3261e"), "{html}");
    assert!(
        html.contains("Wait for &lt;Inbox&gt; &amp; &quot;more&quot;"),
        "{html}"
    );
    assert!(
        html.contains("<strong>Sign &lt;in&gt;</strong>: no &amp; such"),
        "{html}"
    );
    assert!(html.contains("<strong>Sign up</strong></li>"), "{html}");
    assert!(html.contains("and 3 more"), "{html}");
    assert!(html.contains("<strong>Fixed:</strong> Profile"), "{html}");
    assert!(
        html.contains("<td style=\"padding:2px 0\">A &amp; B</td>"),
        "{html}"
    );
    assert!(
        html.contains("<a href=\"https://github.com/acme/shop/actions/runs/7\">"),
        "{html}"
    );
    assert!(
        !html.contains("<Inbox>") && !html.contains("<in>"),
        "{html}"
    );
    assert!(!html.contains("cid:"), "{html}");

    let mut good = full();
    good.details.tone = Tone::Good;
    assert!(render::html(&good).contains("background:#1e7a34"));
    let mut hostile = full();
    hostile.title = "<script>alert(1)</script>".into();
    hostile.details.report = Some(ReportLink {
        label: "Report".into(),
        target: "/runs/\"><script>".into(),
        is_link: false,
        system: String::new(),
    });
    let html = render::html(&hostile);
    assert!(!html.contains("<script>"), "{html}");
    assert!(
        html.contains("<code>/runs/&quot;&gt;&lt;script&gt;</code>"),
        "{html}"
    );
}

#[test]
fn the_html_shows_the_picture_inline_with_a_caption() {
    let mut message = full();
    message.details.picture = Some(Picture {
        png: png(),
        caption: "Sign in: the \"step\" where it failed".into(),
    });
    let html = render::html(&message);
    assert!(
        html.contains("<img src=\"cid:failed-step@jevision\" alt=\"Sign in: the &quot;step&quot; where it failed\""),
        "{html}"
    );
}

#[test]
fn mail_has_a_plain_and_an_html_version_and_the_picture_inline() {
    let server = Smtp::serve(MailSecurity::Tls, 2);
    let mut with_picture = full();
    with_picture.details.picture = Some(Picture {
        png: png(),
        caption: "Sign in: the step where it failed".into(),
    });
    mail_message_to(server.email(MailSecurity::Tls), "right", CA, &with_picture).unwrap();
    let talk = server.talk();
    assert!(talk.contains("Subject: Failed: 1 of 3 tests\r\n"), "{talk}");
    for part in [
        "Content-Type: multipart/alternative",
        "Content-Type: text/plain",
        "Content-Type: multipart/related",
        "Content-Type: text/html",
        "Content-Type: image/png",
        "Content-ID: <failed-step@jevision>",
        "Content-Disposition: inline",
    ] {
        assert!(talk.contains(part), "{part} missing from {talk}");
    }
    // The picture's first bytes in base64: the PNG signature.
    assert!(talk.contains("iVBORw0KGgo"), "{talk}");
    assert!(talk.contains("Took: 5.0 s =C2=B7 Project: A & B"), "{talk}");

    let before = talk.len();
    mail_message_to(server.email(MailSecurity::Tls), "right", CA, &full()).unwrap();
    let second = &server.talk()[before..];
    assert!(
        second.contains("Content-Type: multipart/alternative"),
        "{second}"
    );
    assert!(second.contains("Content-Type: text/html"), "{second}");
    assert!(
        !second.contains("image/png") && !second.contains("Content-ID"),
        "{second}"
    );
}

/// A run folder whose report has one failed step with these pictures.
fn failed_at(name: &str, before: Option<&str>, after: Option<&str>) -> (PathBuf, Finished) {
    let dir = folder(name);
    let mut value = report("failed", Some("run.secret.missing"));
    let mut step = json!({
        "seq": 1, "test": "login/sign-in.flow.json", "block": "b1", "type": "click",
        "status": "failed", "durationMs": 5,
        "failure": {"code": "run.secret.missing", "params": {"name": "PASSWORD"}},
    });
    for (key, picture) in [("before", before), ("after", after)] {
        if let Some(picture) = picture {
            step[key] = json!(picture);
        }
    }
    value["attempts"][0]["steps"] = json!([
        {"seq": 0, "test": "login/sign-in.flow.json", "block": "b0", "type": "click",
         "status": "passed", "durationMs": 5, "after": "step-0-after.png"},
        step,
    ]);
    let finished = Finished {
        run: Run {
            dir: dir.clone(),
            report: serde_json::from_value(value).unwrap(),
        },
        previous: None,
    };
    (dir, finished)
}

#[test]
fn the_picture_is_the_screen_after_the_failed_step() {
    let (dir, finished) = failed_at(
        "picture",
        Some("step-1-before.png"),
        Some("step-1-after.png"),
    );
    fs::write(dir.join("step-0-after.png"), b"\x89PNG\r\n\x1a\nearlier").unwrap();
    fs::write(dir.join("step-1-before.png"), b"\x89PNG\r\n\x1a\nbefore").unwrap();
    fs::write(dir.join("step-1-after.png"), png()).unwrap();
    let picture = failure_picture(std::slice::from_ref(&finished)).unwrap();
    assert_eq!(picture.png, png());
    assert_eq!(
        picture.caption,
        "Sign in: the step where it failed, with private areas covered"
    );

    // With no picture after the step, the one before it.
    let (dir, finished) = failed_at("picture-before", Some("step-1-before.png"), None);
    fs::write(dir.join("step-1-before.png"), b"\x89PNG\r\n\x1a\nbefore").unwrap();
    assert_eq!(
        failure_picture(&[finished]).unwrap().png,
        b"\x89PNG\r\n\x1a\nbefore"
    );
}

#[test]
fn the_first_failed_test_gives_the_picture_and_other_runs_give_none() {
    let (_, passed) = failed_at("picture-passed", None, Some("step-1-after.png"));
    let mut passed = passed;
    passed.run.report.verdict = Verdict::Passed;
    assert!(failure_picture(&[passed]).is_none());

    let (dir, mut quarantined) = failed_at("picture-quarantined", None, Some("step-1-after.png"));
    fs::write(dir.join("step-1-after.png"), png()).unwrap();
    quarantined.run.report.quarantined = true;
    assert!(failure_picture(std::slice::from_ref(&quarantined)).is_none());
    quarantined.run.report.quarantined = false;
    let (other, ok) = failed_at("picture-other", None, Some("step-1-after.png"));
    fs::write(other.join("step-1-after.png"), b"\x89PNG\r\n\x1a\nother").unwrap();
    assert_eq!(
        failure_picture(&[ok, quarantined]).unwrap().png,
        b"\x89PNG\r\n\x1a\nother"
    );
    assert!(failure_picture(&[]).is_none());
}

#[test]
fn only_a_small_png_inside_the_run_folder_is_attached() {
    let try_picture = |label: &str, name: &str, prepare: &dyn Fn(&std::path::Path)| {
        let (dir, finished) = failed_at(&format!("unsafe-{label}"), None, Some(name));
        prepare(&dir);
        failure_picture(&[finished])
    };
    let secret = folder("unsafe-outside");
    fs::write(secret.join("secret.png"), png()).unwrap();
    let outside = secret.join("secret.png");

    assert!(
        try_picture("good", "a.png", &|d| fs::write(d.join("a.png"), png())
            .unwrap())
        .is_some()
    );
    assert!(try_picture("missing", "a.png", &|_| {}).is_none());
    assert!(
        try_picture("text", "a.png", &|d| fs::write(
            d.join("a.png"),
            "not a picture"
        )
        .unwrap())
        .is_none()
    );
    assert!(
        try_picture("empty", "a.png", &|d| fs::write(d.join("a.png"), "")
            .unwrap())
        .is_none()
    );
    assert!(
        try_picture("big", "a.png", &|d| {
            let mut big = png();
            big.resize(5 * 1024 * 1024 + 1, 0);
            fs::write(d.join("a.png"), big).unwrap();
        })
        .is_none()
    );
    assert!(
        try_picture("up", "../unsafe-outside.png", &|d| {
            fs::write(d.parent().unwrap().join("unsafe-outside.png"), png()).unwrap();
        })
        .is_none()
    );
    let absolute = outside.to_string_lossy().into_owned();
    assert!(try_picture("absolute", &absolute, &|_| {}).is_none());
    assert!(
        try_picture("directory", "a.png", &|d| fs::create_dir(d.join("a.png"))
            .unwrap())
        .is_none()
    );
    #[cfg(unix)]
    assert!(
        try_picture("link", "a.png", &|d| std::os::unix::fs::symlink(
            &outside,
            d.join("a.png")
        )
        .unwrap())
        .is_none()
    );
}

#[test]
fn a_ci_link_names_its_system_and_the_layout_speaks_the_message_language() {
    let mut gitlab = full();
    gitlab.lang = "ru".into();
    gitlab.details.report = Some(ReportLink {
        label: "Отчёт".into(),
        target: "https://gitlab.com/acme/shop/-/pipelines/9".into(),
        is_link: true,
        system: "GitLab CI".into(),
    });
    let blocks = &render::slack(&gitlab)["blocks"];
    assert_eq!(
        blocks[5]["elements"][0]["text"],
        "Отчёт: <https://gitlab.com/acme/shop/-/pipelines/9|Открыть прогон в GitLab CI>"
    );
    assert_eq!(blocks[4]["text"]["text"], "*Исправлены:* Profile");
    assert!(
        blocks[3]["text"]["text"]
            .as_str()
            .unwrap()
            .ends_with("\nи ещё 3")
    );
    let html = render::html(&gitlab);
    assert!(html.contains("<html lang=\"ru\">"), "{html}");
    assert!(
        html.contains("<strong>Исправлены:</strong> Profile"),
        "{html}"
    );
}
