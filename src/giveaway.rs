use std::{borrow::BorrowMut, sync::Arc, time::Duration};

use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};
use tokio::time::{interval, sleep, Instant};

use crate::{bot_commands::{reply_to_message, send_message}, commands::Command, models::{ChannelConfig, PermissionLevel}};

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

pub fn handle_giveaway() -> Command {
    Command {
        permission: PermissionLevel::Broadcaster,
        handler: Arc::new(|msg, client, pool, bot_state| {
            let fut = async move {
                let client_clone = Arc::clone(&client);
                let msg_clone = msg.clone();
                let bot_state_clone = Arc::clone(&bot_state);
                let channel = msg.channel().to_string();
                let handler = tokio::spawn(async move {
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
                    drop(bot_state)
                });
                Ok(())
            };
            Box::pin(fut)
        }),
        description: "Starts and ends a giveaway".to_string(),
        usage: "!start_giveaway".to_string(),
    }
}

pub fn pull_giveaway() -> Command {
    Command {
        permission: PermissionLevel::Broadcaster,
        handler: Arc::new(|msg, client, pool, _bot_state| {
            let fut = async move {
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
            };
            Box::pin(fut)
        }),
        description: "Toggles sub only queue".to_string(),
        usage: "!toggle_sub".to_string(),
    }
}

pub fn join_giveaway() -> Command {
    Command {
        permission: PermissionLevel::Follower,
        handler: Arc::new(|msg, client, pool, bot_state| {
            let fut = async move {
                let words: Vec<&str> = msg.text().split_ascii_whitespace().collect();
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
            };
            Box::pin(fut)
        }),
        description: "Join a giveaway using points".to_string(),
        usage: "!ticket <amount>".to_string(),
    }
}

fn make_giveaway_config_command<F>(description: &'static str, usage: &'static str, invalid_msg: &'static str, success_msg: &'static str, setter: F) -> Command 
where F: Fn(&mut ChannelConfig, usize) + Send + Sync + Clone + 'static {
    Command {
        permission: PermissionLevel::Broadcaster,
        handler: Arc::new(move |msg, client, _pool, bot_state| {
            let invalid_msg = invalid_msg.to_string();
            let success_msg = success_msg.to_string();
            let setter = setter.clone();
            
            let fut = async move {
                let words: Vec<&str> = msg.text().split_ascii_whitespace().collect();

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
        }),
        description: description.to_string(),
        usage: usage.to_string(),
    }
}

pub fn change_duration_giveaway() -> Command {
    make_giveaway_config_command(
        "Changes duration of giveaway (in seconds)",
        "!giveaway_duration <seconds>",
        "There was an issue somewhere: Use: !giveaway_duration <seconds>",
        "Duration of giveaway has been updated to {} seconds",
        |config, value| config.giveaway.duration = value,
    )
}

pub fn change_max_tickets_giveaway() -> Command {
    make_giveaway_config_command(
        "Changes maximum number of tickets in giveaway",
        "!giveaway_tickets <number of tickets>",
        "There was an issue somewhere: Use: !giveaway_tickets <number>",
        "Number of maximum tickets has been changed to {}",
        |config, value| config.giveaway.max_tickets = value,
    )
}

pub fn change_price_ticket() -> Command {
    make_giveaway_config_command(
        "Changes price of tickets in giveaway",
        "!giveaway_price <number of tickets>",
        "There was an issue somewhere: Use: !giveaway_price <number>",
        "Price of tickets has been changed to {}",
        |config, value| config.giveaway.ticket_cost = value,
    )
}