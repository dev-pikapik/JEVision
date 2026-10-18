use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde_json::json;

use super::*;

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
        when,
        slack: true,
        ..NotificationSettings::default()
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
        when: NotifyWhen::Every,
        unattended_only: true,
        slack: true,
        telegram: Some(TelegramChat {
            chat: "-100123".into(),
        }),
        email: Some(MailServer {
            server: "smtp.acme.com".into(),
            port: 465,
            security: MailSecurity::StartTls,
            user: "qa@acme.com".into(),
            from: "qa@acme.com".into(),
            to: vec!["team@acme.com".into(), "Ann <ann@acme.com>".into()],
        }),
    };
    settings.save(&project).unwrap();
    let text = fs::read_to_string(project.join(FILE)).unwrap();
    assert!(text.contains("\"unattendedOnly\": true"), "{text}");
    assert!(text.contains("\"security\": \"startTls\""), "{text}");
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

    NotificationSettings::default().save(&project).unwrap();
    assert!(!project.join(FILE).exists());
}

#[test]
fn messages_go_out_when_the_settings_say() {
    let failed = run(report("failed", Some("run.secret.missing")));
    let passed = run(report("passed", None));
    let stopped = run(report("cancelled", None));
    let before_failed = failed.report.clone();
    let before_passed = passed.report.clone();
    let title = |m: Option<Message>| m.map(|m| m.title);

    let fixed = slack_only(NotifyWhen::FailedOrFixed);
    assert_eq!(
        title(message_for(&fixed, &failed, None, false)).as_deref(),
        Some("Failed: Sign in")
    );
    assert_eq!(
        title(message_for(&fixed, &passed, Some(&before_failed), false)).as_deref(),
        Some("Fixed: Sign in")
    );
    assert_eq!(
        message_for(&fixed, &passed, Some(&before_passed), false),
        None
    );
    assert_eq!(message_for(&fixed, &passed, None, false), None);
    assert_eq!(message_for(&fixed, &stopped, None, false), None);

    let only_failed = slack_only(NotifyWhen::Failed);
    assert_eq!(
        message_for(&only_failed, &passed, Some(&before_failed), false),
        None
    );
    let every = slack_only(NotifyWhen::Every);
    assert_eq!(
        title(message_for(&every, &passed, Some(&before_passed), false)).as_deref(),
        Some("Passed: Sign in")
    );
    assert_eq!(message_for(&every, &stopped, None, false), None);

    let mut quarantined = failed.clone();
    quarantined.report.quarantined = true;
    assert_eq!(message_for(&fixed, &quarantined, None, false), None);
    assert!(message_for(&every, &quarantined, None, false).is_some());

    let unattended = NotificationSettings {
        unattended_only: true,
        ..fixed.clone()
    };
    assert_eq!(message_for(&unattended, &failed, None, false), None);
    assert!(message_for(&unattended, &failed, None, true).is_some());
    assert_eq!(
        message_for(&NotificationSettings::default(), &failed, None, true),
        None
    );
}

#[test]
fn a_message_says_why_how_long_and_where_the_report_is() {
    let mut value = report("failed", Some("run.secret.missing"));
    value["schedule"] = json!("Nightly");
    let told = message(&run(value), false);
    assert_eq!(told.title, "Failed: Sign in");
    let lines: Vec<&str> = told.body.lines().collect();
    assert_eq!(lines[0], "Secret PASSWORD is not set.");
    assert_eq!(
        lines[1],
        "Took 12.3 s · login/sign-in.flow.json · schedule “Nightly”"
    );
    match ci_run_url() {
        Some(url) => assert_eq!(lines[2], format!("Report: {url}")),
        None => assert_eq!(
            lines[2],
            "Report on the Mac that ran it: /runs/20261019-093000-sign-in/report.html"
        ),
    }

    let mut rows = report("failed", None);
    rows["attempts"] = json!([
        {"number": 1, "verdict": "passed", "durationMs": 1, "steps": [],
         "row": {"number": 1, "label": "ann"}},
        {"number": 1, "verdict": "failed", "durationMs": 1, "steps": [],
         "row": {"number": 2, "label": "bob"},
         "failure": {"code": "run.secret.missing", "params": {"name": "PASSWORD"}}},
    ]);
    let body = message(&run(rows), false).body;
    assert!(
        body.starts_with("Row 2: bob: Secret PASSWORD is not set.\n"),
        "{body}"
    );

    let mut flaky = report("passed", None);
    flaky["flaky"] = json!(true);
    flaky["attempts"] = json!([
        {"number": 1, "verdict": "failed", "durationMs": 1, "steps": []},
        {"number": 2, "verdict": "passed", "durationMs": 1, "steps": []},
    ]);
    let body = message(&run(flaky), true).body;
    assert!(
        body.starts_with("Passed on retry 1 after failing"),
        "{body}"
    );
    assert_eq!(duration(850), "850 ms");
    assert_eq!(duration(125_000), "2 min 5 s");
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
    Message {
        title: "Failed: Sign in & <out>".into(),
        body: "Took 1 s".into(),
    }
}

fn no_secret(_: NotifyChannel) -> Option<String> {
    None
}

#[test]
fn slack_gets_the_text_through_the_webhook() {
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
    assert!(
        requests[0].ends_with(r#"{"text":"Failed: Sign in &amp; &lt;out&gt;\nTook 1 s"}"#),
        "{}",
        requests[0]
    );

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
        }),
        ..NotificationSettings::default()
    };
    let endpoints = Endpoints {
        telegram_api: server.address.clone(),
        mail_root: None,
    };
    let secret = |_| Some("123:bot-token".to_owned());
    let results = send_all(&settings, &note(), &secret, &endpoints);
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
    let body: serde_json::Value =
        serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(
        body,
        json!({"chat_id": "-100123", "text": "Failed: Sign in & <out>\nTook 1 s",
               "disable_web_page_preview": true})
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
        &note(),
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
    assert!(talk.contains("Took 1 s"), "{talk}");
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
        secure.contains("AUTH PLAIN") && secure.contains("Took 1 s"),
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
