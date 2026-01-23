use std::sync::Arc;

use futures::future::BoxFuture;
use sqlx::PgPool;
use tokio::sync::RwLock;
use twitch_irc::message::PrivmsgMessage;

use crate::{bot::{BotState, TwitchClient}, bot_commands::{self, add_remove_package, mod_action_user_from_queue, modify_command, unban_player_from_queue}, commands::{oldcommands::FnCommand, traits::CommandT, update_dispatcher_if_needed, words, COMMAND_GROUPS}, models::{AliasConfig, BotConfig, BotResult, ChannelConfig, CommandAction, Package, PermissionLevel, TemplateManager}};

pub fn mod_unban() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |msg, client, pool, _bot_state| {
            Box::pin(async move {
                unban_player_from_queue(msg, client, &pool).await?;
                Ok(())
            })
        },
        "Unban a person from the queue.",
        "mod_unban @twitch_name",
        "Mod Unban",
        PermissionLevel::Moderator
    ))
}

pub fn mod_ban() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |msg, client, pool, _bot_state| {
            Box::pin(async move {
                mod_action_user_from_queue(msg, client, &pool, bot_commands::ModAction::Ban).await?;
                Ok(())
            })
        },
        "Ban a person from the queue.",
        "mod_ban @twitch_name",
        "Mod Ban",
        PermissionLevel::Moderator
    ))
}

pub fn mod_timeout() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |msg, client, pool, _bot_state| {
            Box::pin(async move {
                mod_action_user_from_queue(msg, client, &pool, bot_commands::ModAction::Timeout).await?;
                Ok(())
            })
        },
        "Timeout someone from entering the queue.",
        "mod_timeout @twitch_name <seconds> Optional(reason)",
        "Mod Timeout",
        PermissionLevel::Moderator
    ))
}


//TODO! - add a way to reset streaming together only for the one channel not all!!!!
pub fn mod_reset() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |msg, client, pool, bot_state| {
            let fut = async move {
                let mut bot_state = bot_state.write().await;
                bot_state.streaming_together.clear();

                let config = bot_state.config.get_channel_config_mut(&msg.channel_login);
                config.reset(&msg.channel_login);

                bot_state.config.save_all(&pool).await?;

                client.say(msg.channel_login, "Config has been reset!".to_string()).await?;
                Ok(())
            };
            Box::pin(fut)
        },
        "Reset bot. Recommended to do before every stream",
        "!mod_reset",
        "Mod Reset",
        PermissionLevel::Moderator,
    ))
}


pub fn removepackage() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |msg, client, pool, bot_state| {
            let fut = async move {
                add_remove_package(Arc::clone(&bot_state), msg, client, Package::Remove, &pool).await?;
                Ok(())
            };
            Box::pin(fut)
        },
        "Remove a package",
        "remove_package nameOfPackage",
        "Remove Package",
        PermissionLevel::Moderator,
    ))
}