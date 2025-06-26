use std::{borrow::BorrowMut, sync::Arc};

use futures::future::BoxFuture;
use sqlx::SqlitePool;
use tmi::{Client, Privmsg};
use tokio::sync::{Mutex, RwLock};

use crate::{bot::BotState, bot_commands::{self, mod_action_user_from_queue, modify_command, reply_to_message, send_message, unban_player_from_queue}, commands::{oldcommands::FnCommand, traits::CommandT, COMMAND_GROUPS}, models::{BotError, CommandAction, Package, PermissionLevel, TemplateManager}};

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
        "mod_unban",
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
        "mod_ban",
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
        "mod_timeout",
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
        "mod_config",
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
        "connect",
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
        "mod_reset",
        PermissionLevel::Moderator,
    ))
}


pub fn addpackage() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |msg, client, _pool, bot_state| {
            let fut = async move {
                bot_state.write().await.add_remove_package(&msg, client, Package::Add).await?;
                Ok(())
            };
            Box::pin(fut)
        },
        "Add a package",
        "add_package nameOfPackage",
        "add_package",
        PermissionLevel::Moderator,
    ))
}

pub fn removepackage() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |msg, client, _pool, bot_state| {
            let fut = async move {
                bot_state.write().await.add_remove_package(&msg, client, Package::Remove).await?;
                Ok(())
            };
            Box::pin(fut)
        },
        "Remove a package",
        "remove_package nameOfPackage",
        "remove_package",
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
        "packages",
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
        "set_template",
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
        "remove_template",
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

    fn execute(&self, msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, pool: SqlitePool, _state: Arc<RwLock<BotState>>) -> BoxFuture<'static, Result<(), BotError>> {
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
        "addglobalcommand",
        CommandAction::AddGlobal,
        |_| None,
    ))
}

pub fn removecommand() -> Arc<dyn CommandT> {
    Arc::new(ModifyCommand::new(
        PermissionLevel::Moderator,
        "Remove a simple command",
        "!remove_command nameOfCommand",
        "remove_command",
        CommandAction::Remove,
        |msg| Some(msg.channel().to_string()),
    ))
}

pub fn addcommand() -> Arc<dyn CommandT> {
    Arc::new(ModifyCommand::new(
        PermissionLevel::Moderator,
        "Add a simple command to this channel",
        "!addcommand nameOfCommand reply",
        "addcommand",
        CommandAction::Add,
        |msg| Some(msg.channel().to_string()),
    ))
}