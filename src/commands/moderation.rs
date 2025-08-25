use std::{borrow::BorrowMut, sync::Arc};

use futures::future::BoxFuture;
use sqlx::SqlitePool;
use tmi::{Client, Privmsg};
use tokio::sync::{Mutex, RwLock};

use crate::{bot::BotState, bot_commands::{self, mod_action_user_from_queue, modify_command, reply_to_message, send_message, unban_player_from_queue}, commands::{oldcommands::FnCommand, traits::CommandT, update_dispatcher_if_needed, words, COMMAND_GROUPS}, models::{AliasConfig, BotError, BotResult, CommandAction, Package, PermissionLevel, TemplateManager}};

pub fn mod_unban() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |msg, client, pool, _bot_state| {
            Box::pin(async move {
                unban_player_from_queue(&msg, client, &pool).await?;
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
                mod_action_user_from_queue(&msg, client, &pool, bot_commands::ModAction::Ban).await?;
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
                mod_action_user_from_queue(&msg, client, &pool, bot_commands::ModAction::Timeout).await?;
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
                let config = match bot_state.config.get_channel_config(msg.channel()) {
                    Some(cfg) => cfg,
                    None => return Ok(()),
                };

                let queue_reply = format!(
                    "Queue -> Open: {} || Length: {} || Fireteam size: {} || Combined: {} & Queue channel: {}",
                    config.open, config.len, config.teamsize, config.combined, config.queue_channel
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
                    reply_to_message(&msg, client.lock().await.borrow_mut(), &reply).await?;
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

pub fn connect() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |msg, client, _pool, bot_state| {
            Box::pin(async move {
                if let Some((_, channel)) = msg.text().split_once(' ') {
                    let channel = format!("#{}", channel.trim_start_matches('@').to_ascii_lowercase());

                    {
                        let mut bot_state = bot_state.write().await;
                        bot_state.config.get_channel_config_mut(&channel);
                        bot_state.config.save_config();
                    }

                    send_message(&msg, client.lock().await.borrow_mut(), &format!("I will connect to channel {} in 60 seconds", channel)).await?;
                } else {
                    send_message(&msg, client.lock().await.borrow_mut(), "You didn't write the channel to connect to").await?;
                }
                Ok(())
            })
        },
        "Connect krapbott to a new twitch channel.",
        "connect @twitchname",
        "Connect",
        PermissionLevel::Moderator,
    ))
}

pub fn mod_reset() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |msg, client, _pool, bot_state| {
            let fut = async move {
                let mut bot_state = bot_state.write().await;
                bot_state.streaming_together.clear();

                let config = bot_state.config.get_channel_config_mut(&msg.channel());
                config.reset(&msg.channel());

                bot_state.config.save_config();

                send_message(&msg, client.lock().await.borrow_mut(), "Config has been reset!").await?;
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


pub fn addpackage() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |msg, client, pool, bot_state| {
            let fut = async move {
                bot_state.write().await.add_remove_package(&msg, client, Package::Add, &pool).await?;
                Ok(())
            };
            Box::pin(fut)
        },
        "Add a package",
        "add_package nameOfPackage",
        "Add Package",
        PermissionLevel::Moderator,
    ))
}

pub fn removepackage() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |msg, client, pool, bot_state| {
            let fut = async move {
                bot_state.write().await.add_remove_package(&msg, client, Package::Remove, &pool).await?;
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
        |msg: Privmsg<'static>, client: Arc<Mutex<Client>>, _pool: SqlitePool, bot_state: Arc<RwLock<BotState>>| {
            Box::pin(async move {
                let bot_state = bot_state.read().await;
                let config =
                    if let Some(config) = bot_state.config.get_channel_config(msg.channel()) {
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

                send_message(&msg, client.lock().await.borrow_mut(), &reply).await?;
                Ok(())
            })
        },
        "Show all included packages",
        "packages",
        "Packages",
        PermissionLevel::Moderator,
    ))
}

pub fn set_template() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |msg: Privmsg<'static>, client: Arc<Mutex<Client>>, pool: SqlitePool, _bot_state: Arc<RwLock<BotState>>| {
            Box::pin(async move {
                let template_manager = TemplateManager {
                    pool: pool.clone().into(),
                };
                let args: Vec<&str> = msg.text().splitn(4, ' ').collect();
                if args.len() < 4 {
                    send_message(&msg, client.lock().await.borrow_mut(), "Usage: !set_template <package> <command> <template>").await?;
                    return Ok(());
                }

                let package = args[1].to_string();
                let command = args[2].to_string();
                let template = args[3].to_string();

                template_manager
                    .set_template(package, command, template, Some(msg.channel().to_string()))
                    .await?;

                send_message(&msg, client.lock().await.borrow_mut(), "Template updated successfully!").await?;
                Ok(())
            })
        },
        "Sets the template for a command with available template.",
        "set_template <package> <command> <template>",
        "Set template",
        PermissionLevel::Moderator,
    ))
}

pub fn delete_template() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |msg: Privmsg<'static>, client: Arc<Mutex<Client>>, pool: SqlitePool, _bot_state: Arc<RwLock<BotState>>| {
            Box::pin(async move {
                let template_manager = TemplateManager {
                    pool: pool.clone().into(),
                };
                let args: Vec<&str> = msg.text().splitn(2, ' ').collect();
                if args.len() < 2 {
                    send_message(&msg, client.lock().await.borrow_mut(), "Usage: !remove_template <command>").await?;
                    return Ok(());
                }

                let command = args[1].to_string();

                template_manager.remove_template(command, Some(msg.channel().to_string())).await?;
                send_message(&msg, client.lock().await.borrow_mut(), "Template updated successfully!").await?;
                Ok(())
            })
        },
        "Deletes template for given command",
        "remove_template <command>",
        "Remove template",
        PermissionLevel::Moderator,
    ))
}

pub struct ModifyCommand {
    permission: PermissionLevel,
    description: String,
    usage: String,
    name: String,
    action: CommandAction,
    channel_extractor: Box<dyn Fn(&Privmsg<'_>) -> Option<String> + Send + Sync>,
}

impl ModifyCommand {
    pub fn new(permission: PermissionLevel, description: impl Into<String>, usage: impl Into<String>, name: impl Into<String>, action: CommandAction, channel_extractor: impl Fn(&Privmsg<'_>) -> Option<String> + Send + Sync + 'static) -> Self {
        Self {
            permission,
            description: description.into(),
            usage: usage.into(),
            name: name.into(),
            action,
            channel_extractor: Box::new(channel_extractor),
        }
    }
}

impl CommandT for ModifyCommand {
    fn name(&self) -> &str { &self.name }
    fn description(&self) -> &str { &self.description }

    fn usage(&self) -> &str {
        &self.usage
    }

    fn permission(&self) -> PermissionLevel {
        self.permission
    }

    fn execute(&self, msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, pool: SqlitePool, _state: Arc<RwLock<BotState>>, alias_config: Arc<AliasConfig>) -> BoxFuture<'static, BotResult<()>> {
        let action = self.action;
        let channel = (self.channel_extractor)(&msg);
        Box::pin(async move {
            modify_command(&msg, client, &pool, action, channel).await?;
            Ok(())
        })
    }
}

pub fn addglobalcommand() -> Arc<dyn CommandT> {
    Arc::new(ModifyCommand::new(
        PermissionLevel::Moderator,
        "Add a simple command for all channels",
        "!addglobalcommand name reply",
        "Add global command",
        CommandAction::AddGlobal,
        |_| None,
    ))
}

pub fn removecommand() -> Arc<dyn CommandT> {
    Arc::new(ModifyCommand::new(
        PermissionLevel::Moderator,
        "Remove a simple command",
        "!remove_command nameOfCommand",
        "Remove Command",
        CommandAction::Remove,
        |msg| Some(msg.channel().to_string()),
    ))
}

pub fn addcommand() -> Arc<dyn CommandT> {
    Arc::new(ModifyCommand::new(
        PermissionLevel::Moderator,
        "Add a simple command to this channel",
        "!addcommand nameOfCommand reply",
        "Add Command",
        CommandAction::Add,
        |msg| Some(msg.channel().to_string()),
    ))
}

pub struct AliasCommand;

impl CommandT for AliasCommand {
    fn name(&self) -> &str { "Alias" }
    fn usage(&self) -> &str { "!alias add <alias> <command> | !alias remove <alias>" }
    fn description(&self) -> &str { "Add or remove a custom alias for a command." }
    fn permission(&self) -> PermissionLevel { PermissionLevel::Moderator }

    fn execute(&self, msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, pool: SqlitePool, bot_state: Arc<RwLock<BotState>>, alias_config: Arc<AliasConfig>) -> BoxFuture<'static, BotResult<()>> {
        Box::pin(async move {
            let words: Vec<&str> = words(&msg);
            let reply;

            if words.len() < 3 {
                reply = "Usage: !alias add <alias> <command> OR !alias remove <alias>".to_string();
                send_message(&msg, client.lock().await.borrow_mut(), &reply).await?;
                return Ok(());
            }

            let action = words[1].to_lowercase();
            let channel = msg.channel();

            match action.as_str() {
                "add" if words.len() == 4 => {
                    let alias = words[2].to_lowercase();
                    let command = words[3].to_lowercase();

                    sqlx::query!(
                        "INSERT OR REPLACE INTO command_aliases (channel, alias, command) VALUES (?, ?, ?)",
                        channel,
                        alias,
                        command
                    )
                    .execute(&pool)
                    .await?;

                    update_dispatcher_if_needed(channel, &bot_state.read().await.config, &pool, bot_state.read().await.dispatchers.clone()).await?;

                    reply = format!("Added alias '{}' for command '{}'", alias, command);
                }
                "remove" if words.len() == 3 => {
                    let alias = words[2].to_lowercase();

                    sqlx::query!(
                        "DELETE FROM command_aliases WHERE channel = ? AND alias = ?",
                        channel,
                        alias
                    )
                    .execute(&pool)
                    .await?;

                    update_dispatcher_if_needed(channel, &bot_state.read().await.config, &pool, bot_state.read().await.dispatchers.clone()).await?;

                    reply = format!("Removed alias '{}'", alias);
                }
                _ => {
                    reply = "Invalid syntax. Use: !alias add <alias> <command> OR !alias remove <alias>".to_string();
                }
            }

            send_message(&msg, client.lock().await.borrow_mut(), &reply).await?;
            Ok(())
        })
    }
}