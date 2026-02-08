use dashmap::DashMap;
use once_cell::sync::Lazy;
use serde_json::json;
use serde_json::Value;

use crate::bot::{commands::commands::BotResult, state::def::BotError};

static BROADCASTER_CACHE: Lazy<DashMap<String, u64>> = Lazy::new(DashMap::new);

pub fn prime_broadcaster_user_id(channel_slug: &str, broadcaster_user_id: u64) {
    let key = normalize_channel_slug(channel_slug);
    BROADCASTER_CACHE.insert(key, broadcaster_user_id);
}

pub async fn send_kick_message(channel_slug: &str, content: &str, access_token: String) -> BotResult<()> {
    let content = content.trim();
    if content.is_empty() {
        return Ok(());
    }

    let content = truncate_message(content, 500);
    let key = normalize_channel_slug(channel_slug);
    let broadcaster_user_id = get_broadcaster_user_id(&key).await?;
    let mut result = post_kick_chat_user(&content, broadcaster_user_id, &access_token).await;

    // Kick can return generic 500s for stale/incorrect broadcaster ids.
    // Drop cache and retry once with a fresh lookup before surfacing the error.
    if matches!(result, Err(BotError::Custom(ref msg)) if msg.contains("Kick send failed (500")) {
        BROADCASTER_CACHE.remove(&key);
        let fresh_id = get_broadcaster_user_id(&key).await?;
        result = post_kick_chat_user(&content, fresh_id, &access_token).await;
    }

    // Kick behavior is inconsistent across token types.
    // If user payload still fails with 500, fallback once to bot payload.
    if matches!(result, Err(BotError::Custom(ref msg)) if msg.contains("Kick send failed (500")) {
        result = post_kick_chat_bot(&content, &access_token).await;
    }

    result
}

async fn post_kick_chat_user(content: &str, broadcaster_user_id: u64, access_token: &str) -> BotResult<()> {
    let body = json!({
        "type": "user",
        "content": content,
        "broadcaster_user_id": broadcaster_user_id
    });

    let client = reqwest::Client::new();
    let response = client
        .post("https://api.kick.com/public/v1/chat")
        .bearer_auth(access_token)
        .json(&body)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(BotError::Custom(format!(
            "Kick send failed (user payload) ({status}): {text}"
        )));
    }

    Ok(())
}

async fn post_kick_chat_bot(content: &str, access_token: &str) -> BotResult<()> {
    let body = json!({
        "type": "bot",
        "content": content
    });

    let client = reqwest::Client::new();
    let response = client
        .post("https://api.kick.com/public/v1/chat")
        .bearer_auth(access_token)
        .json(&body)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(BotError::Custom(format!(
            "Kick send failed (bot payload) ({status}): {text}"
        )));
    }

    Ok(())
}

async fn get_broadcaster_user_id(channel_slug: &str) -> BotResult<u64> {
    let key = normalize_channel_slug(channel_slug);

    if let Some(id) = BROADCASTER_CACHE.get(&key).map(|v| *v) {
        return Ok(id);
    }

    let id = fetch_broadcaster_user_id_from_public_api(&key).await?;
    BROADCASTER_CACHE.insert(key, id);
    Ok(id)
}

async fn fetch_broadcaster_user_id_from_public_api(channel_slug: &str) -> BotResult<u64> {
    let url = format!("https://kick.com/api/v2/channels/{channel_slug}");
    let client = reqwest::Client::new();
    let response = client
        .get(url)
        .header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36",
        )
        .header("Accept", "application/json, text/plain, */*")
        .header("Referer", format!("https://kick.com/{channel_slug}"))
        .send()
        .await
        .map_err(|e| BotError::Custom(format!("Kick channel lookup failed: {e}")))?;

    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(BotError::Custom(format!(
            "Kick channel lookup failed ({status}): {body}"
        )));
    }

    let value: Value = serde_json::from_str(&body)?;
    let broadcaster_user_id = value
        .get("user")
        .and_then(|u| u.get("id"))
        .and_then(|v| v.as_u64())
        .ok_or_else(|| BotError::Custom("Kick response missing user.id for broadcaster".to_string()))?;

    Ok(broadcaster_user_id)
}

fn truncate_message(input: &str, max_len: usize) -> String {
    let mut out = String::new();
    let mut count = 0usize;
    for ch in input.chars() {
        if count >= max_len {
            break;
        }
        out.push(ch);
        count += 1;
    }
    out
}

fn normalize_channel_slug(channel_slug: &str) -> String {
    channel_slug
        .trim()
        .trim_start_matches('@')
        .to_ascii_lowercase()
}
