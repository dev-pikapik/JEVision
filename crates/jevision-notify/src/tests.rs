use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde_json::json;

use super::*;
use crate::message::duration;

mod chats;
mod rich;
mod summary;

/// The page of the CI run the reports link to, when the tests themselves run on CI.
fn ci_run_url() -> Option<String> {
    jevision_runner::CiOrigin::from_env().and_then(|ci| ci.run_url)
}

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("jevision-notify-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A report as `report.json` holds it; `failure` is the code of the last attempt's failure.
fn report(verdict: &str, failure: Option<&str>) -> serde_json::Value {
    let mut attempt = json!({"number": 1, "verdict": verdict, "durationMs": 12_300, "steps": []});
    if let Some(code) = failure {
        attempt["failure"] = json!({"code": code, "params": {"name": "PASSWORD"}});
    }
    json!({
        "format": 1, "test": "Sign in", "file": "login/sign-in.flow.json", "app": "com.acme.mail",
        "startedAt": "2026-10-19T09:30:00Z", "durationMs": 12_300, "verdict": verdict,
        "flaky": false, "attempts": [attempt]
    })
}

fn run(value: serde_json::Value) -> Run {
    Run {
        dir: PathBuf::from("/runs/20261019-093000-sign-in"),
        report: serde_json::from_value(value).unwrap(),
    }
}

fn slack_only(when: NotifyWhen) -> NotificationSettings {
    NotificationSettings {
        slack: Some(SlackChannel { when }),
        ..NotificationSettings::default()
    }
}

/// A finished run of the test `report` describes, after the run `previous` of it.
fn finished(value: serde_json::Value, previous: Option<serde_json::Value>) -> Finished {
    Finished {
        run: run(value),
        previous: previous.map(|v| serde_json::from_value(v).unwrap()),
    }
}

#[test]
fn settings_are_saved_read_back_and_checked() {
    let project = folder("settings");
    assert_eq!(
        NotificationSettings::load(&project).unwrap(),
        NotificationSettings::default()
    );
    let settings = NotificationSettings {
        unattended_only: true,
        slack: Some(SlackChannel {
            when: NotifyWhen::Every,
        }),
        telegram: Some(TelegramChat {
            chat: "-100123".into(),
            when: NotifyWhen::Changed,
        }),
        email: Some(MailServer {
            server: "smtp.acme.com".into(),
            port: 465,
            security: MailSecurity::StartTls,
            user: "qa@acme.com".into(),
            from: "qa@acme.com".into(),
            to: vec!["team@acme.com".into(), "Ann <ann@acme.com>".into()],
            when: NotifyWhen::Failed,
        }),
    };
    settings.save(&project).unwrap();
    let text = fs::read_to_string(project.join(FILE)).unwrap();
    assert!(text.contains("\"unattendedOnly\": true"), "{text}");
    assert!(text.contains("\"security\": \"startTls\""), "{text}");
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(saved["format"], 2);
    assert_eq!(saved["slack"], json!({"when": "every"}));
    assert_eq!(saved["telegram"]["when"], "changed");
    assert!(saved.get("when").is_none(), "{text}");
    assert!(
        !text.contains("token") && !text.contains("password"),
        "{text}"
    );
    assert_eq!(NotificationSettings::load(&project).unwrap(), settings);

    let mut wrong = settings.clone();
    wrong.email.as_mut().unwrap().to = vec!["team".into()];
    assert_eq!(
        wrong.save(&project),
        Err(NotifyError::Rejected {
            reason: "\"team\" isn't an email address to send to".into()
        })
    );
    assert_eq!(NotificationSettings::load(&project).unwrap(), settings);
    assert!(NotificationSettings::parse(r#"{"format": 1, "telegram": {"chat": " "}}"#).is_err());
    assert!(
        NotificationSettings::parse(r#"{"format": 1, "slack": true, "webhook": "x"}"#).is_err()
    );
    assert!(NotificationSettings::parse(r#"{"format": 2, "slack": true}"#).is_err());
    assert!(NotificationSettings::parse(r#"{"format": 2, "when": "every"}"#).is_err());
    assert_eq!(
        NotificationSettings::parse(r#"{"format": 3}"#),
        Err("\"format\" must be 1 or 2".into())
    );

    NotificationSettings::default().save(&project).unwrap();
    assert!(!project.join(FILE).exists());
}

#[test]
fn a_format_1_file_is_read_with_its_when_for_every_channel_and_saved_as_format_2() {
    let old = r#"{
      "format": 1, "when": "every", "unattendedOnly": true, "slack": true,
      "telegram": {"chat": "-100123"},
      "email": {"server": "smtp.acme.com", "port": 587, "security": "startTls",
                "from": "qa@acme.com", "to": ["team@acme.com"]}
    }"#;
    let settings = NotificationSettings::parse(old).unwrap();
    assert!(settings.unattended_only);
    for channel in NotifyChannel::ALL {
        assert_eq!(
            settings.when(channel),
            Some(NotifyWhen::Every),
            "{channel:?}"
        );
    }

    let project = folder("migrate");
    fs::write(project.join(FILE), old).unwrap();
    let loaded = NotificationSettings::load(&project).unwrap();
    assert_eq!(loaded, settings);
    loaded.save(&project).unwrap();
    let saved: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(project.join(FILE)).unwrap()).unwrap();
    assert_eq!(saved["format"], 2);
    assert!(saved.get("when").is_none());
    assert_eq!(saved["slack"], json!({"when": "every"}));
    assert_eq!(saved["email"]["when"], "every");
    assert_eq!(NotificationSettings::load(&project).unwrap(), settings);

    // No `when` meant the default; `slack: false` and no channel meant off.
    let plain = NotificationSettings::parse(r#"{"format": 1, "slack": true}"#).unwrap();
    assert_eq!(plain, slack_only(NotifyWhen::FailedOrFixed));
    let off =
        NotificationSettings::parse(r#"{"format": 1, "slack": false, "when": "failed"}"#).unwrap();
    assert!(off.channels().is_empty());
    let own = NotificationSettings::parse(
        r#"{"format": 1, "when": "failed", "telegram": {"chat": "1", "when": "every"}}"#,
    )
    .unwrap();
    assert_eq!(own.when(NotifyChannel::Telegram), Some(NotifyWhen::Every));
    assert!(
        NotificationSettings::parse(r#"{"format": 1, "when": "sometimes", "slack": true}"#)
            .unwrap_err()
            .starts_with("\"when\":")
    );
    assert!(
        NotificationSettings::parse(r#"{"format": 2, "slack": {"when": "sometimes"}}"#).is_err()
    );
}

#[test]
fn a_message_goes_to_the_channels_that_ask_for_it() {
    let failed = || report("failed", Some("run.secret.missing"));
    let passed = || report("passed", None);
    let title = |settings: &NotificationSettings, now: &Finished| {
        notice_for(settings, "shop", std::slice::from_ref(now), false).map(|n| n.message.title)
    };

    let fixed = slack_only(NotifyWhen::FailedOrFixed);
    assert_eq!(
        title(&fixed, &finished(failed(), None)).as_deref(),
        Some("Failed: Sign in")
    );
    assert_eq!(
        title(&fixed, &finished(failed(), Some(failed()))).as_deref(),
        Some("Failed: Sign in")
    );
    assert_eq!(
        title(&fixed, &finished(passed(), Some(failed()))).as_deref(),
        Some("Fixed: Sign in")
    );
    assert_eq!(title(&fixed, &finished(passed(), Some(passed()))), None);
    assert_eq!(title(&fixed, &finished(passed(), None)), None);
    assert_eq!(
        title(&fixed, &finished(report("cancelled", None), None)),
        None
    );

    let only_failed = slack_only(NotifyWhen::Failed);
    assert_eq!(
        title(&only_failed, &finished(passed(), Some(failed()))),
        None
    );
    assert!(title(&only_failed, &finished(failed(), Some(failed()))).is_some());

    let changed = slack_only(NotifyWhen::Changed);
    assert!(title(&changed, &finished(failed(), None)).is_some());
    assert!(title(&changed, &finished(failed(), Some(passed()))).is_some());
    assert_eq!(title(&changed, &finished(failed(), Some(failed()))), None);
    assert!(title(&changed, &finished(passed(), Some(failed()))).is_some());
    assert_eq!(title(&changed, &finished(passed(), Some(passed()))), None);

    let every = slack_only(NotifyWhen::Every);
    assert_eq!(
        title(&every, &finished(passed(), Some(passed()))).as_deref(),
        Some("Passed: Sign in")
    );
    assert_eq!(
        title(&every, &finished(report("cancelled", None), None)),
        None
    );
    // A verdict only a newer version knows is never this version's own run.
    assert_eq!(
        title(&every, &finished(report("retried", None), None)),
        None
    );

    let blocked = |reason: serde_json::Value| {
        let mut value = report("blocked", None);
        value["attempts"] = json!([]);
        value["blocked"] = json!({
            "dependency": "login/open.flow.json", "cause": "login/open.flow.json",
            "causeName": "Open", "reason": reason
        });
        finished(value, None)
    };
    let after_failure = blocked(json!({"kind": "failed"}));
    let missing = blocked(json!({"kind": "missing"}));
    assert_eq!(title(&fixed, &after_failure), None);
    assert_eq!(title(&only_failed, &after_failure), None);
    assert_eq!(title(&changed, &after_failure), None);
    assert_eq!(
        title(&every, &after_failure).as_deref(),
        Some("Blocked: Sign in")
    );
    for settings in [&only_failed, &fixed, &changed] {
        assert_eq!(
            title(settings, &missing).as_deref(),
            Some("Blocked: Sign in")
        );
    }
    let told = notice_for(&only_failed, "shop", std::slice::from_ref(&missing), false).unwrap();
    assert!(
        told.message
            .body
            .starts_with("Blocked: login/open.flow.json doesn’t exist\n")
    );

    let mut quarantined = finished(failed(), None);
    quarantined.run.report.quarantined = true;
    assert_eq!(title(&fixed, &quarantined), None);
    assert_eq!(title(&changed, &quarantined), None);
    assert!(title(&every, &quarantined).is_some());

    let unattended = NotificationSettings {
        unattended_only: true,
        ..fixed.clone()
    };
    let now = finished(failed(), None);
    assert_eq!(
        notice_for(&unattended, "shop", std::slice::from_ref(&now), false),
        None
    );
    assert!(notice_for(&unattended, "shop", std::slice::from_ref(&now), true).is_some());
    assert_eq!(
        notice_for(
            &NotificationSettings::default(),
            "shop",
            std::slice::from_ref(&now),
            true
        ),
        None
    );
}

#[test]
fn each_channel_has_its_own_when() {
    let settings = NotificationSettings {
        slack: Some(SlackChannel {
            when: NotifyWhen::Failed,
        }),
        telegram: Some(TelegramChat {
            chat: "-1".into(),
            when: NotifyWhen::Every,
        }),
        email: Some(MailServer {
            when: NotifyWhen::Changed,
            ..MailServer::default()
        }),
        ..NotificationSettings::default()
    };
    let channels = |now: Finished| notice_for(&settings, "shop", &[now], false).map(|n| n.channels);
    let failed = || report("failed", Some("run.secret.missing"));
    let passed = || report("passed", None);
    assert_eq!(
        channels(finished(failed(), None)),
        Some(NotifyChannel::ALL.to_vec())
    );
    assert_eq!(
        channels(finished(failed(), Some(failed()))),
        Some(vec![NotifyChannel::Slack, NotifyChannel::Telegram])
    );
    assert_eq!(
        channels(finished(passed(), Some(failed()))),
        Some(vec![NotifyChannel::Telegram, NotifyChannel::Email])
    );
    assert_eq!(
        channels(finished(passed(), Some(passed()))),
        Some(vec![NotifyChannel::Telegram])
    );
}

#[test]
fn a_message_says_why_how_long_and_where_the_report_is() {
    let mut value = report("failed", Some("run.secret.missing"));
    value["schedule"] = json!("Nightly");
    let told = message("shop", &[finished(value, None)]);
    assert_eq!(told.title, "Failed: Sign in");
    let lines: Vec<&str> = told.body.lines().collect();
    assert_eq!(
        lines[0],
        "The secret PASSWORD isn’t set. Set it in Test details."
    );
    assert_eq!(
        lines[1],
        "Took: 12.3 s · File: login/sign-in.flow.json · Schedule: Nightly"
    );
    match ci_run_url() {
        Some(url) => assert_eq!(lines[2], format!("Report: {url}")),
        None => assert_eq!(
            lines[2],
            "Report on the Mac that ran it: /runs/20261019-093000-sign-in/report.html"
        ),
    }

    let one = |value| message("shop", &[finished(value, None)]);
    let mut rows = report("failed", None);
    rows["attempts"] = json!([
        {"number": 1, "verdict": "passed", "durationMs": 1, "steps": [],
         "row": {"number": 1, "label": "ann"}},
        {"number": 1, "verdict": "failed", "durationMs": 1, "steps": [],
         "row": {"number": 2, "label": "bob"},
         "failure": {"code": "run.secret.missing", "params": {"name": "PASSWORD"}}},
    ]);
    let body = one(rows).body;
    assert!(
        body.starts_with("Row 2: bob: The secret PASSWORD isn’t set. Set it in Test details.\n"),
        "{body}"
    );

    let mut flaky = report("passed", None);
    flaky["flaky"] = json!(true);
    flaky["attempts"] = json!([
        {"number": 1, "verdict": "failed", "durationMs": 1, "steps": []},
        {"number": 2, "verdict": "passed", "durationMs": 1, "steps": []},
    ]);
    let body = one(flaky).body;
    assert!(
        body.starts_with("Passed on retry 1 after failing — the test is flaky.\n"),
        "{body}"
    );
    assert_eq!(duration("en", 850), "850 ms");
    assert_eq!(duration("en", 12_300), "12.3 s");
    assert_eq!(duration("en", 125_000), "2 min 5 s");
    assert_eq!(duration("ru", 850), "850 мс");
    assert_eq!(duration("ru", 12_300), "12,3 с");
    assert_eq!(duration("ru", 125_000), "2 мин 5 с");
}

#[test]
fn a_message_and_its_errors_are_told_in_the_language_asked_for() {
    let told = message_in("ru", "shop", &[finished(report("failed", None), None)]);
    assert_eq!(told.title, "Провален: Sign in");
    assert_eq!(told.lang, "ru");
    assert!(told.body.contains("Длительность: 12,3 с"), "{}", told.body);
    assert_eq!(
        test_message_in("en", Path::new("/p/shop")).title,
        "JEVision test message"
    );
    assert_eq!(
        test_message_in("ru", Path::new("/p/shop")).title,
        "Проверочное сообщение JEVision"
    );

    let missing = NotifyError::NoSecret {
        channel: NotifyChannel::Telegram,
    };
    assert_eq!(
        missing.text_in("en"),
        "the Telegram bot token isn't on this Mac; add it in JEVision Settings → Notifications \
         or set JEVISION_TELEGRAM_TOKEN"
    );
    assert_eq!(
        missing.text_in("ru"),
        "токена бота Telegram нет на этом Mac; добавьте его в Настройках JEVision → Уведомления \
         или задайте JEVISION_TELEGRAM_TOKEN"
    );
    let off = NotifyError::NotSetUp {
        channel: NotifyChannel::Slack,
    };
    assert_eq!(off.text_in("en"), "Slack isn't set up in this project");
    assert_eq!(off.text_in("ru"), "Slack не настроен в этом проекте");
}

#[test]
fn fixed_is_told_against_the_last_finished_run_of_the_same_file() {
    let project = folder("previous");
    let runs = project.join(".jevision").join("runs");
    let save = |name: &str, value: serde_json::Value| {
        let dir = runs.join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("report.json"), value.to_string()).unwrap();
        dir
    };
    save("20261019-080000-sign-in", report("failed", Some("x")));
    save("20261019-083000-sign-in", report("cancelled", None));
    save("20261019-083500-sign-in", report("blocked", None));
    save("20261019-083700-sign-in", report("retried", None));
    let mut other = report("passed", None);
    other["file"] = json!("other/sign-in.flow.json");
    save("20261019-083000-sign-in-2", other);
    save("20261019-084000-sign-up", report("passed", None));
    save("20261019-100000-sign-in", report("passed", None));
    let now = save("20261019-090000-sign-in", report("passed", None));
    let now = Run::load(&now).unwrap();
    assert_eq!(
        previous(&project, &now).map(|r| r.verdict),
        Some(Verdict::Failed)
    );
    assert_eq!(test_part("20261019-090000-sign-in-12"), "sign-in");
    assert_eq!(test_part("20261019-090000-v2"), "v2");
}

/// An HTTP server on 127.0.0.1 answering each request with the next reply, keeping what came.
struct Http {
    address: String,
    requests: Arc<Mutex<Vec<String>>>,
}

impl Http {
    fn serve(replies: Vec<(u16, &str)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = requests.clone();
        let replies: Vec<(u16, String)> = replies.into_iter().map(|(s, b)| (s, b.into())).collect();
        std::thread::spawn(move || {
            for (status, body) in replies {
                let Ok((stream, _)) = listener.accept() else {
                    return;
                };
                let mut reader = BufReader::new(stream);
                let mut request = String::new();
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = v.trim().parse().unwrap();
                    }
                    request.push_str(&line);
                    if line == "\r\n" || line.is_empty() {
                        break;
                    }
                }
                let mut content = vec![0; length];
                reader.read_exact(&mut content).unwrap();
                request.push_str(&String::from_utf8_lossy(&content));
                seen.lock().unwrap().push(request);
                let _ = write!(
                    reader.into_inner(),
                    "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        Self { address, requests }
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

fn note() -> Message {
    Message::new(
        "Failed: Sign in & <out>".into(),
        Details {
            tone: Tone::Bad,
            lead: Some("Secret PASSWORD is not set.".into()),
            facts: vec![("Took".into(), "1 s".into())],
            ..Details::default()
        },
    )
}

const NOTE_TEXT: &str = "Failed: Sign in & <out>\nSecret PASSWORD is not set.\nTook: 1 s";

/// The JSON a request carried.
fn json_of(request: &str) -> serde_json::Value {
    serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap()
}

fn no_secret(_: NotifyChannel) -> Option<String> {
    None
}

#[test]
fn slack_gets_the_blocks_through_the_webhook() {
    let server = Http::serve(vec![(200, "ok"), (404, "no_service")]);
    let webhook = format!("{}/services/T1/B2/abc", server.address);
    let settings = slack_only(NotifyWhen::Every);
    let secret = |_| Some(webhook.clone());
    let endpoints = Endpoints::default();
    send(
        &settings,
        NotifyChannel::Slack,
        &note(),
        &secret,
        &endpoints,
    )
    .unwrap();
    let Err(NotifyError::Failed { reason }) = send(
        &settings,
        NotifyChannel::Slack,
        &note(),
        &secret,
        &endpoints,
    ) else {
        panic!("a gone webhook is an error");
    };
    assert!(reason.contains("no_service"), "{reason}");
    let requests = server.requests();
    assert!(requests[0].starts_with("POST /services/T1/B2/abc "));
    let body = json_of(&requests[0]);
    assert_eq!(
        body["text"],
        "Failed: Sign in &amp; &lt;out&gt;\nSecret PASSWORD is not set.\nTook: 1 s"
    );
    assert_eq!(body["blocks"][0]["type"], "header");
    assert_eq!(body["blocks"][0]["text"]["text"], "Failed: Sign in & <out>");

    assert_eq!(
        send(
            &settings,
            NotifyChannel::Slack,
            &note(),
            &no_secret,
            &endpoints
        ),
        Err(NotifyError::NoSecret {
            channel: NotifyChannel::Slack
        })
    );
    assert_eq!(
        send(
            &settings,
            NotifyChannel::Telegram,
            &note(),
            &secret,
            &endpoints
        ),
        Err(NotifyError::NotSetUp {
            channel: NotifyChannel::Telegram
        })
    );
    let plain = |_| Some("http://hooks.slack.com/services/x".to_owned());
    assert!(send(&settings, NotifyChannel::Slack, &note(), &plain, &endpoints).is_err());
}

#[test]
fn telegram_gets_the_text_from_the_bot() {
    let server = Http::serve(vec![
        (200, r#"{"ok": true, "result": {}}"#),
        (
            400,
            r#"{"ok": false, "description": "Bad Request: chat not found"}"#,
        ),
        (401, r#"{"ok": false, "description": "Unauthorized"}"#),
    ]);
    let settings = NotificationSettings {
        telegram: Some(TelegramChat {
            chat: "-100123".into(),
            when: NotifyWhen::Every,
        }),
        ..NotificationSettings::default()
    };
    let endpoints = Endpoints {
        telegram_api: server.address.clone(),
        mail_root: None,
    };
    let secret = |_| Some("123:bot-token".to_owned());
    let notice = Notice {
        message: note(),
        channels: vec![NotifyChannel::Telegram],
    };
    let results = send_all(&settings, &notice, &secret, &endpoints);
    assert_eq!(
        results,
        vec![Sent {
            channel: NotifyChannel::Telegram,
            result: Ok(())
        }]
    );
    assert_eq!(
        send(
            &settings,
            NotifyChannel::Telegram,
            &note(),
            &secret,
            &endpoints
        ),
        Err(failed("Telegram: Bad Request: chat not found"))
    );
    assert_eq!(
        send(
            &settings,
            NotifyChannel::Telegram,
            &note(),
            &secret,
            &endpoints
        ),
        Err(failed("Telegram didn't accept the bot token"))
    );
    let request = &server.requests()[0];
    assert!(request.starts_with("POST /bot123:bot-token/sendMessage "));
    assert_eq!(
        json_of(request),
        json!({"chat_id": "-100123", "text": NOTE_TEXT, "disable_web_page_preview": true})
    );
}

/// The test authority that signed the mail server's certificate for 127.0.0.1.
const CA: &[u8] = include_bytes!("testdata/ca.der");

/// A mail server on 127.0.0.1 speaking TLS, from the start or after STARTTLS, that takes the
/// password `right` and keeps what it heard.
struct Smtp {
    port: u16,
    talk: Arc<Mutex<String>>,
}

impl Smtp {
    fn serve(security: MailSecurity, sessions: usize) -> Self {
        use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
        let config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from(
                include_bytes!("testdata/server.der").to_vec(),
            )],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
                include_bytes!("testdata/server.key.der").to_vec(),
            )),
        )
        .unwrap();
        let config = Arc::new(config);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let talk = Arc::new(Mutex::new(String::new()));
        let log = talk.clone();
        std::thread::spawn(move || {
            for _ in 0..sessions {
                let Ok((mut tcp, _)) = listener.accept() else {
                    return;
                };
                let tls = |tcp: TcpStream| {
                    let server = rustls::ServerConnection::new(config.clone()).unwrap();
                    BufReader::new(rustls::StreamOwned::new(server, tcp))
                };
                if security == MailSecurity::Tls {
                    converse(tls(tcp), true, &log);
                    continue;
                }
                write!(tcp, "220 mock ESMTP\r\n").unwrap();
                let mut plain = BufReader::new(tcp.try_clone().unwrap());
                let mut line = String::new();
                while plain.read_line(&mut line).unwrap() > 0 {
                    log.lock().unwrap().push_str(&line);
                    let command = line.to_ascii_uppercase();
                    line.clear();
                    if command.starts_with("EHLO") {
                        write!(tcp, "250-mock\r\n250 STARTTLS\r\n").unwrap();
                    } else if command.starts_with("STARTTLS") {
                        write!(tcp, "220 go ahead\r\n").unwrap();
                        break;
                    } else {
                        write!(tcp, "530 STARTTLS first\r\n").unwrap();
                    }
                }
                converse(tls(tcp), false, &log);
            }
        });
        Self { port, talk }
    }

    fn email(&self, security: MailSecurity) -> MailServer {
        MailServer {
            server: "127.0.0.1".into(),
            port: self.port,
            security,
            user: "qa@acme.com".into(),
            from: "JEVision <qa@acme.com>".into(),
            to: vec!["team@acme.com".into()],
            when: NotifyWhen::Every,
        }
    }

    fn talk(&self) -> String {
        self.talk.lock().unwrap().clone()
    }
}

fn converse<S: Read + Write>(mut stream: BufReader<S>, greet: bool, log: &Mutex<String>) {
    let say = |stream: &mut BufReader<S>, text: &str| {
        let _ = stream.get_mut().write_all(text.as_bytes());
        let _ = stream.get_mut().flush();
    };
    if greet {
        say(&mut stream, "220 mock ESMTP\r\n");
    }
    let mut line = String::new();
    while stream.read_line(&mut line).unwrap_or(0) > 0 {
        log.lock().unwrap().push_str(&line);
        let command = line.trim_end().to_owned();
        line.clear();
        let reply = if command.starts_with("EHLO") {
            "250-mock\r\n250 AUTH PLAIN LOGIN\r\n"
        } else if command.starts_with("AUTH PLAIN") {
            // "\0qa@acme.com\0right" in base64.
            if command == "AUTH PLAIN AHFhQGFjbWUuY29tAHJpZ2h0" {
                "235 ok\r\n"
            } else {
                "535 5.7.8 bad credentials\r\n"
            }
        } else if command.starts_with("DATA") {
            say(&mut stream, "354 go\r\n");
            while stream.read_line(&mut line).unwrap_or(0) > 0 && line != ".\r\n" {
                log.lock().unwrap().push_str(&line);
                line.clear();
            }
            line.clear();
            "250 queued\r\n"
        } else if command.starts_with("QUIT") {
            say(&mut stream, "221 bye\r\n");
            return;
        } else {
            "250 ok\r\n"
        };
        say(&mut stream, reply);
    }
}

fn mail_to(email: MailServer, password: &str, root: &[u8]) -> Result<(), NotifyError> {
    mail_message_to(email, password, root, &note())
}

fn mail_message_to(
    email: MailServer,
    password: &str,
    root: &[u8],
    message: &Message,
) -> Result<(), NotifyError> {
    let settings = NotificationSettings {
        email: Some(email),
        ..NotificationSettings::default()
    };
    let endpoints = Endpoints {
        telegram_api: String::new(),
        mail_root: Some(root.to_vec()),
    };
    let password = password.to_owned();
    send(
        &settings,
        NotifyChannel::Email,
        message,
        &|_| Some(password.clone()),
        &endpoints,
    )
}

#[test]
fn mail_goes_over_tls_from_the_start() {
    let server = Smtp::serve(MailSecurity::Tls, 2);
    mail_to(server.email(MailSecurity::Tls), "right", CA).unwrap();
    let talk = server.talk();
    assert!(talk.contains("MAIL FROM:<qa@acme.com>"), "{talk}");
    assert!(talk.contains("RCPT TO:<team@acme.com>"), "{talk}");
    assert!(
        talk.contains("Subject: Failed: Sign in & <out>\r\n"),
        "{talk}"
    );
    assert!(talk.contains("Took: 1 s"), "{talk}");
    assert_eq!(
        mail_to(server.email(MailSecurity::Tls), "wrong", CA),
        Err(failed(
            "the mail server didn't accept the user name or password"
        ))
    );
}

#[test]
fn mail_goes_over_starttls_and_never_in_the_clear() {
    let server = Smtp::serve(MailSecurity::StartTls, 1);
    mail_to(server.email(MailSecurity::StartTls), "right", CA).unwrap();
    let talk = server.talk();
    let (plain, secure) = talk.split_once("STARTTLS\r\n").expect("upgraded");
    assert!(
        !plain.contains("AUTH") && !plain.contains("MAIL"),
        "{plain}"
    );
    assert!(
        secure.contains("AUTH PLAIN") && secure.contains("Took: 1 s"),
        "{secure}"
    );

    // A server that can't upgrade hears nothing but hello.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let heard = Arc::new(Mutex::new(String::new()));
    let log = heard.clone();
    std::thread::spawn(move || {
        let (tcp, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(tcp);
        let mut line = String::new();
        let _ = reader.get_mut().write_all(b"220 old\r\n");
        while reader.read_line(&mut line).unwrap_or(0) > 0 {
            log.lock().unwrap().push_str(&line);
            line.clear();
            let _ = reader.get_mut().write_all(b"250 ok\r\n");
        }
    });
    let old = MailServer {
        port,
        ..server.email(MailSecurity::StartTls)
    };
    assert!(mail_to(old, "right", CA).is_err());
    let heard = heard.lock().unwrap().clone();
    assert!(
        !heard.contains("AUTH") && !heard.contains("MAIL"),
        "{heard}"
    );
}

#[test]
fn mail_to_a_server_of_an_untrusted_authority_is_refused() {
    let server = Smtp::serve(MailSecurity::Tls, 1);
    let other = include_bytes!("testdata/server.der");
    let Err(NotifyError::Failed { reason }) =
        mail_to(server.email(MailSecurity::Tls), "right", other)
    else {
        panic!("an untrusted server must be refused");
    };
    assert!(reason.contains("secure connection"), "{reason}");
    assert!(!server.talk().contains("AUTH"));
}
