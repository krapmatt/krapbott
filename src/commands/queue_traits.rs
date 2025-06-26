use std::{borrow::BorrowMut, collections::HashSet, sync::Arc};

use futures::future::BoxFuture;
use sqlx::SqlitePool;
use tmi::Privmsg;
use tokio::sync::{Mutex, RwLock};

use crate::{bot::BotState, bot_commands::{bungiename, register_user, send_message}, commands::{oldcommands::FnCommand, traits::CommandT, words}, models::{BotError, PermissionLevel, SharedQueueGroup, TwitchUser}, queue::{self, process_queue_entry}};

pub struct JoinCommand;

impl CommandT for JoinCommand {
    fn name(&self) -> &str { "join" }

    fn description(&self) -> &str { "Join the queue" }

    fn usage(&self) -> &str { "!join" }

    fn permission(&self) -> PermissionLevel { PermissionLevel::Follower }

    fn execute(&self, msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, pool: SqlitePool, bot_state: Arc<RwLock<BotState>>) -> BoxFuture<'static, Result<(), BotError>> {
        Box::pin(async move {
            let bot_state = bot_state.read().await;
            bot_state.handle_join(&msg, client, &pool).await?;
            Ok(())
        })
    }
}

pub struct NextComamnd;

impl CommandT for NextComamnd {
    fn name(&self) -> &str { "next" }
    fn description(&self) -> &str { "Advance the queue" }
    fn usage(&self) -> &str { "!next" }
    fn permission(&self) -> PermissionLevel { PermissionLevel::Broadcaster }

    fn execute(&self, msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, pool: SqlitePool, bot_state: Arc<RwLock<BotState>>) -> BoxFuture<'static, Result<(), BotError>> {
        Box::pin(async move {
            let mut bot_state = bot_state.write().await;
            let reply = bot_state.handle_next(msg.channel().to_string(), &pool).await?;
            send_message(&msg, client.lock().await.borrow_mut(), &reply).await?;
            Ok(())
        })
    }
}

pub struct QueueSize;

impl CommandT for QueueSize {
    fn name(&self) -> &str { "queue_size" }
    fn description(&self) -> &str { "Update size of group" }
    fn usage(&self) -> &str { "!queue_size number" }
    fn permission(&self) -> PermissionLevel { PermissionLevel::Moderator }
    fn execute(&self, msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, _pool: SqlitePool, bot_state: Arc<RwLock<BotState>>) -> BoxFuture<'static, Result<(), BotError>> {
        Box::pin(async move {
            let words: Vec<&str> = words(&msg);
            let reply;
            if words.len() == 2 {
                if let Ok(size) = words[1].parse::<usize>() {
                    let bot_state_guard = bot_state.write().await;
                    let mut config = bot_state_guard.config.clone();

                    if let Some(cfg) = config.channels.get(msg.channel()) {
                        if cfg.combined {
                            let group = bot_state_guard.streaming_group(msg.channel());
                            for chan in group {
                                if let Some(cfg) = config.channels.get_mut(&chan) {
                                    cfg.teamsize = size;
                                }
                            }
                        } else {
                            if let Some(cfg) = config.channels.get_mut(msg.channel()) {
                                cfg.teamsize = size;
                            }
                        }

                        config.save_config();
                        reply = format!("Queue fireteam size updated to {}.", size);
                    } else {
                        reply = "Channel configuration not found.".to_string();
                    }
                } else {
                    reply = "Invalid size provided. Use: !queue_size <number>".to_string();
                }
            } else {
                reply = "Incorrect command format. Use: !queue_size <number>".to_string();
            }
            send_message(&msg, client.lock().await.borrow_mut(), &reply).await?;
            Ok(())
        })    
    }
}

pub struct QueueLength;

impl CommandT for QueueLength {
    fn name(&self) -> &str { "queue_len" }
    fn usage(&self) -> &str { "!queue_len number" }
    fn description(&self) -> &str { "Change the lenght of queue" }
    fn permission(&self) -> PermissionLevel { PermissionLevel::Moderator }
    fn execute(&self, msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, _pool: SqlitePool, bot_state: Arc<RwLock<BotState>>) -> BoxFuture<'static, Result<(), BotError>> {
        Box::pin(async move {
            let words: Vec<&str> = words(&msg);
            let reply;
            if words.len() == 2 {
                if let Ok(len) = words[1].parse::<usize>() {
                    let bot_state_guard = bot_state.write().await;
                    let mut config = bot_state_guard.config.clone();

                    if let Some(cfg) = config.channels.get(msg.channel()) {
                        if cfg.combined {
                            let group = bot_state_guard.streaming_group(msg.channel());
                            for chan in group {
                                if let Some(cfg) = config.channels.get_mut(&chan) {
                                    cfg.len = len;
                                }
                            }
                        } else {
                            if let Some(cfg) = config.channels.get_mut(msg.channel()) {
                                cfg.len = len;
                            }
                        }

                        config.save_config();
                        reply = format!("Queue length updated to {}.", len);
                    } else {
                        reply = "Channel configuration not found.".to_string();
                    }
                } else {
                    reply = "Invalid length provided. Use: !queue_len <number>".to_string();
                }
            } else {
                reply = "Incorrect command format. Use: !queue_len <number>".to_string();
            }
            send_message(&msg, client.lock().await.borrow_mut(), &reply).await?;
            Ok(())
        })    
    }
}

pub fn addplayertoqueue() -> Arc<dyn CommandT>  {
    Arc::new(FnCommand::new(
        |msg, client, pool, bot_state| {
            Box::pin(async move {
                let words: Vec<&str> = words(&msg);
                let twitch_name = words[1].strip_prefix("@").unwrap_or(&words[1]).to_string();
                //add twitchname bungiename
                let user = TwitchUser {
                    twitch_name: twitch_name,
                    bungie_name: words[2..].join(" ").to_string(),
                };
                let bot_state = bot_state.read().await;
                let config = bot_state.config.get_channel_config(msg.channel()).unwrap();
                let queue_len = config.len;
                let queue_channel = &config.queue_channel;

                process_queue_entry(&msg, client.lock().await.borrow_mut(), queue_len, &pool, user, queue_channel, queue::Queue::ForceJoin).await?;
                Ok(())
            })
        },
        "Force Add an user to queue",
        "!add @twitchname bungiename",
        "addtoqueue",
        PermissionLevel::Moderator
    ))
}

//Both for open and close queue
pub fn toggle_queue(open: bool, name: &'static str, desc: &'static str, usage: &'static str) -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        move |msg, client, _, bot_state| {
            Box::pin(async move {
                let text = if open { "open" } else { "closed" };
                let emoji = if open { "✅" } else { "❌" };

                let bot_state = bot_state.write().await.to_owned();
                let mut config = bot_state.config;

                let mut queue_channel = String::new();
                if let Some(channel_config) = config.get_channel_config(msg.channel()) {
                    queue_channel = channel_config.queue_channel.clone();
                }

                let affected_channels = config.channels.iter_mut().filter_map(|(ch, chan_config)| {
                    if chan_config.queue_channel == queue_channel {
                        chan_config.open = open;
                        Some(ch.to_string())
                    } else {
                        None
                    }
                }).collect::<Vec<_>>();

                config.save_config();

                for channel in affected_channels {
                    let reply = format!("{emoji} The queue is {text}!");
                    client.lock().await.privmsg(&channel, &reply).send().await?;
                }

                Ok(())
            })
        },
        desc,
        usage,
        name,
        PermissionLevel::Moderator
    ))
}

pub fn list() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |msg, client, pool, bot_state| {
            Box::pin(async move {
                let words: Vec<&str> = msg.text().split_ascii_whitespace().collect();
                if words.len() == 1 {
                    let bot_state = bot_state.read().await;
                    bot_state.handle_queue(&msg, client, &pool).await?;
                } else {
                    let bot_state = bot_state.read().await;
                    bot_state.handle_join(&msg, client, &pool).await?;
                }
                Ok(())
            })
        },
        "Shows the queue list or site",
        "!list, !queue",
        "list",
        PermissionLevel::Follower
    ))
}

//TODO! -> Shared queue clear from any channel
pub fn clear() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |msg, client, pool, _bot_state| {
            Box::pin(async move {
                let channel = msg.channel().to_owned();
                // Clear the queue for the given channel
                sqlx::query!("DELETE FROM queue WHERE channel_id = ?", channel).execute(&pool).await?;
                let mut client = client.lock().await;
                send_message(&msg, client.borrow_mut(), "Queue has been cleared").await?;
                Ok(())
            })
        },
        "Clear the queue",
        "!clear",
        "clear",
        PermissionLevel::Moderator
    ))
}

pub fn remove() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |msg, client, pool, bot_state| {
            Box::pin(async move {
                let bot_state = bot_state.read().await;
                bot_state.handle_remove(&msg, client, &pool).await?;
                Ok(())
            })
        },
        "Force remove a player from queue",
        "!remove @twitchname",
        "remove",
        PermissionLevel::Moderator
    ))
}

pub fn pos() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |msg, client, pool, bot_state| {
            Box::pin(async move {
                let bot_state = bot_state.read().await;
                bot_state.handle_pos(&msg, client, &pool).await?;
                Ok(())
            })
        },
        "Show position in queue",
        "!pos",
        "position",
        PermissionLevel::User
    ))
}

pub fn move_user() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |msg, client, pool, bot_state| {
            Box::pin(async move {
                bot_state.read().await.move_groups(&msg, client, &pool).await?;
                Ok(())
            })
        },
        "Move user back a group",
        "!move @twitchname",
        "move",
        PermissionLevel::Moderator
    ))
}

pub fn leave() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |msg, client, pool, bot_state| {
            Box::pin(async move {
                let bot_state = bot_state.read().await;
                bot_state.handle_leave(&msg, client, &pool).await?;
                Ok(())
            })
        },
        "Leave the queue",
        "!leave",
        "leave",
        PermissionLevel::User
    ))
}
//TODO! Split prio into two commands? To stay in first group and to move to second group?
pub fn prio() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |msg, client, pool, bot_state| {
            Box::pin(async move {
                bot_state.read().await.prio(&msg, client, &pool).await?;
                Ok(())
            })
        },
        "Priority runs for people",
        "!prio name number of runs -> use in first group to increase the number of runs OR !prio name -> moves to next group",
        "prio",
        PermissionLevel::Moderator
    ))
}

pub fn deprio() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |msg, client, pool, bot_state| {
            Box::pin(async move {
                bot_state.read().await.deprio(&msg, client, &pool).await?;
                Ok(())
            })
        },
        "Deprio command",
        "!!deprio <twitch_name>",
        "deprio",
        PermissionLevel::Moderator
    ))
}

pub fn register_command() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |msg, client, pool, _bot_state| {
            let fut = async move {
                let reply = if let Some((_, bungie_name)) = msg.text().split_once(' ') {
                    register_user(&pool, &msg.sender().name(), bungie_name).await?
                } else {
                    "Invalid command format! Use: !register bungiename#1234".to_string()
                };

                send_message(&msg, client.lock().await.borrow_mut(), &reply).await?;
                Ok(())
            };
            Box::pin(fut)
        },
        "Register your Bungie name with the bot",
        "!register bungiename#1234",
        "register",
        PermissionLevel::User,
    ))
}

pub fn mod_register_command() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |msg, client, pool, _bot_state| {
            let fut = async move {
                let words: Vec<&str> = msg.text().split_whitespace().collect();
                let reply = if words.len() >= 3 {
                    let mut twitch_name = words[1].to_string();
                    let bungie_name = &words[2..].join(" ");

                    if twitch_name.starts_with('@') {
                        twitch_name.remove(0);
                    }

                    register_user(&pool, &twitch_name, bungie_name).await?
                } else {
                    "You are a mod... || Use: !mod_register twitchname bungiename".to_string()
                };

                send_message(&msg, client.lock().await.borrow_mut(), &reply).await?;
                Ok(())
            };
            Box::pin(fut)
        },
        "Manually register a user as a mod",
        "!mod_register twitchname bungiename",
        "mod_register",
        PermissionLevel::Moderator,
    ))
}

pub fn bungie_name_command() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |msg, client, pool, _bot_state| {
            let fut = async move {
                let mut client = client.lock().await;
                // If the message is only 11 characters long, assume it's just the command (use self)
                if msg.text().trim_end().len() == 11 {
                    bungiename(&msg, &mut client, &pool, msg.sender().name().to_string()).await?;
                } else {
                    let (_, twitch_name) = msg
                        .text()
                        .split_once(' ')
                        .expect("How did it panic, what happened? // Always is something here");

                    let mut twitch_name = twitch_name.to_string();
                    if twitch_name.starts_with('@') {
                        twitch_name.remove(0);
                    }

                    bungiename(&msg, &mut client, &pool, twitch_name).await?;
                }

                Ok(())
            };

            Box::pin(fut)
        },
        "Shows registered Bungie name",
        "!bungiename [@twitchname]",
        "bungiename",
        PermissionLevel::User,
    ))
}

pub fn toggle_combined() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |msg, client, _pool, bot_state| {
            Box::pin(async move {
                let mut bot_state = bot_state.write().await;
                bot_state.toggle_combined_queue(&msg, client).await?;
                Ok(()) 
            })
        },
        "Toggle the state of combined queue (Need to have added streaming together to work)",
        "!toggle_combined",
        "toggle_combined",
        PermissionLevel::Moderator
    ))
}

pub fn streaming_together() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, _pool, bot_state| {
            Box::pin(async move {
                let vec_msg: Vec<&str> = words(&msg);
                if vec_msg.len() < 3 {
                    send_message(&msg, client.lock().await.borrow_mut(), "Usage: !streaming_together @main_channel @other_channel1 @other_channel2 ...").await?;
                    return Ok(());
                }
                let main_channel = format!("#{}", vec_msg[1].strip_prefix('@').unwrap_or(&vec_msg[1]).to_ascii_lowercase());
                
                let other_channels: HashSet<String> = vec_msg[2..].iter().map(|channel| {
                    format!("#{}", channel.strip_prefix('@').unwrap_or(channel).to_ascii_lowercase())
                }).collect();

                let mut bot_state = bot_state.write().await;

                let (queue_length, team_size) = if let Some(cfg) = bot_state.config.get_channel_config(&main_channel) {
                    (cfg.len, cfg.teamsize)
                } else {
                    (5, 3) // default values if config missing
                };

                bot_state.shared_groups.insert(
                    main_channel.clone(),
                    SharedQueueGroup::new(main_channel.clone(), other_channels.clone(), queue_length, team_size),
                );
                // Update reverse lookup channel_to_main map for all members
                for member in &other_channels {
                    bot_state.channel_to_main.insert(member.clone(), main_channel.clone());
                }
                // Also map main channel to itself
                bot_state.channel_to_main.insert(format!("#{}", main_channel.clone()), main_channel.clone());

                drop(bot_state);
                send_message(&msg, client.lock().await.borrow_mut(), &format!("Streaming together groups set: main channel '{main_channel}' with others {:?}", other_channels)).await?;

                Ok(())

            })
        },
        "Manually define groups of streamers streaming together (shared queue)",
        "!streaming_together @main_channel @other_channel1 @other_channel2 ...",
        "streaming_together",
        PermissionLevel::Moderator
    ))
}