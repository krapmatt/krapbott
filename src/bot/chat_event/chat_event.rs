use core::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::bot::permissions::permissions::PermissionLevel;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum Platform { Twitch, Kick, Obs }
#[derive(Debug, Clone)]
pub struct ChatEvent {
    pub platform: Platform,
    pub channel: String,
    pub user: Option<ChatUser>,
    pub message: String,
    //Twitch Data
    pub follower: Option<bool>,
    pub broadcaster_id: Option<String>,
}
#[derive(Debug, Clone)]
pub struct ChatUser {
    pub identity: UserIdentity,
    pub name: DisplayName,
    pub permission: PermissionLevel
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UserIdentity {
    pub platform: Platform,
    pub platform_user_id: String, // twitch user_id / kick user_id
}

#[derive(Debug, Clone)]
pub struct DisplayName {
    pub login: String,        // lowercase (twitch login)
    pub display: String,      // FancyName
}


impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Platform::Twitch => "twitch",
            Platform::Kick => "kick",
            Platform::Obs => "obs",
        };
        write!(f, "{}", s)
    }
}

impl FromStr for Platform {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "twitch" => Ok(Platform::Twitch),
            "kick" => Ok(Platform::Kick),
            "obs" => Ok(Platform::Obs),
            _ => Err("Invalid platform"),
        }
    }
}
