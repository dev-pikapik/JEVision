use super::*;

const ME: &str = r#"{"ok": true, "result": {"id": 1, "is_bot": true, "first_name": "Acme", "username": "acme_bot"}}"#;

fn find(replies: Vec<(u16, &str)>) -> (Result<FoundChats, NotifyError>, Vec<String>) {
    let server = Http::serve(replies);
    let found = find_chats(&server.address, "123:tok");
    (found, server.requests())
}

#[test]
fn the_chats_a_bot_has_seen_are_listed_latest_first_without_repeats() {
    let updates = json!({"ok": true, "result": [
        {"update_id": 1, "message": {"chat": {"id": 111, "type": "private",
            "first_name": "Ann", "last_name": "Lee", "username": "ann"}}},
        {"update_id": 2, "message": {"chat": {"id": -1001, "type": "supergroup", "title": "QA team"}}},
        {"update_id": 3, "my_chat_member": {"chat": {"id": -1002, "type": "channel", "title": "Releases"}}},
        {"update_id": 4, "edited_message": {"chat": {"id": 111, "type": "private",
            "first_name": "Ann", "last_name": "Lee", "username": "ann"}}},
        {"update_id": 5, "callback_query": {"message": {"chat": {"id": 5, "type": "private"}}}},
        {"update_id": 6, "poll": {"id": "x"}},
        {"update_id": 7, "channel_post": {"chat": {"id": -1003, "type": "channel", "username": "news"}}},
        {"update_id": 8, "message": {"chat": {"id": 9, "type": "group", "title": "Old group"}}},
        {"update_id": 9, "message": {"chat": {"id": 6, "type": "private", "first_name": "Bob"}}},
    ]})
    .to_string();
    let (found, requests) = find(vec![(200, ME), (200, &updates)]);
    let chat = |id: &str, title: &str, kind| FoundChat {
        id: id.into(),
        title: title.into(),
        kind,
    };
    assert_eq!(
        found,
        Ok(FoundChats {
            bot: "@acme_bot".into(),
            chats: vec![
                chat("6", "Bob", ChatKind::Person),
                chat("9", "Old group", ChatKind::Group),
                chat("-1003", "@news", ChatKind::Channel),
                chat("5", "Chat 5", ChatKind::Person),
                chat("111", "Ann Lee (@ann)", ChatKind::Person),
                chat("-1002", "Releases", ChatKind::Channel),
                chat("-1001", "QA team", ChatKind::Group),
            ]
        })
    );
    assert!(
        requests[0].starts_with("POST /bot123:tok/getMe "),
        "{}",
        requests[0]
    );
    assert!(
        requests[1].starts_with("POST /bot123:tok/getUpdates "),
        "{}",
        requests[1]
    );
    let asked = json_of(&requests[1]);
    assert_eq!(asked["limit"], 100);
    assert!(
        asked["allowed_updates"]
            .as_array()
            .unwrap()
            .contains(&json!("my_chat_member"))
    );
}

#[test]
fn a_bot_nobody_wrote_to_has_no_chats() {
    let (found, _) = find(vec![(200, ME), (200, r#"{"ok": true, "result": []}"#)]);
    assert_eq!(
        found,
        Ok(FoundChats {
            bot: "@acme_bot".into(),
            chats: Vec::new()
        })
    );
    let no_name = r#"{"ok": true, "result": {"id": 1, "first_name": "Acme"}}"#;
    let (found, _) = find(vec![(200, no_name), (200, r#"{"ok": true, "result": []}"#)]);
    assert_eq!(found.unwrap().bot, "Acme");
}

#[test]
fn a_wrong_token_or_a_webhook_is_explained() {
    let (found, requests) = find(vec![(
        401,
        r#"{"ok": false, "description": "Unauthorized"}"#,
    )]);
    assert_eq!(found, Err(failed("Telegram didn't accept the bot token")));
    assert_eq!(requests.len(), 1);

    let webhook = r#"{"ok": false, "error_code": 409, "description": "Conflict: can't use getUpdates method while webhook is active; use deleteWebhook to delete the webhook first"}"#;
    let Err(NotifyError::Failed { reason }) = find(vec![(200, ME), (409, webhook)]).0 else {
        panic!("a bot with a webhook has no updates to read");
    };
    assert!(
        reason.starts_with("This bot hands its messages to a webhook"),
        "{reason}"
    );
    assert!(!reason.contains("123:tok"));

    let busy = r#"{"ok": false, "error_code": 409, "description": "Conflict: terminated by other getUpdates request; make sure that only one bot instance is running"}"#;
    assert_eq!(
        find(vec![(200, ME), (409, busy)]).0,
        Err(failed(
            "Telegram: Conflict: terminated by other getUpdates request; make sure that only one bot instance is running"
        ))
    );
}
