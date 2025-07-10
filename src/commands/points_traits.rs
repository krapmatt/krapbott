use std::{borrow::BorrowMut, sync::Arc, time::Duration};

use futures::future::BoxFuture;
use rand::{Rng, SeedableRng};
use sqlx::SqlitePool;
use tmi::{Client, Privmsg};
use tokio::{sync::{Mutex, RwLock}, time::{interval, Instant}};

use crate::{bot::BotState, bot_commands::{reply_to_message, send_message}, commands::{normalize_twitch_name, oldcommands::FnCommand, traits::CommandT, words}, models::{AliasConfig, BotError, ChannelConfig, PermissionLevel}};

pub struct GiveawayHandler;

impl CommandT for GiveawayHandler {
    fn name(&self) -> &str { "giveaway_handler" }

    fn description(&self) -> &str { "Starts and ends a giveaway" }

    fn usage(&self) -> &str { "!start_giveaway" }

    fn permission(&self) -> crate::models::PermissionLevel { PermissionLevel::Broadcaster }

    fn execute(&self, msg: tmi::Privmsg<'static>, client: std::sync::Arc<Mutex<tmi::Client>>, pool: sqlx::SqlitePool, bot_state: Arc<RwLock<BotState>>, alias_config: Arc<AliasConfig>) -> BoxFuture<'static, Result<(), BotError>> {
        Box::pin(async move {
            let client_clone = Arc::clone(&client);
            let msg_clone = msg.clone();
            let bot_state_clone = Arc::clone(&bot_state);
            let channel = msg.channel().to_string();
            //Start of giveaway 
            //Get default values of tickets (Change with commands)
            let (duration, number_of_tickets, price);
            
            sqlx::query!("DELETE FROM giveaway WHERE channel_id = ?", channel).execute(&pool).await;
            {
                let mut bot_state = bot_state_clone.write().await;
                let config = bot_state.config.get_channel_config_mut(msg.channel());
                duration = config.giveaway.duration;
                number_of_tickets = config.giveaway.max_tickets;
                price = config.giveaway.ticket_cost;
                config.giveaway.active = true;
                bot_state.config.save_config();
                drop(bot_state)
            }
            {
                send_message(&msg_clone, client_clone.lock().await.borrow_mut(), &format!("🎁 Giveaway has been started. You can buy a maximum of {} tickets for price of {} points for 1 ticket // USE !ticket <number>", number_of_tickets, price)).await.unwrap();
            }
            let mut ticker = interval(Duration::from_secs(5*60));
            let start_time = Instant::now();
            
            loop {
                ticker.tick().await;

                if start_time.elapsed() >= Duration::from_secs(duration.try_into().unwrap()) {
                    break;
                }

                // Periodic reminder message
                if let Err(e) = send_message(&msg_clone, client_clone.lock().await.borrow_mut(),&format!("⏰ Reminder: Giveaway is active! Use !ticket <1 - {}> to enter. Each ticket costs {} points!", number_of_tickets, price)).await {
                    eprintln!("Failed to send giveaway reminder: {:?}", e);
                }
            }

            let mut bot_state = bot_state_clone.write().await;
            let config = bot_state.config.get_channel_config_mut(msg.channel());
            config.giveaway.active = false;
            bot_state.config.save_config();
            {
                send_message(&msg_clone, client_clone.lock().await.borrow_mut(), "❗ Giveaway has ended. Winner will be pulled soon!").await.unwrap();
            }
            drop(bot_state);
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
    fn execute(&self, msg: tmi::Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, pool: sqlx::SqlitePool, bot_state: Arc<RwLock<crate::bot::BotState>>, alias_config: Arc<AliasConfig>) -> BoxFuture<'static, Result<(), BotError>> {
        Box::pin(async move {
            let words: Vec<&str> = words(&msg);
            let name = msg.sender().name().to_string();
            let channel = msg.channel().to_string();
            let channel_id = msg.channel_id();
            println!("{}",name);
            if words.len() != 2 {
                reply_to_message(&msg, client.lock().await.borrow_mut(), &format!("{}, to enter a raffle use: !ticket <number of tickets>", name)).await?;
                return Ok(());
            }
            //Ticket number is negative
            let num_tickets: i32 = match words[1].parse() {
                Ok(x) if x > 0 => x,
                _ => {
                    reply_to_message(&msg, client.lock().await.borrow_mut(), &format!("{}, ticket count must be a positive number!", name)).await?;
                    return Ok(());
                }
            };

            let bot_state = bot_state.read().await;
            let config = bot_state.config.get_channel_config(&channel).unwrap();

            //Giveaway closed
            if !config.giveaway.active {
                reply_to_message(&msg, client.lock().await.borrow_mut(), &format!("{}, there is no giveaway running right now.", name)).await?;
                return Ok(());
            }

            let ticket_price = config.giveaway.ticket_cost as i32;
            let max_tickets = config.giveaway.max_tickets as i32;

            // Get current ticket count if already entered
            let existing_entry = sqlx::query!(
                "SELECT tickets FROM giveaway WHERE twitch_name = ?1 AND channel_id = ?2",
                name, channel
            ).fetch_optional(&pool).await?;

            let total_after = num_tickets + existing_entry.map(|x| x.tickets as i32).unwrap_or(0);
            if total_after > max_tickets {
                reply_to_message(&msg, client.lock().await.borrow_mut(), &format!("{}, can't enter with more than {} tickets total!", name, max_tickets)).await?;
                return Ok(());
            }

            let cost = ticket_price * num_tickets;

            let currency_record = sqlx::query!(
                "SELECT points FROM currency WHERE twitch_name = ?1 AND channel = ?2",
                name, channel_id
            ).fetch_optional(&pool).await?;
            //Not enough points to buy tickets
            let Some(record) = currency_record else {
                reply_to_message(&msg, client.lock().await.borrow_mut(), &format!("{}, doesn't have any points to enter!", name)).await?;
                return Ok(());
            };

            if record.points < cost as i64 {
                reply_to_message(&msg, client.lock().await.borrow_mut(), &format!("{}, you don't have enough points! Needed: {}, You have: {}", name, cost, record.points)).await?;
                return Ok(());
            }

            // Add/update giveaway entry
            sqlx::query!(
                "INSERT INTO giveaway (channel_id, twitch_name, tickets)
                    VALUES (?1, ?2, ?3)
                    ON CONFLICT(twitch_name, channel_id) DO UPDATE SET tickets = tickets + ?3",
                channel, name, num_tickets
            ).execute(&pool).await?;

            // Deduct points
            sqlx::query!(
                "UPDATE currency SET points = points - ?1 WHERE twitch_name = ?2 AND channel = ?3",
                cost, name, channel_id
            ).execute(&pool).await?;

            reply_to_message(&msg, client.lock().await.borrow_mut(), &format!("{}, you successfully entered the giveaway with {} tickets!", name, num_tickets)).await?;

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

    fn execute(&self, msg: Privmsg<'static>, client: Arc<Mutex<Client>>, pool: SqlitePool, _: Arc<RwLock<BotState>>, alias_config: Arc<AliasConfig>) -> BoxFuture<'static, Result<(), BotError>> {
        let mode = self.mode.clone();
        Box::pin(async move {
            let Some((_, args)) = msg.text().split_once(" ") else {
                return Ok(());
            };

            let Some((twitch, points)) = args.trim().split_once(" ") else {
                return Ok(());
            };

            let points: i64 = points.parse().unwrap_or(0);
            let twitch_user = normalize_twitch_name(twitch);

            let result = sqlx::query!(
                "SELECT points FROM currency WHERE twitch_name = ? COLLATE NOCASE",
                twitch_user
            ).fetch_optional(&pool).await?;

            if let Some(row) = result {
                let new_points = match mode {
                    ChangeMode::Add => row.points + points,
                    ChangeMode::Remove => row.points - points,
                };

                sqlx::query!(
                    "UPDATE currency SET points = ? WHERE twitch_name = ? COLLATE NOCASE",
                    new_points, twitch_user
                ).execute(&pool).await?;

                send_message(&msg, client.lock().await.borrow_mut(), &format!("{twitch_user} now has {new_points} points")).await?;
            } else {
                send_message(&msg, client.lock().await.borrow_mut(), "User not found in the database").await?;
            }
            Ok(())
        })
    }
}

//Box::new(ChangePointsCommand { mode: ChangeMode::Add }) as Box<dyn CommandT>
//Box::new(ChangePointsCommand { mode: ChangeMode::Remove }) as Box<dyn CommandT>

pub fn pull_giveaway() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |msg, client, pool, _bot_state| {
            Box::pin(async move {
                let channel = msg.channel();
                let total_tickets = sqlx::query_scalar!(
                    "SELECT SUM(tickets) FROM giveaway WHERE channel_id = ?",
                    channel,
                ).fetch_one(&pool).await?;
                if total_tickets.is_none() {
                    println!("nobody joined loser");
                    return Ok(());
                }
                let mut rng = rand::rngs::StdRng::from_os_rng();
                let winning_ticket = rng.random_range(1..=total_tickets.unwrap());
                let winner_row = sqlx::query!(
                    r#"
                    SELECT twitch_name, SUM(tickets) OVER (ORDER BY id) as cumulative
                    FROM giveaway
                    WHERE channel_id = ?
                    "#,
                    channel,
                ).fetch_all(&pool).await?.into_iter().find(|row| row.cumulative >= winning_ticket);
                {
                    send_message(&msg, client.lock().await.borrow_mut(), &format!("The winner is: {}", winner_row.unwrap().twitch_name)).await?;
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
        move |msg, client, _pool, bot_state| {
            let invalid_msg = invalid_msg.to_string();
            let success_msg = success_msg.to_string();
            let setter = setter.clone();

            let fut = async move {
                let words: Vec<&str> = words(&msg);

                if words.len() != 2 {
                    send_message(&msg, client.lock().await.borrow_mut(), &invalid_msg).await?;
                    return Ok(());
                }

                let reply = if let Ok(value) = words[1].parse::<usize>() {
                    let mut bot_state = bot_state.write().await;
                    let config = bot_state.config.get_channel_config_mut(msg.channel());
                    setter(config, value);
                    bot_state.config.save_config();
                    success_msg.replace("{}", &value.to_string())
                } else {
                    "Put in a valid number".to_string()
                };

                send_message(&msg, client.lock().await.borrow_mut(), &reply).await?;
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
        |msg, client, pool, _| {
            let fut = async move {
                let channel = msg.channel_id();
                let twitch_name = msg.sender().name().to_string();

                let points = sqlx::query!(
                    "SELECT points FROM currency WHERE twitch_name = ? AND channel = ?",
                    twitch_name,
                    channel
                )
                .fetch_optional(&pool)
                .await?;

                let response = if let Some(row) = points {
                    format!("You have {} kilograms of dirt!", row.points)
                } else {
                    "You have 0 kilograms of dirt!".to_string()
                };

                send_message(&msg, client.lock().await.borrow_mut(), &response).await?;
                Ok(())
            };
            Box::pin(fut)
        },
        "Get your dirt (points)",
        "!dirt",
        "Points",
        PermissionLevel::User,
    ))
}

