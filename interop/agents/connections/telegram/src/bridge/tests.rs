use link_secretive::Duplicate;

use super::*;

/// The owner's direct chats are each allowed user's own, as `owner`
/// says them for Link to ask on.
#[test]
fn the_owners_chats_are_the_allowed_users_own() {
    let bridge = Bridge {
        settings: json!({"bot_token": "1:x", "allowed_users": [42, 77]}).into(),
        ..Bridge::default()
    };
    assert_eq!(owner(&bridge).unwrap(), ["42", "77"]);
}

/// An edit on the way, told to wait, doesn't: it fails at once, and
/// the bot holds its edits until then. Only the whole answer's waits.
#[tokio::test]
async fn an_edit_on_the_way_doesnt_wait_out_a_rate_limit() {
    let told = axum::Router::new().fallback(|| async {
        axum::Json(json!({
            "ok": false,
            "error_code": 429,
            "description": "Too Many Requests: retry after 30",
            "parameters": {"retry_after": 30}
        }))
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let api = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, told).await });
    // SAFETY: no other test of this binary reads it.
    unsafe { std::env::set_var("LNK_TELEGRAM_API", &api) };
    let bot = Bot::new(&link_secretive::Secret::from("1:x".to_string()));
    let edit = json!({"chat_id": 42, "message_id": 1, "text": "so far"});
    let started = Instant::now();
    assert!(
        bot.call_waiting("editMessageText", edit, false)
            .await
            .is_err()
    );
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(bot.held(), "edits held until Telegram's time");
}

/// A message nobody asked for goes to an allowed user's own chat
/// alone: a group's, or anyone else's, is refused before Telegram is
/// asked.
#[tokio::test]
async fn send_refuses_a_chat_not_the_owners() {
    let bridge = Bridge {
        settings: json!({"bot_token": "1:x", "allowed_users": [42]}).into(),
        ..Bridge::default()
    };
    for chat in ["43", "-1001234", "me"] {
        let err = send(bridge.duplicate(), chat.into(), "hi".into()).await;
        let err = format!("{:#}", err.unwrap_err());
        assert!(
            err.contains("not sending there") || err.contains("is a number"),
            "{chat}: {err}"
        );
    }
}

fn me() -> Me {
    Me {
        id: 123456,
        username: "link_main_bot".into(),
    }
}

#[test]
fn only_the_owners_direct_messages() {
    let message = |from: i64, chat: &str, is_bot: bool| {
        json!({"from": {"id": from, "is_bot": is_bot}, "chat": {"id": from, "type": chat},
            "text": "hi"})
    };
    assert_eq!(
        owners(&message(42, "private", false), &[42], &me()),
        Some(Message {
            question_reply: None,
            event_id: String::new(),
            chat: 42,
            topic: None,
            text: Some("hi".into()),
            files: vec![],
            reply_to: None,
            from: None,
        })
    );
    assert!(owners(&message(43, "private", false), &[42], &me()).is_none());
    assert!(owners(&message(42, "group", false), &[42], &me()).is_none());
    assert!(owners(&message(42, "private", true), &[42], &me()).is_none());
    assert!(owners(&json!({}), &[42], &me()).is_none());
}

#[test]
fn a_photo_or_document_is_held_by_its_file() {
    let photo = json!({"from": {"id": 42}, "chat": {"id": 42, "type": "private"},
        "caption": "what's this?", "photo": [
            {"file_id": "small", "file_size": 900},
            {"file_id": "large", "file_size": 90000}]});
    let held = owners(&photo, &[42], &me()).unwrap();
    assert_eq!(held.text.as_deref(), Some("what's this?"));
    assert_eq!(
        held.files,
        vec![connection::Held {
            media: "image/jpeg".into(),
            name: None,
            size: Some(90000),
            at: "large".into(),
        }]
    );
    let document = json!({"from": {"id": 42}, "chat": {"id": 42, "type": "private"},
        "document": {"file_id": "d", "mime_type": "application/pdf",
            "file_name": "a.pdf", "file_size": 10}});
    let held = owners(&document, &[42], &me()).unwrap();
    assert_eq!(held.text, None);
    assert_eq!(held.files[0].name.as_deref(), Some("a.pdf"));
    assert_eq!(held.files[0].media, "application/pdf");
    // A voice note, Telegram's Ogg.
    let voice = json!({"from": {"id": 42}, "chat": {"id": 42, "type": "private"},
        "voice": {"file_id": "v", "duration": 2, "mime_type": "audio/ogg", "file_size": 3000}});
    let held = owners(&voice, &[42], &me()).unwrap();
    assert_eq!(
        held.files,
        vec![connection::Held {
            media: "audio/ogg".into(),
            name: None,
            size: Some(3000),
            at: "v".into(),
        }]
    );
}

#[test]
fn shared_members_and_sharing_are_separate_permissions() {
    let owners = [42];
    let shared = ["-1001234".into()];
    let mut message = json!({"message_id": 7, "from": {"id": 43, "first_name": "Alice", "last_name": "Jones", "is_bot": false},
        "chat": {"id": -1001234, "type": "supergroup"}, "text": "@link_main_bot K7Q2M9XA",
        "entities": [{"type": "mention", "offset": 0, "length": 14}]});
    let code = link_secretive::Secret::new("K7Q2M9XA".to_string());
    let member = carried(&message, &owners, &shared, &me())
        .unwrap()
        .from
        .unwrap();
    assert_eq!(member.person.as_str(), "member:telegram:43");
    assert_eq!(member.name.as_deref(), Some("Alice Jones"));
    assert!(carried(&message, &owners, &[], &me()).is_none());
    assert!(shared_code(&message, &owners, &me(), &code).is_none());
    message["from"]["id"] = json!(42);
    assert!(shared_code(&message, &owners, &me(), &code).is_some());
    assert!(
        shared_code(
            &message,
            &owners,
            &me(),
            &link_secretive::Secret::new("WRONG".into())
        )
        .is_none()
    );
    for (field, value) in [
        ("from", json!({"id": 43, "is_bot": true})),
        ("sender_chat", json!({"id": -42})),
        ("chat", json!({"id": 43, "type": "private"})),
    ] {
        let mut refused = message.clone();
        refused["from"]["id"] = json!(43);
        refused[field] = value;
        assert!(
            carried(&refused, &owners, &shared, &me()).is_none(),
            "{field}"
        );
    }
    message["from"]["id"] = json!(43);
    message["text"] = json!("reply");
    message["entities"] = json!([]);
    message["reply_to_message"] = json!({"from": {"id": 123456}});
    assert!(carried(&message, &owners, &shared, &me()).is_some());
    assert!(shared_code(&message, &owners, &me(), &code).is_none());
}

#[test]
fn the_owners_mention_in_a_group_is_carried_without_it() {
    let mention = |from: i64, chat: &str, text: &str, offset: u64| {
        json!({"message_id": 7, "from": {"id": from, "is_bot": false},
            "chat": {"id": -1001234567890i64, "type": chat}, "text": text,
            "entities": [{"type": "mention", "offset": offset, "length": 14}]})
    };
    let carried = |text: &str| {
        Some(Message {
            question_reply: None,
            event_id: "7".into(),
            chat: -1001234567890,
            topic: None,
            text: Some(text.into()),
            files: vec![],
            reply_to: Some(7),
            from: None,
        })
    };
    let hi = "@link_main_bot hi";
    assert_eq!(
        owners(&mention(42, "group", hi, 0), &[42], &me()),
        carried("hi")
    );
    assert_eq!(
        owners(&mention(42, "supergroup", hi, 0), &[42], &me()),
        carried("hi")
    );
    // Offsets count UTF-16 units: an emoji is two.
    let emoji = "😀 @Link_Main_Bot hi";
    assert_eq!(
        owners(&mention(42, "group", emoji, 3), &[42], &me()),
        carried("😀  hi")
    );
    // The mention alone leaves nothing to carry.
    let alone = "@link_main_bot ";
    assert!(owners(&mention(42, "group", alone, 0), &[42], &me()).is_none());
    // Someone else mentioned, a stranger, a channel's post.
    let other = "@someone_else1 hi";
    assert!(owners(&mention(42, "group", other, 0), &[42], &me()).is_none());
    assert!(owners(&mention(43, "group", hi, 0), &[42], &me()).is_none());
    assert!(owners(&mention(42, "channel", hi, 0), &[42], &me()).is_none());
    // The name in the text without a mention entity isn't one.
    let mut plain = mention(42, "group", hi, 0);
    plain["entities"] = json!([]);
    assert!(owners(&plain, &[42], &me()).is_none());
    // A reply to the bot's message is addressed to it, and kept whole.
    plain["reply_to_message"] = json!({"from": {"id": 123456, "is_bot": true}});
    assert_eq!(owners(&plain, &[42], &me()), carried(hi));
    // A mention in a photo's caption.
    let caption = json!({"message_id": 7, "from": {"id": 42, "is_bot": false},
        "chat": {"id": -1001234567890i64, "type": "group"}, "caption": hi,
        "caption_entities": [{"type": "mention", "offset": 0, "length": 14}]});
    assert_eq!(owners(&caption, &[42], &me()), carried("hi"));
    // Without a username from getMe, nothing is a mention.
    let nameless = Me::default();
    assert!(owners(&mention(42, "group", hi, 0), &[42], &nameless).is_none());
    // In a forum, the topic it's in; a reply's thread in a group that
    // isn't one is no topic.
    let mut topic = mention(42, "supergroup", hi, 0);
    topic["message_thread_id"] = json!(5);
    assert_eq!(owners(&topic, &[42], &me()), carried("hi"));
    topic["is_topic_message"] = json!(true);
    let in_topic = owners(&topic, &[42], &me()).unwrap();
    assert_eq!(in_topic.topic, Some(5));
    assert_eq!(
        thread(in_topic.chat, in_topic.topic),
        "telegram:-1001234567890.5"
    );
}

#[test]
fn only_the_bots_own_mentions_are_taken_out() {
    let entities = json!([
        {"type": "mention", "offset": 0, "length": 14},
        {"type": "mention", "offset": 19, "length": 6},
        {"type": "mention", "offset": 30, "length": 14}]);
    let text = "@link_main_bot ask @alice and @LINK_MAIN_BOT";
    assert_eq!(without_mention(text, &entities, &me()), "ask @alice and");
    // An entity past the text marks nothing.
    let odd = json!([{"type": "mention", "offset": 40, "length": 14}]);
    assert_eq!(without_mention(" hi ", &odd, &me()), "hi");
}

#[test]
fn a_thread_is_the_chats_id() {
    assert_eq!(thread(42, None), "telegram:42");
    assert_eq!(thread(-1001234567890, None), "telegram:-1001234567890");
    // A forum's topic is a conversation of its own, as send takes it.
    assert_eq!(thread(-1001234567890, Some(7)), "telegram:-1001234567890.7");
    assert_eq!(place("-1001234567890.7"), Some((-1001234567890, Some(7))));
    assert_eq!(place("42"), Some((42, None)));
    for bad in ["", "me", "42.", ".7", "42.x", "42.7.1", "-42.-7", "-42.0"] {
        assert_eq!(place(bad), None, "{bad}");
    }
}
