use std::{collections::HashMap, time::Instant};
use crate::{api::twitch_api::create_twitch_app_token, bot::{chat_event::chat_event::Platform, commands::{commands::BotResult, queue::logic::QueueKey}, db::ChannelId, state::def::{AliasConfig, AppState, BotConfig, BotError, BotSecrets, ChannelConfig}, web::obs::ObsCommandInfo}};


impl ChannelConfig {
    pub fn new(channel_id: ChannelId) -> Self {
        ChannelConfig { 
            open: false,
            queue_target: QueueKey::Single(channel_id),
            size: 1, 
            teamsize: 1, 
            packages: vec!["moderation".to_string()], 
            runs: 0,   
            prefix: "!".to_string(), 
            random_queue: false,
        }
    }
}

impl BotSecrets {
    pub fn from_env() -> BotResult<Self> {
        Ok(Self {
            bot_id: std::env::var("TWITCH_CLIENT_ID")?,
            client_secret: std::env::var("CLIENT_SECRET")?,
            user_access_token: std::env::var("TWITCH_USER_ACCESS_TOKEN")?,
            x_api_key: std::env::var("XAPIKEY")?,
            kick_access_token: std::env::var("KICK_ACCESS_TOKEN").ok(),
            kick_refresh_token: std::env::var("KICK_REFRESH_TOKEN").ok(),
            kick_client_id: std::env::var("KICK_CLIENT_ID").ok(),
            kick_client_secret: std::env::var("KICK_CLIENT_SECRET").ok(),
            kick_redirect_uri: std::env::var("KICK_REDIRECT_URI").ok(),
        })
    }

}
impl BotConfig {
    pub fn new() -> Self {
        let mut hash = HashMap::new();
        hash.insert(ChannelId::new(Platform::Twitch, "krapmatt".to_string()), ChannelConfig {open: true, size: 1, teamsize: 2, packages: vec!["queue".to_string()], runs: 0, queue_target: QueueKey::Single(ChannelId::new(Platform::Twitch, "krapmatt".to_string())), prefix: "!".to_string(), random_queue: false });
        BotConfig {
            channels: hash,
        }
    }

    pub fn get_channel_config(&self, channel_id: &ChannelId) -> Option<&ChannelConfig> {
        self.channels.get(channel_id)
    }

    pub fn get_channel_config_mut(&mut self, key: ChannelId) -> &mut ChannelConfig {
        self.channels.entry(key.clone()).or_insert_with(|| ChannelConfig::new(key))
    }

    pub fn is_group_allowed(&self, channel_id: &ChannelId, group_name: &str) -> bool {
        if let Some(channel_config) = self.channels.get(channel_id) {
            channel_config.packages.contains(&group_name.to_string())
        } else {
            false
        }
    }
}

impl From<()> for BotError {
    fn from(_: ()) -> Self {
        BotError::Custom("unit error".to_string())
    }
}
impl BotError {
    pub fn chat(msg: impl Into<String>) -> Self {
        BotError::Chat(msg.into())
    }
}

impl AliasConfig {
    pub fn get_aliases(&self, name: &str) -> Vec<String> {
        self.aliases.iter().filter_map(|(key, val)| if val == name { Some(key.to_owned()) } else { None }).collect()
    }
    pub fn get_removed_aliases(&self, name: &str) -> bool {
        self.removed_aliases.get(name).is_some()
    }
}

pub async fn get_twitch_access_token(state: &AppState) -> BotResult<String> {
    {
        let auth = state.twitch_auth.read().await;
        if auth.expires_at > Instant::now() {
            return Ok(auth.access_token.clone());
        }
    }

    // Expired → refresh (write lock)
    let mut auth = state.twitch_auth.write().await;

    // Double-check after acquiring write lock
    if auth.expires_at > Instant::now() {
        return Ok(auth.access_token.clone());
    }

    let new_token = create_twitch_app_token(&state.secrets).await?;
    *auth = new_token;

    Ok(auth.access_token.clone())
}

impl AppState {
    pub fn all_commands_for_obs(&self) -> Vec<ObsCommandInfo> {
        self.registry
            .groups
            .values()
            .flat_map(|g| g.commands.iter())
            .map(|reg| ObsCommandInfo {
                name: reg.command.name().to_string(),
                description: reg.command.description().to_string(),
                default_aliases: reg.aliases.clone(),
            }).collect()
    }
}
