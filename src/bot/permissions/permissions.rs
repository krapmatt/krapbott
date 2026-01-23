
use core::fmt;
use std::fmt::Display;

use serde::Deserialize;

use crate::{api::twitch_api::is_follower, bot::{chat_event::chat_event::{ChatEvent, Platform}, state::def::BotSecrets}};

#[derive(Clone, Copy, Deserialize, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum PermissionLevel {
    Broadcaster,
    LeadModerator,
    Moderator,
    Vip,
    Subscriber,
    Follower,
    Everyone,
}

impl Display for PermissionLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            PermissionLevel::Broadcaster => "broadcaster",
            PermissionLevel::LeadModerator => "lead moderator",
            PermissionLevel::Moderator => "moderator",
            PermissionLevel::Vip => "vip",
            PermissionLevel::Subscriber => "subscriber",
            PermissionLevel::Follower => "follower",
            PermissionLevel::Everyone => "How you managed that?",
        };
        write!(f, "{}", s)
    }
}

pub async fn has_permission(event: &mut ChatEvent, required: PermissionLevel, secrets: &BotSecrets, apptoken: &str) -> bool {
    let Some(user) = &event.user else {
        return false;
    };

    // Fast path: already sufficient
    if user.permission <= required {
        return true;
    }

    // Follower is special
    if required == PermissionLevel::Follower {
        // Twitch-only async check
        if event.platform == Platform::Twitch {
           let result = is_follower(&event,apptoken, &secrets.bot_id).await;

            event.follower = Some(result);
            return result;
        }

    }

    false
}
