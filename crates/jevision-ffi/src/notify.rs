//! Messages about runs to Slack, Telegram and email: the project's settings for them, the
//! secrets kept in the Keychain, a test message, and the message after a run.

use std::path::Path;

use jevision_notify::{
    Endpoints, FILE, MailSecurity, MailServer, NotificationSettings, NotifyChannel, NotifyError,
    NotifyWhen, TelegramChat,
};

#[uniffi::remote(Enum)]
pub enum NotifyWhen {
    Failed,
    FailedOrFixed,
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
pub struct TelegramChat {
    pub chat: String,
}

#[uniffi::remote(Record)]
pub struct MailServer {
    pub server: String,
    pub port: u16,
    pub security: MailSecurity,
    pub user: String,
    pub from: String,
    pub to: Vec<String>,
}

#[uniffi::remote(Record)]
pub struct NotificationSettings {
    pub when: NotifyWhen,
    pub unattended_only: bool,
    pub slack: bool,
    pub telegram: Option<TelegramChat>,
    pub email: Option<MailServer>,
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
    NotificationSettings::load(Path::new(&project))
}

/// Saves the notification settings of the project; with every channel off the file is removed.
#[uniffi::export]
pub fn save_notification_settings(
    project: String,
    settings: NotificationSettings,
) -> Result<(), NotifyError> {
    settings.save(Path::new(&project))
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
    let settings = NotificationSettings::load(project)?;
    jevision_notify::send(
        &settings,
        channel,
        &jevision_notify::test_message(project),
        &|c| jevision_notify::secret(project, c),
        &Endpoints::default(),
    )
}

/// Tells the project's channels about the run in folder `dir` when its settings ask for it;
/// one result per channel the message went to, none when no message was due. Takes as long as
/// the services take to answer, so call it off the main thread.
#[uniffi::export]
pub fn notify_run(project: String, dir: String) -> Result<Vec<NotifySent>, NotifyError> {
    Ok(
        jevision_notify::notify(Path::new(&project), Path::new(&dir))?
            .into_iter()
            .map(|sent| NotifySent {
                channel: sent.channel,
                failure: sent.result.err(),
            })
            .collect(),
    )
}

/// Why a message wasn't sent, in English, for the command line.
#[uniffi::export]
pub fn notify_error_message(error: NotifyError) -> String {
    error.to_string()
}

/// `Slack`, `Telegram` or `Email`, for the command line.
#[uniffi::export]
pub fn notify_channel_name(channel: NotifyChannel) -> String {
    channel.name().to_owned()
}
