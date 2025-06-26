pub mod traits;
pub mod oldcommands;
pub mod queue_traits;
pub mod points_traits;
pub mod announcement;
pub mod moderation;
pub mod time;
pub mod bungie_traits;

use crate::api::get_master_challenges;
use crate::api::get_membershipid;
use crate::bot_commands;
use crate::commands::announcement::add_announcement_command;
use crate::commands::announcement::announcement_freq_command;
use crate::commands::announcement::announcement_state_command;
use crate::commands::announcement::play_announcement_command;
use crate::commands::announcement::remove_announcement_command;
use crate::commands::bungie_traits::MasterChalCommand;
use crate::commands::bungie_traits::TotalCommand;
use crate::commands::moderation::addcommand;
use crate::commands::moderation::addglobalcommand;
use crate::commands::moderation::addpackage;
use crate::commands::moderation::connect;
use crate::commands::moderation::delete_template;
use crate::commands::moderation::list_of_packages;
use crate::commands::moderation::mod_ban;
use crate::commands::moderation::mod_config;
use crate::commands::moderation::mod_reset;
use crate::commands::moderation::mod_timeout;
use crate::commands::moderation::mod_unban;
use crate::commands::moderation::removecommand;
use crate::commands::moderation::removepackage;
use crate::commands::moderation::set_template;
use crate::commands::oldcommands::so;
use crate::commands::points_traits::change_duration_giveaway;
use crate::commands::points_traits::change_max_tickets_giveaway;
use crate::commands::points_traits::change_price_ticket;
use crate::commands::points_traits::get_points_command;
use crate::commands::points_traits::pull_giveaway;
use crate::commands::points_traits::ChangeMode;
use crate::commands::points_traits::ChangePointsCommand;
use crate::commands::points_traits::GiveawayHandler;
use crate::commands::points_traits::JoinGiveaway;
use crate::commands::queue_traits::addplayertoqueue;
use crate::commands::queue_traits::bungie_name_command;
use crate::commands::queue_traits::clear;
use crate::commands::queue_traits::deprio;
use crate::commands::queue_traits::leave;
use crate::commands::queue_traits::list;
use crate::commands::queue_traits::mod_register_command;
use crate::commands::queue_traits::move_user;
use crate::commands::queue_traits::pos;
use crate::commands::queue_traits::prio;
use crate::commands::queue_traits::register_command;
use crate::commands::queue_traits::remove;
use crate::commands::queue_traits::streaming_together;
use crate::commands::queue_traits::toggle_combined;
use crate::commands::queue_traits::toggle_queue;
use crate::commands::queue_traits::JoinCommand;
use crate::commands::queue_traits::NextComamnd;
use crate::commands::queue_traits::QueueLength;
use crate::commands::queue_traits::QueueSize;
use crate::commands::time::cindi_time;
use crate::commands::time::matt_time;
use crate::commands::time::samosa_time;
use crate::commands::traits::CommandT;
use crate::queue;
use crate::queue::is_valid_bungie_name;
use crate::queue::process_queue_entry;
use crate::twitch_api;
use crate::twitch_api::announcement;
use crate::twitch_api::get_twitch_user_id;
use crate::bot_commands::mod_action_user_from_queue;
use crate::bot_commands::modify_command;
use crate::bot_commands::register_user;
use crate::bot_commands::reply_to_message;
use crate::bot_commands::unban_player_from_queue;
use crate::database::load_membership;
//use crate::giveaway::{change_duration_giveaway, change_max_tickets_giveaway, change_price_ticket, handle_giveaway, join_giveaway, pull_giveaway};
use crate::models::{AnnouncementState, CommandAction, TemplateManager, TwitchUser};
use crate::BotConfig;
use crate::{
    bot::BotState,
    bot_commands::{bungiename, send_message},
    models::{BotError, PermissionLevel},
};
use chrono_tz::CET;
use chrono_tz::US::Pacific;
use futures::future::BoxFuture;
use serde::Deserialize;
use sqlx::SqlitePool;
use std::collections::HashSet;
use std::time::Duration;
use std::vec;
use std::{borrow::BorrowMut, collections::HashMap, sync::Arc};
use tmi::Privmsg;
use tokio::sync::Mutex;
use tokio::sync::RwLock;




type CommandHandler = Arc<
    dyn Fn(
            Privmsg<'static>,
            Arc<Mutex<tmi::Client>>,
            SqlitePool,
            Arc<RwLock<BotState>>,
        ) -> BoxFuture<'static, Result<(), BotError>>
        + Send
        + Sync,
>;

#[derive(Deserialize)]
pub struct CommandConfig {
    pub command_group: HashMap<String, Vec<String>>,
}
pub struct CommandGroup {
    pub name: String,
    pub commands: Vec<CommandRegistration>,
}

#[derive(Clone)]
pub struct Command {
    pub permission: PermissionLevel,
    pub handler: CommandHandler,
    pub description: String,
    pub usage: String,
}
#[derive(Clone)]
pub struct CommandRegistration {
    pub aliases: Vec<String>,
    pub command: Arc<dyn CommandT>
}

macro_rules! cmd {
    ($command:expr, $($alias:expr), +) => {
        CommandRegistration {
            aliases: vec![$($alias.to_string()), +],
            command: $command,
        }        
    };
}

lazy_static::lazy_static! {
    pub static ref COMMAND_GROUPS: HashMap<&'static str, &'static CommandGroup> = {
        let mut map = HashMap::new();
        map.insert("queue", &*QUEUE_COMMANDS);
        map.insert("shoutout", &*SHOUTOUT);
        map.insert("time", &*TIME);
        map.insert("points", &*POINTS);
        map.insert("bungie_api", &*BUNGIE_API);
        map.insert("moderation", &*MODERATION);
        map.insert("announcement", &*ANNOUNCEMENT);
        map
    };
}

lazy_static::lazy_static!{
    pub static ref QUEUE_COMMANDS: CommandGroup = CommandGroup { name: "queue".to_string(), 
        commands: vec![
            cmd!(Arc::new(JoinCommand), "j"),
            cmd!(Arc::new(NextComamnd), "next"),
            cmd!(Arc::new(QueueSize), "queue_size"),
            cmd!(Arc::new(QueueLength), "queue_len"),
            cmd!(addplayertoqueue(), "add"),
            cmd!(toggle_queue(true, "open_queue", "Opens queue", "open"),"open", "open_queue"),
            cmd!(toggle_queue(false, "close_queue", "Closes queue", "close"),"close", "close_queue"),
            cmd!(list(), "list", "queue"),
            cmd!(clear(), "clear"),
            cmd!(remove(), "remove"),
            cmd!(pos(), "pos", "p", "position"),
            cmd!(move_user(), "move"),
            cmd!(leave(), "leave", "quit"),
            cmd!(prio(), "prio"),
            cmd!(deprio(), "deprio"),
            cmd!(register_command(), "register"),
            cmd!(mod_register_command(), "mod_register"),
            cmd!(bungie_name_command(), "bungiename", "bungie"),
            cmd!(toggle_combined(), "toggle_combined"),
            cmd!(streaming_together(), "streaming_together"),
        ], 
    };
    pub static ref SHOUTOUT: CommandGroup = CommandGroup { name: "shoutout".to_string(), 
        commands: vec![
            cmd!(so(), "so", "shoutout")
        ], 
    };
    
    pub static ref TIME: CommandGroup = CommandGroup { name: "time".to_string(), 
        commands: vec![
            cmd!(matt_time(), "mattbed"),
            cmd!(samosa_time(), "samoanbed"),
            cmd!(cindi_time(), "cindibed"),
        ], 
    };

    pub static ref POINTS: CommandGroup = CommandGroup { name: "points".to_string(),
        commands: vec![
            cmd!(Arc::new(GiveawayHandler), "startgiveaway", "start_giveaway"),
            cmd!(Arc::new(JoinGiveaway), "ticket"),
            cmd!(Arc::new(ChangePointsCommand { mode: ChangeMode::Add}) as Arc<dyn CommandT>, "add_points", "add_dirt"),
            cmd!(Arc::new(ChangePointsCommand { mode: ChangeMode::Remove}) as Arc<dyn CommandT>, "remove_points", "remove_dirt"),
            cmd!(pull_giveaway(), "pull"),
            cmd!(change_max_tickets_giveaway(), "giveaway_tickets"),
            cmd!(change_duration_giveaway(), "giveaway_duration"),
            cmd!(change_price_ticket(), "giveaway_price"),
            cmd!(get_points_command(), "points", "dirt"),
        ], 
    };

    pub static ref MODERATION: CommandGroup = CommandGroup { name: "moderation".to_string(),
        commands: vec![
            cmd!(mod_unban(), "mod_unban"),
            cmd!(mod_ban(), "mod_ban"),
            cmd!(mod_timeout(), "mod_timeout"),
            cmd!(mod_config(), "mod_config"),
            cmd!(connect(), "connect"),
            cmd!(mod_reset(), "mod_reset"),
            cmd!(addpackage(), "addpackage", "add_package"),
            cmd!(removepackage(), "remove_package", "removepackage"),
            cmd!(list_of_packages(), "packages"),
            cmd!(set_template(), "set_template"),
            cmd!(delete_template(), "delete_template"),
            cmd!(addcommand(), "addcommand", "add_command"),
            cmd!(removecommand(), "remove_command", "removecommand"),
            cmd!(addglobalcommand(), "addglobalcommand", "add_globalcommand"),
        ]
    };

    pub static ref BUNGIE_API: CommandGroup = CommandGroup { name: "bungie api".to_string(),
        commands: vec![
            cmd!(Arc::new(TotalCommand), "total"),
            cmd!(Arc::new(MasterChalCommand), "cr"),
        ]
    };

    pub static ref ANNOUNCEMENT: CommandGroup = CommandGroup { name: "announcement".to_string(), 
        commands: vec![
            cmd!(add_announcement_command(), "add_announcement"),
            cmd!(remove_announcement_command(), "remove_announcement"),
            cmd!(play_announcement_command(), "play_announcement", "announce"),
            cmd!(announcement_freq_command(), "announcement_interval"),
            cmd!(announcement_state_command(), "announcement_state")
        ]
    };
}

pub fn words<'a>(msg: &'a Privmsg<'a>) -> Vec<&'a str> {
    msg.text().split_ascii_whitespace().collect()
}

pub fn normalize_twitch_name(s: &str) -> &str {
    s.strip_prefix("@").unwrap_or(s)
}
pub struct CommandOverride {
    enabled: bool,
    command_name: String
}

pub fn create_command_dispatcher(config: &BotConfig, channel_name: &str, custom_aliases: Option<&HashMap<String, CommandOverride>>) -> HashMap<String, Arc<dyn CommandT + Send + Sync>> {
    let mut commands: HashMap<String, Arc<dyn CommandT + Send + Sync>> = HashMap::new();
    let mut default_by_name: HashMap<String, Arc<dyn CommandT + Send + Sync>> = HashMap::new();

    if let Some(channel_config) = config.channels.get(channel_name) {
        for package_name in &channel_config.packages {
            if let Some(group) = COMMAND_GROUPS.get(package_name.to_lowercase().as_str()) {
                for registration in &group.commands {
                    for alias in &registration.aliases {
                        default_by_name.insert(alias.to_lowercase(), registration.command.clone());
                    }
                }
            }
        }
    }
    //Custom Aliases
    
    if let Some(custom) = custom_aliases {
        for (alias, override_data) in custom {
            if override_data.enabled {
                if let Some(cmd) = default_by_name.get(&override_data.command_name).cloned() {
                    commands.insert(alias.to_lowercase(), cmd);
                }
            } else {
                // Remove this alias if it exists (disable it)
                commands.remove(&alias.to_lowercase());
            }
        }
    }
    

    commands
}

pub fn parse_template(template: &str, variables: &HashMap<String, String>) -> String {
    let mut result = template.to_string();
    for (key, value) in variables {
        let placeholder = format!("%{}%", key);
        result = result.replace(&placeholder, value);
    }
    result
}
pub fn generate_variables(msg: &Privmsg<'_>) -> HashMap<String, String> {
    let mut variables = HashMap::new();
    variables.insert("sender".to_string(), msg.sender().name().to_string());
    variables.insert("channel".to_string(), msg.channel().to_string());
    variables.insert("receiver".to_string(), {
        let words: Vec<&str> = msg.text().split_ascii_whitespace().collect();
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

/*lazy_static::lazy_static! {
    
    pub static ref TIME: CommandGroup = CommandGroup { name: "Time".to_string(),
        command: vec![
            cmd!(matt_time(), "mattbed"),
            cmd!(samosa_time(), "samoanbed"),
            cmd!(cindi_time(), "cindibed"),

        ]
    };
    

    pub static ref BUNGIE_API: CommandGroup = CommandGroup { name: "Bungie API".to_string(),
        command: vec![
            cmd!(total(), "total"),
            cmd!(master_chal(), "cr")
        ]
    };

    pub static ref MODERATION: CommandGroup = CommandGroup { name: "Moderation".to_string(),
        command: vec![
            cmd!(connect(), "connect"),
            cmd!(mod_config(), "mod_config"),
            cmd!(addcommand(), "addcommand", "add_command"),
            cmd!(removecommand(), "removecommand", "remove_command"),
            cmd!(addglobalcommand(), "addglobalcommand", "add_globalcommand"),
            cmd!(addpackage(), "add_package", "addpackage"),
            cmd!(removepackage(), "remove_package", "removepackage"),
            cmd!(list_of_packages(), "packages"),
            cmd!(set_template(), "set_template"),
            cmd!(delete_template(), "remove_template"),
            cmd!(add_streaming_together(), "streaming_together"),
            cmd!(mod_reset(), "mod_reset"),
            cmd!(help_command(), "help"),
        ]
    };
}
fn total() -> Command {
    Command {
        permission: PermissionLevel::User,
        handler: Arc::new(|msg, client, pool, bot_state| {
            let fut = async move {
                bot_state
                    .read()
                    .await
                    .total_raid_clears(&msg, client.lock().await.borrow_mut(), &pool)
                    .await?;
                Ok(())
            };
            Box::pin(fut)
        }),
        description: "Shows all the raid clears of bungie name".to_string(),
        usage: "!total Optional<Bungiename>".to_string(),
    }
}



fn toggle_combine() -> Command {
    Command {
        permission: PermissionLevel::Moderator,
        handler: Arc::new(
            |msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, _, bot_state| {
                let fut = async move {
                    
                };
                Box::pin(fut)
            },
        ),
        description:
            "Toggle the state of combined queue (Need to have added streaming together to work)"
                .to_string(),
        usage: "!toggle_combined".to_string(),
    }
}
/// Add manually Streamers streaming together
///
/// Use: !streaming_together [@KrapMatt] <- Main channel [@Samoan_317,...] <- all others
fn add_streaming_together() -> Command {
    Command {
        permission: PermissionLevel::Broadcaster,
        handler: Arc::new(
            |msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, _, bot_state| {
                let fut = async move {
                    let vec_msg: Vec<&str> = msg.text().split_ascii_whitespace().collect();

                    if vec_msg.len() < 2 {
                        send_message(
                            &msg,
                            client.lock().await.borrow_mut(),
                            "Use: !streaming_together main_channel other channels",
                        )
                        .await?;
                        return Ok(());
                    }

                    let main_channel = vec_msg[1]
                        .strip_prefix("@")
                        .unwrap_or(&vec_msg[1])
                        .to_ascii_lowercase();

                    let other_channels: HashSet<String> = vec_msg[2..]
                        .iter()
                        .map(|channel| {
                            format!(
                                "{}{}",
                                "#",
                                channel
                                    .strip_prefix('@')
                                    .unwrap_or(channel)
                                    .to_ascii_lowercase()
                            )
                        })
                        .collect();
                    let mut bot_state = bot_state.write().await;
                    bot_state
                        .streaming_together
                        .insert(format!("{}{}", "#", main_channel), other_channels.clone());
                    drop(bot_state);
                    let other_channel_vec: Vec<&String> = other_channels.iter().collect();
                    send_message(
                        &msg,
                        client.lock().await.borrow_mut(),
                        &format!(
                            "Streaming together are now: {} and {:?}",
                            main_channel, other_channel_vec
                        ),
                    )
                    .await?;

                    Ok(())
                };
                Box::pin(fut)
            },
        ),
        description: "Add manually Streamers streaming together".to_string(),
        usage: "!streaming_together [@KrapMatt] <- Main channel [@Samoan_317,...] <- all others"
            .to_string(),
    }
}

fn master_chal() -> Command {
    Command {
        permission: PermissionLevel::User,
        handler: Arc::new(|msg, client, pool, bot_state| {
            let fut = async move {
                let bot_state = bot_state.read().await;
                let words: Vec<&str> = msg.text().split_ascii_whitespace().collect();

                if words.len() <= 1 {
                    reply_to_message(&msg, client.lock().await.borrow_mut(), "Usage: !cr <activity> bungiename").await?;
                    return Ok(());
                }

                let activity = words[1].to_string();
                let membership = if words.len() == 2 {
                    load_membership(&pool, msg.sender().name().to_string()).await
                } else {
                    let name = words[2..].join(" ");
                    if let Some(bungie_name) = is_valid_bungie_name(&name) {
                        Some(get_membershipid(&bungie_name, &bot_state.x_api_key).await?)
                    } else {
                        load_membership(&pool, name.strip_prefix("@").unwrap_or(&name).to_owned()).await
                    }
                };
                let membership = match membership {
                    Some(m) if m.type_m != -1 => m,
                    _ => {
                        reply_to_message(&msg, client.lock().await.borrow_mut(), "Use a correct bungiename!").await?;
                        return Ok(());
                    }
                };
                let chall_vec = get_master_challenges(membership.type_m, membership.id, &bot_state.x_api_key, activity.to_string()).await?;
                reply_to_message(&msg, client.lock().await.borrow_mut(), &chall_vec.join(" || ")).await?;
                
                Ok(())
            };
            Box::pin(fut)
        }),
        description: "Get the number of challenges done in a master raid".to_string(),
        usage: "!cr <activity> <name>".to_string(),
    }
}



fn find_command<'a>(command_name: &str) -> Option<(&'a String, &'a Command)> {
    for group in COMMAND_GROUPS.values() {
        for registration in &group.command {
            if registration.aliases.iter().any(|alias| alias == command_name) {
                return Some((&group.name, &registration.command));
            }
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
                        format!(
                            "Unknown command: {}. Use !help for a list of available commands.",
                            words[1]
                        )
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
                client
                    .lock()
                    .await
                    .privmsg(msg.channel(), &reply)
                    .send()
                    .await?;
                Ok(())
            })
        }),
        description: "Displays this help message or details about a specific command.".to_string(),
        usage: "!help [!<command>]".to_string(),
    }
}
*/