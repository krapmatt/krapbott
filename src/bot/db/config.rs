use std::collections::HashMap;

use sqlx::PgPool;

use crate::bot::{commands::commands::BotResult, db::ChannelId, state::def::{BotConfig, BotError, ChannelConfig}};

pub const CONFIG_TABLE: &str = "CREATE TABLE IF NOT EXISTS krapbott_v2.channel_config (
        channel_id TEXT PRIMARY KEY,
        config JSONB NOT NULL
    );";

pub async fn load_bot_config_from_db(pool: &PgPool) -> BotResult<BotConfig> {
    let rows = sqlx::query!(
        r#"
        SELECT channel_id, config
        FROM krapbott_v2.channel_config
        "#
    ).fetch_all(pool).await?;

    let mut channels = HashMap::new();

    for row in rows {
        let channel_id: ChannelId = row.channel_id.parse()
            .map_err(|e| BotError::Custom(format!("Failed to parse channel_id: {e}")))?;

        let config: ChannelConfig = serde_json::from_value(row.config)
            .map_err(|e| BotError::Custom(format!("Invalid config for {channel_id}: {e}")))?;

        channels.insert(channel_id, config);
    }

    Ok(BotConfig { channels })
}

pub async fn save_channel_config(
    pool: &PgPool,
    channel_id: &ChannelId,
    config: &BotConfig,
) -> BotResult<()> {
    let cfg = config
        .channels
        .get(channel_id)
        .ok_or(BotError::ConfigMissing(channel_id.clone()))?;

    let json = serde_json::to_value(cfg)?;

    sqlx::query!(
        r#"
        INSERT INTO krapbott_v2.channel_config (channel_id, config)
        VALUES ($1, $2)
        ON CONFLICT (channel_id)
        DO UPDATE SET config = EXCLUDED.config
        "#,
        channel_id.as_str(),
        json
    ).execute(pool).await?;

    Ok(())
}