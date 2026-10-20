//! Telling people how test runs went: a message to Slack (incoming webhook), Telegram (a bot)
//! or email (SMTP over TLS) after a run, as the project's `jevision.notifications.json` asks.
//!
//! The file says which channels to use and when; it holds nothing secret and is committed with
//! the tests. The webhook, the bot token and the mail password are kept in the Keychain of each
//! Mac, per project, or come from environment variables on CI.

use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{fs, io};

use jevision_core::BlockReason;
use jevision_runner::{RetryVerdict, Run, RunReport, Verdict, describe};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[cfg(test)]
mod tests;

/// Name of the notification settings file in the project folder.
pub const FILE: &str = "jevision.notifications.json";
const FORMAT: u32 = 1;
/// Telegram's Bot API.
pub const TELEGRAM_API: &str = "https://api.telegram.org";
const TIMEOUT: Duration = Duration::from_secs(20);

/// Which runs are told about.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NotifyWhen {
    /// Every run that failed.
    Failed,
    /// Every run that failed, and the first pass after a failure.
    #[default]
    FailedOrFixed,
    /// Every run that finished.
    Every,
}

/// How the connection to the mail server is encrypted. There is no unencrypted mail.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MailSecurity {
    /// TLS from the first byte, usually on port 465.
    #[default]
    Tls,
    /// A plain connection upgraded with STARTTLS, usually on port 587; fails if the server
    /// can't upgrade.
    StartTls,
}

/// A Telegram chat a bot posts to.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TelegramChat {
    /// The chat's id, like `-1001234567890` for a group, or `@channel` for a public channel.
    pub chat: String,
}

/// Mail sent through an SMTP server.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MailServer {
    /// The server's host name, like `smtp.gmail.com`.
    pub server: String,
    pub port: u16,
    #[serde(default)]
    pub security: MailSecurity,
    /// The name to sign in with; empty for a server that takes mail without signing in.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub user: String,
    /// The sender's address.
    pub from: String,
    /// Addresses the mail goes to.
    pub to: Vec<String>,
}

/// A project's notification settings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NotificationSettings {
    pub when: NotifyWhen,
    /// Only runs started by a schedule or on CI are told about, not the ones someone watched.
    pub unattended_only: bool,
    /// Post to the Slack channel of the webhook.
    pub slack: bool,
    pub telegram: Option<TelegramChat>,
    pub email: Option<MailServer>,
}

/// Where a message goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifyChannel {
    Slack,
    Telegram,
    Email,
}

impl NotifyChannel {
    pub const ALL: [Self; 3] = [Self::Slack, Self::Telegram, Self::Email];

    pub fn name(self) -> &'static str {
        match self {
            Self::Slack => "Slack",
            Self::Telegram => "Telegram",
            Self::Email => "Email",
        }
    }

    /// The environment variable that holds the channel's secret on CI.
    pub fn variable(self) -> &'static str {
        match self {
            Self::Slack => "JEVISION_SLACK_WEBHOOK",
            Self::Telegram => "JEVISION_TELEGRAM_TOKEN",
            Self::Email => "JEVISION_SMTP_PASSWORD",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Slack => "slack",
            Self::Telegram => "telegram",
            Self::Email => "email",
        }
    }
}

/// Why a message wasn't sent, in a sentence for the user.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NotifyError {
    #[error("{file} can't be read: {reason}")]
    Unreadable { file: String, reason: String },
    #[error("{reason}")]
    Rejected { reason: String },
    #[error("{file} can't be saved: {reason}")]
    Unwritable { file: String, reason: String },
    #[error("{} isn't set up in this project", channel.name())]
    NotSetUp { channel: NotifyChannel },
    #[error("{}", missing_secret(*channel))]
    NoSecret { channel: NotifyChannel },
    #[error("{reason}")]
    Failed { reason: String },
}

fn missing_secret(channel: NotifyChannel) -> String {
    let what = match channel {
        NotifyChannel::Slack => "the Slack webhook",
        NotifyChannel::Telegram => "the Telegram bot token",
        NotifyChannel::Email => "the mail password",
    };
    format!(
        "{what} isn't on this Mac; add it in JEVision Settings → Notifications or set {}",
        channel.variable()
    )
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct File {
    format: u32,
    #[serde(default)]
    when: NotifyWhen,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    unattended_only: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    slack: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    telegram: Option<TelegramChat>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    email: Option<MailServer>,
}

impl NotificationSettings {
    /// Reads the text of a settings file; `Err` says what is wrong with it.
    pub fn parse(text: &str) -> Result<Self, String> {
        let file: File = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if file.format != FORMAT {
            return Err(format!("\"format\" must be {FORMAT}"));
        }
        let settings = Self {
            when: file.when,
            unattended_only: file.unattended_only,
            slack: file.slack,
            telegram: file.telegram,
            email: file.email,
        };
        settings.check()?;
        Ok(settings)
    }

    /// Whether the settings can be used; `Err` names what is missing or wrong.
    pub fn check(&self) -> Result<(), String> {
        if let Some(telegram) = &self.telegram
            && telegram.chat.trim().is_empty()
        {
            return Err("Telegram needs the id of the chat to post to".into());
        }
        if let Some(email) = &self.email {
            if email.server.trim().is_empty() {
                return Err("Email needs the address of the mail server".into());
            }
            if email.port == 0 {
                return Err("Email needs the mail server's port, such as 465 or 587".into());
            }
            if !is_address(&email.from) {
                return Err(format!(
                    "\"{}\" isn't an email address to send from",
                    email.from
                ));
            }
            if email.to.is_empty() {
                return Err("Email needs at least one address to send to".into());
            }
            if let Some(bad) = email.to.iter().find(|a| !is_address(a)) {
                return Err(format!("\"{bad}\" isn't an email address to send to"));
            }
        }
        Ok(())
    }

    /// The settings file of the project folder `project`; none means no notifications.
    pub fn load(project: &Path) -> Result<Self, NotifyError> {
        match fs::read_to_string(project.join(FILE)) {
            Ok(text) => Self::parse(&text).map_err(unreadable),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(unreadable(e.to_string())),
        }
    }

    /// Writes the settings to the project folder; settings without a channel remove the file.
    pub fn save(&self, project: &Path) -> Result<(), NotifyError> {
        self.check()
            .map_err(|reason| NotifyError::Rejected { reason })?;
        let path = project.join(FILE);
        if self.channels().is_empty() {
            return match fs::remove_file(&path) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => Err(unwritable(e.to_string())),
                _ => Ok(()),
            };
        }
        let file = File {
            format: FORMAT,
            when: self.when,
            unattended_only: self.unattended_only,
            slack: self.slack,
            telegram: self.telegram.clone(),
            email: self.email.clone(),
        };
        let mut text = serde_json::to_string_pretty(&file).expect("settings always serialize");
        text.push('\n');
        fs::write(path, text).map_err(|e| unwritable(e.to_string()))
    }

    /// The channels that are on.
    pub fn channels(&self) -> Vec<NotifyChannel> {
        let on = [self.slack, self.telegram.is_some(), self.email.is_some()];
        NotifyChannel::ALL
            .into_iter()
            .zip(on)
            .filter_map(|(channel, on)| on.then_some(channel))
            .collect()
    }
}

fn unreadable(reason: String) -> NotifyError {
    NotifyError::Unreadable {
        file: FILE.into(),
        reason,
    }
}

fn unwritable(reason: String) -> NotifyError {
    NotifyError::Unwritable {
        file: FILE.into(),
        reason,
    }
}

fn failed(reason: impl Into<String>) -> NotifyError {
    NotifyError::Failed {
        reason: reason.into(),
    }
}

fn is_address(text: &str) -> bool {
    text.parse::<lettre::message::Mailbox>().is_ok()
}

/// What a message says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// The first line, also the mail's subject, like `Failed: Sign in`.
    pub title: String,
    /// The lines after the title.
    pub body: String,
}

impl Message {
    fn text(&self) -> String {
        format!("{}\n{}", self.title, self.body)
    }
}

/// Where messages go besides the real services, for tests.
#[derive(Debug, Clone)]
pub struct Endpoints {
    pub telegram_api: String,
    /// The only certificate authority mail servers are trusted by, in DER, instead of the
    /// system's.
    pub mail_root: Option<Vec<u8>>,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            telegram_api: TELEGRAM_API.into(),
            mail_root: None,
        }
    }
}

/// The message about `run` when the settings ask for one: `previous` is the run of the same test
/// before it, and `unattended` says the run was started by a schedule or on CI.
pub fn message_for(
    settings: &NotificationSettings,
    run: &Run,
    previous: Option<&RunReport>,
    unattended: bool,
) -> Option<Message> {
    let report = &run.report;
    if settings.channels().is_empty() || (settings.unattended_only && !unattended) {
        return None;
    }
    let fixed =
        report.verdict == Verdict::Passed && previous.is_some_and(|p| p.verdict == Verdict::Failed);
    let send = match (report.verdict, settings.when) {
        (Verdict::Cancelled, _) => false,
        (_, NotifyWhen::Every) => true,
        (Verdict::Failed, _) => !report.quarantined,
        // A failed dependency sends its own message; a missing one or a circle sends none.
        (Verdict::Blocked, _) => report
            .blocked
            .as_ref()
            .is_some_and(|b| matches!(b.reason, BlockReason::Missing | BlockReason::Cycle { .. })),
        (Verdict::Passed, NotifyWhen::FailedOrFixed) => fixed,
        (Verdict::Passed, NotifyWhen::Failed) => false,
    };
    send.then(|| message(run, fixed))
}

/// The message about `run`: what happened, why, how long it took and where the report is.
pub fn message(run: &Run, fixed: bool) -> Message {
    let report = &run.report;
    let outcome = match report.verdict {
        Verdict::Failed => "Failed",
        Verdict::Passed if fixed => "Fixed",
        Verdict::Passed => "Passed",
        Verdict::Cancelled => "Stopped",
        Verdict::Blocked => "Blocked",
    };
    let mut lines = Vec::new();
    if let Some(reason) = reason(report) {
        lines.push(reason);
    }
    let mut facts = vec![format!("Took {}", duration(report.duration_ms))];
    facts.push(report.file.clone());
    if let Some(schedule) = &report.schedule {
        facts.push(format!("schedule “{schedule}”"));
    }
    if let Some(environment) = &report.environment {
        facts.push(format!("environment “{environment}”"));
    }
    lines.push(facts.join(" · "));
    lines.push(match ci_run_url() {
        Some(url) => format!("Report: {url}"),
        None => format!(
            "Report on the Mac that ran it: {}",
            run.dir.join("report.html").display()
        ),
    });
    Message {
        title: format!("{outcome}: {}", report.test),
        body: lines.join("\n"),
    }
}

/// Why the run failed or was blocked, or that it passed only on a retry, in one sentence.
fn reason(report: &RunReport) -> Option<String> {
    if report.verdict == Verdict::Passed {
        return RetryVerdict::of(report.verdict, report.flaky, report.attempts.len())
            .map(RetryVerdict::describe);
    }
    if let Some(blocker) = &report.blocked {
        return Some(blocker.describe());
    }
    let rows = report.rows();
    if let Some(row) = rows.iter().find(|r| r.verdict == Verdict::Failed) {
        let why = row.failure.as_ref().map(describe).unwrap_or_default();
        let total = rows.len();
        let failed = rows.iter().filter(|r| r.verdict == Verdict::Failed).count();
        let more = match failed {
            1 => String::new(),
            n => format!(" ({n} of {total} rows failed)"),
        };
        return Some(
            format!("{}: {why}{more}", row.row.title())
                .trim()
                .to_owned(),
        );
    }
    report
        .attempts
        .last()
        .and_then(|a| a.failure.as_ref())
        .map(describe)
}

/// `850 ms`, `12.3 s` or `2 min 5 s`.
fn duration(ms: u64) -> String {
    match ms {
        0..1_000 => format!("{ms} ms"),
        1_000..60_000 => format!("{:.1} s", ms as f64 / 1000.0),
        _ => format!("{} min {} s", ms / 60_000, ms % 60_000 / 1000),
    }
}

/// The page of the GitHub Actions run this is part of, where its reports are kept.
fn ci_run_url() -> Option<String> {
    let var = |name| std::env::var(name).ok().filter(|v| !v.is_empty());
    Some(format!(
        "{}/{}/actions/runs/{}",
        var("GITHUB_SERVER_URL")?,
        var("GITHUB_REPOSITORY")?,
        var("GITHUB_RUN_ID")?
    ))
}

/// The run is on CI, where no one is watching.
pub fn on_ci() -> bool {
    std::env::var("CI").is_ok_and(|v| !v.is_empty() && v != "false" && v != "0")
}

/// The run of the same test file before `run`, skipping stopped ones.
pub fn previous(project: &Path, run: &Run) -> Option<RunReport> {
    let runs = project.join(".jevision").join("runs");
    let name = run.dir.file_name()?.to_string_lossy().into_owned();
    let test = test_part(&name);
    let mut earlier: Vec<String> = fs::read_dir(runs)
        .ok()?
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| *n < name && test_part(n) == test)
        .collect();
    earlier.sort_unstable_by(|a, b| b.cmp(a));
    earlier
        .iter()
        .filter_map(|n| Run::load(&project.join(".jevision").join("runs").join(n)).ok())
        .map(|r| r.report)
        .find(|r| {
            r.file == run.report.file && !matches!(r.verdict, Verdict::Cancelled | Verdict::Blocked)
        })
}

/// `login` of a run folder named `20261019-093000-login` or `20261019-093000-login-2`.
fn test_part(name: &str) -> &str {
    let rest = name.get(16..).unwrap_or_default();
    match rest.rsplit_once('-') {
        Some((test, n)) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => test,
        _ => rest,
    }
}

/// How sending to one channel went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sent {
    pub channel: NotifyChannel,
    pub result: Result<(), NotifyError>,
}

/// Tells the project's channels about the run `dir` of `project` when its settings ask for it.
/// Nothing at all when they don't.
pub fn notify(project: &Path, dir: &Path) -> Result<Vec<Sent>, NotifyError> {
    let settings = NotificationSettings::load(project)?;
    if settings.channels().is_empty() {
        return Ok(Vec::new());
    }
    let run = Run::load(dir).map_err(|e| failed(e.to_string()))?;
    let unattended = run.report.schedule.is_some() || on_ci();
    let Some(message) = message_for(
        &settings,
        &run,
        previous(project, &run).as_ref(),
        unattended,
    ) else {
        return Ok(Vec::new());
    };
    Ok(send_all(
        &settings,
        &message,
        &|c| secret(project, c),
        &Endpoints::default(),
    ))
}

/// Sends `message` to every channel of `settings` that is on.
pub fn send_all(
    settings: &NotificationSettings,
    message: &Message,
    secret: &dyn Fn(NotifyChannel) -> Option<String>,
    endpoints: &Endpoints,
) -> Vec<Sent> {
    settings
        .channels()
        .into_iter()
        .map(|channel| Sent {
            channel,
            result: send(settings, channel, message, secret, endpoints),
        })
        .collect()
}

/// A message to show that `channel` works, for the Send Test button.
pub fn test_message(project: &Path) -> Message {
    let name = project
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    Message {
        title: "JEVision test message".into(),
        body: format!(
            "Messages about runs of the tests in “{name}” will come here, as the project's \
             notification settings say."
        ),
    }
}

/// Sends `message` to `channel`.
pub fn send(
    settings: &NotificationSettings,
    channel: NotifyChannel,
    message: &Message,
    secret: &dyn Fn(NotifyChannel) -> Option<String>,
    endpoints: &Endpoints,
) -> Result<(), NotifyError> {
    if !settings.channels().contains(&channel) {
        return Err(NotifyError::NotSetUp { channel });
    }
    let secret = || secret(channel).ok_or(NotifyError::NoSecret { channel });
    match channel {
        NotifyChannel::Slack => slack(&secret()?, message),
        NotifyChannel::Telegram => {
            let chat = &settings.telegram.as_ref().expect("on").chat;
            telegram(&endpoints.telegram_api, &secret()?, chat, message)
        }
        NotifyChannel::Email => {
            let email = settings.email.as_ref().expect("on");
            let password = if email.user.is_empty() {
                String::new()
            } else {
                secret()?
            };
            mail(email, &password, message, endpoints.mail_root.as_deref())
        }
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(TIMEOUT))
        .user_agent(concat!("JEVision/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

/// Posts JSON; the status and the answer.
fn post(url: &str, body: &serde_json::Value, service: &str) -> Result<(u16, String), NotifyError> {
    let mut response = agent()
        .post(url)
        .content_type("application/json")
        .send(body.to_string())
        .map_err(|_| failed(format!("{service} can't be reached")))?;
    let status = response.status().as_u16();
    let answer = response.body_mut().read_to_string().unwrap_or_default();
    Ok((status, answer))
}

fn slack(webhook: &str, message: &Message) -> Result<(), NotifyError> {
    if !webhook.starts_with("https://") && !webhook.starts_with("http://127.0.0.1") {
        return Err(failed(
            "the Slack webhook should start with https://hooks.slack.com/",
        ));
    }
    // Slack reads &, < and > as markup.
    let text = message
        .text()
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    let (status, answer) = post(webhook, &json!({ "text": text }), "Slack")?;
    match status {
        200 => Ok(()),
        403 | 404 | 410 => Err(failed(format!(
            "Slack didn't accept the webhook ({}); make a new one for the channel",
            answer.trim()
        ))),
        _ => Err(failed(format!(
            "Slack answered with error {status}: {}",
            answer.trim()
        ))),
    }
}

fn telegram(api: &str, token: &str, chat: &str, message: &Message) -> Result<(), NotifyError> {
    let url = format!("{}/bot{token}/sendMessage", api.trim_end_matches('/'));
    let body =
        json!({ "chat_id": chat.trim(), "text": message.text(), "disable_web_page_preview": true });
    let (status, answer) = post(&url, &body, "Telegram")?;
    let answer: serde_json::Value = serde_json::from_str(&answer).unwrap_or_default();
    if status == 200 && answer["ok"] == true {
        return Ok(());
    }
    let description = answer["description"].as_str().unwrap_or_default();
    Err(failed(match status {
        401 | 404 => "Telegram didn't accept the bot token".into(),
        _ if description.is_empty() => format!("Telegram answered with error {status}"),
        _ => format!("Telegram: {description}"),
    }))
}

fn mail(
    email: &MailServer,
    password: &str,
    message: &Message,
    root: Option<&[u8]>,
) -> Result<(), NotifyError> {
    use lettre::message::header::ContentType;
    use lettre::message::{Mailbox, MessageBuilder};
    use lettre::transport::smtp::authentication::Credentials;
    use lettre::transport::smtp::client::{Certificate, CertificateStore, Tls, TlsParameters};
    use lettre::{SmtpTransport, Transport};

    let broken = |e: &dyn std::fmt::Display| failed(format!("Email: {e}"));
    let mailbox = |a: &str| a.parse::<Mailbox>().map_err(|e| broken(&e));
    let mut builder = MessageBuilder::new()
        .from(mailbox(&email.from)?)
        .subject(&message.title)
        .header(ContentType::TEXT_PLAIN);
    for to in &email.to {
        builder = builder.to(mailbox(to)?);
    }
    let mail = builder.body(message.text()).map_err(|e| broken(&e))?;

    let server = email.server.trim();
    let mut tls = TlsParameters::builder(server.to_owned());
    if let Some(root) = root {
        tls = tls
            .certificate_store(CertificateStore::None)
            .add_root_certificate(Certificate::from_der(root.to_vec()).map_err(|e| broken(&e))?);
    }
    let tls = tls.build_rustls().map_err(|e| broken(&e))?;
    let mut transport = SmtpTransport::builder_dangerous(server)
        .port(email.port)
        .timeout(Some(TIMEOUT))
        .tls(match email.security {
            MailSecurity::Tls => Tls::Wrapper(tls),
            MailSecurity::StartTls => Tls::Required(tls),
        });
    if !email.user.is_empty() {
        transport = transport.credentials(Credentials::new(email.user.clone(), password.into()));
    }
    transport
        .build()
        .send(&mail)
        .map(drop)
        .map_err(|e| mail_failure(&e, server))
}

fn mail_failure(e: &lettre::transport::smtp::Error, server: &str) -> NotifyError {
    let text = e.to_string();
    failed(if e.status().is_some_and(|c| c.to_string() == "535") {
        "the mail server didn't accept the user name or password".into()
    } else if e.is_tls() || text.contains("certificate") {
        // A failed handshake comes as a network error.
        let reason = text.trim_start_matches("network error: ");
        format!("Email: a secure connection to {server} couldn't be made: {reason}")
    } else {
        format!("Email: {text}")
    })
}

/// The Keychain service of the secrets; the app saves them, the core only reads them.
pub const KEYCHAIN_SERVICE: &str = "JEVision Notifications";

/// The Keychain account of the secret of `channel` for the project folder `project`, in
/// [`KEYCHAIN_SERVICE`].
pub fn keychain_account(project: &Path, channel: NotifyChannel) -> String {
    let project = fs::canonicalize(project).unwrap_or_else(|_| PathBuf::from(project));
    format!("{} {}", project.display(), channel.key())
}

/// The secret of `channel`: its environment variable, otherwise the project's Keychain item.
pub fn secret(project: &Path, channel: NotifyChannel) -> Option<String> {
    std::env::var(channel.variable())
        .ok()
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
        .or_else(|| keychain(&keychain_account(project, channel)))
}

#[cfg(target_os = "macos")]
fn keychain(account: &str) -> Option<String> {
    let value =
        security_framework::passwords::get_generic_password(KEYCHAIN_SERVICE, account).ok()?;
    String::from_utf8(value)
        .ok()
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
}

/// Elsewhere secrets come only from environment variables.
#[cfg(not(target_os = "macos"))]
fn keychain(_: &str) -> Option<String> {
    None
}
