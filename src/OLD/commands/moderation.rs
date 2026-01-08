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

pub fn mod_config() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |msg, client, _pool, bot_state| {
            Box::pin(async move {
                let bot_state = bot_state.read().await;
                let config = match bot_state.config.get_channel_config(&msg.channel_login) {
                    Some(cfg) => cfg,
                    None => return Ok(()),
                };

                let queue_reply = format!(
                    "Queue -> Open: {} || Length: {} || Fireteam size: {} || Combined: {} & Queue channel: {}",
                    config.open, 
                    config.len, 
                    config.teamsize, 
                    config.combined, 
                    config.queue_channel
                );

                let package_reply = format!("Packages: {}", config.packages.join(", "));
                let giveaway_reply = format!(
                    "Duration: {} || Max tickets: {} || Price of ticket: {}",
                    config.giveaway.duration,
                    config.giveaway.max_tickets,
                    config.giveaway.ticket_cost
                );

                drop(bot_state);

                for reply in [queue_reply, package_reply, giveaway_reply] {
                    client.say(msg.channel_login.clone(), reply).await?;
                }

                Ok(())
            })
        },
        "Shows the settings of one's queue and packages.",
        "!mod_config",
        "Mod Config",
        PermissionLevel::Moderator,
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

pub fn list_of_packages() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |msg, client, _pool, bot_state| {
            Box::pin(async move {
                let bot_state = bot_state.read().await;
                let config =
                    if let Some(config) = bot_state.config.get_channel_config(&msg.channel_login) {
                        config
                    } else {
                        return Ok(());
                    };
                let streamer_packages = &config.packages;
                let mut missing_packages: Vec<&str> = vec![];

                for package in COMMAND_GROUPS.values() {
                    if !streamer_packages.contains(&package.name) {
                        missing_packages.push(&package.name);
                    }
                }

                let reply = if missing_packages.is_empty() {
                    "You have all packages activated!".to_string()
                } else {
                    format!(
                        "Currently you have these packages on your channel: {}. And you can add: {}. Use: !add_package <name>",
                        streamer_packages.join(", "), missing_packages.join(", ")
                    )
                };

                client.say(msg.channel_login, reply).await?;
                Ok(())
            })
        },
        "Show all included packages",
        "packages",
        "Packages",
        PermissionLevel::Moderator,
    ))
}
