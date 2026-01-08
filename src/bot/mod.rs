use std::sync::Arc;

use sqlx::PgPool;
use tracing::info;

use crate::bot::{chat_event::chat_event::ChatEvent, commands::commands::BotResult, handler::handler::{handle_event, init_bot_runtime}, state::def::AppState};

pub mod state;
pub mod chat_event;
pub mod dispatcher;
pub mod commands;
pub mod platforms;
pub mod permissions;
pub mod db;
pub mod handler;
pub mod runtime;
pub mod web;
pub mod scheduler;

pub async fn run_event_loop(pool: PgPool, state: Arc<AppState>, mut rx: tokio::sync::mpsc::UnboundedReceiver<ChatEvent>) -> BotResult<()> {
    init_bot_runtime(state.clone(), &pool).await?;

    while let Some(mut event) = rx.recv().await {
        let pool = pool.clone();
        let state = state.clone();

        if let Err(e) = handle_event(&mut event, pool.clone(), state.clone()).await{
            tracing::error!("Event error: {e:?}");
        }
    }

    Ok(())
}