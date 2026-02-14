use std::{collections::HashSet, str::FromStr};

use crate::bot::{chat_event::chat_event::Platform, db::ChannelId, state::def::BotError};

pub fn get_cookie(cookies: &str, name: &str) -> Option<String> {
    cookies
        .split(';')
        .map(|c| c.trim())
        .find_map(|c| {
            let mut parts = c.splitn(2, '=');
            if parts.next()? == name {
                parts.next().map(|v| v.to_string())
            } else {
                None
            }
        })
}

pub fn platform_session_cookie(platform: Platform) -> &'static str {
    match platform {
        Platform::Twitch => "session_twitch",
        Platform::Kick => "session_kick",
        Platform::Obs => "session_obs",
    }
}

pub fn default_cookie_attributes() -> &'static str {
    "Path=/; HttpOnly; SameSite=None; Secure"
}

pub fn session_cookie_header(name: &str, value: &str) -> String {
    format!("{name}={value}; {}", default_cookie_attributes())
}

async fn channel_from_session_id(session_id: &str, pool: &sqlx::PgPool) -> Result<ChannelId, BotError> {
    let row = sqlx::query!(
        r#"
        SELECT platform, login
        FROM krapbott_v2.sessions
        WHERE session_id = $1
        "#,
        session_id
    )
    .fetch_optional(pool)
    .await?
    .ok_or(BotError::Custom("Invalid session".into()))?;

    let platform = Platform::from_str(&row.platform).map_err(|_| BotError::Custom("Invalid platform".into()))?;
    Ok(ChannelId::new(platform, row.login))
}

pub async fn channel_from_session(cookies: Option<String>, pool: &sqlx::PgPool) -> Result<ChannelId, BotError> {
    let cookies = cookies.ok_or(BotError::Custom("No cookies".into()))?;
    if let Some(session_id) = get_cookie(&cookies, "session_id") {
        if let Ok(channel) = channel_from_session_id(&session_id, pool).await {
            return Ok(channel);
        }
    }

    for platform in [Platform::Twitch, Platform::Kick] {
        if let Some(session_id) = get_cookie(&cookies, platform_session_cookie(platform)) {
            if let Ok(channel) = channel_from_session_id(&session_id, pool).await {
                return Ok(channel);
            }
        }
    }

    Err(BotError::Custom("No session".into()))
}

#[derive(Debug, Clone)]
pub struct WebSession {
    pub session_id: String,
    pub channel: ChannelId,
}

pub async fn sessions_from_cookies(cookies: Option<String>, pool: &sqlx::PgPool) -> Result<Vec<WebSession>, BotError> {
    let cookies = cookies.ok_or(BotError::Custom("No cookies".into()))?;
    let mut out = Vec::new();
    let mut seen_channels = HashSet::new();

    if let Some(session_id) = get_cookie(&cookies, "session_id") {
        if let Ok(channel) = channel_from_session_id(&session_id, pool).await {
            seen_channels.insert(channel.as_str().to_string());
            out.push(WebSession { session_id, channel });
        }
    }

    for platform in [Platform::Twitch, Platform::Kick] {
        let Some(session_id) = get_cookie(&cookies, platform_session_cookie(platform)) else {
            continue;
        };

        if let Ok(channel) = channel_from_session_id(&session_id, pool).await {
            if seen_channels.contains(channel.as_str()) {
                continue;
            }
            seen_channels.insert(channel.as_str().to_string());
            out.push(WebSession { session_id, channel });
        }
    }

    Ok(out)
}
