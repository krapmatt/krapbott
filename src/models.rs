use core::fmt;
use serde::{Deserialize, Serialize};
use sqlx::{pool, SqlitePool};
use std::{
    collections::HashMap,
    error::Error,
    fs::File,
    io::{Read, Write},
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};
use tmi::{
    client::{read::RecvError, write::SendError, ReconnectError},
    Client, MessageParseError,
};
use tokio::sync::Mutex;

use crate::bot_commands::{is_broadcaster, is_follower, is_moderator, is_vip};

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

pub async fn has_permission(
    msg: &tmi::Privmsg<'_>,
    client: Arc<Mutex<Client>>,
    level: PermissionLevel,
) -> bool {
    match level {
        PermissionLevel::User => true,
        PermissionLevel::Follower => is_follower(msg, Arc::clone(&client)).await,
        PermissionLevel::Moderator => is_moderator(msg, Arc::clone(&client)).await,
        PermissionLevel::Broadcaster => is_broadcaster(msg, Arc::clone(&client)).await,
        PermissionLevel::Vip => is_vip(msg, Arc::clone(&client)).await,
    }
}

#[derive(Debug)]
pub struct BotError {
    pub error_code: usize,
    pub string: Option<String>,
}

impl fmt::Display for BotError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &self.string {
            Some(s) => write!(f, "Error code {}: {}", self.error_code, s),
            None => write!(f, "Error code {}", self.error_code),
        }
    }
}

impl Error for BotError {}
impl From<RecvError> for BotError {
    fn from(err: RecvError) -> BotError {
        BotError {
            error_code: 101,
            string: Some(err.to_string()),
        }
    }
}
impl From<SendError> for BotError {
    fn from(err: SendError) -> BotError {
        BotError {
            error_code: 102,
            string: Some(err.to_string()),
        }
    }
}
impl From<MessageParseError> for BotError {
    fn from(err: MessageParseError) -> BotError {
        BotError {
            error_code: 103,
            string: Some(err.to_string()),
        }
    }
}
impl From<ReconnectError> for BotError {
    fn from(err: ReconnectError) -> BotError {
        BotError {
            error_code: 104,
            string: Some(err.to_string()),
        }
    }
}
impl From<reqwest::Error> for BotError {
    fn from(err: reqwest::Error) -> BotError {
        BotError {
            error_code: 105,
            string: Some(err.to_string()),
        }
    }
}
impl From<serenity::Error> for BotError {
    fn from(err: serenity::Error) -> BotError {
        BotError {
            error_code: 106,
            string: Some(err.to_string()),
        }
    }
}
impl From<serde_json::Error> for BotError {
    fn from(err: serde_json::Error) -> BotError {
        BotError {
            error_code: 107,
            string: Some(err.to_string()),
        }
    }
}
impl From<sqlx::Error> for BotError {
    fn from(err: sqlx::Error) -> BotError {
        BotError {
            error_code: 108,
            string: Some(err.to_string()),
        }
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
    #[serde(with = "serde_millis")]
    pub last_sent: Option<Instant>,
}

impl AnnouncementConfig {
    fn new() -> AnnouncementConfig {
        AnnouncementConfig {
            state: AnnouncementState::Paused,
            interval: Duration::from_secs(5 * 60),
            last_sent: None,
        }
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
        let config_path = "D:/program/krapbott/configs/config.json";

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
        let config_path = "D:/program/krapbott/configs/config.json";
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
    ) -> Result<(), BotError> {
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

    pub async fn remove_template(
        &self,
        command: String,
        channel_id: Option<String>,
    ) -> Result<(), BotError> {
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
