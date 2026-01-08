use std::sync::Arc;

use sqlx::PgPool;

use crate::bot::{commands::{CommandRegistry, commands::BotResult}, db::{ChannelId, aliases::fetch_aliases_from_db, config::load_bot_config_from_db}, dispatcher::dispatcher::build_dispatcher_for_channel, runtime::channel_runtime::ChannelRuntime, state::def::AppState};

pub async fn start_channel(channel_id: ChannelId, state: Arc<AppState>, pool: &PgPool) -> BotResult<()> {
    let aliases = fetch_aliases_from_db(&channel_id, pool).await?;
    {
        let mut cfg = state.runtime.alias_config.write().await;
        *cfg = aliases;

    }
    // Build dispatcher
    let dispatcher =
        build_dispatcher_for_channel(&channel_id, state.clone(), &state.registry).await?;

    let mut runtime = ChannelRuntime::new(dispatcher);

    // (Later) attach per-channel tasks here
    // Example:
    // let task = start_points_loop(channel_id.clone(), state.clone());
    // runtime.add_task(task);

    state.runtime.dispatchers.write().await.insert(channel_id, runtime);

    Ok(())
}

pub async fn stop_channel(channel_id: &ChannelId, state: Arc<AppState>) -> BotResult<()> {
    if let Some(runtime) = state.runtime.dispatchers.write().await.remove(channel_id) {
        runtime.shutdown();
    }

    Ok(())
}

pub async fn reload_channel(channel_id: ChannelId, state: Arc<AppState>, pool: &PgPool) -> BotResult<()> {
    stop_channel(&channel_id, state.clone()).await?;
    start_channel(channel_id, state, pool).await?;
    Ok(())
}

pub async fn start_channels_from_config(state: Arc<AppState>, pool: &PgPool) -> BotResult<()> {
    let channel_ids: Vec<ChannelId> = {
        let cfg = state.config.read().await;
        cfg.channels.keys().cloned().collect()
    };

    for channel_id in channel_ids {
        if let Err(e) = start_channel(channel_id, state.clone(), pool).await {
            tracing::error!("Failed to start channel: {e:?}");
        }
    }

    Ok(())
}