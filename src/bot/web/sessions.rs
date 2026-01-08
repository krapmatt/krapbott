use std::str::FromStr;

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

pub async fn channel_from_session(cookies: Option<String>, pool: &sqlx::PgPool) -> Result<ChannelId, BotError> {
    let cookies = cookies.ok_or(BotError::Custom("No cookies".into()))?;
    let session_id = get_cookie(&cookies, "session_id")
        .ok_or(BotError::Custom("No session".into()))?;

    let row = sqlx::query!(
        r#"
        SELECT platform, login
        FROM krapbott_v2.sessions
        WHERE session_id = $1
        "#,
        session_id
    ).fetch_optional(pool).await?.ok_or(BotError::Custom("Invalid session".into()))?;

    let platform = Platform::from_str(&row.platform).map_err(|_| BotError::Custom("Invalid platform".into()))?;

    Ok(ChannelId::new(platform, row.login))
}