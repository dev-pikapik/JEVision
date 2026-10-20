use super::*;

/// The report of the test `name`, as it finished after 1 s.
fn of(name: &str, verdict: &str, failure: Option<&str>) -> serde_json::Value {
    let mut value = report(verdict, failure);
    value["test"] = json!(name);
    value["file"] = json!(format!("{name}.flow.json"));
    value["durationMs"] = json!(1_000);
    value
}

fn batch(reports: Vec<serde_json::Value>) -> Vec<Finished> {
    reports.into_iter().map(|r| finished(r, None)).collect()
}

fn blocked(name: &str) -> serde_json::Value {
    let mut value = of(name, "blocked", None);
    value["attempts"] = json!([]);
    value["blocked"] = json!({
        "dependency": "login.flow.json", "cause": "login.flow.json",
        "causeName": "Login", "reason": {"kind": "failed"}
    });
    value
}

fn lines(message: &Message) -> Vec<&str> {
    message.body.lines().collect()
}

#[test]
fn a_batch_is_told_in_one_summary() {
    let mut quarantined = of("quarantined", "failed", Some("run.secret.missing"));
    quarantined["quarantined"] = json!(true);
    let mut unchecked = of("unchecked", "passed", None);
    unchecked["nothingChecked"] = json!(true);
    let mut scheduled = of("ok", "passed", None);
    scheduled["schedule"] = json!("Nightly");
    let runs = batch(vec![
        scheduled,
        unchecked,
        of("sign-in", "failed", Some("run.secret.missing")),
        quarantined,
        blocked("profile"),
    ]);

    let told = message("shop", &runs);
    assert_eq!(told.title, "Failed: 1 of 5 tests");
    assert_eq!(told.details.tone, Tone::Bad);
    assert_eq!(
        told.details.facts,
        [
            ("Passed", "1"),
            ("Failed", "1"),
            ("Skipped", "2"),
            ("Blocked", "1"),
            ("Took", "5.0 s"),
            ("Project", "shop"),
            ("Schedule", "Nightly"),
        ]
        .map(|(l, v)| (l.to_owned(), v.to_owned()))
    );
    assert_eq!(
        told.details.failures,
        [FailedTest {
            test: "sign-in".into(),
            reason: "The secret PASSWORD isn’t set. Set it in Test details.".into()
        }]
    );
    let lines = lines(&told);
    assert_eq!(
        lines[0],
        "• sign-in: The secret PASSWORD isn’t set. Set it in Test details."
    );
    assert_eq!(
        lines[1],
        "Passed: 1 · Failed: 1 · Skipped: 2 · Blocked: 1 · Took: 5.0 s · Project: shop · Schedule: Nightly"
    );
    let report = told.details.report.as_ref().unwrap();
    if ci_run_url().is_none() {
        assert_eq!(report.label, "Reports on the Mac that ran them");
        assert_eq!(report.target, "/runs");
    }
}

#[test]
fn the_summary_title_tells_the_most_important_thing() {
    let title = |runs: Vec<Finished>| message("shop", &runs).title;
    let passed = || of("a", "passed", None);
    assert_eq!(
        title(batch(vec![passed(), passed(), passed()])),
        "Passed: 3 tests"
    );
    assert_eq!(
        title(batch(vec![passed(), blocked("b"), blocked("c")])),
        "Blocked: 2 of 3 tests"
    );
    assert_eq!(
        title(batch(vec![of("a", "failed", Some("x")), blocked("b")])),
        "Failed: 1 of 2 tests"
    );

    let after_failing = |value| finished(value, Some(of("a", "failed", Some("x"))));
    let fixed = vec![
        after_failing(passed()),
        finished(passed(), None),
        finished(passed(), None),
    ];
    let told = message("shop", &fixed);
    assert_eq!(told.title, "Fixed: 1 test passing again");
    assert_eq!(told.details.tone, Tone::Good);
    assert_eq!(told.details.fixed, ["a"]);
    assert!(told.body.contains("Fixed: a"), "{}", told.body);

    // A test the report doesn't name goes by its file.
    let mut nameless = of("", "failed", Some("x"));
    nameless["file"] = json!("login/a.flow.json");
    let told = message("shop", &batch(vec![nameless, passed()]));
    assert_eq!(told.details.failures[0].test, "login/a.flow.json");
}

#[test]
fn the_summary_title_is_told_in_the_language_asked_for() {
    let passed = || of("a", "passed", None);
    let title = |lang: &str, runs: &[Finished]| message_in(lang, "shop", runs).title;
    let all_passed = |n: usize| batch((0..n).map(|_| passed()).collect());
    let failed = |n: usize| {
        let mut runs = vec![of("a", "failed", Some("x"))];
        runs.extend((1..n).map(|_| passed()));
        batch(runs)
    };
    let fixed = |n: usize| {
        let mut runs: Vec<Finished> = (0..n)
            .map(|_| finished(passed(), Some(of("a", "failed", Some("x")))))
            .collect();
        runs.push(finished(passed(), None));
        runs
    };
    assert_eq!(title("en", &all_passed(2)), "Passed: 2 tests");
    assert_eq!(title("ru", &all_passed(2)), "Пройдено: 2 теста");
    assert_eq!(title("ru", &all_passed(5)), "Пройдено: 5 тестов");
    assert_eq!(title("ru", &all_passed(21)), "Пройдено: 21 тест");
    assert_eq!(title("en", &failed(5)), "Failed: 1 of 5 tests");
    assert_eq!(title("ru", &failed(2)), "Провалено: 1 из 2 тестов");
    assert_eq!(title("ru", &failed(21)), "Провалено: 1 из 21 теста");
    for (n, en, ru) in [
        (
            1,
            "Fixed: 1 test passing again",
            "Исправлено: 1 тест снова проходит",
        ),
        (
            2,
            "Fixed: 2 tests passing again",
            "Исправлено: 2 теста снова проходят",
        ),
        (
            5,
            "Fixed: 5 tests passing again",
            "Исправлено: 5 тестов снова проходят",
        ),
    ] {
        assert_eq!(title("en", &fixed(n)), en);
        assert_eq!(title("ru", &fixed(n)), ru);
    }
    let told = message_in("ru", "shop", &fixed(1));
    assert!(told.body.contains("Исправлены: a"), "{}", told.body);
    assert!(told.body.contains("Проект: shop"), "{}", told.body);
}

#[test]
fn a_summary_names_ten_failures_and_counts_the_rest() {
    let mut reports: Vec<_> = (1..=12)
        .map(|n| of(&format!("test {n}"), "failed", Some("run.secret.missing")))
        .collect();
    reports[0]["attempts"][0]["failure"]["params"]["name"] = json!("X".repeat(500));
    let told = message("shop", &batch(reports));
    assert_eq!(told.title, "Failed: 12 of 12 tests");
    assert_eq!(told.details.failures.len(), 10);
    assert_eq!(told.details.unlisted, 2);
    assert_eq!(told.details.failures[9].test, "test 10");
    assert!(told.body.contains("\nand 2 more\n"), "{}", told.body);

    let long = &told.details.failures[0].reason;
    assert_eq!(long.chars().count(), 241, "{long}");
    assert!(long.ends_with('…'), "{long}");
}

#[test]
fn a_stopped_batch_sends_nothing() {
    let settings = slack_only(NotifyWhen::Every);
    let runs = batch(vec![
        of("a", "failed", Some("x")),
        of("b", "cancelled", None),
    ]);
    assert_eq!(notice_for(&settings, "shop", &runs, true), None);
    assert_eq!(notice_for(&settings, "shop", &[], true), None);
}

#[test]
fn a_batch_gets_the_channels_that_ask_for_it_in_one_notice() {
    let settings = NotificationSettings {
        slack: Some(SlackChannel {
            when: NotifyWhen::Failed,
        }),
        telegram: Some(TelegramChat {
            chat: "-1".into(),
            when: NotifyWhen::Changed,
        }),
        ..NotificationSettings::default()
    };
    let runs = vec![
        finished(of("a", "passed", None), Some(of("a", "failed", Some("x")))),
        finished(
            of("b", "failed", Some("x")),
            Some(of("b", "failed", Some("x"))),
        ),
    ];
    let notice = notice_for(&settings, "shop", &runs, false).unwrap();
    assert_eq!(notice.message.title, "Failed: 1 of 2 tests");
    assert_eq!(
        notice.channels,
        [NotifyChannel::Slack, NotifyChannel::Telegram]
    );

    let steady = vec![
        finished(of("a", "passed", None), Some(of("a", "passed", None))),
        finished(
            of("b", "failed", Some("x")),
            Some(of("b", "failed", Some("x"))),
        ),
    ];
    assert_eq!(
        notice_for(&settings, "shop", &steady, false)
            .unwrap()
            .channels,
        [NotifyChannel::Slack]
    );
}

#[test]
fn a_batch_reaches_each_channel_once() {
    let slack = Http::serve(vec![(200, "ok")]);
    let telegram = Http::serve(vec![(200, r#"{"ok": true, "result": {}}"#)]);
    let settings = NotificationSettings {
        slack: Some(SlackChannel {
            when: NotifyWhen::Every,
        }),
        telegram: Some(TelegramChat {
            chat: "-1".into(),
            when: NotifyWhen::Every,
        }),
        ..NotificationSettings::default()
    };
    let runs = batch(vec![
        of("a", "passed", None),
        of("b", "failed", Some("run.secret.missing")),
        of("c", "failed", Some("run.secret.missing")),
    ]);
    let notice = notice_for(&settings, "shop", &runs, false).unwrap();
    let webhook = format!("{}/services/T/B/x", slack.address);
    let results = send_all(
        &settings,
        &notice,
        &|channel| match channel {
            NotifyChannel::Slack => Some(webhook.clone()),
            _ => Some("1:tok".to_owned()),
        },
        &Endpoints {
            telegram_api: telegram.address.clone(),
            mail_root: None,
        },
    );
    assert!(results.iter().all(|s| s.result.is_ok()), "{results:?}");
    assert_eq!(slack.requests().len(), 1);
    assert_eq!(telegram.requests().len(), 1);
    let said = json_of(&telegram.requests()[0]);
    assert!(
        said["text"]
            .as_str()
            .unwrap()
            .starts_with("Failed: 2 of 3 tests\n"),
        "{said}"
    );
}

#[test]
fn notify_sends_nothing_without_channels_and_refuses_a_missing_report() {
    let project = folder("notify");
    let run = project
        .join(".jevision")
        .join("runs")
        .join("20261019-093000-a");
    fs::create_dir_all(&run).unwrap();
    fs::write(
        run.join("report.json"),
        of("a", "failed", Some("x")).to_string(),
    )
    .unwrap();
    assert_eq!(notify(&project, std::slice::from_ref(&run)), Ok(Vec::new()));
    assert_eq!(notify(&project, &[]), Ok(Vec::new()));

    slack_only(NotifyWhen::Every).save(&project).unwrap();
    assert!(matches!(
        notify(&project, &[project.join("missing")]),
        Err(NotifyError::Failed { .. })
    ));
}
