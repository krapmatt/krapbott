use std::{str::FromStr, sync::Arc, time::Duration};
use futures::future::BoxFuture;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tokio::{sync::{RwLock}, time::{sleep, Instant}};
use tracing::info;
use twitch_irc::message::PrivmsgMessage;

use crate::{bot::{BotState, TwitchClient}, commands::{normalize_twitch_name, oldcommands::FnCommand, traits::CommandT, words}, models::{AliasConfig, BotResult, ChannelConfig, PermissionLevel}};
 
#[derive(Deserialize, Serialize, Debug, Clone, Copy)]
pub struct Giveaway {
    pub duration: usize,
    pub max_tickets: usize,
    pub ticket_cost: usize,
    pub active: bool
}

impl Giveaway {
 pub fn new() -> Self {
    Self { duration: 3600, max_tickets: 100, ticket_cost: 15, active: false }
 }
}

pub struct GiveawayHandler;

impl CommandT for GiveawayHandler {
    fn name(&self) -> &str { "giveaway_handler" }

    fn description(&self) -> &str { "Starts and ends a giveaway" }

    fn usage(&self) -> &str { "!start_giveaway" }

    fn permission(&self) -> crate::models::PermissionLevel { PermissionLevel::Broadcaster }

    fn execute(&self, msg: PrivmsgMessage, client: TwitchClient, pool: sqlx::PgPool, bot_state: Arc<RwLock<BotState>>, _alias_config: Arc<AliasConfig>) -> BoxFuture<'static, BotResult<()>> {
        Box::pin(async move {
            let msg_clone = msg.clone();
            let bot_state_clone = Arc::clone(&bot_state);
            let channel = msg.channel_login;
            //Start of giveaway 
            //Get default values of tickets (Change with commands)
            let (duration, number_of_tickets, price);
            
            sqlx::query!("DELETE FROM giveaway WHERE channel_id = $1", channel).execute(&pool).await?;
            {
                let mut bot_state = bot_state_clone.write().await;
                let config = bot_state.config.get_channel_config_mut(&channel);
                duration = config.giveaway.duration;
                number_of_tickets = config.giveaway.max_tickets;
                price = config.giveaway.ticket_cost;
                config.giveaway.active = true;
                bot_state.config.save_all(&pool).await?;
                drop(bot_state)
            }
            client.say(channel, format!("🎁 Giveaway has been started. You can buy a maximum of {} tickets for price of {} points for 1 ticket // USE !ticket <number>", number_of_tickets, price)).await?;
            let start_time = Instant::now();
            
            tokio::spawn(async move {
                loop {
                    sleep(Duration::from_secs(5*60)).await;

                    if start_time.elapsed() >= Duration::from_secs(duration.try_into().unwrap()) {
                        break;
                    }

                    // Periodic reminder message
                    if let Err(e) = client.say(msg_clone.channel_login.clone(), format!("⏰ Reminder: Giveaway is active! Use !ticket <1 - {}> to enter. Each ticket costs {} points!", number_of_tickets, price)).await {
                        info!("Failed to send giveaway reminder: {:?}", e);
                    }
                }
                let mut bot_state = bot_state_clone.write().await;
                let config = bot_state.config.get_channel_config_mut(&msg_clone.channel_login);
                config.giveaway.active = false;
                let _ = bot_state.config.save_all(&pool).await;
                
                client.say(msg_clone.channel_login, "❗ Giveaway has ended. Winner will be pulled soon!".to_string()).await.unwrap();
                
                drop(bot_state);
            });
            Ok(())
        })
    }
}

pub struct JoinGiveaway;

impl CommandT for JoinGiveaway {
    fn description(&self) -> &str { "Join a giveaway using points" }
    fn usage(&self) -> &str { "!ticket <amount>" }
    fn name(&self) -> &str { "enter_giveaway" }
    fn permission(&self) -> PermissionLevel { PermissionLevel::Follower }
    fn execute(&self, msg: PrivmsgMessage, client: TwitchClient, pool: sqlx::PgPool, bot_state: Arc<RwLock<crate::bot::BotState>>, _alias_config: Arc<AliasConfig>) -> BoxFuture<'static, BotResult<()>> {
        Box::pin(async move {
            let words: Vec<&str> = words(&msg);
            let name = msg.sender.name.clone();
            let channel = msg.channel_login.clone();
            let channel_id = msg.channel_id.clone();

            if words.len() != 2 {
                client.say_in_reply_to(&msg, format!("{}, to enter a raffle use: !ticket <number of tickets>", name)).await?;
                return Ok(());
            }
            //Ticket number is negative
            let num_tickets: i32 = match words[1].parse() {
                Ok(x) if x > 0 => x,
                _ => {
                    client.say_in_reply_to(&msg, format!("{}, ticket count must be a positive number!", name)).await?;
                    return Ok(());
                }
            };

            let bot_state = bot_state.read().await;
            let config = bot_state.config.get_channel_config(&channel).unwrap();

            //Giveaway closed
            if !config.giveaway.active {
                client.say_in_reply_to(&msg, format!("{}, there is no giveaway running right now.", name)).await?;
                return Ok(());
            }

            let ticket_price = config.giveaway.ticket_cost as i32;
            let max_tickets = config.giveaway.max_tickets as i32;

            // Get current ticket count if already entered
            let existing_entry = sqlx::query!(
                "SELECT tickets FROM giveaway WHERE twitch_name = $1 AND channel_id = $2",
                name, channel
            ).fetch_optional(&pool).await?;

            let total_after = num_tickets + existing_entry.map(|x| x.tickets as i32).unwrap_or(0);
            if total_after > max_tickets {
                client.say_in_reply_to(&msg, format!("{}, can't enter with more than {} tickets total!", name, max_tickets)).await?;
                return Ok(());
            }

            let cost = ticket_price * num_tickets;

            let currency_record = sqlx::query!(
                "SELECT points FROM currency WHERE twitch_name = $1 AND channel = $2",
                name, channel_id
            ).fetch_optional(&pool).await?;
            //Not enough points to buy tickets
            let Some(record) = currency_record else {
                client.say_in_reply_to(&msg, format!("{}, doesn't have any points to enter!", name)).await?;
                return Ok(());
            };

            if record.points < cost {
                client.say_in_reply_to(&msg, format!("{}, you don't have enough points! Needed: {}, You have: {}", name, cost, record.points)).await?;
                return Ok(());
            }

            // Add/update giveaway entry
            sqlx::query!(
                "INSERT INTO giveaway (channel_id, twitch_name, tickets)
                    VALUES ($1, $2, $3)
                    ON CONFLICT(twitch_name, channel_id) DO UPDATE SET tickets = giveaway.tickets + $3",
                channel, name, num_tickets
            ).execute(&pool).await?;

            // Deduct points
            sqlx::query!(
                "UPDATE currency SET points = points - $1 WHERE twitch_name = $2 AND channel = $3",
                cost, name, channel_id
            ).execute(&pool).await?;

            client.say_in_reply_to(&msg, format!("{}, you successfully entered the giveaway with {} tickets!", name, num_tickets)).await?;

            Ok(())
        })
    }
}

pub struct ChangePointsCommand {
    pub mode: ChangeMode,
}


#[derive(Clone)]
pub enum ChangeMode {
    Add,
    Remove,
}

impl CommandT for ChangePointsCommand {
    fn name(&self) -> &str {
        match self.mode {
            ChangeMode::Add => "add_points",
            ChangeMode::Remove => "remove_points",
        }
    }

    fn description(&self) -> &str {
        match self.mode {
            ChangeMode::Add => "Give someone points",
            ChangeMode::Remove => "Remove points from someone",
        }
    }

    fn usage(&self) -> &str {
        "!<command> @twitchname amount"
    }

    fn permission(&self) -> PermissionLevel {
        PermissionLevel::Moderator
    }

    fn execute(&self, msg: PrivmsgMessage, client: TwitchClient, pool: PgPool, bot_state: Arc<RwLock<BotState>>, _alias_config: Arc<AliasConfig>) -> BoxFuture<'static, BotResult<()>> {
        let mode = self.mode.clone();
        Box::pin(async move {
            let Some((_, args)) = msg.message_text.split_once(" ") else {
                return Ok(());
            };

            let Some((twitch, points)) = args.trim().split_once(" ") else {
                return Ok(());
            };

            let points: i64 = points.parse().unwrap_or(0);
            let twitch_user = normalize_twitch_name(twitch);
            let channel = msg.channel_id;
            let result = sqlx::query!(
                "SELECT points FROM currency WHERE channel = $1 AND LOWER(twitch_name) = LOWER($2)",
                channel, twitch_user
            ).fetch_optional(&pool).await?;

            if let Some(row) = result {
                let new_points = match mode {
                    ChangeMode::Add => row.points + points as i32,
                    ChangeMode::Remove => row.points - points as i32,
                };
                let config = &bot_state.read().await.config;
                let config = config.get_channel_config(&msg.channel_login).unwrap();
                sqlx::query!(
                    "INSERT INTO currency (twitch_name, points, channel) VALUES ($1, $2, $3) 
                    ON CONFLICT(twitch_name, channel) DO UPDATE SET points = $2",
                    twitch_user, new_points, channel
                ).execute(&pool).await?;

                client.say(msg.channel_login, format!("{twitch_user} now has {new_points} {}", config.points_config.name)).await?;
            } else {
                client.say(msg.channel_login, "User not found in the database".to_string()).await?;
            }
            Ok(())
        })
    }
}

pub fn pull_giveaway() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |msg, client, pool, _bot_state| {
            Box::pin(async move {
                let channel = msg.channel_login.clone();
                let total_tickets = sqlx::query_scalar!(
                    "SELECT SUM(tickets) FROM giveaway WHERE channel_id = $1",
                    channel,
                ).fetch_one(&pool).await?;
                if total_tickets.is_none() || total_tickets.unwrap() == 0 {
                    client.say(channel, "Nobody joined the giveaway. Sadge".to_string()).await?;
                    return Ok(());
                }

                let total_tickets = total_tickets.unwrap();

                let mut rng = rand::rngs::StdRng::from_os_rng();
                let winning_ticket = rng.random_range(1..=total_tickets);

                let rows = sqlx::query!(
                    r#"
                    SELECT twitch_name, tickets, SUM(tickets) OVER (ORDER BY id) as cumulative
                    FROM giveaway
                    WHERE channel_id = $1
                    "#,
                    channel,
                ).fetch_all(&pool).await?.into_iter().find(|row| row.cumulative.unwrap() >= winning_ticket);

                if let Some(winner_row) =
                    rows.into_iter().find(|row| row.cumulative.unwrap() >= winning_ticket)
                {
                    let winner = winner_row.twitch_name;
                    let winner_tickets = winner_row.tickets;
                    let chance = (winner_tickets as f64 / total_tickets as f64) * 100.0;

                    client.say(channel,format!("🎉 The winner is: {}! They had {} ticket(s) out of {} ({:.2}% chance).", winner, winner_tickets, total_tickets, chance)).await?;
                } else {
                    client.say(channel, "Something went wrong pulling a winner.".to_string()).await?;
                }
                Ok(())
            })
        },
        "Pulls the winner of finished giveaway",
        "pull",
        "Pull",
        PermissionLevel::Broadcaster
    ))
}

pub fn make_giveaway_config_command<F>(name: &'static str, description: &'static str, usage: &'static str, invalid_msg: &'static str, success_msg: &'static str, setter: F) -> Arc<dyn CommandT>
where F: Fn(&mut ChannelConfig, usize) + Send + Sync + Clone + 'static {
    let cmd = FnCommand::new(
        move |msg, client, pool, bot_state| {
            let invalid_msg = invalid_msg.to_string();
            let success_msg = success_msg.to_string();
            let setter = setter.clone();

            let fut = async move {
                let words: Vec<&str> = words(&msg);

                if words.len() != 2 {
                    client.say(msg.channel_login, invalid_msg).await?;
                    return Ok(());
                }

                let reply = if let Ok(value) = words[1].parse::<usize>() {
                    let mut bot_state = bot_state.write().await;
                    let config = bot_state.config.get_channel_config_mut(&msg.channel_login);
                    
                    bot_state.config.update_channel(&pool, &msg.channel_login, |cfg| {
                        setter(cfg, value);
                    }).await?;
                    success_msg.replace("{}", &value.to_string())
                } else {
                    "Put in a valid number".to_string()
                };

                client.say(msg.channel_login, reply).await?;
                Ok(())
            };

            Box::pin(fut)
        },
        description,
        usage,
        name,
        PermissionLevel::Broadcaster,
    );

    Arc::new(cmd)
}

pub fn change_duration_giveaway() -> Arc<dyn CommandT> {
    make_giveaway_config_command(
        "Giveaway Duration",
        "Changes duration of giveaway (in seconds)",
        "!giveaway_duration <seconds>",
        "There was an issue somewhere: Use: !giveaway_duration <seconds>",
        "Duration of giveaway has been updated to {} seconds",
        |config, value| config.giveaway.duration = value,
    )
}

pub fn change_max_tickets_giveaway() -> Arc<dyn CommandT> {
    make_giveaway_config_command(
        "Giveaway Max Tickets",
        "Changes maximum number of tickets in giveaway",
        "!giveaway_tickets <number of tickets>",
        "There was an issue somewhere: Use: !giveaway_tickets <number>",
        "Number of maximum tickets has been changed to {}",
        |config, value| config.giveaway.max_tickets = value,
    )
}

pub fn change_price_ticket() -> Arc<dyn CommandT> {
    make_giveaway_config_command(
        "Giveaway Ticket Price",
        "Changes price of tickets in giveaway",
        "!giveaway_price <price>",
        "There was an issue somewhere: Use: !giveaway_price <number>",
        "Price of tickets has been changed to {}",
        |config, value| config.giveaway.ticket_cost = value,
    )
}

pub fn get_points_command() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |msg, client, pool, bot_state| {
            let fut = async move {
                let channel = msg.channel_id.clone();
                let twitch_name = msg.sender.name.clone();

                let points = sqlx::query!(
                    "SELECT points FROM currency WHERE twitch_name = $1 AND channel = $2",
                    twitch_name, channel
                ).fetch_optional(&pool).await?;
                let bot_state = bot_state.read().await;
                let config = bot_state.config.get_channel_config(&msg.channel_login).unwrap();

                let response = if let Some(row) = points {
                    format!("You have {} {}!", row.points, config.points_config.name)
                } else {
                    format!("You have no {}", config.points_config.name)
                };

                client.say(msg.channel_login, response).await?;
                Ok(())
            };
            Box::pin(fut)
        },
        "Get your points (points)",
        "!points",
        "Points",
        PermissionLevel::User,
    ))
}

pub fn make_points_config_command<T, F>(name: &'static str, description: &'static str, usage: &'static str, invalid_msg: &'static str, success_msg: &'static str, setter: F) -> Arc<dyn CommandT>
where T: FromStr + ToString + Send + Sync + Clone + 'static, <T as FromStr>::Err: Send, F: Fn(&mut ChannelConfig, T) + Send + Sync + Clone + 'static {
    let cmd = FnCommand::new(
        move |msg, client, pool, bot_state| {
            let invalid_msg = invalid_msg.to_string();
            let success_msg = success_msg.to_string();
            let setter = setter.clone();

            let fut = async move {
                let words: Vec<&str> = words(&msg);

                if words.len() != 2 {
                    client.say(msg.channel_login, invalid_msg).await?;
                    return Ok(());
                }

                let reply = if let Ok(value) = words[1].parse::<T>() {
                    let mut bot_state = bot_state.write().await;
                    
                    bot_state.config.update_channel(&pool, &msg.channel_login, |cfg| {
                        setter(cfg, value.clone());
                    }).await?;
                    success_msg.replace("{}", &value.to_string())
                } else {
                    "Put in a valid number".to_string()
                };

                client.say(msg.channel_login, reply).await?;
                Ok(())
            };

            Box::pin(fut)
        },
        description,
        usage,
        name,
        PermissionLevel::Broadcaster,
    );

    Arc::new(cmd)
}

pub fn change_name_points() -> Arc<dyn CommandT> {
    make_points_config_command::<String, _>(
        "Change Point Name",
        "Changes name of points",
        "!change_points <name>",
        "There was an issue somewhere: Use: !change_points <name>",
        "Name of points has been updated to {}",
        |config, value| config.points_config.name = value,
    )
}

pub fn change_points_interval() -> Arc<dyn CommandT> {
    make_points_config_command::<usize, _>(
        "Points Interval",
        "Changes interval of when give out points",
        "!points_interval <duration>",
        "There was an issue somewhere: Include number!",
        "Interval of points has been changed to {}",
        |config, value| config.points_config.interval = value as u64,
    )
}

pub fn change_points_per_interval() -> Arc<dyn CommandT> {
    make_points_config_command::<isize, _>(
        "Points Per Interval",
        "Changes how many points you will get per interval",
        "!points_amount <number>",
        "There was an issue somewhere: Use: <number>",
        "Number of points has been changed to {}",
        |config, value| config.points_config.points_per_time = value as i32,
    )
}