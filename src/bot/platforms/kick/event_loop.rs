use std::{sync::Arc, time::Duration};

use dashmap::DashMap;
use kick_rust::{ChatMessageEvent, KickApiClient, KickClient, RawMessage};
use serde_json::Value;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{info, warn};

use crate::bot::{
    chat_event::chat_event::{ChatEvent, Platform},
    commands::commands::BotResult,
    platforms::kick::kick::map_kick_msg,
    state::def::AppState,
};

pub async fn run_kick_loop(tx: UnboundedSender<ChatEvent>, state: Arc<AppState>) -> BotResult<()> {
    let channels: Vec<String> = {
        let config = state.config.read().await;
        config.channels.keys().filter(|id| id.platform() == Platform::Kick).map(|id| id.channel().to_string()).collect()
    };

    if channels.is_empty() {
        info!("No Kick channels configured");
        return Ok(());
    }

    for channel in channels {
        spawn_kick_channel(channel, tx.clone()).await?;
    }

    loop {
        tokio::time::sleep(Duration::from_secs(60)).await;
    }
}

pub async fn spawn_kick_channel(channel: String, tx: UnboundedSender<ChatEvent>) -> BotResult<()> {
    let tx_for_task = tx.clone();
    let channel_for_task = channel.clone();

    tokio::spawn(async move {
        let client = KickClient::new();
        let raw_cache: Arc<DashMap<String, String>> = Arc::new(DashMap::new());

        let raw_cache_for_raw = Arc::clone(&raw_cache);
        client
            .on_raw_message(move |raw: RawMessage| {
                if raw.event_type == "App\\Events\\ChatMessageEvent" {
                    if let Some(id) = extract_chat_message_id(&raw.raw_json) {
                        raw_cache_for_raw.insert(id, raw.raw_json.clone());
                    }
                }
            })
            .await;

        let tx_for_chat = tx_for_task.clone();
        let channel_for_chat = channel_for_task.clone();
        let raw_cache_for_chat = Arc::clone(&raw_cache);
        client
            .on_chat_message(move |msg: ChatMessageEvent| {
                let raw = raw_cache_for_chat.remove(&msg.id).map(|(_, v)| v);
                let mut event = map_kick_msg(msg, raw.as_deref());

                if event.channel.is_empty()
                    || event.channel == event.broadcaster_id.clone().unwrap_or_default()
                {
                    event.channel = channel_for_chat.clone();
                }

                info!(
                    "Kick [{}] {}: {}",
                    channel_for_chat,
                    event
                        .user
                        .as_ref()
                        .map(|u| u.name.display.as_str())
                        .unwrap_or("unknown"),
                    event.message
                );
                let _ = tx_for_chat.send(event);
            })
            .await;

        if let Err(e) = client.connect(&channel_for_task).await {
            warn!("Kick connect error for {}: {}", channel_for_task, e);
            return;
        }

        info!("Connected to Kick channel: {}", channel_for_task);

        loop {
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    });

    Ok(())
}

fn extract_chat_message_id(raw_json: &str) -> Option<String> {
    let value: Value = serde_json::from_str(raw_json).ok()?;
    let data = value.get("data")?;

    if let Some(id) = data.get("id").and_then(|v| v.as_str()) {
        return Some(id.to_string());
    }

    if let Some(inner_str) = data.as_str() {
        let inner: Value = serde_json::from_str(inner_str).ok()?;
        if let Some(id) = inner.get("id").and_then(|v| v.as_str()) {
            return Some(id.to_string());
        }
        if let Some(id) = inner
            .get("message")
            .and_then(|m| m.get("id"))
            .and_then(|v| v.as_str())
        {
            return Some(id.to_string());
        }
    }

    if let Some(id) = data
        .get("message")
        .and_then(|m| m.get("id"))
        .and_then(|v| v.as_str())
    {
        return Some(id.to_string());
    }

    warn!("Kick raw message missing chat id");
    None
}
