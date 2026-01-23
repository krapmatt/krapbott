use std::{collections::{HashMap, HashSet}, io, sync::Arc, time::Instant};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::{sync::{RwLock, broadcast::error::SendError}};
use twitch_irc::{login::StaticLoginCredentials, transport::tcp::{TCPTransport, TLS}, validate};

use crate::bot::{commands::{CommandRegistry, queue::logic::QueueKey}, db::ChannelId, dispatcher::dispatcher::DispatcherCache, handler::handler::UnifiedChatClient, web::sse::{SseBus, SseEvent}};

pub struct AppState {
    pub secrets: Arc<BotSecrets>,
    pub config: Arc<RwLock<BotConfig>>,
    pub runtime: Arc<BotRuntime>,
    pub chat_client: Arc<UnifiedChatClient>,
    pub registry: Arc<CommandRegistry>,
    pub sse_bus: SseBus,
    pub twitch_auth: Arc<RwLock<TwitchAppToken>>,
}

pub struct TwitchAppToken {
    pub access_token: String,
    pub expires_at: Instant
}

pub struct BotSecrets {
    pub bot_id: String,
    pub x_api_key: String,
    pub client_secret: String,
    pub user_access_token: String,
}

pub struct BotRuntime {
    pub dispatchers: RwLock<DispatcherCache>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct BotConfig {
    pub channels: HashMap<ChannelId, ChannelConfig>, // Holds configuration for all channels
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ChannelConfig {
    //Stav Queue
    #[serde(default)]
    pub open: bool,
    #[serde(default)]
    pub size: usize,
    #[serde(default)]
    pub teamsize: usize,
    pub queue_target: QueueKey,
    #[serde(default)]
    pub random_queue: bool,
    //Které commandy jsou povolené
    #[serde(default)]
    pub packages: Vec<String>,
    //Statistiky
    #[serde(default)]
    pub runs: usize,
    //Nastavení příkazu
    #[serde(default = "default_prefix")]
    pub prefix: String,
}

fn default_prefix() -> String {
    "!".into()
}

#[derive(Debug, Error)]
pub enum BotError {
    #[error("HTTP request error: {0}")]
    RequestError(#[from] reqwest::Error),
    #[error("JSON deserialization error: {0}")]
    JsonError(#[from] serde_json::Error),
    #[error("I/O error: {0}")]
    IoError(#[from] io::Error),
    #[error("Database error: {0}")]
    SqlxError(#[from] sqlx::Error),
    #[error("TwitchIRC Error: {0}")]
    TwitchIrc(#[from] twitch_irc::Error<TCPTransport<TLS>, StaticLoginCredentials>),
    #[error("Validate Error: {0}")]
    Validate(#[from] validate::Error),
    #[error("Configuration missing for channel: {0}")]
    ConfigMissing(ChannelId),
    #[error("Send Error: {0}")]
    SendError(#[from] SendError<SseEvent>),
    #[error("{0}")]
    Chat(String),
    #[error("{0}")]
    Custom(String),

}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AliasConfig {
    pub aliases: HashMap<String, String>,
    pub disabled_commands: HashSet<String>,
    pub removed_aliases: HashSet<String>,
}

#[derive(serde::Serialize, Debug)]
pub struct ObsQueueEntry {
    pub position: i32,
    pub display_name: String,
    pub bungie_name: String,
    pub user_id: String
}

