use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use thiserror::Error;
use std::{
    collections::{HashMap, HashSet}, fs::File, io::{self, Read, Write}, path::Path, sync::Arc, time::{Duration, SystemTime, UNIX_EPOCH}
};
use tmi::{
    client::{read::RecvError, write::SendError, ConnectError, ReconnectError}, Badge, Client, MessageParseError
};
use tokio::sync::Mutex;

use crate::{commands::points_traits::Giveaway, twitch_api::is_follower};

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
pub async fn is_broadcaster(msg: &tmi::Privmsg<'_>, client: Arc<tokio::sync::Mutex<tmi::Client>>) -> bool {
    if msg.badges().into_iter().any(|badge| badge.as_badge_data().name() == "broadcaster") || ADMINS.contains(&&*msg.sender().name().to_string()) {
        return true;
    } else {
        _ = client.lock().await.privmsg(msg.channel(), "You are not a broadcaster. You can't use this command").send().await;
        return false;
    }
    
}

pub fn is_subscriber(msg: &tmi::Privmsg<'_>) -> bool {
    println!("{:?}", msg.badges().into_iter().collect::<Vec<&Badge<'_>>>());
    if msg.badges().into_iter().any(|badge| badge.as_badge_data().name() == "subscriber" || badge.as_badge_data().name() == "moderator" ) || ADMINS.contains(&&*msg.sender().name().to_string()) {
        true
    } else {
        false
    }
}

pub async fn is_moderator(msg: &tmi::Privmsg<'_>, client: Arc<tokio::sync::Mutex<tmi::Client>>) -> bool {
    if msg.badges().into_iter().any(|badge| badge.as_badge_data().name() == "moderator" || badge.as_badge_data().name() == "broadcaster") || ADMINS.contains(&&*msg.sender().name().to_string()) {
        return true;
    } else {
        _ = client.lock().await.privmsg(msg.channel(), "You are not a moderator/broadcaster. You can't use this command").send().await;
        return false;
    }
    
}
pub async fn is_vip(msg: &tmi::Privmsg<'_>, client: Arc<tokio::sync::Mutex<tmi::Client>>) -> bool {
    if msg.badges().into_iter().any(|badge| badge.as_badge_data().name() == "moderator" || badge.as_badge_data().name() == "broadcaster" || badge.as_badge_data().name() == "vip") || ADMINS.contains(&&*msg.sender().name().to_string()) {
        return true;
    } else {
        _ = client.lock().await.privmsg(msg.channel(), "You are not a VIP/Moderator. You can't use this command").send().await;
        return false;
    }
    
}

pub async fn has_permission(msg: &tmi::Privmsg<'_>, client: Arc<Mutex<Client>>, level: PermissionLevel) -> bool {
    match level {
        PermissionLevel::User => true,
        PermissionLevel::Follower => is_follower(msg, Arc::clone(&client)).await,
        PermissionLevel::Moderator => is_moderator(msg, Arc::clone(&client)).await,
        PermissionLevel::Broadcaster => is_broadcaster(msg, Arc::clone(&client)).await,
        PermissionLevel::Vip => is_vip(msg, Arc::clone(&client)).await,
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
    #[error("Send error: {0}")]
    SendError(#[from] SendError),
    #[error("Receive error: {0}")]
    RecvError(#[from] RecvError),
    #[error("Message parse error: {0}")]
    MessageParseError(#[from] MessageParseError),
    #[error("Reconnect error: {0}")]
    ReconnectError(#[from] ReconnectError),
    #[error("Serenity (Discord) error: {0}")]
    SerenityError(#[from] serenity::Error),
    #[error("Database error: {0}")]
    SqlxError(#[from] sqlx::Error),
    #[error("Connect error: {0}")]
    ConnectError(#[from] ConnectError),
    #[error("{0}")]
    Custom(String),

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
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct BotConfig {
    pub channels: HashMap<String, ChannelConfig>, // Holds configuration for all channels
}

impl BotConfig {
    pub fn new() -> Self {
        BotConfig {
            channels: HashMap::new(),
        }
    }

    /// Load or create a unified config file for all channels
    pub fn load_config() -> Self {
        let config_path = "configs/config.json";

        if Path::new(config_path).exists() {
            let mut file = File::open(config_path).expect("Failed to open config file.");
            let mut content = String::new();
            file.read_to_string(&mut content)
                .expect("Failed to read config file.");
            serde_json::from_str(&content).expect("Failed to parse config file.")
        } else {
            let new_config = BotConfig::new();
            new_config.save_config();
            new_config
        }
    }

    /// Save the unified config file
    pub fn save_config(&self) {
        let config_path = "configs/config.json";
        let content = serde_json::to_string_pretty(self).expect("Failed to serialize config.");
        let mut file = File::create(config_path).expect("Failed to create config file.");
        file.write_all(content.as_bytes())
            .expect("Failed to write config file.");
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

    pub fn print_all_configs(&self) {
        for (channel, config) in &self.channels {
            println!("Channel: {}\nConfig: {:#?}", channel, config);
        }
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
    pub pool: Arc<SqlitePool>,
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
                "SELECT template FROM commands_template WHERE package = ? AND command = ? AND channel_id = ?",
                package, command, channel
            ).fetch_optional(&*self.pool).await;
            res.ok().flatten().map(|row| row.template)
        } else {
            let res = sqlx::query!(
                "SELECT template FROM commands_template WHERE package = ? AND command = ? AND channel_id IS NULL",
                package, command
            ).fetch_optional(&*self.pool).await;
            res.ok().flatten().map(|row| row.template)
        };
        return result;
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
                VALUES (?, ?, ?, ?) 
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
                "DELETE FROM commands_template WHERE command = ? AND channel_id = ?",
                command,
                channel
            )
            .execute(&*self.pool)
            .await?;
        } else {
            sqlx::query!(
                "DELETE FROM commands_template WHERE command = ? AND channel_id IS NULL",
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