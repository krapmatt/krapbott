use dashmap::DashMap;
use once_cell::sync::Lazy;
use serde_json::json;

use kick_rust::KickApiClient;

use crate::bot::{commands::commands::BotResult, state::def::BotError};

static BROADCASTER_CACHE: Lazy<DashMap<String, u64>> = Lazy::new(DashMap::new);

pub async fn send_kick_message(channel_slug: &str, content: &str, access_token: &str) -> BotResult<()> {
    let content = content.trim();
    if content.is_empty() {
        return Ok(());
    }

    let content = truncate_message(content, 500);
    let broadcaster_user_id = get_broadcaster_user_id(channel_slug).await?;

    let body = json!({
        "type": "bot",
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
            "Kick send failed ({status}): {text}"
        )));
    }

    Ok(())
}

async fn get_broadcaster_user_id(channel_slug: &str) -> BotResult<u64> {
    if let Some(id) = BROADCASTER_CACHE.get(channel_slug).map(|v| *v) {
        return Ok(id);
    }

    let api = KickApiClient::new().map_err(|e| {
        BotError::Custom(format!("Kick API client error: {e}"))
    })?;

    let channel = api
        .get_channel(channel_slug)
        .await
        .map_err(|e| BotError::Custom(format!("Kick channel lookup failed: {e}")))?;

    let user = channel.user.ok_or_else(|| {
        BotError::Custom("Kick channel missing user info".to_string())
    })?;

    BROADCASTER_CACHE.insert(channel_slug.to_string(), user.id);
    Ok(user.id)
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
