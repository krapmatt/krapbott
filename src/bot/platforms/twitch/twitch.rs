use std::collections::HashSet;

use tokio::sync::mpsc;
use twitch_irc::message::ServerMessage;
use twitch_irc::{SecureTCPTransport, login::StaticLoginCredentials, message::PrivmsgMessage};
use twitch_irc::{ClientConfig, TwitchIRCClient};

use crate::bot::chat_event::chat_event::{ChatEvent, ChatUser, DisplayName, Platform, UserIdentity};
use crate::bot::permissions::permissions::PermissionLevel;

pub type TwitchClient = TwitchIRCClient<SecureTCPTransport, StaticLoginCredentials>;

pub fn map_privmsg(msg: &PrivmsgMessage) -> ChatEvent {
    let permission = if msg.badges.iter().any(|b| b.name == "broadcaster") {
        PermissionLevel::Broadcaster
    } else if msg.badges.iter().any(|b| b.name == "lead_moderator") {
        PermissionLevel::LeadModerator
    } else if msg.badges.iter().any(|b| b.name == "moderator") {
        PermissionLevel::Moderator
    } else if msg.badges.iter().any(|b| b.name == "vip") {
        PermissionLevel::Vip
    } else if msg.badges.iter().any(|b| b.name == "subscriber") {
        PermissionLevel::Subscriber
    } else {
        PermissionLevel::Everyone
    };

    ChatEvent {
        platform: Platform::Twitch,
        channel: msg.channel_login.clone(),
        message: msg.message_text.clone(),
        broadcaster_id: Some(msg.channel_id.clone()),
        user: Some(ChatUser {
            identity: UserIdentity {
                platform: Platform::Twitch,
                platform_user_id: msg.sender.id.clone(),
            },
            name: DisplayName {
                login: msg.sender.login.clone(),
                display: msg.sender.name.clone(),
            },
            permission,
            
        }),
        follower: None,
    }
}

pub fn build_twitch_client(nick: String, oauth: String) -> (mpsc::UnboundedReceiver<ServerMessage>, TwitchClient) {
    let creds = StaticLoginCredentials::new(nick, Some(oauth));
    let config = ClientConfig::new_simple(creds);
    TwitchIRCClient::<SecureTCPTransport, _>::new(config)
}