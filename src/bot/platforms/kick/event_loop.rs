use std::{sync::Arc, time::Duration};

use futures::{SinkExt, StreamExt};
use kick_rust::{KickEventData, MessageParser};
use serde_json::Value;
use tokio::sync::mpsc::UnboundedSender;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{info, warn};

use crate::api::kick_api::prime_broadcaster_user_id;
use crate::bot::{
    chat_event::chat_event::{ChatEvent, Platform},
    commands::commands::BotResult,
    platforms::kick::kick::map_kick_msg,
    state::def::{AppState, BotError},
};

pub async fn run_kick_loop(tx: UnboundedSender<ChatEvent>, state: Arc<AppState>) -> BotResult<()> {
    let channels: Vec<String> = {
        let config = state.config.read().await;
        config
            .channels
            .keys()
            .filter(|id| id.platform() == Platform::Kick)
            .map(|id| id.channel().to_string())
            .collect()
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
    tokio::spawn(async move {
        let chatroom_id = match fetch_chatroom_id_from_api(&channel).await {
            Ok(id) => id,
            Err(e) => {
                warn!("Kick [{}] failed to resolve chatroom id: {}", channel, e);
                return;
            }
        };

        if let Err(e) = run_kick_ws_reader(channel.clone(), chatroom_id, tx).await {
            warn!("Kick [{}] websocket reader stopped: {}", channel, e);
        }
    });

    Ok(())
}

async fn run_kick_ws_reader(
    channel: String,
    chatroom_id: u64,
    tx: UnboundedSender<ChatEvent>,
) -> BotResult<()> {
    let ws_url = "wss://ws-us2.pusher.com/app/32cbd69e4b950bf97679?protocol=7&client=js&version=8.4.0&flash=false";

    loop {
        let (mut ws, _response) = connect_async(ws_url)
            .await
            .map_err(|e| BotError::Custom(format!("Kick ws connect failed: {e}")))?;

        info!("Kick [{}] websocket connected", channel);
        let mut subscribed = false;

        while let Some(frame) = ws.next().await {
            match frame {
                Ok(Message::Text(text)) => {
                    let raw = text.to_string();

                    if !subscribed && is_pusher_connection_established(&raw) {
                        let sub_msg = serde_json::json!({
                            "event": "pusher:subscribe",
                            "data": {
                                "auth": "",
                                "channel": format!("chatrooms.{}.v2", chatroom_id)
                            }
                        })
                        .to_string();

                        if let Err(e) = ws.send(Message::Text(sub_msg.into())).await {
                            warn!("Kick [{}] subscribe send failed: {}", channel, e);
                            break;
                        }
                        subscribed = true;
                        info!("Connected to Kick channel: {}", channel);
                        continue;
                    }

                    match MessageParser::parse_message(&raw) {
                        Ok(Some(parsed)) => {
                            if let KickEventData::ChatMessage(chat_msg) = parsed.data {
                                prime_broadcaster_user_id(&channel, chat_msg.chatroom.channel_id);
                                if !chat_msg.chatroom.name.is_empty() {
                                    prime_broadcaster_user_id(
                                        &chat_msg.chatroom.name,
                                        chat_msg.chatroom.channel_id,
                                    );
                                }
                                let mut event = map_kick_msg(chat_msg, Some(&raw));
                                event.channel = channel.clone();

                                info!(
                                    "Kick [{}] {}: {}",
                                    channel,
                                    event
                                        .user
                                        .as_ref()
                                        .map(|u| u.name.display.as_str())
                                        .unwrap_or("unknown"),
                                    event.message
                                );
                                let _ = tx.send(event);
                            }
                        }
                        Ok(None) => {}
                        Err(_e) => {}
                    }
                }
                Ok(Message::Ping(payload)) => {
                    let _ = ws.send(Message::Pong(payload)).await;
                }
                Ok(Message::Close(_)) => {
                    warn!("Kick [{}] websocket closed", channel);
                    break;
                }
                Err(e) => {
                    warn!("Kick [{}] websocket frame error: {}", channel, e);
                    break;
                }
                _ => {}
            }
        }

        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}

async fn fetch_chatroom_id_from_api(channel: &str) -> BotResult<u64> {
    let url = format!("https://kick.com/api/v2/channels/{channel}");
    let client = reqwest::Client::new();
    let response = client
        .get(&url)
        .header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36",
        )
        .header("Accept", "application/json, text/plain, */*")
        .header("Referer", format!("https://kick.com/{channel}"))
        .send()
        .await?;

    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        return Err(BotError::Custom(format!(
            "Kick channel lookup failed ({status}): {body}"
        )));
    }

    let value: Value = serde_json::from_str(&body)?;
    let chatroom_id = value
        .get("chatroom")
        .and_then(|v| v.get("id"))
        .and_then(|v| v.as_u64())
        .ok_or_else(|| BotError::Custom("Kick response missing chatroom.id".to_string()))?;

    Ok(chatroom_id)
}

fn is_pusher_connection_established(raw: &str) -> bool {
    serde_json::from_str::<Value>(raw)
        .ok()
        .and_then(|v| v.get("event").and_then(|e| e.as_str()).map(|e| e == "pusher:connection_established"))
        .unwrap_or(false)
}
