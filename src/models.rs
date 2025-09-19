use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use thiserror::Error;
use tracing::error;
use twitch_irc::{login::StaticLoginCredentials, message::PrivmsgMessage, transport::tcp::{TCPTransport, TLS}, validate};
use std::{
    collections::{HashMap, HashSet}, fs::File, io::{self, Read, Write}, path::Path, sync::Arc, time::{Duration, SystemTime, UNIX_EPOCH}
};

use crate::{bot::TwitchClient, commands::points_traits::Giveaway, twitch_api::is_follower};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TwitchUser {
    pub twitch_name: String,
    pub bungie_name: String,
}

impl Default for TwitchUser {
    fn default() -> Self {
        TwitchUser {
            twitch_name: String::new(),
            bungie_name: String::new(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SharedState {
    pub run_count: usize,
}

impl SharedState {
    pub fn new() -> Self {
        Self { run_count: 0 }
    }

    pub fn add_stats(&mut self, run_count: usize) {
        self.run_count = run_count
    }
}

#[derive(Clone, Copy)]
pub enum CommandAction {
    Add,
    Remove,
    AddGlobal,
}

#[derive(Clone, Copy, Deserialize)]
pub enum PermissionLevel {
    User,
    Follower,
    Vip,
    Moderator,
    Broadcaster,
}
pub const ADMINS: &[&str] = &["KrapMatt", "ThatJK", "Samoan_317"];

/// Generic permission checker for Twitch roles
async fn has_perm(msg: &PrivmsgMessage, client: &TwitchClient, allowed_roles: &[&str], error_message: &str) -> bool {
    let user_name = msg.sender.name.to_lowercase();

    let has_role = msg.badges.iter().any(|badge| allowed_roles.contains(&badge.name.as_str()))
        || ADMINS.contains(&user_name.as_str());

    if has_role {
        true
    } else {
        let _ = client.say(msg.channel_login.clone(), error_message.to_string()).await;
        false
    }
}

pub async fn is_broadcaster(msg: &PrivmsgMessage, client: &TwitchClient) -> bool {
    has_perm(msg, client, &["broadcaster"],"You are not a broadcaster. You can't use this command").await
}


pub async fn is_subscriber(msg: &PrivmsgMessage, client: &TwitchClient) -> bool {
    has_perm(msg, client, &["moderator", "subscriber"],"You are not a sub. You can't use this command").await
}

pub async fn is_moderator(msg: &PrivmsgMessage, client: &TwitchClient) -> bool {
    has_perm(msg, client, &["moderator", "broadcaster"],"You are not a moderator/broadcaster. You can't use this command").await
}

pub async fn is_vip(msg: &PrivmsgMessage, client: &TwitchClient) -> bool {
    has_perm(msg, client, &["moderator", "broadcaster", "vip"], "You are not a VIP/Moderator. You can't use this command").await
}

pub async fn has_permission(msg: &PrivmsgMessage, client: TwitchClient, level: PermissionLevel, oauth_token: &str, client_id: &str) -> bool {
    match level {
        PermissionLevel::User => true,
        PermissionLevel::Follower => is_follower(msg, client, oauth_token, client_id).await,
        PermissionLevel::Moderator => is_moderator(msg, &client).await,
        PermissionLevel::Broadcaster => is_broadcaster(msg, &client).await,
        PermissionLevel::Vip => is_vip(msg, &client).await,
    }
}

pub type BotResult<T> = Result<T, BotError>;

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
    #[error("{0}")]
    Custom(String),

}

impl From<()> for BotError {
    fn from(_: ()) -> Self {
        BotError::Custom("unit error".to_string())
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub enum AnnouncementState {
    Paused,
    Active,
    Custom(String), //Pro specifické aktivity
}
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct AnnouncementConfig {
    pub state: AnnouncementState,
    pub interval: Duration,
    pub last_sent: u128,
}

impl AnnouncementConfig {
    fn new() -> AnnouncementConfig {
        AnnouncementConfig {
            state: AnnouncementState::Paused,
            interval: Duration::from_secs(5 * 60),
            last_sent: SystemTime::now().duration_since(UNIX_EPOCH).expect("Time went backwords").as_millis(),
        }
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct PointsConfig {
    pub name: String,
    pub interval: u64,
    pub points_per_time: i32,
}

impl PointsConfig {
    fn new() -> PointsConfig {
        PointsConfig { name: "Points".to_string(), interval: 600, points_per_time: 10 }
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ChannelConfig {
    pub open: bool,
    pub len: usize,
    pub teamsize: usize,
    pub combined: bool,
    pub queue_channel: String,
    pub packages: Vec<String>,
    pub runs: usize,
    pub announcement_config: AnnouncementConfig,
    pub sub_only: bool,
    pub prefix: String,
    pub random_queue: bool,
    pub giveaway: Giveaway,
    pub points_config: PointsConfig
}

impl ChannelConfig {
    pub fn reset(&mut self, channel_id: &str) {
        self.runs = 0;
        self.combined = false;
        self.queue_channel = channel_id.to_string();
        self.random_queue = false;
    }

    pub fn new(channel: String) -> Self {
        ChannelConfig { 
            open: false, 
            len: 1, 
            teamsize: 1, 
            combined: false, 
            queue_channel: channel,
            packages: vec!["Moderation".to_string()], 
            runs: 0, 
            announcement_config: AnnouncementConfig { state: AnnouncementState::Paused, interval: Duration::from_secs(10000), last_sent: 100 }, 
            sub_only: false, 
            prefix: "!".to_string(), 
            random_queue: false, 
            giveaway: Giveaway { duration: 100, max_tickets: 100, ticket_cost: 100, active: false }, 
            points_config: PointsConfig { name: "Points".to_string(), interval: 10000, points_per_time: 0 } 
        }
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct BotConfig {
    pub channels: HashMap<String, ChannelConfig>, // Holds configuration for all channels
}

impl BotConfig {
    pub fn new() -> Self {
        let mut hash = HashMap::new();
        hash.insert("krapmatt".to_string(), ChannelConfig {open: false, len: 0, teamsize: 0, combined: false, queue_channel: "krapmatt".to_string(), packages: vec!["Moderation".to_string()], runs: 0, announcement_config: AnnouncementConfig { state: AnnouncementState::Paused, interval: Duration::from_secs(600), last_sent: 0 }, sub_only: false, prefix: "!".to_string(), random_queue: false, giveaway: Giveaway { duration: 1000, max_tickets: 10, ticket_cost: 10, active: false }, points_config: PointsConfig { name: "Dirt".to_string(), interval: 1000, points_per_time: 15 }});
        BotConfig {
            channels: hash,
        }
    }

    /// Load all channel configs from the database
    pub async fn load_from_db(pool: &PgPool) -> BotResult<Self> {
        let rows = sqlx::query!("SELECT channel_id, config_json FROM bot_config").fetch_all(pool).await?;

        let mut channels = HashMap::new();
        for row in rows {
            let config: ChannelConfig = serde_json::from_value(row.config_json)
                .map_err(|e| BotError::Custom(format!("Failed to parse config for channel {}: {:?}", row.channel_id, e)))?;
            channels.insert(row.channel_id.clone(), config);
        }

        Ok(BotConfig { channels })
    }

    pub async fn save_channel(&self, pool: &PgPool, channel_id: &str) -> Result<(), BotError> {
        if let Some(channel_config) = self.channels.get(channel_id) {
            let config_json = serde_json::to_value(channel_config)
                .map_err(|e| BotError::Custom(format!("Failed to serialize config: {:?}", e)))?;
            sqlx::query!(
                r#"
                INSERT INTO bot_config (channel_id, config_json)
                VALUES ($1, $2)
                ON CONFLICT (channel_id) DO UPDATE
                SET config_json = $2
                "#,
                channel_id, config_json
            ).execute(pool).await?;
        }
        Ok(())
    }

    pub async fn save_all(&self, pool: &PgPool) -> BotResult<()> {
        for (channel_id, channel_config) in &self.channels {
            let config_json = serde_json::to_value(channel_config)?;
            
            sqlx::query!(
                r#"
                INSERT INTO bot_config (channel_id, config_json)
                VALUES ($1, $2)
                ON CONFLICT (channel_id)
                DO UPDATE SET config_json = $2
                "#,
                channel_id,
                config_json
            )
            .execute(pool)
            .await?;
        }
        Ok(())
    }

    pub async fn update_channel<F>(&mut self, pool: &PgPool, channel_id: &str, mutator: F) -> BotResult<()> where F: FnOnce(&mut ChannelConfig) {
        let cfg = self.channels.entry(channel_id.to_string()).or_insert_with(|| ChannelConfig::new(channel_id.to_string()));

        mutator(cfg);
        self.save_channel(pool, channel_id).await?;
        Ok(())
    }

    pub fn get_channel_config(&self, channel_name: &str) -> Option<&ChannelConfig> {
        self.channels.get(channel_name)
    }

    pub fn get_channel_config_mut(&mut self, channel_name: &str) -> &mut ChannelConfig {
        self.channels
            .entry(channel_name.to_string())
            .or_insert_with(|| ChannelConfig {
                open: false,
                len: 1,
                teamsize: 1,
                combined: false,
                queue_channel: channel_name.to_string(),
                packages: vec!["Moderation".to_string()],
                runs: 0,
                announcement_config: AnnouncementConfig::new(),
                sub_only: false,
                prefix: "!".to_string(),
                random_queue: false,
                giveaway: Giveaway::new(),
                points_config: PointsConfig::new()
            })
    }

    pub fn is_group_allowed(&self, channel: &str, group_name: &str) -> bool {
        if let Some(channel_config) = self.channels.get(channel) {
            channel_config.packages.contains(&group_name.to_string())
        } else {
            false
        }
    }
}

pub struct TemplateManager {
    pub pool: Arc<PgPool>,
}

impl TemplateManager {
    pub async fn get_template(
        &self,
        package: String,
        command: String,
        channel_id: Option<String>,
    ) -> Option<String> {
        let result = if let Some(channel) = channel_id {
            let res = sqlx::query!(
                "SELECT template FROM commands_template WHERE package = $1 AND command = $2 AND channel_id = $3",
                package, command, channel
            ).fetch_optional(&*self.pool).await;
            res.ok().flatten().map(|row| row.template)
        } else {
            let res = sqlx::query!(
                "SELECT template FROM commands_template WHERE package = $1 AND command = $2 AND channel_id IS NULL",
                package, command
            ).fetch_optional(&*self.pool).await;
            res.ok().flatten().map(|row| row.template)
        };
        return result?;
    }

    pub async fn set_template(
        &self,
        package: String,
        command: String,
        template: String,
        channel_id: Option<String>,
    ) -> BotResult<()> {
        if let Some(channel) = channel_id {
            sqlx::query!(
                "INSERT INTO commands_template (package, command, template, channel_id) 
                VALUES ($1, $2, $3, $4) 
                ON CONFLICT(channel_id, command) DO UPDATE SET template = excluded.template",
                package,
                command,
                template,
                channel
            )
            .execute(&*self.pool)
            .await?;
        }
        Ok(())
    }

    pub async fn remove_template(&self, command: String, channel_id: Option<String>) -> BotResult<()> {
        if let Some(channel) = channel_id {
            sqlx::query!(
                "DELETE FROM commands_template WHERE command = $1 AND channel_id = $2",
                command,
                channel
            )
            .execute(&*self.pool)
            .await?;
        } else {
            sqlx::query!(
                "DELETE FROM commands_template WHERE command = $1 AND channel_id IS NULL",
                command
            )
            .execute(&*self.pool)
            .await?;
        }
        Ok(())
    }
}

pub enum Package {
    Add,
    Remove
}
#[derive(Clone, Debug)]
pub struct SharedQueueGroup {
    pub main_channel: String,
    pub member_channels: HashSet<String>,
    pub combined_enabled: bool,
    pub queue_length: usize,
    pub team_size: usize,
}

impl SharedQueueGroup {
    pub fn new(main_channel: String, members: HashSet<String>, queue_length: usize, team_size: usize) -> Self {
        Self {
            main_channel,
            member_channels: members,
            combined_enabled: false,
            queue_length,
            team_size,
        }
    }

    /// Toggle the combined queue state on/off.
    /// Returns the new state (true if enabled)
    pub fn toggle_combined(&mut self) -> bool {
        self.combined_enabled = !self.combined_enabled;
        self.combined_enabled
    }

    /// Get all channels including the main one
    pub fn all_channels(&self) -> HashSet<String> {
        let mut all = self.member_channels.clone();
        all.insert(self.main_channel.clone());
        all
    }
}

#[derive(Debug)]
pub struct AliasConfig {
    pub aliases: HashMap<String, String>,
    pub disabled_commands: HashSet<String>,
    pub removed_aliases: HashSet<String>,
}

impl AliasConfig {
    pub fn get_aliases(&self, name: &str) -> Vec<String> {
        self.aliases.iter().filter_map(|(key, val)| if val == name { Some(key.to_owned()) } else { None }).collect()
    }
    pub fn get_removed_aliases(&self, name: &str) -> bool {
        self.removed_aliases.get(name).is_some()
    }
}