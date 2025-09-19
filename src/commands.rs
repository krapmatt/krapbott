pub mod traits;
pub mod oldcommands;
pub mod queue_traits;
pub mod points_traits;
pub mod announcement;
pub mod moderation;
pub mod time;
pub mod bungie_traits;

use crate::bot::CommandMap;
use crate::bot::DispatcherCache;
use crate::bot::TwitchClient;
use crate::commands::bungie_traits::MasterChalCommand;
use crate::commands::bungie_traits::TotalCommand;
use crate::commands::announcement::add_announcement_command;
use crate::commands::announcement::announcement_freq_command;
use crate::commands::announcement::announcement_state_command;
use crate::commands::announcement::play_announcement_command;
use crate::commands::announcement::remove_announcement_command;
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
use crate::commands::moderation::AliasCommand;
use crate::commands::oldcommands::so;
use crate::commands::points_traits::change_duration_giveaway;
use crate::commands::points_traits::change_max_tickets_giveaway;
use crate::commands::points_traits::change_name_points;
use crate::commands::points_traits::change_points_interval;
use crate::commands::points_traits::change_points_per_interval;
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
use crate::commands::queue_traits::random;
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
use crate::database::fetch_aliases_from_db;
use crate::models::AliasConfig;
use crate::models::BotResult;
use crate::BotConfig;
use crate::{
    bot::BotState,
    models::{BotError, PermissionLevel},
};

use futures::future::BoxFuture;
use serde::Deserialize;
use sqlx::PgPool;
use twitch_irc::message::PrivmsgMessage;

use std::collections::HashSet;
use std::vec;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;
use tokio::sync::RwLock;




type CommandHandler = Arc<
    dyn Fn(
            PrivmsgMessage,
            Arc<Mutex<TwitchClient>>,
            PgPool,
            Arc<RwLock<BotState>>,
            AliasConfig
        ) -> BoxFuture<'static, BotResult<()>>
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
        map.insert("bungie api", &*BUNGIE_API);
        map.insert("moderation", &*MODERATION);
        map.insert("announcement", &*ANNOUNCEMENT);
        map
    };
}

lazy_static::lazy_static!{
    pub static ref QUEUE_COMMANDS: CommandGroup = CommandGroup { name: "Queue".to_string(), 
        commands: vec![
            cmd!(Arc::new(NextComamnd), "next"),
            cmd!(Arc::new(JoinCommand), "j", "join"),
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
            cmd!(random(), "random"),

        ], 
    };
    pub static ref SHOUTOUT: CommandGroup = CommandGroup { name: "Shoutout".to_string(), 
        commands: vec![
            cmd!(so(), "so", "shoutout")
        ], 
    };
    
    pub static ref TIME: CommandGroup = CommandGroup { name: "Time".to_string(), 
        commands: vec![
            cmd!(matt_time(), "mattbed"),
            cmd!(samosa_time(), "samoanbed"),
            cmd!(cindi_time(), "cindibed"),
        ], 
    };

    pub static ref POINTS: CommandGroup = CommandGroup { name: "Points".to_string(),
        commands: vec![
            cmd!(Arc::new(GiveawayHandler), "startgiveaway", "start_giveaway"),
            cmd!(Arc::new(JoinGiveaway), "ticket"),
            cmd!(Arc::new(ChangePointsCommand { mode: ChangeMode::Add}) as Arc<dyn CommandT>, "add_points"),
            cmd!(Arc::new(ChangePointsCommand { mode: ChangeMode::Remove}) as Arc<dyn CommandT>, "remove_points"),
            cmd!(pull_giveaway(), "pull"),
            cmd!(change_max_tickets_giveaway(), "giveaway_tickets"),
            cmd!(change_duration_giveaway(), "giveaway_duration"),
            cmd!(change_price_ticket(), "giveaway_price"),
            cmd!(get_points_command(), "points"),
            cmd!(change_name_points(), "points_name"),
            cmd!(change_points_interval(), "points_interval"),
            cmd!(change_points_per_interval(), "points_amount")
        ], 
    };

    pub static ref MODERATION: CommandGroup = CommandGroup { name: "Moderation".to_string(),
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
            cmd!(Arc::new(AliasCommand), "alias"),
        ]
    };

    pub static ref BUNGIE_API: CommandGroup = CommandGroup { name: "Bungie API".to_string(),
        commands: vec![
            cmd!(Arc::new(TotalCommand), "total"),
            cmd!(Arc::new(MasterChalCommand), "cr"),
        ]
    };

    pub static ref ANNOUNCEMENT: CommandGroup = CommandGroup { name: "Announcement".to_string(), 
        commands: vec![
            cmd!(add_announcement_command(), "add_announcement"),
            cmd!(remove_announcement_command(), "remove_announcement"),
            cmd!(play_announcement_command(), "play_announcement", "announce"),
            cmd!(announcement_freq_command(), "announcement_interval"),
            cmd!(announcement_state_command(), "announcement_state")
        ]
    };
}

pub fn words<'a>(msg: &'a PrivmsgMessage) -> Vec<&'a str> {
    msg.message_text.split_ascii_whitespace().collect()
}

pub fn normalize_twitch_name(s: &str) -> &str {
    s.strip_prefix("@").unwrap_or(s)
}
pub struct CommandOverride {
    enabled: bool,
    command_name: String
}

pub async fn update_dispatcher_if_needed(channel: &str, config: &BotConfig, pool: &PgPool, dispatcher_cache: Arc<RwLock<DispatcherCache>>) -> BotResult<()> {
    println!("Here7,5");
    let alias_config = fetch_aliases_from_db(channel, pool).await?;
    let new_dispatcher = create_dispatcher(config, channel, &alias_config);
    println!("Here8");
    {
        let mut cache = dispatcher_cache.write().await;
        let old_dispatcher = cache.get(channel);
        println!("Here9");
        match old_dispatcher {
            Some(existing) => {
                let existing_keys: HashSet<_> = existing.keys().collect();
                let new_keys: HashSet<_> = new_dispatcher.keys().collect();
                if existing_keys == new_keys {
                    return Ok(()); // early exit, lock dropped here
                }
            }
            None => {
                println!("Dispatcher created for {channel}");
            }
        };

        println!(
            "Dispatcher updated for channel: {}: {:?}",
            channel,
            new_dispatcher.keys()
        );
        cache.insert(channel.to_string(), new_dispatcher);
    }

    Ok(())
}

pub fn create_dispatcher(config: &BotConfig, channel_name: &str, alias_config: &AliasConfig) -> CommandMap {
    let mut commands: CommandMap = HashMap::new();
    let mut all_commands_by_name: HashMap<String, Arc<dyn CommandT>> = HashMap::new();

    if let Some(channel_config) = config.channels.get(channel_name) {
        for package_name in &channel_config.packages {
            if let Some(group) = COMMAND_GROUPS.get(package_name.to_lowercase().as_str()) {
                for registration in &group.commands {
                    let base_name = registration.command.name().to_lowercase();

                    // Skip entire command if disabled
                    if alias_config.disabled_commands.contains(&base_name) {
                        continue;
                    }

                    // Add to global name-based lookup
                    all_commands_by_name.insert(base_name.clone(), registration.command.clone());

                    // Add default aliases if not removed
                    for alias in &registration.aliases {
                        let alias = alias.to_lowercase();
                        if alias_config.removed_aliases.contains(&alias) {
                            continue;
                        }
                        commands.insert(alias, registration.command.clone());
                    }
                }
            }
        }
    }

    // Handle custom aliases — now using the global map instead of dispatcher
    for (alias, command_name) in &alias_config.aliases {
        if let Some(cmd) = all_commands_by_name.get(&command_name.to_lowercase()).cloned() {
            commands.insert(alias.to_string(), cmd);
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
pub fn generate_variables(msg: &PrivmsgMessage) -> HashMap<String, String> {
    let mut variables = HashMap::new();
    variables.insert("sender".to_string(), msg.sender.name.clone());
    variables.insert("channel".to_string(), msg.channel_login.clone());
    variables.insert("receiver".to_string(), {
        let words: Vec<&str> = words(msg);
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



/*fn help_command() -> Command {
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