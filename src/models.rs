use core::fmt;
use std::{error::Error, fs::File, io::{Read, Write}, sync::Arc};

use async_sqlite::rusqlite;
use serde::{Deserialize, Serialize};
use tmi::{client::{read::RecvError, write::SendError, ReconnectError}, Client, MessageParseError};
use tokio::sync::Mutex;

use crate::bot_commands::{is_follower, is_moderator, is_vip};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TwitchUser {
    pub twitch_name: String,
    pub bungie_name: String,
}

impl Default for TwitchUser {
    fn default() -> Self {
        TwitchUser { twitch_name: String::new(), bungie_name: String::new() }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SharedState {
    pub run_count: usize
}

impl SharedState {
    pub fn new() -> Self {
        Self {
            run_count: 0
        }
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

#[derive(Clone, Copy)]
pub enum PermissionLevel {
    User,
    Follower,
    Vip,
    Moderator,
    Broadcaster
}

pub async fn has_permission(msg: &tmi::Privmsg<'_>, client:Arc<Mutex<Client>>, level: PermissionLevel) -> bool {
    match level {
        PermissionLevel::User => true,
        PermissionLevel::Follower => is_follower(msg, Arc::clone(&client)).await,
        PermissionLevel::Moderator => is_moderator(msg, Arc::clone(&client)).await,
        PermissionLevel::Broadcaster => todo!(),
        PermissionLevel::Vip => is_vip(msg, Arc::clone(&client)).await,
    }
}

#[derive(Debug)]
pub struct BotError {
    pub error_code: usize,
    pub string: Option<String>
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
impl From<async_sqlite::Error> for BotError {
    fn from(err: async_sqlite::Error) -> BotError {
        BotError { error_code: 99, string: Some(err.to_string()) }
    }
}
impl From<rusqlite::Error> for BotError {
    fn from(err: rusqlite::Error) -> BotError {
        BotError { error_code: 100, string: Some(err.to_string()) }
    }
}
impl From<RecvError> for BotError {
    fn from(err: RecvError) -> BotError {
        BotError { error_code: 101, string: Some(err.to_string()) }
    }
}
impl From<SendError> for BotError {
    fn from(err: SendError) -> BotError {
        BotError { error_code: 102, string: Some(err.to_string()) }
    }
}
impl From<MessageParseError> for BotError {
    fn from(err: MessageParseError) -> BotError {
        BotError { error_code: 103, string: Some(err.to_string()) }
    }
}
impl From<ReconnectError> for BotError {
    fn from(err: ReconnectError) -> BotError {
        BotError { error_code: 104, string: Some(err.to_string()) }
    }
}
impl From<reqwest::Error> for BotError {
    fn from(err: reqwest::Error) -> BotError {
        BotError { error_code: 105, string: Some(err.to_string()) }
    }
}
impl From<serenity::Error> for BotError {
    fn from(err: serenity::Error) -> BotError {
        BotError { error_code: 106, string: Some(err.to_string()) }
    }
}
impl From<serde_json::Error> for BotError {
    fn from(err: serde_json::Error) -> BotError {
        BotError { error_code: 107, string: Some(err.to_string()) }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct BotConfig {
    pub open: bool,
    pub len: usize,
    pub teamsize: usize,
    pub channel_id: Option<String>,
    pub combined: bool,
}

impl BotConfig {
    pub fn new() -> Self {
        BotConfig {
            open: false,
            len: 0,
            teamsize: 0,
            channel_id: None,
            combined: false,
        }
    }
    
    pub fn load_config(channel_name: &str) -> Self {
        let mut file = File::open(format!("D:/program/krapbott/configs/{}.json", channel_name)).expect("Failed to load config. Create file Config.json");
        let mut string = String::new();
        let _ = file.read_to_string(&mut string);
        let bot_config: BotConfig = serde_json::from_str(&string).expect("Always will be correct format");
        bot_config
    }

    pub fn save_config(&self, channel_name: &str) {
        let content = serde_json::to_string_pretty(self).expect("Json serialization is wrong? Check save_config function");
        let mut file = File::create(format!("D:/program/krapbott/configs/{}.json", channel_name)).expect("Still the config file doesnt exist?");
        file.write_all(content.as_bytes()).unwrap();
        
    }
}