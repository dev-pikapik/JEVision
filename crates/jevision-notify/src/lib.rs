//! Telling people how test runs went: a message to Slack (incoming webhook), Telegram (a bot)
//! or email (SMTP over TLS) after a run, as the project's `jevision.notifications.json` asks.
//!
//! The file says which channels to use and, for each, when; it holds nothing secret and is
//! committed with the tests. The webhook, the bot token and the mail password are kept in the Keychain of each
//! Mac, per project, or come from environment variables on CI.

use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{fs, io};

use jevision_l10n::{language, tr, tr_in};
use jevision_runner::{Run, RunReport, Verdict};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

mod chats;
mod message;
mod render;
#[cfg(test)]
mod tests;

pub use chats::{ChatKind, FoundChat, FoundChats, find_chats};
pub use message::{
    Details, FailedTest, Finished, Message, Notice, Picture, ReportLink, Tone, failure_picture,
    failure_picture_in, message, message_in, notice_for, notice_for_in,
};

/// Name of the notification settings file in the project folder.
pub const FILE: &str = "jevision.notifications.json";
/// The layout the file is saved in. Format 1, with one `when` for every channel, is still read.
const FORMAT: u32 = 2;
/// Telegram's Bot API.
pub const TELEGRAM_API: &str = "https://api.telegram.org";
const TIMEOUT: Duration = Duration::from_secs(20);

/// Which runs a channel is told about.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NotifyWhen {
    /// Every run that failed.
    Failed,
    /// Every run that failed, and the first pass after a failure.
    #[default]
    FailedOrFixed,
    /// The first failure of a test that passed before, and the first pass after a failure; a
    /// test that keeps failing says nothing more.
    Changed,
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

/// The Slack channel of the webhook.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SlackChannel {
    #[serde(default)]
    pub when: NotifyWhen,
}

/// A Telegram chat a bot posts to.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TelegramChat {
    /// The chat's id, like `-1001234567890` for a group, or `@channel` for a public channel.
    pub chat: String,
    #[serde(default)]
    pub when: NotifyWhen,
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
    #[serde(default)]
    pub when: NotifyWhen,
}

/// A project's notification settings: the channels that are on, each with its own `when`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NotificationSettings {
    /// Only runs started by a schedule or on CI are told about, not the ones someone watched.
    pub unattended_only: bool,
    pub slack: Option<SlackChannel>,
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

    /// The channel's name in the process language.
    pub fn name(self) -> String {
        self.name_in(&language())
    }

    /// The channel's name in `lang`.
    pub fn name_in(self, lang: &str) -> String {
        match self {
            Self::Slack => "Slack".into(),
            Self::Telegram => "Telegram".into(),
            Self::Email => tr_in!(lang, "Email"),
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotifyError {
    Unreadable {
        file: String,
        reason: String,
    },
    /// `reason` is already a sentence for the user.
    Rejected {
        reason: String,
    },
    Unwritable {
        file: String,
        reason: String,
    },
    NotSetUp {
        channel: NotifyChannel,
    },
    NoSecret {
        channel: NotifyChannel,
    },
    /// `reason` is already a sentence for the user.
    Failed {
        reason: String,
    },
}

impl NotifyError {
    /// The sentence for the user in `lang`.
    pub fn text_in(&self, lang: &str) -> String {
        match self {
            Self::Unreadable { file, reason } => {
                tr_in!(lang, "%@ can’t be read: %@", file, reason)
            }
            Self::Unwritable { file, reason } => {
                tr_in!(lang, "%@ can’t be saved: %@", file, reason)
            }
            Self::NotSetUp { channel } => {
                tr_in!(lang, "notify.error.notSetUp", channel.name_in(lang))
            }
            Self::NoSecret { channel } => {
                let variable = channel.variable();
                match channel {
                    NotifyChannel::Slack => tr_in!(lang, "notify.error.noSecret.slack", variable),
                    NotifyChannel::Telegram => {
                        tr_in!(lang, "notify.error.noSecret.telegram", variable)
                    }
                    NotifyChannel::Email => tr_in!(lang, "notify.error.noSecret.email", variable),
                }
            }
            Self::Rejected { reason } | Self::Failed { reason } => reason.clone(),
        }
    }
}

impl std::fmt::Display for NotifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text_in(&language()))
    }
}

impl std::error::Error for NotifyError {}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct File {
    format: u32,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    unattended_only: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    slack: Option<SlackChannel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    telegram: Option<TelegramChat>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    email: Option<MailServer>,
}

/// Format 1 had one `when` for every channel and `slack` as `true` or `false`; format 2 gives
/// each channel its own `when`. Turns a format 1 file into format 2.
fn migrate(file: &mut Value) -> Result<(), String> {
    let Some(file) = file
        .as_object_mut()
        .filter(|f| f.get("format") == Some(&json!(1)))
    else {
        return Ok(());
    };
    let when = file
        .remove("when")
        .unwrap_or_else(|| json!(NotifyWhen::default()));
    serde_json::from_value::<NotifyWhen>(when.clone()).map_err(|e| format!("\"when\": {e}"))?;
    for channel in ["telegram", "email"] {
        if let Some(channel) = file.get_mut(channel).and_then(Value::as_object_mut) {
            channel.entry("when").or_insert_with(|| when.clone());
        }
    }
    match file.get("slack") {
        Some(Value::Bool(true)) => {
            file.insert("slack".into(), json!({ "when": when }));
        }
        Some(Value::Bool(false)) => {
            file.remove("slack");
        }
        _ => {}
    }
    file.insert("format".into(), json!(FORMAT));
    Ok(())
}

impl NotificationSettings {
    /// Reads the text of a settings file, of format 1 or 2; `Err` says what is wrong with it.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut value: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        migrate(&mut value)?;
        let file: File = serde_json::from_value(value).map_err(|e| e.to_string())?;
        if file.format != FORMAT {
            return Err(tr!("notify.settings.format", FORMAT));
        }
        let settings = Self {
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
            return Err(tr!("notify.settings.telegramChat"));
        }
        if let Some(email) = &self.email {
            if email.server.trim().is_empty() {
                return Err(tr!("notify.settings.mailServer"));
            }
            if email.port == 0 {
                return Err(tr!("notify.settings.mailPort"));
            }
            if !is_address(&email.from) {
                return Err(tr!("notify.settings.badFrom", &email.from));
            }
            if email.to.is_empty() {
                return Err(tr!("notify.settings.noRecipients"));
            }
            if let Some(bad) = email.to.iter().find(|a| !is_address(a)) {
                return Err(tr!("notify.settings.badTo", bad));
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
        NotifyChannel::ALL
            .into_iter()
            .filter(|c| self.when(*c).is_some())
            .collect()
    }

    /// Which runs `channel` is told about; none when it is off.
    pub fn when(&self, channel: NotifyChannel) -> Option<NotifyWhen> {
        match channel {
            NotifyChannel::Slack => self.slack.map(|s| s.when),
            NotifyChannel::Telegram => self.telegram.as_ref().map(|t| t.when),
            NotifyChannel::Email => self.email.as_ref().map(|e| e.when),
        }
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

/// The run is on CI, where no one is watching.
pub fn on_ci() -> bool {
    std::env::var("CI").is_ok_and(|v| !v.is_empty() && v != "false" && v != "0")
}

/// The run of the same test file before `run`, skipping stopped and blocked ones: they don't
/// say whether the test was passing.
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
            r.file == run.report.file
                && !matches!(
                    r.verdict,
                    Verdict::Cancelled | Verdict::Blocked | Verdict::Unknown
                )
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

/// Tells the project's channels about the runs in `dirs`, one batch of `project`, as far as
/// their settings ask for it: one message about a single run, one summary about several. The
/// channels that aren't due get nothing, and nothing at all goes out when none is.
pub fn notify(project: &Path, dirs: &[PathBuf]) -> Result<Vec<Sent>, NotifyError> {
    notify_in(&language(), project, dirs)
}

/// [`notify`] with the message in `lang`.
pub fn notify_in(lang: &str, project: &Path, dirs: &[PathBuf]) -> Result<Vec<Sent>, NotifyError> {
    let settings = NotificationSettings::load(project)?;
    if settings.channels().is_empty() {
        return Ok(Vec::new());
    }
    let runs = dirs
        .iter()
        .map(|dir| {
            let run = Run::load(dir).map_err(|e| failed(e.to_string()))?;
            Ok(Finished {
                previous: previous(project, &run),
                run,
            })
        })
        .collect::<Result<Vec<_>, NotifyError>>()?;
    let unattended = on_ci() || runs.iter().any(|r| r.run.report.schedule.is_some());
    let name = project
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let Some(mut notice) = notice_for_in(lang, &settings, &name, &runs, unattended) else {
        return Ok(Vec::new());
    };
    if notice.channels.contains(&NotifyChannel::Email) {
        notice.message.details.picture = failure_picture_in(lang, &runs);
    }
    Ok(send_all(
        &settings,
        &notice,
        &|c| secret(project, c),
        &Endpoints::default(),
    ))
}

/// Sends the message of `notice` to the channels it names.
pub fn send_all(
    settings: &NotificationSettings,
    notice: &Notice,
    secret: &dyn Fn(NotifyChannel) -> Option<String>,
    endpoints: &Endpoints,
) -> Vec<Sent> {
    notice
        .channels
        .iter()
        .map(|&channel| Sent {
            channel,
            result: send(settings, channel, &notice.message, secret, endpoints),
        })
        .collect()
}

/// A message to show that `channel` works, for the Send Test button.
pub fn test_message(project: &Path) -> Message {
    test_message_in(&language(), project)
}

/// [`test_message`] in `lang`.
pub fn test_message_in(lang: &str, project: &Path) -> Message {
    let name = project
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    Message::new_in(
        lang,
        tr_in!(lang, "notify.test.title"),
        Details {
            lead: Some(tr_in!(lang, "notify.test.lead", name)),
            ..Details::default()
        },
    )
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
fn post(url: &str, body: &Value, service: &str) -> Result<(u16, String), NotifyError> {
    let mut response = agent()
        .post(url)
        .content_type("application/json")
        .send(body.to_string())
        .map_err(|_| failed(tr!("notify.error.unreachable", service)))?;
    let status = response.status().as_u16();
    let answer = response.body_mut().read_to_string().unwrap_or_default();
    Ok((status, answer))
}

fn slack(webhook: &str, message: &Message) -> Result<(), NotifyError> {
    if !webhook.starts_with("https://") && !webhook.starts_with("http://127.0.0.1") {
        return Err(failed(tr!("notify.error.slackAddress")));
    }
    let rich = render::slack(message);
    let (mut status, mut answer) = post(webhook, &rich, "Slack")?;
    if status == 400 && answer.contains("invalid_blocks") {
        // The news matters more than its layout.
        (status, answer) = post(webhook, &json!({ "text": rich["text"] }), "Slack")?;
    }
    match status {
        200 => Ok(()),
        403 | 404 | 410 => Err(failed(tr!("notify.error.slackRejected", answer.trim()))),
        _ => Err(failed(tr!(
            "notify.error.slackStatus",
            status,
            answer.trim()
        ))),
    }
}

fn telegram(api: &str, token: &str, chat: &str, message: &Message) -> Result<(), NotifyError> {
    let body =
        json!({ "chat_id": chat.trim(), "text": message.text(), "disable_web_page_preview": true });
    telegram_call(api, token, "sendMessage", &body).map(drop)
}

/// Calls `method` of Telegram's Bot API; the `result` it answers with.
fn telegram_call(api: &str, token: &str, method: &str, body: &Value) -> Result<Value, NotifyError> {
    let url = format!("{}/bot{token}/{method}", api.trim_end_matches('/'));
    let (status, answer) = post(&url, body, "Telegram")?;
    let mut answer: Value = serde_json::from_str(&answer).unwrap_or_default();
    if status == 200 && answer["ok"] == true {
        return Ok(answer["result"].take());
    }
    let description = answer["description"].as_str().unwrap_or_default();
    Err(failed(match status {
        401 | 404 => tr!("notify.error.telegramToken"),
        _ if description.is_empty() => tr!("notify.error.telegramStatus", status),
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
    use lettre::message::{Attachment, Mailbox, MessageBuilder, MultiPart, SinglePart};
    use lettre::transport::smtp::authentication::Credentials;
    use lettre::transport::smtp::client::{Certificate, CertificateStore, Tls, TlsParameters};
    use lettre::{SmtpTransport, Transport};

    let broken = |e: &dyn std::fmt::Display| failed(format!("{}: {e}", tr!("Email")));
    let mailbox = |a: &str| a.parse::<Mailbox>().map_err(|e| broken(&e));
    let mut builder = MessageBuilder::new()
        .from(mailbox(&email.from)?)
        .subject(&message.title);
    for to in &email.to {
        builder = builder.to(mailbox(to)?);
    }
    let (plain, html) = (message.text(), render::html(message));
    let body = match &message.details.picture {
        None => MultiPart::alternative_plain_html(plain, html),
        Some(picture) => {
            let png = ContentType::parse("image/png").map_err(|e| broken(&e))?;
            MultiPart::alternative()
                .singlepart(SinglePart::plain(plain))
                .multipart(
                    MultiPart::related()
                        .singlepart(SinglePart::html(html))
                        .singlepart(
                            Attachment::new_inline(render::PICTURE_ID.into())
                                .body(picture.png.clone(), png),
                        ),
                )
        }
    };
    let mail = builder.multipart(body).map_err(|e| broken(&e))?;

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
        tr!("notify.error.mailSignIn")
    } else if e.is_tls() || text.contains("certificate") {
        // A failed handshake comes as a network error.
        let reason = text.trim_start_matches("network error: ");
        tr!("notify.error.mailTls", server, reason)
    } else {
        format!("{}: {text}", tr!("Email"))
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
