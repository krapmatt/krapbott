
use crate::bot_commands;
use crate::bot_commands::announcement;
use crate::bot_commands::ban_player_from_queue;
use crate::bot_commands::get_twitch_user_id;
use crate::bot_commands::modify_command;
use crate::bot_commands::process_queue_entry;
use crate::bot_commands::unban_player_from_queue;
use crate::models::CommandAction;
use crate::models::TemplateManager;
use crate::models::TwitchUser;
use crate::BotConfig;
use crate::bot_commands::register_user;
use std::collections::HashSet;
use std::{borrow::BorrowMut, collections::HashMap, sync::Arc};

use crate::{bot::BotState, bot_commands::{bungiename, is_moderator, send_message}, models::{BotError, PermissionLevel}};
use async_sqlite::rusqlite::params;
use async_sqlite::Client as SqliteClient;
use chrono::FixedOffset;
use futures::future::BoxFuture;
use serde::Deserialize;
use tmi::Privmsg;
use tokio::sync::Mutex;


type CommandHandler = Arc<dyn Fn(Privmsg<'static>, Arc<Mutex<tmi::Client>>, SqliteClient, Arc<Mutex<BotState>>) -> BoxFuture<'static, Result<(), BotError>> + Send + Sync>;

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
    pub handler: CommandHandler
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
        &*ANNOUNCMENT,
    ];
}

lazy_static::lazy_static! {
    pub static ref ANNOUNCMENT: CommandGroup = CommandGroup { name: "Announcment".to_string(), 
        command: vec![
            ("!add_announcement".to_string(), add_announcement()),
            ("!remove_announcement".to_string(), remove_announcement()),
            ("!play_announcement".to_string(), play_announcement()),    
        ].into_iter().collect() 
    };
    pub static ref TIME: CommandGroup = CommandGroup { name: "Time".to_string(), 
        command: vec![
            ("!mattbed".to_string(), matt_time()),
            ("!samoanbed".to_string(), samosa_time()),  
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
        handler: Arc::new(move |msg: Privmsg<'_>, client: Arc<Mutex<tmi::Client>>, conn, _bot_state| {
            let template_manager = TemplateManager {conn: Arc::new(conn)};
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
    }
}

pub fn delete_template() -> Command {
    Command {
        permission: PermissionLevel::Moderator,
        handler: Arc::new(move |msg: Privmsg<'_>, client: Arc<Mutex<tmi::Client>>, conn, _bot_state| {
            let template_manager = TemplateManager {conn: Arc::new(conn)};
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
    }
}

pub fn lurk() -> Command {
    Command {
        permission: PermissionLevel::User,
        handler: Arc::new(move |msg: Privmsg<'_>, client: Arc<Mutex<tmi::Client>>, conn, _bot_state| {
            let template_manager = TemplateManager {conn: Arc::new(conn)};
            let fut = async move {
                // Fetch template from the database
                let template = template_manager
                    .get_template("Lurk".to_string(), "!lurk".to_string(), Some(msg.channel().to_string())).await.unwrap_or("Thank you %sender% for lurking!".to_string());

                // Generate variables
                let variables = generate_variables(&msg);

                // Replace placeholders
                let reply = parse_template(&template, &variables);

                // Send the reply
                client.lock().await.privmsg(msg.channel(), &reply).send().await?;
                Ok(())
            };
            Box::pin(fut)
        }),
    }
}

pub fn so() -> Command {
    Command {
        permission: PermissionLevel::Vip,
        handler: Arc::new(move |msg: Privmsg<'_>, client: Arc<Mutex<tmi::Client>>, conn, bot_state| {
            let template_manager = TemplateManager {conn: Arc::new(conn)};
            let fut = async move {
                let template = template_manager
                    .get_template("Shoutout".to_string(), "!so".to_string(), Some(msg.channel().to_string())).await.unwrap_or("Let's give a big Shoutout to https://www.twitch.tv/%receiver% ! Make sure to check them out and give them a FOLLOW <3! They are amazing person!".to_string());
                let variables = generate_variables(&msg);

                let reply = parse_template(&template, &variables);
                let bot_state = bot_state.lock().await;
                let twitch_name: Vec<&str> = msg.text().split_ascii_whitespace().collect();
                let mut twitch_name = twitch_name[1].to_string();
                if twitch_name.contains("@") {
                    twitch_name.remove(0);
                }
                bot_commands::shoutout(&bot_state.oauth_token_bot, bot_state.clone().bot_id, &get_twitch_user_id(&twitch_name).await?, msg.channel_id()).await;
                client.lock().await.privmsg(msg.channel(), &reply).send().await?;
                Ok(())
            };
            Box::pin(fut)
        }),
    }
}

fn mod_unban() -> Command {
    Command {
        permission: PermissionLevel::Moderator,
        handler: Arc::new(|msg, client, conn, _bot_state| {
        let fut = async move {
            unban_player_from_queue(&msg, client, &conn).await?;
            Ok(())
        };
        Box::pin(fut)
    })}
}
fn mod_ban() -> Command {
    Command {
        permission: PermissionLevel::Moderator,
        handler: Arc::new(|msg, client, conn, _bot_state| {
        let fut = async move {
            ban_player_from_queue(&msg, client, &conn).await?;
            Ok(())
        };
        Box::pin(fut)
    })}
}
fn addglobalcommand() -> Command {
    Command {
        permission: PermissionLevel::Moderator,
        handler: Arc::new(|msg, client, conn, _bot_state| {
        let fut = async move {
            modify_command(&msg, client, conn, CommandAction::AddGlobal, None).await?;
            Ok(())
        };
        Box::pin(fut)
    })}
}
fn removecommand() -> Command {
    Command {
        permission: PermissionLevel::Moderator,
        handler: Arc::new(|msg, client, conn, _bot_state| {
        let fut = async move {
                modify_command(&msg, client, conn, CommandAction::Remove, Some(msg.channel().to_string())).await?;
            Ok(())
        };
        Box::pin(fut)
    })}
}

fn addcommand() -> Command {
    Command {
        permission: PermissionLevel::Moderator,
        handler: Arc::new(|msg, client, conn, _bot_state| {
        let fut = async move {
            modify_command(&msg, client, conn, CommandAction::Add, Some(msg.channel().to_string())).await?;
            Ok(())
        };
        Box::pin(fut)
    })}
}

fn mod_config() -> Command {
    Command {
        permission: PermissionLevel::Moderator,
        handler: Arc::new(|msg, client, _conn, bot_state| {
        let fut = async move {
            if is_moderator(&msg, Arc::clone(&client)).await {
                let mut bot_state = bot_state.lock().await;
                let config = bot_state.config.get_channel_config(msg.channel());
                let reply = format!("Queue: {} || Length: {} || Fireteam size: {} || Packages: {}", config.open, config.len, config.teamsize, config.packages.join(", ").to_string());
                send_message(&msg, client.lock().await.borrow_mut(), &reply).await?;
            }
            Ok(())
        };
        Box::pin(fut)
    })}
}

fn connect() -> Command {
    Command {
        permission: PermissionLevel::Moderator,
        handler: Arc::new(|msg, client, _conn, bot_state| {
        let fut = async move {
            let mut client = client.lock().await;
            if let Some((_, channel)) = msg.text().split_once(" ") {
                
                let mut channel = format!("#{}", channel.to_string().to_ascii_lowercase());
                if channel.contains("@") {
                    channel.remove(1);
                }
                bot_state.lock().await.config.get_channel_config(&channel);
                client.join(format!("#{}", channel)).await?;
                bot_state.lock().await.config.save_config();
                client.privmsg(msg.channel(), &format!("I have connected to channel {}", channel)).send().await?;
            } else {
                client.privmsg(msg.channel(), "You didn't write the channel to connect to").send().await?;
            }
            Ok(())
        };
        Box::pin(fut)
    })}
}

fn bungie_name() -> Command {
    Command {
        permission: PermissionLevel::User,
        handler: Arc::new(|msg, client, conn, _bot_state| {
        let fut = async move {
            let mut client = client.lock().await;
            if msg.text().trim_end().len() == 11 {
                bungiename(&msg, &mut client , &conn, msg.sender().name().to_string()).await?;
            } else {
                let (_, twitch_name) = msg.text().split_once(" ").expect("How did it panic, what happened? //Always is something here");
                let mut twitch_name = twitch_name.to_string();
                if twitch_name.starts_with("@") {
                    twitch_name.remove(0);
                }
                bungiename(&msg, &mut client, &conn, twitch_name).await?;
            }
            Ok(())
        };
        Box::pin(fut)
    })}
}

fn register() -> Command {
    Command {
        permission: PermissionLevel::User,
        handler: Arc::new(|msg, client, conn, _bot_state| {
        let fut = async move {
            let reply;
                if let Some((_, bungie_name)) = msg.text().split_once(" ") {
                    reply = register_user(&conn, &msg.sender().name(), bungie_name).await?;
                } else {
                    reply = "Invalid command format! Use: !register bungiename#1234".to_string();
                }
                send_message(&msg, client.lock().await.borrow_mut(), &reply).await?;
            Ok(())
        };
        Box::pin(fut)
    })}
}

fn mod_register() -> Command {
    Command {
        permission: PermissionLevel::Moderator,
        handler: Arc::new(|msg, client, conn, _bot_state| {
        let fut = async move {
            let words: Vec<&str> = msg.text().split_whitespace().collect();
            let reply;
            if words.len() >= 3 {
                let mut twitch_name = words[1].to_string();
                let bungie_name = &words[2..].join(" ");
                if twitch_name.starts_with("@") {
                    twitch_name.remove(0);
                }
                reply = register_user(&conn, &twitch_name, bungie_name).await?;
            } else {
                reply = "You are a mod. . . || If you forgot use: !mod_register twitchname bungoname".to_string();
            }
            send_message(&msg, client.lock().await.borrow_mut(), &reply).await?;
            Ok(())
        };
        Box::pin(fut)
    })}
}

fn total() -> Command  {
    Command {
        permission: PermissionLevel::User,
        handler: Arc::new(|msg, client, conn, bot_state| {
        let fut = async move {
            bot_state.lock().await.total_raid_clears(&msg, client.lock().await.borrow_mut(), &conn).await?;
            Ok(())
        };
        Box::pin(fut)
    })}
}

fn random() -> Command {
    Command {
        permission: PermissionLevel::Moderator,
        handler: Arc::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, conn: SqliteClient, bot_state: Arc<Mutex<BotState>>| {
        let fut = async move {
            let mut bot_state = bot_state.lock().await;
            bot_state.random(&msg, client, &conn).await?;
            Ok(())
        };
        Box::pin(fut)
    })}
}

fn join_cmd() -> Command { 
    Command {
        permission: PermissionLevel::Follower,
        handler: Arc::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, conn: SqliteClient, bot_state: Arc<Mutex<BotState>>| {
        let fut = async move {
            let mut bot_state = bot_state.lock().await;
            bot_state.handle_join(&msg, client, &conn).await?;
            Ok(())
        };
        Box::pin(fut)
    })}
}

fn toggle_combine() -> Command { 
    Command {
        permission: PermissionLevel::Moderator,
        handler: Arc::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, _conn: SqliteClient, bot_state: Arc<Mutex<BotState>>| {
        let fut = async move {
            let mut bot_state = bot_state.lock().await;
            bot_state.toggle_combined_queue(&msg, client).await?;
            Ok(())
        };
        Box::pin(fut)
    })}
}
/// Add manually Streamers streaming together
/// 
/// Use: !streaming_together [@KrapMatt] <- Main channel [@Samoan_317,...] <- all others
fn add_streaming_together() -> Command {
    Command {
        permission: PermissionLevel::Broadcaster,
        handler: Arc::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, _conn: SqliteClient, bot_state: Arc<Mutex<BotState>>| {
        let fut = async move {
            let mut bot_state = bot_state.lock().await;
            
            let vec_msg: Vec<&str> = msg.text().split_ascii_whitespace().collect();

            
            if vec_msg.len() < 2 {
                send_message(&msg, client.lock().await.borrow_mut(), "Use: !streaming_together main_channel other channels").await?;
                return Ok(());
            }

            let main_channel = vec_msg[1].strip_prefix("@").unwrap_or(&vec_msg[1]).to_ascii_lowercase();
            
            let other_channels: HashSet<String> = vec_msg[2..].iter().map(|channel| format!("{}{}","#", channel.strip_prefix('@').unwrap_or(channel).to_ascii_lowercase())).collect();
            
            bot_state.streaming_together.insert(format!("{}{}", "#", main_channel), other_channels.clone());
            let other_channel_vec: Vec<&String> = other_channels.iter().collect();
            send_message(&msg, client.lock().await.borrow_mut(), &format!("Streaming together are now: {} and {:?}", main_channel, other_channel_vec)).await?;

            Ok(())
        };
        Box::pin(fut)
    })}
}

fn next() -> Command {
    Command {  
        permission: PermissionLevel::Moderator,
        handler: Arc::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, conn: SqliteClient, bot_state: Arc<Mutex<BotState>>| {
        let fut = async move {
            let mut bot_state = bot_state.lock().await;
            bot_state.handle_next(&msg, client, &conn).await?;
            Ok(())
        };
        Box::pin(fut)
    })}
}

fn remove() -> Command {
    Command {  
        permission: PermissionLevel::Moderator,
        handler: Arc::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, conn: SqliteClient, bot_state: Arc<Mutex<BotState>>| {
        let fut = async move {
            let mut bot_state = bot_state.lock().await;
            bot_state.handle_remove(&msg, client, &conn).await?;
            Ok(())
        };
        Box::pin(fut)
    })}
}

fn pos() -> Command {
    Command {     
        permission: PermissionLevel::User,
        handler: Arc::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, conn: SqliteClient, bot_state: Arc<Mutex<BotState>>| {
        let fut = async move {
            let mut bot_state = bot_state.lock().await;
            bot_state.handle_pos(&msg, client, &conn).await?;
            Ok(())
        };
        Box::pin(fut)
    })}
}

fn move_cmd() -> Command {
    Command {    
        permission: PermissionLevel::Moderator,
        handler: Arc::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, conn: SqliteClient, bot_state: Arc<Mutex<BotState>>| {
        let fut = async move {
            bot_state.lock().await.move_groups(&msg, client, &conn).await?;
            Ok(())
        };
        Box::pin(fut)
    })}
}

fn leave() -> Command {
    Command {     
        permission: PermissionLevel::User,
        handler: Arc::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, conn: SqliteClient, bot_state: Arc<Mutex<BotState>>| {
        let fut = async move {
            let mut bot_state = bot_state.lock().await;
            bot_state.handle_leave(&msg, client, &conn).await?;
            Ok(())
        };
        Box::pin(fut)
    })}
}

fn queue_size() -> Command {
    Command {
        permission: PermissionLevel::Moderator,
        handler: Arc::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, _conn: SqliteClient, bot_state: Arc<Mutex<BotState>>| {
        let fut = async move {
            let words: Vec<&str> = msg.text().split_whitespace().collect();
            let reply;
            if words.len() == 2 && is_moderator(&msg, Arc::clone(&client)).await {
                let length = words[1].to_owned();
                let mut bot_state = bot_state.lock().await;
                let config = bot_state.config.get_channel_config(msg.channel());
                config.teamsize = length.parse().unwrap();
                bot_state.config.save_config();
                reply = format!("Queue fireteam size has been changed to {}", length);
            } else {
                reply = "Are you sure you had the right command? In case !queue_size <fireteam size>".to_string();
            }
            client.lock().await.privmsg(msg.channel(), &reply).send().await?;
            Ok(())
        };
        Box::pin(fut)
    })}
}
///Clear the queue
/// 
///Use: !clear
fn clear() -> Command {
    Command {
        permission: PermissionLevel::Moderator,
        handler: Arc::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, conn: SqliteClient, _bot_state: Arc<Mutex<BotState>>| {
            let fut = async move {
                let mut client = client.lock().await;
                let channel = msg.channel().to_owned();
                conn.conn(move |conn| Ok(conn.execute("DELETE FROM queue WHERE channel_id = ?", [channel])?)).await?;
                send_message(&msg, client.borrow_mut(), "Queue has been cleared").await?;
                Ok(())
            };
            Box::pin(fut) 
        })
    }
}
///Change the lenght of queue
/// 
///Use: !queue_len <number>
fn queue_len() -> Command {
    Command {
        permission: PermissionLevel::Moderator,
        handler: Arc::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, _conn: SqliteClient, bot_state: Arc<Mutex<BotState>>| {
            let fut = async move {
                let words: Vec<&str> = msg.text().split_whitespace().collect();
                let reply;
                if words.len() == 2 && is_moderator(&msg, Arc::clone(&client)).await {
                    let length = words[1].to_owned();
                    let mut bot_state = bot_state.lock().await;
                    bot_state.config.get_channel_config(msg.channel()).len = length.parse().unwrap();
                    bot_state.config.save_config();
                    reply = format!("Queue length has been changed to {}", length);
                } else {
                    reply = "Are you sure you had the right command? In case !queue_len <queue length>".to_string();
                }
                client.lock().await.privmsg(msg.channel(), &reply).send().await?;
                Ok(())
            };
            Box::pin(fut)
        })
    }
}
///Shows the whole queue
/// 
///Use: !queue || !list
fn queue_command() -> Command {
    Command {
        permission: PermissionLevel::User,
        handler: Arc::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, conn: SqliteClient, bot_state: Arc<Mutex<BotState>>| {
        let fut = async move {
            let mut bot_state = bot_state.lock().await;
            bot_state.handle_queue(&msg, client, &conn).await?;
            Ok(())
        };
        Box::pin(fut)
    })}
}
///Prio command to make people prioed
/// 
///Use: !prio name number of runs -> use in first group to increase the number of runs
/// 
///Use: !prio name -> moves to ,,next" group
fn prio_command() -> Command {
    Command {
        permission: PermissionLevel::Moderator,
        handler: Arc::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, conn: SqliteClient, bot_state: Arc<Mutex<BotState>>| {
        let fut = async move {
            bot_state.lock().await.prio(&msg, client, &conn).await?;
            Ok(())
        };
        Box::pin(fut)
    })}
}
///Open the queue
/// 
///Use: !open_queue
fn open_command() -> Command {
    Command {
        permission: PermissionLevel::Moderator,
        handler: Arc::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, _: SqliteClient, botstate: Arc<Mutex<BotState>>| {
        let fut = async move {
            let mut bot_state = botstate.lock().await;
            bot_state.config.get_channel_config(msg.channel()).open = true;
            send_message(&msg, client.lock().await.borrow_mut(), "The queue is now open!").await?;
            bot_state.config.save_config();
            Ok(())
        };
        Box::pin(fut) 
    })}
}
///Close the queue
/// 
/// Use: !close_queue
fn close_command() -> Command {
    Command { 
        permission: PermissionLevel::Moderator, 
        handler: Arc::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, _: SqliteClient, bot_state: Arc<Mutex<BotState>>| {
        let fut = async move {
            let mut bot_state = bot_state.lock().await;
            bot_state.config.get_channel_config(msg.channel()).open = false;
            send_message(&msg, client.lock().await.borrow_mut(), "The queue is now closed!").await?;
            bot_state.config.save_config();
            Ok(())
        };
        Box::pin(fut)
    })}
}

fn add_announcement() -> Command {
    Command { 
        permission: PermissionLevel::Moderator, 
        handler: Arc::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, conn: SqliteClient, _bot_state: Arc<Mutex<BotState>>| {
        let fut = async move {
            
            let channel = "216105918";
            
            let msg_vec: Vec<&str> = msg.text().split_ascii_whitespace().collect();
            
            let name = msg_vec[1].to_string();
            let announcment = msg_vec[2..].to_owned().join(" ");
            conn.conn(move |conn| conn.execute("INSERT INTO announcments (name, announcment, channel) VALUES (?1, ?2, ?3) ON CONFLICT(name, channel) DO UPDATE SET announcment = excluded.announcment", 
                params![name, announcment, channel])
            ).await?;
            send_message(&msg, client.lock().await.borrow_mut(), &format!("Announcement {} has been added!", msg_vec[1])).await?;
            Ok(())
        };
        Box::pin(fut)
    })}
}

fn remove_announcement() -> Command {
    Command { 
        permission: PermissionLevel::Moderator, 
        handler: Arc::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, conn: SqliteClient, _bot_state: Arc<Mutex<BotState>>| {
        let fut = async move {
            let channel = "216105918";
            let msg_vec: Vec<&str> = msg.text().split_ascii_whitespace().collect();

            let reply = if msg_vec.len() <= 1 {
                "Use: !remove_announcemnt <name>"
            } else if msg_vec.len() == 2 {
                let name = msg_vec[1].to_string();
                conn.conn(move |conn| conn.execute("DELETE FROM announcments WHERE name = ?1 AND channel = ?2", 
                    params![name, channel])
                ).await?;
                "Announcement has been removed!"
            } else {
                "How did you mess up?"
            };
            send_message(&msg, client.lock().await.borrow_mut(), &reply).await?;
            Ok(())
        };
        Box::pin(fut)
    })}
}

fn play_announcement()-> Command {
    Command { 
        permission: PermissionLevel::Moderator, 
        handler: Arc::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, conn: SqliteClient, bot_state: Arc<Mutex<BotState>>| {
        let fut = async move {
            let channel = "216105918";
            let msg_vec: Vec<&str> = msg.text().split_ascii_whitespace().collect();

            if msg_vec.len() == 2 {
                let name = msg_vec[1].to_string();
                let announ = conn.conn(move |conn| conn.query_row("SELECT announcment FROM announcments WHERE name = ?1 AND channel = ?2", 
                    params![name, channel], |row| Ok(row.get::<_, String>(0)?))
                ).await?;
                let bot_state = bot_state.lock().await;
                announcement(&channel, "1091219021", &bot_state.oauth_token_bot, bot_state.clone().bot_id, announ).await?;
            } 
            
            Ok(())
        };
        Box::pin(fut)
    })}
}


fn list_of_packages() -> Command {
    Command {
        permission: PermissionLevel::Moderator, 
        handler: Arc::new(|msg, client, _conn, bot_state| {
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
    })}
}

fn addplayertoqueue() -> Command {
    Command {
        permission: PermissionLevel::Moderator, 
        handler: Arc::new(|msg, client, conn, bot_state| {
        let fut = async move {
            let words: Vec<&str> = msg.text().split_ascii_whitespace().collect();
            let twitch_name = words[1].strip_prefix("@").unwrap_or(&words[1]).to_string();
            //add twitchname bungiename
            let user = TwitchUser {
                twitch_name: twitch_name,
                bungie_name: words[2..].join(" ").to_string(),
            };
            let queue_len = bot_state.lock().await.config.get_channel_config(msg.channel()).len;
            process_queue_entry(&msg, client.lock().await.borrow_mut(), queue_len, &conn, user, Some(msg.channel().to_string())).await?;
            Ok(())
        };
        Box::pin(fut)
    })}
}

fn matt_time() -> Command {
    Command {
        permission: PermissionLevel::User, 
        handler: Arc::new(|msg, client, _conn, _bot_state| {
        let fut = async move {
            let time = chrono::Utc::now().with_timezone(&FixedOffset::east_opt(3600).unwrap());
            send_message(&msg, client.lock().await.borrow_mut(), &format!("Matt time: {}", time.time().format("%-I:%M %p"))).await?;
            Ok(())
        };
        Box::pin(fut)
    })}
}

fn samosa_time() -> Command {
    Command {
        permission: PermissionLevel::User, 
        handler: Arc::new(|msg, client, _conn, _bot_state| {
        let fut = async move {
            let time = chrono::Utc::now().with_timezone(&FixedOffset::west_opt(3600 * 5).unwrap());
            send_message(&msg, client.lock().await.borrow_mut(), &format!("Samoan time: {}", time.time().format("%-I:%M %p"))).await?;
            println!("{}", time);
            Ok(())
        };
        Box::pin(fut)
    })}
}

fn addpackage() -> Command {
    Command {
        permission: PermissionLevel::Moderator, 
        handler: Arc::new(|msg, client, _conn, bot_state| {
        let fut = async move {
            bot_state.lock().await.add_package(&msg, client).await?;
            Ok(())
        };
        Box::pin(fut)
    })}
}