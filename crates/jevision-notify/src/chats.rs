//! Finding the Telegram chat to post to: the chats a bot has been written in, from the updates
//! Telegram keeps for it.

use serde_json::{Value, json};

use crate::{NotifyError, failed, telegram_call};

/// What kind of chat it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatKind {
    /// One person writing to the bot.
    Person,
    /// A group or supergroup.
    Group,
    Channel,
}

/// A chat the bot has seen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundChat {
    /// The id to put in the settings, like `-1001234567890`.
    pub id: String,
    /// The group's or channel's name, or the person's.
    pub title: String,
    pub kind: ChatKind,
}

/// The bot and the chats it has been written in, the latest first; none when nobody has
/// written to it lately.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundChats {
    /// The bot's address, like `@acme_jevision_bot`.
    pub bot: String,
    pub chats: Vec<FoundChat>,
}

/// Asks Telegram, through `api`, which chats the bot of `token` has seen.
pub fn find_chats(api: &str, token: &str) -> Result<FoundChats, NotifyError> {
    let me = telegram_call(api, token, "getMe", &json!({}))?;
    let bot = match me["username"].as_str() {
        Some(name) => format!("@{name}"),
        None => me["first_name"].as_str().unwrap_or("the bot").to_owned(),
    };
    let updates = telegram_call(
        api,
        token,
        "getUpdates",
        &json!({
            "limit": 100,
            "allowed_updates": ["message", "edited_message", "channel_post", "my_chat_member", "callback_query"],
        }),
    )
    .map_err(|e| match e {
        NotifyError::Failed { reason } if reason.contains("webhook") => failed(
            "This bot hands its messages to a webhook, so Telegram won't list its chats here. \
             Make a new bot with @BotFather for JEVision, or type the chat ID yourself",
        ),
        e => e,
    })?;
    Ok(FoundChats {
        bot,
        chats: chats_in(&updates),
    })
}

/// The distinct chats of `updates`, the latest first.
fn chats_in(updates: &Value) -> Vec<FoundChat> {
    let mut found: Vec<FoundChat> = Vec::new();
    for update in updates.as_array().into_iter().flatten().rev() {
        let chat = [
            &update["message"],
            &update["edited_message"],
            &update["channel_post"],
            &update["my_chat_member"],
            &update["callback_query"]["message"],
        ]
        .into_iter()
        .map(|part| &part["chat"])
        .find(|chat| chat["id"].is_i64());
        let Some(chat) = chat else { continue };
        let id = chat["id"].to_string();
        if found.iter().any(|c| c.id == id) {
            continue;
        }
        let kind = match chat["type"].as_str() {
            Some("channel") => ChatKind::Channel,
            Some("group" | "supergroup") => ChatKind::Group,
            _ => ChatKind::Person,
        };
        found.push(FoundChat {
            title: title_of(chat, &id),
            id,
            kind,
        });
    }
    found
}

fn title_of(chat: &Value, id: &str) -> String {
    let text = |key: &str| chat[key].as_str().unwrap_or_default().trim();
    if !text("title").is_empty() {
        return text("title").to_owned();
    }
    let name = format!("{} {}", text("first_name"), text("last_name"));
    let name = name.trim();
    match (name.is_empty(), text("username")) {
        (true, "") => format!("Chat {id}"),
        (true, user) => format!("@{user}"),
        (false, "") => name.to_owned(),
        (false, user) => format!("{name} (@{user})"),
    }
}
