
use crate::bot_commands;
use crate::bot_commands::announcement;
use crate::bot_commands::ban_player_from_queue;
use crate::bot_commands::get_twitch_user_id;
use crate::bot_commands::modify_command;
use crate::bot_commands::process_queue_entry;
use crate::bot_commands::unban_player_from_queue;
use crate::database::initialize_currency_database;
use crate::models::AnnouncementState;
use crate::models::CommandAction;
use crate::models::TemplateManager;
use crate::models::TwitchUser;
use crate::BotConfig;
use crate::bot_commands::register_user;
use std::collections::HashSet;
use std::time::Duration;
use std::{borrow::BorrowMut, collections::HashMap, sync::Arc};

use crate::{bot::BotState, bot_commands::{bungiename, is_moderator, send_message}, models::{BotError, PermissionLevel}};
use async_sqlite::rusqlite::params;
use async_sqlite::Client as SqliteClient;
use chrono::FixedOffset;
use futures::future::BoxFuture;
use serde::Deserialize;
use sqlx::SqlitePool;
use tmi::Privmsg;
use tokio::sync::Mutex;


type CommandHandler = Arc<dyn Fn(Privmsg<'static>, Arc<Mutex<tmi::Client>>, SqlitePool, Arc<Mutex<BotState>>) -> BoxFuture<'static, Result<(), BotError>> + Send + Sync>;

#[derive(Deserialize)]
pub struct CommandConfig {
    pub command_group: HashMap<String, Vec<String>>,
}
pub struct CommandGroup {
    pub name: String,
    pub command: HashMap<String, Command>
}

#[derive(Clone)]
pub struct Command {
    pub permission: PermissionLevel,
    pub handler: CommandHandler,
    pub description: String,
    pub usage: String,
}

lazy_static::lazy_static! {
    pub static ref COMMAND_GROUPS: Vec<&'static CommandGroup> = vec![
        &*QUEUE_COMMANDS,
        &*SHOUTOUT,
        &*LURK,
        &*RANDOM_QUEUE,
        &*BUNGIE_API,
        &*DATABASE_FOR_QUEUE,
        &*TIME,
        &*MODERATION,
        &*ANNOUNCEMENT,
    ];
}

lazy_static::lazy_static! {
    pub static ref ANNOUNCEMENT: CommandGroup = CommandGroup { name: "Announcement".to_string(), 
        command: vec![
            ("!add_announcement".to_string(), add_announcement()),
            ("!remove_announcement".to_string(), remove_announcement()),
            ("!play_announcement".to_string(), play_announcement()),
            ("!announcement_interval".to_string(), announcement_freq()),
            ("!announcement_state".to_string(), announcement_state()),    

        ].into_iter().collect() 
    };
    pub static ref TIME: CommandGroup = CommandGroup { name: "Time".to_string(), 
        command: vec![
            ("!mattbed".to_string(), matt_time()),
            ("!samoanbed".to_string(), samosa_time()),
            ("!cindibed".to_string(), cindi_time()),  

        ].into_iter().collect() 
    };
    pub static ref QUEUE_COMMANDS: CommandGroup = CommandGroup { name: "Queue".to_string(), 
        command: vec![
            ("!clear".to_string(), clear()),
            ("!queue_len".to_string(), queue_len()),
            ("!queue_size".to_string(), queue_size()),
            ("!join".to_string(), join_cmd()),
            ("!next".to_string(), next()),
            ("!remove".to_string(), remove()),
            ("!pos".to_string(), pos()),
            ("!leave".to_string(), leave()),
            ("!move".to_string(), move_cmd()),
            ("!queue".to_string(), queue_command()),
            ("!list".to_string(), queue_command()),
            ("!prio".to_string(), prio_command()),
            ("!bribe".to_string(), prio_command()),
            ("!open".to_string(), open_command()),
            ("!open_queue".to_string(), open_command()),
            ("!close".to_string(), close_command()),
            ("!close_queue".to_string(), close_command()),
            ("!add".to_string(), addplayertoqueue()),
            ("!toggle_combined".to_string(), toggle_combine()),
        ].into_iter().collect() 
    };
    pub static ref SHOUTOUT: CommandGroup = CommandGroup { name: "Shoutout".to_string(), 
        command: vec![
            ("!so".to_string(), so())
        ].into_iter().collect() 
    };

    pub static ref LURK: CommandGroup = CommandGroup { name: "Lurk".to_string(), 
        command: vec![
            ("!lurk".to_string(), lurk())
        ].into_iter().collect() 
    };

    pub static ref RANDOM_QUEUE: CommandGroup = CommandGroup { name: "Random Queue".to_string(), 
        command: vec![
            ("!random".to_string(), random()),
        ].into_iter().collect() 
    };

    pub static ref BUNGIE_API: CommandGroup = CommandGroup { name: "Bungie API".to_string(), 
        command: vec![
            ("!total".to_string(), total()),
        ].into_iter().collect() 
    };

    pub static ref DATABASE_FOR_QUEUE: CommandGroup = CommandGroup { name: "Database for queue".to_string(), 
        command: vec![
            ("!register".to_string(), register()),
            ("!mod_register".to_string(), mod_register()),
            ("!bungiename".to_string(), bungie_name()),
            ("!add_to_database".to_string(), add_manually_to_database()),
        ].into_iter().collect() 
    };

    pub static ref MODERATION: CommandGroup = CommandGroup { name: "Moderation".to_string(), 
        command: vec![
            ("!connect".to_string(), connect()),
            ("!mod_config".to_string(), mod_config()),
            ("!addcommand".to_string(), addcommand()),
            ("!removecommand".to_string(), removecommand()),
            ("!addglobalcommand".to_string(), addglobalcommand()),
            ("!mod_ban".to_string(), mod_ban()),
            ("!mod_unban".to_string(), mod_unban()),
            ("!add_package".to_string(), addpackage()),
            ("!packages".to_string(), list_of_packages()),
            ("!set_template".to_string(), set_template()),
            ("!remove_template".to_string(), delete_template()),
            ("!streaming_together".to_string(), add_streaming_together()),
            ("!mod_reset".to_string(), mod_reset()),
            ("!help".to_string(), help_command())
        ].into_iter().collect() 
    };
}

pub fn create_command_dispatcher(config: &BotConfig, channel_name: &str) -> HashMap<String, Command> {
    let mut commands: HashMap<String, Command> = HashMap::new();
    if let Some(channel_config) = config.channels.get(channel_name) {
        let available_packages = &*COMMAND_GROUPS;
        
        for package_name in &channel_config.packages {
            if let Some(group) = available_packages.iter().find(|g| &g.name == package_name) {
                commands.extend(group.command.clone());
            }
        }
    }
   // !join 󠀀

    commands
}

fn parse_template(template: &str, variables: &HashMap<String, String>) -> String {
    let mut result = template.to_string();
    for (key, value) in variables {
        let placeholder = format!("%{}%", key);
        result = result.replace(&placeholder, value);
    }
    result
}

fn generate_variables(msg: &Privmsg<'_>) -> HashMap<String, String> {
    let mut variables = HashMap::new();
    variables.insert("sender".to_string(), msg.sender().name().to_string());
    variables.insert("channel".to_string(), msg.channel().to_string());
    variables.insert("receiver".to_string(), {
        let words:Vec<&str> = msg.text().split_ascii_whitespace().collect();
        if words.len() > 1 {
            let mut twitch_name = words[1].to_string();
            if twitch_name.starts_with("@") {
                twitch_name.remove(0);
            }
            twitch_name
        } else {
            "Nothing".to_string()
        }
        
    });
    variables
}

pub fn set_template() -> Command {
    Command {
        permission: PermissionLevel::Moderator,
        handler: Arc::new(move |msg: Privmsg<'_>, client: Arc<Mutex<tmi::Client>>, pool, _bot_state| {
            let template_manager = TemplateManager {pool: pool.clone().into()};
            let fut = async move {
                let args: Vec<&str> = msg.text().splitn(4, ' ').collect();
                if args.len() < 4 {
                    client.lock().await.privmsg(msg.channel(), "Usage: !set_template <package> <command> <template>").send().await?;
                    return Ok(());
                }

                let package = args[1].to_string();
                let command = args[2].to_string();
                let template = args[3].to_string();

                // Update template in the database
                template_manager
                    .set_template(package, command, template, Some(msg.channel().to_string()))
                    .await?;

                client.lock().await.privmsg(msg.channel(), "Template updated successfully!").send().await?;
                Ok(())
            };
            Box::pin(fut)
        }),
        description: "Sets the template for a command with available template.".to_string(),
        usage: "!set_template <package> <command> <template>".to_string()
    }
}

pub fn delete_template() -> Command {
    Command {
        permission: PermissionLevel::Moderator,
        handler: Arc::new(move |msg: Privmsg<'_>, client: Arc<Mutex<tmi::Client>>, pool, _bot_state| {
            let template_manager = TemplateManager {pool: pool.clone().into()};
            let fut = async move {
                let args: Vec<&str> = msg.text().splitn(2, ' ').collect();
                if args.len() < 2 {
                    client.lock().await.privmsg(msg.channel(), "Usage: !remove_template <command>").send().await?;
                    return Ok(());
                }

                
                let command = args[1].to_string();
                

                // Update template in the database
                template_manager
                    .remove_template(command, Some(msg.channel().to_string()))
                    .await?;

                client.lock().await.privmsg(msg.channel(), "Template updated successfully!").send().await?;
                Ok(())
            };
            Box::pin(fut)
        }),
        description: "Deletes template for given command".to_string(),
        usage: "!remove_template <command>".to_string()
    }
}

fn join_cmd() -> Command { 
    Command {
        permission: PermissionLevel::Follower,
        handler: Arc::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, pool, bot_state: Arc<Mutex<BotState>>| {
            let fut = async move {
                let mut bot_state = bot_state.lock().await;
                bot_state.handle_join(&msg, client, &pool).await?;
                Ok(())
            };
            Box::pin(fut)
        }),
        description: "Join the queue".to_string(),
        usage: "!join bungiename#0000".to_string()
    }
}

pub fn lurk() -> Command {
    Command {
        permission: PermissionLevel::User,
        handler: Arc::new(move |msg: Privmsg<'_>, client: Arc<Mutex<tmi::Client>>, pool, _bot_state| {
            let template_manager = TemplateManager {pool: pool.clone().into()};
            let fut = async move {
                // Fetch template from the database
                let template = template_manager
                    .get_template("Lurk".to_string(), "!lurk".to_string(), Some(msg.channel().to_string())).await.unwrap_or("Thank you %sender% for lurking!".to_string());

                // Generate variables
                let variables = generate_variables(&msg);

                // Replace placeholders
                let reply = parse_template(&template, &variables);

                // Send the reply
                {send_message(&msg, client.lock().await.borrow_mut(), &reply).await?}
                Ok(())
            };
            Box::pin(fut)
        }),
        description: "Just a lurk command. Has template".to_string(),
        usage: "!lurk".to_string()
    }
}

pub fn so() -> Command {
    Command {
        permission: PermissionLevel::Vip,
        handler: Arc::new(|msg: Privmsg<'_>, client: Arc<Mutex<tmi::Client>>, pool, bot_state| {
            let template_manager = TemplateManager {pool: pool.clone().into()};
            let fut = async move {
                let words: Vec<&str> = msg.text().split_ascii_whitespace().collect();
                let reply =  if words.len() == 2 {
                    let template = template_manager
                    .get_template("Shoutout".to_string(), "!so".to_string(), Some(msg.channel().to_string())).await.unwrap_or("Let's give a big Shoutout to https://www.twitch.tv/%receiver% ! Make sure to check them out and give them a FOLLOW <3! They are amazing person!".to_string());
                    let variables = generate_variables(&msg);
                    let twitch_name = words[1].strip_prefix("@").unwrap_or(words[1]).to_string();
                    {
                        let bot_state = bot_state.lock().await;
                        bot_commands::shoutout(&bot_state.oauth_token_bot, bot_state.clone().bot_id, &get_twitch_user_id(&twitch_name).await?, msg.channel_id()).await;
                        drop(bot_state);
                    }
                    parse_template(&template, &variables)
                } else {
                    "Arent you missing something?".to_string()
                };
                {
                    client.lock().await.privmsg(msg.channel(), &reply).send().await?;
                }
                Ok(())
            };
            Box::pin(fut)
        }),
        description: "Shoutout a channel. Has template".to_string(),
        usage: "!so @channel".to_string()
    }
}

fn mod_unban() -> Command {
    Command {
        permission: PermissionLevel::Moderator,
        handler: Arc::new(|msg, client, pool, _bot_state| {
            let fut = async move {
                unban_player_from_queue(&msg, client, &pool).await?;
                Ok(())
            };
            Box::pin(fut)
        }),
        description: "Unban a person".to_string(),
        usage: "!mod_unban @twitch_name".to_string()
    }
}
fn mod_ban() -> Command {
    Command {
        permission: PermissionLevel::Moderator,
        handler: Arc::new(|msg, client, pool, _bot_state| {
            let fut = async move {
                ban_player_from_queue(&msg, client, &pool).await?;
                Ok(())
            };
            Box::pin(fut)
        }),
        description: "Ban somebody from entering the queue".to_string(),
        usage: "!mod_ban @twitch_name Optional(reason)".to_string()
    }
}
fn addglobalcommand() -> Command {
    Command {
        permission: PermissionLevel::Moderator,
        handler: Arc::new(|msg, client, pool, _bot_state| {
            let fut = async move {
                modify_command(&msg, client, &pool, CommandAction::AddGlobal, None).await?;
                Ok(())
            };
            Box::pin(fut)
        }),
        description: "Add a simple command for all channels".to_string(),
        usage: "!addglobalcommand name reply".to_string()  
    }
}
fn removecommand() -> Command {
    Command {
        permission: PermissionLevel::Moderator,
        handler: Arc::new(|msg, client, pool, _bot_state| {
            let fut = async move {
                    modify_command(&msg, client, &pool, CommandAction::Remove, Some(msg.channel().to_string())).await?;
                Ok(())
            };
            Box::pin(fut)
        }),
        description: "Remove a simple command".to_string(),
        usage: "!remove_command nameOfCommand".to_string()
    }
}

fn addcommand() -> Command {
    Command {
        permission: PermissionLevel::Moderator,
        handler: Arc::new(|msg, client, pool, _bot_state| {
            let fut = async move {
                modify_command(&msg, client, &pool, CommandAction::Add, Some(msg.channel().to_string())).await?;
                Ok(())
            };
            Box::pin(fut)
        }),
        description: "Add a simple command to this channel".to_string(),
        usage: "!addcommnad nameOfCommand reply".to_string()
    }
}

fn mod_config() -> Command {
    Command {
        permission: PermissionLevel::Moderator,
        handler: Arc::new(|msg, client, _pool, bot_state| {
            let fut = async move {
                if is_moderator(&msg, Arc::clone(&client)).await {
                    let mut bot_state = bot_state.lock().await;
                    let config = bot_state.config.get_channel_config(msg.channel());
                    let queue_reply = format!("Queue -> Open: {} || Length: {} || Fireteam size: {} || Combined: {} & Queue channel: {}", config.open, config.len, config.teamsize, config.combined, config.queue_channel);
                    let package_reply = format!("Packages: {}", config.packages.join(", ").to_string());
                    drop(bot_state);
                    let reply = vec![queue_reply, package_reply];
                    for reply in reply {
                        client.lock().await.privmsg(msg.channel(), &reply).reply_to(msg.id()).send().await?;
                    }
                }
                Ok(())
            };
            Box::pin(fut)
        }),
        description: "Shows the settings of ones queue and packages".to_string(),
        usage: "!mod_config".to_string()
    }
}

fn connect() -> Command {
    Command {
        permission: PermissionLevel::Broadcaster,
        handler: Arc::new(|msg, client, _pool, bot_state| {
            let fut = async move {
                if let Some((_, channel)) = msg.text().split_once(" ") {
                    
                    let mut channel = format!("#{}", channel.to_string().to_ascii_lowercase());
                    if channel.contains("@") {
                        channel.remove(1);
                    }
                    {
                        let mut bot_state = bot_state.lock().await;
                        bot_state.config.get_channel_config(&channel);
                        bot_state.config.save_config();
                    }
                    
                    send_message(&msg, client.lock().await.borrow_mut(), &format!("I have connected to channel {}", channel)).await?;
                } else {
                    send_message(&msg, client.lock().await.borrow_mut(), "You didn't write the channel to connect to").await?;
                }
                Ok(())
            };
            Box::pin(fut)
        }),
        description: "Connect krapbott to a new twitch channel".to_string(),
        usage: "!connect @twitchname".to_string()
    }
}

fn bungie_name() -> Command {
    Command {
        permission: PermissionLevel::User,
        handler: Arc::new(|msg, client, pool, _bot_state| {
            let fut = async move {
                let mut client = client.lock().await;
                if msg.text().trim_end().len() == 11 {
                    bungiename(&msg, &mut client , &pool, msg.sender().name().to_string()).await?;
                } else {
                    let (_, twitch_name) = msg.text().split_once(" ").expect("How did it panic, what happened? //Always is something here");
                    let mut twitch_name = twitch_name.to_string();
                    if twitch_name.starts_with("@") {
                        twitch_name.remove(0);
                    }
                    bungiename(&msg, &mut client, &pool, twitch_name).await?;
                }
                Ok(())
            };
            Box::pin(fut)
        }),
        description: "Shows bungie name".to_string(),
        usage: "!bungiename @twitchname".to_string()
    }
}

fn register() -> Command {
    Command {
        permission: PermissionLevel::User,
        handler: Arc::new(|msg, client, pool, _bot_state| {
            let fut = async move {
                let reply;
                    if let Some((_, bungie_name)) = msg.text().split_once(" ") {
                        reply = register_user(&pool, &msg.sender().name(), bungie_name).await?;
                    } else {
                        reply = "Invalid command format! Use: !register bungiename#1234".to_string();
                    }
                    send_message(&msg, client.lock().await.borrow_mut(), &reply).await?;
                Ok(())
            };
            Box::pin(fut)
        }),
        description: "Register your bungiename with bot".to_string(),
        usage: "!register bungiename#1234".to_string()
    }
}

fn mod_register() -> Command {
    Command {
        permission: PermissionLevel::Moderator,
        handler: Arc::new(|msg, client, pool, _bot_state| {
            let fut = async move {
                let words: Vec<&str> = msg.text().split_whitespace().collect();
                let reply;
                if words.len() >= 3 {
                    let mut twitch_name = words[1].to_string();
                    let bungie_name = &words[2..].join(" ");
                    if twitch_name.starts_with("@") {
                        twitch_name.remove(0);
                    }
                    reply = register_user(&pool, &twitch_name, bungie_name).await?;
                } else {
                    reply = "You are a mod. . . || If you forgot use: !mod_register twitchname bungoname".to_string();
                }
                send_message(&msg, client.lock().await.borrow_mut(), &reply).await?;
                Ok(())
            };
            Box::pin(fut)
        }),
        description: "Mod register user to database".to_string(),
        usage: "!mod_register twitchname bungoname".to_string()
    }
}

fn total() -> Command  {
    Command {
        permission: PermissionLevel::User,
        handler: Arc::new(|msg, client, pool, bot_state| {
            let fut = async move {
                bot_state.lock().await.total_raid_clears(&msg, client.lock().await.borrow_mut(), &pool).await?;
                Ok(())
            };
            Box::pin(fut)
        }),
        description: "Shows all the raid clears of bungie name".to_string(),
        usage: "!total Optional<Bungiename>".to_string()
    }
}

fn random() -> Command {
    Command {
        permission: PermissionLevel::Moderator,
        handler: Arc::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, pool, bot_state: Arc<Mutex<BotState>>| {
            let fut = async move {
                let mut bot_state = bot_state.lock().await;
                bot_state.random(&msg, client, &pool).await?;
                Ok(())
            };
            Box::pin(fut)
        }),
        description: "Select random users in queue".to_string(),
        usage: "!random".to_string()
    }
}

fn toggle_combine() -> Command { 
    Command {
        permission: PermissionLevel::Moderator,
        handler: Arc::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, _, bot_state: Arc<Mutex<BotState>>| {
            let fut = async move {
                let mut bot_state = bot_state.lock().await;
                bot_state.toggle_combined_queue(&msg, client).await?;
                Ok(())
            };
            Box::pin(fut)
        }),
        description: "Toggle the state of combined queue (Need to have added streaming together to work)".to_string(),
        usage: "!toggle_combined".to_string()
    }
}
/// Add manually Streamers streaming together
/// 
/// Use: !streaming_together [@KrapMatt] <- Main channel [@Samoan_317,...] <- all others
fn add_streaming_together() -> Command {
    Command {
        permission: PermissionLevel::Broadcaster,
        handler: Arc::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, _, bot_state: Arc<Mutex<BotState>>| {
            let fut = async move {
                let vec_msg: Vec<&str> = msg.text().split_ascii_whitespace().collect();

                
                if vec_msg.len() < 2 {
                    send_message(&msg, client.lock().await.borrow_mut(), "Use: !streaming_together main_channel other channels").await?;
                    return Ok(());
                }

                let main_channel = vec_msg[1].strip_prefix("@").unwrap_or(&vec_msg[1]).to_ascii_lowercase();
                
                let other_channels: HashSet<String> = vec_msg[2..].iter().map(|channel| format!("{}{}","#", channel.strip_prefix('@').unwrap_or(channel).to_ascii_lowercase())).collect();
                let mut bot_state = bot_state.lock().await;
                bot_state.streaming_together.insert(format!("{}{}", "#", main_channel), other_channels.clone());
                drop(bot_state);
                let other_channel_vec: Vec<&String> = other_channels.iter().collect();
                send_message(&msg, client.lock().await.borrow_mut(), &format!("Streaming together are now: {} and {:?}", main_channel, other_channel_vec)).await?;

                Ok(())
            };
            Box::pin(fut)
        }),
        description: "Add manually Streamers streaming together".to_string(),
        usage: "!streaming_together [@KrapMatt] <- Main channel [@Samoan_317,...] <- all others".to_string()
    }
}

fn next() -> Command {
    Command {  
        permission: PermissionLevel::Broadcaster,
        handler: Arc::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, pool, bot_state: Arc<Mutex<BotState>>| {
            let fut = async move {
                let mut bot_state = bot_state.lock().await;
                let reply = bot_state.handle_next(msg.channel().to_string(), &pool).await?;
                send_message(&msg, client.lock().await.borrow_mut(), &reply).await?;
                Ok(())
            };
            Box::pin(fut)
        }),
        description: "Pushed next group to live group".to_string(),
        usage: "!next".to_string()
    }
}

fn remove() -> Command {
    Command {  
        permission: PermissionLevel::Moderator,
        handler: Arc::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, pool, bot_state: Arc<Mutex<BotState>>| {
            let fut = async move {
                let mut bot_state = bot_state.lock().await;
                bot_state.handle_remove(&msg, client, &pool).await?;
                Ok(())
            };
            Box::pin(fut)
        }),
        description: "Force remove a player from queue".to_string(),
        usage: "!remove @twitchname".to_string()
    }
}

fn pos() -> Command {
    Command {     
        permission: PermissionLevel::User,
        handler: Arc::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, pool, bot_state: Arc<Mutex<BotState>>| {
            let fut = async move {
                let mut bot_state = bot_state.lock().await;
                bot_state.handle_pos(&msg, client, &pool).await?;
                Ok(())
            };
            Box::pin(fut)
        }),
        description: "Show position in queue".to_string(),
        usage: "!pos".to_string()
    }
}

fn move_cmd() -> Command {
    Command {    
        permission: PermissionLevel::Moderator,
        handler: Arc::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, pool, bot_state: Arc<Mutex<BotState>>| {
            let fut = async move {
                bot_state.lock().await.move_groups(&msg, client, &pool).await?;
                Ok(())
            };
            Box::pin(fut)
        }),
        description: "Move user back a group".to_string(),
        usage: "!move @twitchname".to_string()
    }
}

fn leave() -> Command {
    Command {     
        permission: PermissionLevel::User,
        handler: Arc::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, pool, bot_state: Arc<Mutex<BotState>>| {
            let fut = async move {
                let mut bot_state = bot_state.lock().await;
                bot_state.handle_leave(&msg, client, &pool).await?;
                Ok(())
            };
            Box::pin(fut)
        }),
        description: "Leave the queue".to_string(),
        usage: "!leave".to_string()
    }
}

fn queue_size() -> Command {
    Command {
        permission: PermissionLevel::Moderator,
        handler: Arc::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, _, bot_state: Arc<Mutex<BotState>>| {
            let fut = async move {
                let words: Vec<&str> = msg.text().split_whitespace().collect();
                let reply;
                
                if words.len() == 2 {
                    if let Ok(size) = words[1].parse::<usize>() {
                        let mut bot_state = bot_state.lock().await.to_owned();
                        let config = &mut bot_state.config;

                        // Update main channel's queue size
                        let channel_config = config.channels.get_mut(msg.channel());
                        if let Some(channel_config) = channel_config {
                            channel_config.teamsize = size;

                            // Check for combined queue
                            if channel_config.combined {
                                if let Some(channels) = bot_state.streaming_together.get(msg.channel()) {
                                    for channel in channels {
                                        if let Some(related_config) = config.channels.get_mut(channel) {
                                            related_config.teamsize = size;
                                        }
                                    }
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
            };
            Box::pin(fut)
        }),
        description: "Update size of group".to_string(),
        usage: "!queue_size number".to_string()
    }
}
///Clear the queue
/// 
///Use: !clear
fn clear() -> Command {
    Command {
        permission: PermissionLevel::Moderator,
        handler: Arc::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, pool, _bot_state: Arc<Mutex<BotState>>| {
            let fut = async move {
                let channel = msg.channel().to_owned();
                // Clear the queue for the given channel
                sqlx::query!("DELETE FROM queue WHERE channel_id = ?", channel).execute(&pool).await?;
                let mut client = client.lock().await;
                send_message(&msg, client.borrow_mut(), "Queue has been cleared").await?;
                Ok(())
            };
            Box::pin(fut) 
        }),
        description: "Clear the queue".to_string(),
        usage: "!clear".to_string()
    }
}
///Change the lenght of queue
/// 
///Use: !queue_len <number>
fn queue_len() -> Command {
    Command {
        permission: PermissionLevel::Moderator,
        handler: Arc::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, _, bot_state: Arc<Mutex<BotState>>| {
            let fut = async move {
                let words: Vec<&str> = msg.text().split_whitespace().collect();
                let reply;
                if words.len() == 2 {
                    if let Ok(len) = words[1].parse::<usize>() {
                        let mut bot_state = bot_state.lock().await.to_owned();
                        let config = &mut bot_state.config;
                        let channel_config = config.channels.get_mut(msg.channel());
                        if let Some(channel_config) = channel_config {
                            channel_config.len = len;
                            if channel_config.combined {
                                if let Some(channels) = bot_state.streaming_together.get(msg.channel()) {
                                    for channel in channels {
                                        if let Some(related_config) = config.channels.get_mut(channel) {
                                            related_config.len = len;
                                        }
                                    }
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
            };
            Box::pin(fut)
        }),
        description: "Change the lenght of queue".to_string(),
        usage: "!queue_len number".to_string()
    }
}
///Shows the whole queue
/// 
///Use: !queue || !list
fn queue_command() -> Command {
    Command {
        permission: PermissionLevel::User,
        handler: Arc::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, pool, bot_state: Arc<Mutex<BotState>>| {
            let fut = async move {
                let mut bot_state = bot_state.lock().await;
                bot_state.handle_queue(&msg, client, &pool).await?;
                Ok(())
            };
            Box::pin(fut)
        }),
        description: "Shows the whole queue".to_string(),
        usage: "!queue or !list".to_string()
    }
}
///Prio command to make people prioed
/// 
///Use: !prio name number of runs -> use in first group to increase the number of runs
/// 
///Use: !prio name -> moves to ,,next" group
fn prio_command() -> Command {
    Command {
        permission: PermissionLevel::Moderator,
        handler: Arc::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, pool, bot_state: Arc<Mutex<BotState>>| {
            let fut = async move {
                bot_state.lock().await.prio(&msg, client, &pool).await?;
                Ok(())
            };
            Box::pin(fut)
        }),
        description: "Prio command to make people prioed".to_string(),
        usage: "!prio name number of runs -> use in first group to increase the number of runs OR !prio name -> moves to next group".to_string()
    }
}
///Open the queue
/// 
///Use: !open_queue
fn open_command() -> Command {
    Command {
        permission: PermissionLevel::Moderator,
        handler: Arc::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, _, bot_state: Arc<Mutex<BotState>>| {
            let fut = async move {
                let mut bot_state = bot_state.lock().await.to_owned();
                let config = &mut bot_state.config;
                let channel_config = config.channels.get_mut(msg.channel());
                let reply = if let Some(channel_config) = channel_config {
                    channel_config.open = true;
                    if channel_config.combined {
                        if let Some(channels) = bot_state.streaming_together.get(msg.channel()) {
                            for channel in channels {
                                if let Some(related_config) = config.channels.get_mut(channel) {
                                    related_config.open = true;
                                }
                            }
                        }
                    }
                    config.save_config();
                    "✅ The queue is open!"
                } else {
                    "Channel configuration not found."
                };
                send_message(&msg, client.lock().await.borrow_mut(), reply).await?;
                Ok(())
            };
            Box::pin(fut) 
        }),
        description: "Open the queue".to_string(),
        usage: "!open OR !open_queue".to_string()
    }
}
///Close the queue
/// 
/// Use: !close_queue
fn close_command() -> Command {
    Command { 
        permission: PermissionLevel::Moderator, 
        handler: Arc::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, _, bot_state: Arc<Mutex<BotState>>| {
            let fut = async move {
                let mut bot_state = bot_state.lock().await.to_owned();
                let config = &mut bot_state.config;
                let channel_config = config.channels.get_mut(msg.channel());
                let reply = if let Some(channel_config) = channel_config {
                    channel_config.open = false;
                    if channel_config.combined {
                        if let Some(channels) = bot_state.streaming_together.get(msg.channel()) {
                            for channel in channels {
                                if let Some(related_config) = config.channels.get_mut(channel) {
                                    related_config.open = false;
                                }
                            }
                        }
                    }
                    config.save_config();
                    "❌ The queue is closed!"
                } else {
                    "Channel configuration not found."
                };
                send_message(&msg, client.lock().await.borrow_mut(), reply).await?;
                Ok(())
            };
            Box::pin(fut)
        }),
        description: "Close the queue".to_string(),
        usage: "!close_queue OR !close".to_string()
    }
}

fn add_announcement() -> Command {
    Command { 
        permission: PermissionLevel::Moderator, 
        handler: Arc::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, pool, _bot_state: Arc<Mutex<BotState>>| {
            let fut = async move {
                let channel = msg.channel_id().to_string();
                let msg_vec: Vec<&str> = msg.text().split_ascii_whitespace().collect();
                
                let reply = if msg_vec.len() >= 4 {
                    let state = msg_vec[1].to_lowercase();
                    let name = msg_vec[2].to_string();
                    let announcement = msg_vec[3..].join(" ");
                    // Insert or update announcement
                    sqlx::query!(
                        "INSERT INTO announcements (name, announcement, channel, state) 
                         VALUES (?, ?, ?, ?) 
                         ON CONFLICT(name, channel) 
                         DO UPDATE SET announcement = excluded.announcement",
                        name, announcement, channel, state
                    ).execute(&pool).await?;

                    format!("✅ Announcement '{}' has been added!", name)
                } else {
                    "❌ Usage: !add_announcement <state: Active/ActivityName> <name> <Message>".to_string()
                };
                send_message(&msg, client.lock().await.borrow_mut(), &reply).await?;
                Ok(())
            };
            Box::pin(fut)
        }),
        description: "Add an announcement".to_string(),
        usage: "!add_announcement <state: Active/NameofActivity> <name> <Message>".to_string()
    }
}

fn remove_announcement() -> Command {
    Command { 
        permission: PermissionLevel::Moderator, 
        handler: Arc::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, pool, _bot_state: Arc<Mutex<BotState>>| {
            let fut = async move {
                let msg_vec: Vec<&str> = msg.text().split_ascii_whitespace().collect();
                let channel_id = msg.channel_id().to_string();

                let reply = if msg_vec.len() <= 1 {
                    "❌ Usage: !remove_announcement <name>".to_string()
                } else if msg_vec.len() == 2 {
                    let name = msg_vec[1].to_string();

                    let result = sqlx::query!(
                        "DELETE FROM announcements WHERE name = ? AND channel = ?",
                        name, channel_id
                    ).execute(&pool).await?;
                    if result.rows_affected() > 0 {
                        "✅ Announcement has been removed!".to_string()
                    } else {
                        "⚠️ No announcement found with that name.".to_string()
                    }
                } else {
                    "❌ Invalid usage. Try again: !remove_announcement <name>".to_string()
                };
                send_message(&msg, client.lock().await.borrow_mut(), &reply).await?;
                Ok(())
            };
            Box::pin(fut)
        }),
        description: "Remove an annoucement".to_string(),
        usage: "!remove_announcemnt <name>".to_string()
    }
}

fn play_announcement()-> Command {
    Command { 
        permission: PermissionLevel::Moderator, 
        handler: Arc::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, pool, bot_state: Arc<Mutex<BotState>>| {
            let fut = async move {
                let msg_vec: Vec<&str> = msg.text().split_ascii_whitespace().collect();
                if msg_vec.len() != 2 {
                    return Ok(()); 
                }
                let channel_id = msg.channel_id().to_string();
                let name = msg_vec[1].to_string();
                // Fetch the announcement
                let result = sqlx::query!(
                    "SELECT announcement FROM announcements WHERE name = ? AND channel = ?",
                    name, channel_id
                ).fetch_optional(&pool).await?;

                if let Some(row) = result {
                    let announ = row.announcement;
                    let bot_state = bot_state.lock().await;
                    announcement(msg.channel_id(), "1091219021", &bot_state.oauth_token_bot, bot_state.bot_id.clone(), announ).await?;
                }

                Ok(())
            };
            Box::pin(fut)
        }),
        description: "Play an announcement".to_string(),
        usage: "!play_announcement nameOfAnnouncement".to_string()
    }
}


fn list_of_packages() -> Command {
    Command {
        permission: PermissionLevel::Moderator, 
        handler: Arc::new(|msg, client, _pool, bot_state| {
            let fut = async move {
                let mut bot_state = bot_state.lock().await; 
                let config = bot_state.config.get_channel_config(msg.channel());
                let streamer_packages = &config.packages;
                let mut missing_packages: Vec<&str>= vec![];
                for package in &*COMMAND_GROUPS {
                    if !streamer_packages.contains(&package.name) {
                        missing_packages.push(&package.name);
                    }
                }
                let reply = if missing_packages.is_empty() {
                    format!("You have all packages activated!")
                } else {
                    format!("Currently you have these packages on your channel: {}. And you can add: {}. Use: !add_package <name>", streamer_packages.join(", "), missing_packages.join(", "))
                };

                send_message(&msg, client.lock().await.borrow_mut(), &reply).await?;
                Ok(())
            };
            Box::pin(fut)
        }),
        description: "Show all included packages".to_string(),
        usage: "!packages".to_string()
    }
}

fn addplayertoqueue() -> Command {
    Command {
        permission: PermissionLevel::Moderator, 
        handler: Arc::new(|msg, client, pool, bot_state| {
            let fut = async move {
                let words: Vec<&str> = msg.text().split_ascii_whitespace().collect();
                let twitch_name = words[1].strip_prefix("@").unwrap_or(&words[1]).to_string();
                //add twitchname bungiename
                let user = TwitchUser {
                    twitch_name: twitch_name,
                    bungie_name: words[2..].join(" ").to_string(),
                };
                let queue_len = bot_state.lock().await.config.get_channel_config(msg.channel()).len;
                process_queue_entry(&msg, client.lock().await.borrow_mut(), queue_len, &pool, user, &msg.channel()).await?;
                Ok(())
            };
            Box::pin(fut)
        }),
        description: "Force Add an user to queue".to_string(),
        usage: "!add @twitchname bungiename".to_string()
    }
}

fn matt_time() -> Command {
    Command {
        permission: PermissionLevel::User, 
        handler: Arc::new(|msg, client, _pool, _bot_state| {
            let fut = async move {
                let time = chrono::Utc::now().with_timezone(&FixedOffset::east_opt(3600).unwrap());
                send_message(&msg, client.lock().await.borrow_mut(), &format!("Matt time: {}", time.time().format("%-I:%M %p"))).await?;
                Ok(())
            };
            Box::pin(fut)
        }),
        description: "Shows current time of KrapMatt".to_string(),
        usage: "!mattbed".to_string()
    }
}

fn samosa_time() -> Command {
    Command {
        permission: PermissionLevel::User, 
        handler: Arc::new(|msg, client, _pool, _bot_state| {
            let fut = async move {
                let time = chrono::Utc::now().with_timezone(&FixedOffset::west_opt(3600 * 5).unwrap());
                send_message(&msg, client.lock().await.borrow_mut(), &format!("Samoan time: {}", time.time().format("%-I:%M %p"))).await?;
                Ok(())
            };
            Box::pin(fut)
        }),
        description: "Shows current time of Sandisnapoutofit".to_string(),
        usage: "!samoanbed".to_string()
    }
}

fn cindi_time() -> Command {
    Command {
        permission: PermissionLevel::User, 
        handler: Arc::new(|msg, client, _pool, _bot_state| {
            let fut = async move {
                let time = chrono::Utc::now();
                send_message(&msg, client.lock().await.borrow_mut(), &format!("Cindi time: {}", time.time().format("%-I:%M %p"))).await?;
                Ok(())
            };
            Box::pin(fut)
        }),
        description: "Shows current time of Cindi".to_string(),
        usage: "!cindibed".to_string()
    }
}

fn addpackage() -> Command {
    Command {
        permission: PermissionLevel::Moderator, 
        handler: Arc::new(|msg, client, _pool, bot_state| {
            let fut = async move {
                bot_state.lock().await.add_package(&msg, client).await?;
                Ok(())
            };
            Box::pin(fut)
        }),
        description: "Add a package".to_string(),
        usage: "!add_package nameOfPackage".to_string()
    }
}

fn announcement_freq() -> Command {
    Command {
        permission: PermissionLevel::Moderator, 
        handler: Arc::new(|msg, client, _pool, bot_state| {
            let fut = async move {
                let msg_vec: Vec<&str> = msg.text().split_ascii_whitespace().collect();
                if msg_vec.len() == 2 {
                    let mut bot_state = bot_state.lock().await;
                    match msg_vec[1].to_string().parse() {
                        Ok(res) => {
                            bot_state.config.get_channel_config(msg.channel()).announcement_config.interval = Duration::from_secs(res);
                            bot_state.config.save_config();
                            send_message(&msg, client.lock().await.borrow_mut(), "Frequency has been updated").await?;
                        },
                        Err(_) => ()
                    }
                }

                Ok(())
            };
            Box::pin(fut)
        }),
        description: "Change interval of announcement frequency".to_string(),
        usage: "!announcement_interval numberOfSecs".to_string()
    }
}

fn announcement_state() -> Command {
    Command {
        permission: PermissionLevel::Moderator, 
        handler: Arc::new(|msg, client, _pool, bot_state| {
            let fut = async move {
                let msg_vec: Vec<&str> = msg.text().split_ascii_whitespace().collect();
                if msg_vec.len() == 2 {
                    let mut bot_state = bot_state.lock().await;
                    let mes = msg_vec[1..].join(" ").to_string().to_lowercase();
                    let state = if mes =="paused".to_owned() {
                        AnnouncementState::Paused
                    } else if mes == "active".to_owned() {
                        AnnouncementState::Active
                    } else {
                        AnnouncementState::Custom(mes)
                    };

                    send_message(&msg, client.lock().await.borrow_mut(), &format!("State of announcements is {:?}", state)).await?;
                    bot_state.config.get_channel_config(msg.channel()).announcement_config.state = state;
                    bot_state.config.save_config();
                }

                Ok(())
            };
            Box::pin(fut)
        }),
        description: "Change state of announcement (Paused, Active, nameOfActivity)".to_string(),
        usage: "!announcement_state state".to_string()
    }
}

fn mod_reset() -> Command {
    Command {
        permission: PermissionLevel::Moderator, 
        handler: Arc::new(|msg, client, _pool, bot_state| {
            let fut = async move {
                let mut bot_state = bot_state.lock().await;
                bot_state.streaming_together.clear();
                let config = bot_state.config.get_channel_config(&msg.channel());
                config.runs = 0;
                config.combined = false;
                config.queue_channel = msg.channel().to_string();
                config.announcement_config.last_sent = None;
                bot_state.config.save_config();
                send_message(&msg, client.lock().await.borrow_mut(), "Config has been reset!").await?;
                Ok(())
            };
            Box::pin(fut)
        }),
        description: "Reset bot. Recommended to do before every stream".to_string(),
        usage: "!mod_reset".to_string()
    }
}

fn find_command(command_name: &str) -> Option<(String, &Command)> {
    for group in COMMAND_GROUPS.iter() {
        if let Some(command) = group.command.get(command_name) {
            return Some((group.name.clone(), command));
        }
    }
    None
}

fn help_command() -> Command {
    Command {
        permission: PermissionLevel::User, // Allow all users to use this command
        handler: Arc::new(|msg, client, _pool, _bot_state| {
            Box::pin(async move {
                let words: Vec<&str> = msg.text().split_whitespace().collect();

                let reply = if words.len() == 2 {
                    // Specific command help
                    let command_name = words[1];
                    if let Some((package_name, command)) = find_command(command_name) {
                        format!(
                            "Command: {} || Group: {} || Description: {} || Usage: {}",
                            command_name, package_name, command.description, command.usage
                        )
                    } else {
                        format!("Unknown command: {}. Use !help for a list of available commands.", words[1])
                    }
                } else {
                    // General help
                    /*let mut help_text = String::from("Available commands:\n");
                    for group in COMMAND_GROUPS.iter() {
                        help_text.push_str(&format!("Group: {}\n", group.name));
                        for (name, command) in &group.command {
                            help_text.push_str(&format!("  !{} - {}\n", name, command.description));
                        }
                        help_text.push('\n');
                    }*/
                    "Use !help !<command> for more details about a specific command.".to_string()  
                };

                // Send the reply
                client.lock().await.privmsg(msg.channel(), &reply).send().await?;
                Ok(())
            })
        }),
        description: "Displays this help message or details about a specific command.".to_string(),
        usage: "!help [!<command>]".to_string(),
    }
}

fn add_manually_to_database() -> Command {
    Command {
        permission: PermissionLevel::Moderator, // Allow all users to use this command
        handler: Arc::new(|msg, client, pool, _bot_state| {
            let msg_text = msg.text().to_string();
            Box::pin(async move {
                let words: Vec<&str> = msg_text.split_whitespace().collect();
                let reply = if words.len() == 3 {
                    let twitch_name = words[1].strip_prefix("@").unwrap_or(words[1]).to_string();
                    let bungie_name = words[2].to_string();
                    let result = sqlx::query!(
                        "INSERT INTO user (twitch_name, bungie_name) 
                         VALUES (?, ?) 
                         ON CONFLICT(twitch_name) DO UPDATE SET bungie_name = excluded.bungie_name",
                        twitch_name, bungie_name
                    ).execute(&pool).await;
                    match result {
                        Ok(_) => format!("{} has been added as {}", twitch_name, bungie_name),
                        Err(_) => "Failed to add user to database.".to_string(),
                    }
                } else {
                    "Usage: !add_to_database @twitch_name bungie_name#0000".to_string()
                };
                send_message(&msg, client.lock().await.borrow_mut(), &reply).await?;
                Ok(())
            })
        }),
        description: "Adds a user to the database.".to_string(),
        usage: "!add_to_database @twitch_name bungie_name#0000".to_string()
    }
}
