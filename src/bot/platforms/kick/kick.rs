use kick_rust::ChatMessageEvent;
use serde_json::Value;
use tracing::info;

use crate::bot::{
    chat_event::chat_event::{ChatEvent, ChatUser, DisplayName, Platform, UserIdentity},
    permissions::permissions::PermissionLevel,
};

pub fn map_kick_msg(msg: ChatMessageEvent, raw_json: Option<&str>) -> ChatEvent {
    info!("Received Kick message: {:?}", raw_json);
    let permission = raw_json
        .and_then(extract_permission_from_raw)
        .unwrap_or(PermissionLevel::Follower);
    info!("Extracted permission: {:?}", permission);
    let display = msg
        .sender
        .display_name
        .clone()
        .unwrap_or_else(|| msg.sender.username.clone());

    let channel = if msg.chatroom.name.is_empty() {
        msg.chatroom.channel_id.to_string()
    } else {
        msg.chatroom.name.clone()
    };

    ChatEvent {
        platform: Platform::Kick,
        channel,
        message: msg.content.clone(),
        broadcaster_id: Some(msg.chatroom.channel_id.to_string()),
        user: Some(ChatUser {
            identity: UserIdentity {
                platform: Platform::Kick,
                platform_user_id: msg.sender.id.to_string(),
            },
            name: DisplayName {
                login: msg.sender.username.clone(),
                display,
            },
            permission,
        }),
        follower: None,
    }
}

fn extract_permission_from_raw(raw_json: &str) -> Option<PermissionLevel> {
    let mut value: Value = serde_json::from_str(raw_json).ok()?;
    decode_embedded_data_json(&mut value);
    let badges = extract_badge_types(&value);
    if badges.is_empty() {
        return None;
    }

    Some(permission_from_badges(&badges))
}

fn decode_embedded_data_json(root: &mut Value) {
    let Some(obj) = root.as_object_mut() else {
        return;
    };

    let Some(data_str) = obj.get("data").and_then(|v| v.as_str()) else {
        return;
    };

    let Ok(parsed_data) = serde_json::from_str::<Value>(data_str) else {
        return;
    };

    obj.insert("data".to_string(), parsed_data);
}

fn permission_from_badges(badges: &[String]) -> PermissionLevel {
    let has = |needle: &str| badges.iter().any(|b| b == needle);

    if has("broadcaster") {
        PermissionLevel::Broadcaster
    } else if has("moderator") {
        PermissionLevel::Moderator
    } else if has("vip") {
        PermissionLevel::Vip
    } else if has("subscriber") {
        PermissionLevel::Subscriber
    } else {
        PermissionLevel::Everyone
    }
}

fn extract_badge_types(root: &Value) -> Vec<String> {
    let candidates = [
        &["data", "sender", "identity", "badges"][..],
        &["data", "sender", "badges"][..],
        &["data", "sender", "roles"][..],
    ];

    for path in candidates {
        if let Some(Value::Array(items)) = get_path(root, path) {
            let mut out = Vec::new();
            for item in items {
                if let Some(s) = item.as_str() {
                    out.push(s.to_ascii_lowercase());
                    continue;
                }
                if let Some(obj) = item.as_object() {
                    if let Some(s) = obj.get("type").and_then(|v| v.as_str()) {
                        out.push(s.to_ascii_lowercase());
                        continue;
                    }
                    if let Some(s) = obj.get("name").and_then(|v| v.as_str()) {
                        out.push(s.to_ascii_lowercase());
                        continue;
                    }
                    if let Some(s) = obj.get("badge").and_then(|v| v.as_str()) {
                        out.push(s.to_ascii_lowercase());
                        continue;
                    }
                }
            }
            if !out.is_empty() {
                return out;
            }
        }
    }

    Vec::new()
}

fn get_path<'a>(root: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut current = root;
    for key in path {
        current = current.get(*key)?;
    }
    Some(current)
}
