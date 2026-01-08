use std::sync::Arc;

use sqlx::PgPool;

use crate::bot::{commands::commands::BotResult, db::{ChannelId, config::save_channel_config}, runtime::channel_lifecycle::start_channel, state::def::{AppState, ChannelConfig}};

pub mod commands;

pub async fn connect_channel(channel_id: ChannelId, state: Arc<AppState>, pool: &PgPool) -> BotResult<()> {
    //Update in-memory config
    {
        let mut cfg = state.config.write().await;
        cfg.channels.entry(channel_id.clone()).or_insert_with(|| {
            ChannelConfig::new(channel_id.clone())
        });
        save_channel_config(pool, &channel_id, &cfg).await?;
    }

    // Start runtime (dispatcher, tasks)
    start_channel(channel_id.clone(), state.clone(), pool).await?;

    // Join platform chat
    state.chat_client.join_channel(&channel_id).await?;

    Ok(())
}