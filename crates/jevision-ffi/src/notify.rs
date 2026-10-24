//! Messages about runs to Slack, Telegram and email: the project's settings for them, the
//! secrets kept in the Keychain, a test message, and the message after a run.

use std::path::{Path, PathBuf};

use jevision_notify::{
    ChatKind, Endpoints, FILE, FoundChat, FoundChats, MailSecurity, MailServer,
    NotificationSettings as CoreSettings, NotifyChannel, NotifyError, NotifyWhen, SlackChannel,
    TelegramChat,
};

#[uniffi::remote(Enum)]
pub enum NotifyWhen {
    Failed,
    FailedOrFixed,
    Changed,
    Every,
}

#[uniffi::remote(Enum)]
pub enum MailSecurity {
    Tls,
    StartTls,
}

#[uniffi::remote(Enum)]
pub enum NotifyChannel {
    Slack,
    Telegram,
    Email,
}

#[uniffi::remote(Record)]
pub struct SlackChannel {
    pub when: NotifyWhen,
}

#[uniffi::remote(Record)]
pub struct TelegramChat {
    pub chat: String,
    pub when: NotifyWhen,
}

#[uniffi::remote(Record)]
pub struct MailServer {
    pub server: String,
    pub port: u16,
    pub security: MailSecurity,
    pub user: String,
    pub from: String,
    pub to: Vec<String>,
    pub when: NotifyWhen,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct NotificationSettings {
    pub unattended_only: bool,
    pub slack: Option<SlackChannel>,
    pub telegram: Option<TelegramChat>,
    pub email: Option<MailServer>,
}

impl From<CoreSettings> for NotificationSettings {
    fn from(settings: CoreSettings) -> Self {
        Self {
            unattended_only: settings.unattended_only,
            slack: settings.slack,
            telegram: settings.telegram,
            email: settings.email,
        }
    }
}

impl From<NotificationSettings> for CoreSettings {
    fn from(settings: NotificationSettings) -> Self {
        Self {
            unattended_only: settings.unattended_only,
            slack: settings.slack,
            telegram: settings.telegram,
            email: settings.email,
            ..Self::default()
        }
    }
}

#[uniffi::remote(Enum)]
pub enum ChatKind {
    Person,
    Group,
    Channel,
}

#[uniffi::remote(Record)]
pub struct FoundChat {
    pub id: String,
    pub title: String,
    pub kind: ChatKind,
}

#[uniffi::remote(Record)]
pub struct FoundChats {
    pub bot: String,
    pub chats: Vec<FoundChat>,
}

#[uniffi::remote(Error)]
pub enum NotifyError {
    Unreadable { file: String, reason: String },
    Rejected { reason: String },
    Unwritable { file: String, reason: String },
    NotSetUp { channel: NotifyChannel },
    NoSecret { channel: NotifyChannel },
    Failed { reason: String },
}

/// How sending to one channel went.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct NotifySent {
    pub channel: NotifyChannel,
    /// Why the message didn't go; `None` when it did.
    pub failure: Option<NotifyError>,
}

/// The notification settings of the project folder `project`; a project without the file has
/// every channel off.
#[uniffi::export]
pub fn notification_settings(project: String) -> Result<NotificationSettings, NotifyError> {
    CoreSettings::load(Path::new(&project)).map(Into::into)
}

/// Saves the notification settings of the project; with every channel off the file is removed.
#[uniffi::export]
pub fn save_notification_settings(
    project: String,
    settings: NotificationSettings,
) -> Result<(), NotifyError> {
    CoreSettings::from(settings).save(Path::new(&project))
}

/// Optional fields and requirement diagnostics of the notification settings file.
#[uniffi::export]
pub fn notification_settings_issues(project: String) -> Vec<crate::Issue> {
    jevision_notify::settings_issues(Path::new(&project))
}

/// Name of the notification settings file in a project folder.
#[uniffi::export]
pub fn notification_settings_file_name() -> String {
    FILE.to_owned()
}

/// The Keychain service and account where the app keeps the webhook, bot token or mail
/// password of `channel` for the project.
#[uniffi::export]
pub fn notification_keychain_item(project: String, channel: NotifyChannel) -> KeychainItem {
    KeychainItem {
        service: jevision_notify::KEYCHAIN_SERVICE.to_owned(),
        account: jevision_notify::keychain_account(Path::new(&project), channel),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct KeychainItem {
    pub service: String,
    pub account: String,
}

/// The environment variable that gives `channel` its secret on CI.
#[uniffi::export]
pub fn notification_secret_variable(channel: NotifyChannel) -> String {
    channel.variable().to_owned()
}

/// Sends a test message to `channel` as the project's saved settings say.
#[uniffi::export]
pub fn send_test_notification(project: String, channel: NotifyChannel) -> Result<(), NotifyError> {
    let project = Path::new(&project);
    let settings = CoreSettings::load(project)?;
    jevision_notify::send(
        &settings,
        channel,
        &jevision_notify::test_message(project),
        &|c| jevision_notify::secret(project, c),
        &Endpoints::default(),
    )
}

/// Tells the project's channels about the runs in the folders `dirs`, one batch, when the
/// settings ask for it: one message about a single run, one summary about several. One result
/// per channel the message went to, none when no message was due. Takes as long as the services
/// take to answer, so call it off the main thread.
#[uniffi::export]
pub fn notify_runs(project: String, dirs: Vec<String>) -> Result<Vec<NotifySent>, NotifyError> {
    let dirs: Vec<PathBuf> = dirs.into_iter().map(PathBuf::from).collect();
    Ok(jevision_notify::notify(Path::new(&project), &dirs)?
        .into_iter()
        .map(|sent| NotifySent {
            channel: sent.channel,
            failure: sent.result.err(),
        })
        .collect())
}

/// The chats the project's Telegram bot has been written in, the latest first, found with the
/// bot token on this Mac. Asks Telegram, so call it off the main thread.
#[uniffi::export]
pub fn find_telegram_chats(project: String) -> Result<FoundChats, NotifyError> {
    let channel = NotifyChannel::Telegram;
    let token = jevision_notify::secret(Path::new(&project), channel)
        .ok_or(NotifyError::NoSecret { channel })?;
    jevision_notify::find_chats(jevision_notify::TELEGRAM_API, &token)
}

/// Why a message wasn't sent, in the current language, for the command line.
#[uniffi::export]
pub fn notify_error_message(error: NotifyError) -> String {
    error.to_string()
}

/// `Slack`, `Telegram` or `Email`, for the command line.
#[uniffi::export]
pub fn notify_channel_name(channel: NotifyChannel) -> String {
    channel.name().to_owned()
}
