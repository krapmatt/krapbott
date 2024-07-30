use core::fmt;
use std::error::Error;

use serde::{Deserialize, Serialize};
use tmi::{client::{read::RecvError, write::SendError, ReconnectError}, MessageParseError};

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
pub struct ChatMessage {
    pub channel: String,
    pub user: String,
    pub text: String,
}

pub struct SharedState {
    pub messages: Vec<ChatMessage>,
}

impl SharedState {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
        }
    }

    pub fn add_message(&mut self, message: ChatMessage) {
        self.messages.push(message);
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