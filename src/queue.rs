use std::{borrow::BorrowMut, sync::Arc};

use chrono::{DateTime, NaiveDateTime, Utc};
use regex::Regex;
use sqlx::SqlitePool;
use tmi::Client;

use crate::{api::get_membershipid, bot::BotState, bot_commands::{reply_to_message, send_message}, database::{is_bungiename, save_to_user_database, user_exists_in_database}, models::{is_subscriber, AliasConfig, BotError, TwitchUser}};

lazy_static::lazy_static!{
    static ref BUNGIE_REGEX: Regex = Regex::new(r"^(?P<name>.+)#(?P<digits>\d{4})").unwrap();
}

pub enum Queue {
    Join,
    ForceJoin
}

impl BotState {
    pub async fn handle_join(&self, msg: &tmi::Privmsg<'_>, client: Arc<tokio::sync::Mutex<tmi::Client>>, pool: &SqlitePool, alias_config: Arc<AliasConfig>) -> Result<(), BotError> {
        let mut client = client.lock().await;
    
        let config = match self.config.get_channel_config(msg.channel()) {
            Some(config) => config,
            None => return Ok(()),
        };
        let join_mode = config.random_queue;
        let channel = config.clone().queue_channel;

        if !config.open {
            let response = match join_mode {
                true => format!("Raffle is currently closed! @{}", msg.sender().name()),
                false => format!("Queue is currently closed! @{}", msg.sender().name())
            };
            send_message(&msg, &mut client, &response).await?;
            return Ok(());
        }

        if config.sub_only && !is_subscriber(msg) {
            let response = match join_mode {
                true => format!("Raffle is only open to subscribers! @{}", msg.sender().name()),
                false => format!("Queue is only open to subscribers! @{}", msg.sender().name())
            };
            send_message(msg, &mut client, &response).await?;
            return Ok(());
        }

        
        let twitch_name = msg.sender().name().to_string();
        // Fetch stored Bungie name from the database
        let stored_bungie_name = user_exists_in_database(pool, msg.sender().name().to_string()).await;

        // Check if user provided a Bungie name manually
        let mut provided_bungie_name = msg.text().split_once(" ").map(|(_, name)| name.trim().to_string());
        if provided_bungie_name == Some(" ".to_string()) {
            provided_bungie_name = None
        }
        let bungie_name_to_use = if let Some(provided) = &provided_bungie_name {
            if let Some(stored) = &stored_bungie_name {
                if stored == provided {
                    Some(stored.clone()) // Matches database, use stored one
                } else if is_valid_bungie_name(provided).is_some() && is_bungiename(self.x_api_key.clone(), provided, &msg.sender().name(), pool).await {
                    // New valid Bungie name, update database
                    save_to_user_database(pool, TwitchUser {twitch_name: twitch_name.clone(), bungie_name: provided.clone()}, self.x_api_key.clone()).await?;
                    
                    Some(provided.clone())
                } else {
                    // Invalid Bungie name provided, fall back to stored one
                    Some(stored.clone())
                }
            } else {
                // No stored Bungie name, validate the provided one
                if is_valid_bungie_name(provided).is_some() && is_bungiename(self.x_api_key.clone(), provided, &msg.sender().name(), pool).await {
                    // Save new Bungie name to database
                    save_to_user_database(pool, TwitchUser {twitch_name: twitch_name.clone(), bungie_name: provided.clone()}, self.x_api_key.clone()).await?;


                    Some(provided.clone())
                } else {
                    // Provided Bungie name is invalid
                    None
                }
            }
        } else {
            stored_bungie_name
        };

        // If Bungie name is valid, add user to the queue
        if let Some(name) = bungie_name_to_use {
            let new_user = TwitchUser {
                twitch_name: msg.sender().name().to_string(),
                bungie_name: name.clone(),
            };
            if is_banned_from_queue(msg, pool, &mut client, &name, &self.x_api_key).await? {
                return Ok(());
            }
            process_queue_entry(msg, &mut client, config.len, pool, new_user, &channel, Queue::Join, join_mode).await?;
        } else {
            send_invalid_name_reply(msg, &mut client, alias_config).await?;
        }
    
       
        Ok(())
    }

    pub async fn handle_next(&mut self, channel_id: String, pool: &SqlitePool) -> Result<String, BotError> {
        let config = self.config.get_channel_config_mut(&channel_id);
        let channel = config.queue_channel.clone();
        let teamsize: i32 = config.teamsize.try_into().unwrap();
        let result = if config.random_queue {
            randomize_queue(pool, &channel, teamsize).await?
        } else {
            next_handler(&channel, teamsize, pool).await?
        };
        config.runs += 1;
        self.config.save_config();
    
        Ok(result)
    }
    pub async fn deprio(&self, msg: &tmi::Privmsg<'_>, client: Arc<tokio::sync::Mutex<tmi::Client>>, pool: &SqlitePool) -> Result<(), BotError> {
        let text = msg.text().to_owned();
        let words: Vec<&str> = text.split_ascii_whitespace().collect();
    
        if words.len() < 2 {
            let reply = "Wrong usage!";
            send_message(msg, client.lock().await.borrow_mut(), reply).await?;
            return Ok(());
        }

        let word = words[1].to_string();
        let twitch_name = word.strip_prefix("@").unwrap_or(words[1]);

        let config = if let Some(config) = self.config.get_channel_config(msg.channel()) {
            config
        } else {
            return Ok(())
        };
        let channel = config.queue_channel.clone();
        let queue_len = config.len;

        let mut tx = pool.begin().await?;
        let name = sqlx::query!(
            "SELECT bungie_name FROM queue WHERE twitch_name = ? AND channel_id = ?",
            twitch_name, channel
        ).fetch_optional(&mut *tx).await?;

        let name = if let Some(name) = name {
            name
        } else {
            return Ok(());
        };

        sqlx::query!(
            "DELETE FROM queue WHERE twitch_name = ? AND channel_id = ?",
            twitch_name, channel
        ).execute(&mut *tx).await?;

        let entries = sqlx::query!(
            "SELECT twitch_name FROM queue WHERE channel_id = ? ORDER BY position ASC",
            channel
        ).fetch_all(&mut *tx).await?;
        for (new_position, entry) in entries.iter().enumerate() {
            let new_position: i32 = (new_position + 1).try_into().unwrap();
            sqlx::query!(
                "UPDATE queue SET position = ? WHERE twitch_name = ? AND channel_id = ?",
                new_position, entry.twitch_name, channel
            ).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        add_to_queue(queue_len, pool, &TwitchUser { twitch_name: twitch_name.to_owned(), bungie_name: name.bungie_name}, &channel, Queue::Join, false).await?;

        send_message(&msg, client.lock().await.borrow_mut(), &format!("{} has been deprioed.", twitch_name)).await?;

        Ok(())
    }
    pub async fn prio(&self, msg: &tmi::Privmsg<'_>, client: Arc<tokio::sync::Mutex<tmi::Client>>, pool: &SqlitePool) -> Result<(), BotError> {
        let text = msg.text().to_owned();
        let words: Vec<&str> = text.split_ascii_whitespace().collect();
    
        if words.len() < 2 {
            let reply = "Wrong usage! Use: <twitch_name> [runs]";
            send_message(msg, client.lock().await.borrow_mut(), reply).await?;
            return Ok(());
        }
    
        let word = words[1].to_string();
        let twitch_name = word.strip_prefix("@").unwrap_or(words[1]);
    
        let config = if let Some(config) = self.config.get_channel_config(msg.channel()) {
            config
        } else {
            return Ok(())
        };
        let channel = config.queue_channel.clone();
        let teamsize = config.teamsize;
    
        let mut tx = pool.begin().await?;
    
        // 🔹 Check if user exists in the queue
        let existing_position = sqlx::query_scalar!(
            "SELECT position FROM queue WHERE twitch_name = ? AND channel_id = ?",
            twitch_name, channel
        ).fetch_optional(&mut *tx).await?;
    
        if existing_position.is_none() {
            let reply = format!("User {} not found in the queue", twitch_name);
            send_message(msg, client.lock().await.borrow_mut(), &reply).await?;
            return Ok(());
        }
    
        let reply = if words.len() == 2 {
            // 🔹 Move to the second group (teamsize + 1)
            let second_group: i32 = (teamsize + 1).try_into().unwrap();
    
            sqlx::query!(
                "UPDATE queue SET position = position + 10000 WHERE channel_id = ? AND position >= ?",
                channel, second_group
            ).execute(&mut *tx).await?;
    
            sqlx::query!(
                "UPDATE queue SET position = ? WHERE twitch_name = ? AND channel_id = ?",
                second_group, twitch_name, channel
            ).execute(&mut *tx).await?;
    
            // 🔹 Reorder positions for users after moving
            let mut new_position = second_group + 1;
            let queue_entries = sqlx::query!(
                "SELECT twitch_name FROM queue WHERE channel_id = ? AND position > 10000 ORDER BY position ASC",
                channel
            ).fetch_all(&mut *tx).await?;
    
            for entry in queue_entries {
                sqlx::query!(
                    "UPDATE queue SET position = ? WHERE twitch_name = ? AND channel_id = ?",
                    new_position,
                    entry.twitch_name,
                    channel
                )
                .execute(&mut *tx)
                .await?;
                new_position += 1;
            }
    
            format!("{} has been pushed to the second group", twitch_name)
        } else if words.len() == 3 {
            let runs: i32 = words[2].parse().unwrap_or(1);
            let second_group: i32 = (teamsize + 1).try_into().unwrap();
    
            sqlx::query!(
                "UPDATE queue SET position = position + 10000 WHERE channel_id = ? AND position >= ?",
                channel, second_group
            ).execute(&mut *tx).await?;
            sqlx::query!("UPDATE queue SET position = ?, group_priority = 1, priority_runs_left = COALESCE(?, priority_runs_left), locked_first = FALSE
                WHERE twitch_name = ? AND channel_id = ?",
                second_group, runs, twitch_name, channel
            ).execute(&mut *tx).await?;
    
            // 🔹 Reorder the rest of the queue
            
            let queue_entries = sqlx::query!(
                "SELECT twitch_name FROM queue 
                WHERE channel_id = ? 
                AND position > 10000 
                AND twitch_name != ? 
                ORDER BY position ASC",
                channel, twitch_name
            ).fetch_all(&mut *tx).await?;
    
            let mut new_position = second_group + 1;
            for entry in queue_entries {
                sqlx::query!(
                    "UPDATE queue SET position = ? WHERE twitch_name = ? AND channel_id = ?",
                    new_position, entry.twitch_name, channel
                ).execute(&mut *tx).await?;
                new_position += 1;
            }
    
            format!("{} has been promoted to priority for {} runs", twitch_name, runs)
        } else {
            "Wrong usage! Use: <twitch_name> [runs]".to_string()
        };
    
        tx.commit().await?;
    
        send_message(msg, client.lock().await.borrow_mut(), &reply).await?;
        Ok(())
    }

    //Moderator can remove player from queue
    pub async fn handle_remove(&self, msg: &tmi::Privmsg<'_>, client: Arc<tokio::sync::Mutex<tmi::Client>>, pool: &SqlitePool) -> Result<(), BotError> {
        let parts: Vec<&str> = msg.text().split_whitespace().collect();
        if parts.len() != 2 {
            return Ok(()); // No valid username provided
        }

        let mut twitch_name = parts[1].to_string();
        if twitch_name.starts_with("@") {
            twitch_name.remove(0);
        }

        let config = if let Some(config) = self.config.get_channel_config(&msg.channel()) {
            config
        } else {
            return Ok(());
        };
        let channel = config.queue_channel.clone();

        let mut tx = pool.begin().await?; // Start transaction

        // 🔹 Check if user exists
        let position = sqlx::query_scalar!(
            "SELECT position FROM queue WHERE twitch_name = ? AND channel_id = ?",
            twitch_name, channel
        ).fetch_optional(&mut *tx).await?;

        let reply = if let Some(_) = position {
            // 🔹 Remove user from queue
            sqlx::query!(
                "DELETE FROM queue WHERE twitch_name = ? AND channel_id = ?",
                twitch_name, channel
            ).execute(&mut *tx).await?;

            // 🔹 Fetch remaining queue, sorted by position
            let queue_entries = sqlx::query!(
                "SELECT twitch_name FROM queue WHERE channel_id = ? ORDER BY position ASC",
                channel
            ).fetch_all(&mut *tx).await?;

            // 🔹 Recalculate positions
            for (index, entry) in queue_entries.iter().enumerate() {
                let index: i32 = (index + 1).try_into().unwrap();
                sqlx::query!(
                    "UPDATE queue SET position = ? WHERE twitch_name = ? AND channel_id = ?",
                    index, entry.twitch_name, channel
                ).execute(&mut *tx).await?;
            }

            format!("{} has been removed from the queue.", twitch_name)
        } else {
            format!("User {} not found in the queue. FailFish", twitch_name)
        };

        tx.commit().await?;

        let mut client = client.lock().await;
        send_message(msg, &mut client, &reply).await?;
        
        Ok(())
    }   

    //Show the user where he is in queue
    pub async fn handle_pos(&self, msg: &tmi::Privmsg<'_>, client: Arc<tokio::sync::Mutex<tmi::Client>>, pool: &SqlitePool) -> Result<(), BotError> {
        let sender_name = msg.sender().name().to_string();
        let config = if let Some(config) = self.config.get_channel_config(msg.channel()) {
            config
        } else {
            return Ok(());
        };
        let teamsize = config.teamsize as i64;
        let channel = config.queue_channel.clone();
        let max_count: i64 = sqlx::query_scalar!(
            "SELECT COUNT(twitch_name) FROM queue WHERE channel_id = ?",
            channel
        ).fetch_one(pool).await?;
        // 🔹 Fetch position using a ranked query
        let result: Option<i64> = sqlx::query_scalar!(
            r#"
            WITH RankedQueue AS (
                SELECT twitch_name, ROW_NUMBER() OVER (ORDER BY position) AS position
                FROM queue WHERE channel_id = ?
            )
            SELECT position FROM RankedQueue WHERE twitch_name = ?
            "#,
            channel, sender_name
        ).fetch_optional(pool).await?;
        let sender = msg.sender().name().to_string();
        let reply = if !config.random_queue {
            match result {
                Some(index) => {
                    let group = (index - 1) / teamsize + 1;
                    if group == 1 {
                        format!("You are at position {}/{} and in LIVE group! DinoDance", index, max_count)
                    } else if group == 2 {
                        format!("You are at position {}/{} and in NEXT group! GoldPLZ", index, max_count)
                    } else {
                        format!("You are at position {}/{} (Group {}) !", index, max_count, group)
                    }
                },
                None => {
                    
                    if !config.open {
                        format!("The queue is CLOSED 🚫 and you are not in queue, {} ", sender)
                    } else if max_count >= TryInto::<i64>::try_into(config.len).unwrap() {
                        format!("Queue is FULL and you are not in queue, {}", sender)
                    } else {
                        format!("You are not in queue, {}. There is {} users in queue", sender, max_count)
                    }

                }
            }
        } else {
            match result {
                Some(_) => format!("✅ You are entered in the raffle, {sender}"),
                None => format!("❌ You are not entered in the raffle, {sender}")
            }
        };
        reply_to_message(&msg, client.lock().await.borrow_mut(), &reply).await?;
        Ok(())
    }

    //User leaves queue
    pub async fn handle_leave(&self, msg: &tmi::Privmsg<'_>, client: Arc<tokio::sync::Mutex<tmi::Client>>, pool: &SqlitePool) -> Result<(), BotError> {
        let name = msg.sender().name().to_string();
        let config = self.config.get_channel_config(msg.channel()).unwrap();
        let teamsize: i32 = config.teamsize.try_into().unwrap();
        let channel = config.queue_channel.clone();

        // 🔹 Fetch the player's position
        let position_to_leave = sqlx::query_scalar!(
            "SELECT position FROM queue WHERE twitch_name = ? AND channel_id = ?",
            name, channel
        ).fetch_optional(pool).await?;
        let reply = if let Some(position) = position_to_leave {
            
            if position <= teamsize.into() {
                format!("You cannot leave the live group! If you want to be removed ask streamer or wait for !next")
            } else {
                let mut tx = pool.begin().await?;
                
                // 🔹 Remove player from the queue
                sqlx::query!(
                    "DELETE FROM queue WHERE twitch_name = ? AND channel_id = ?",
                    name, channel
                ).execute(&mut *tx).await?;

                let entries = sqlx::query!(
                    "SELECT twitch_name FROM queue WHERE channel_id = ? ORDER BY position ASC",
                    channel
                ).fetch_all(&mut *tx).await?;
                
                for (new_position, entry) in entries.iter().enumerate() {
                    let new_position: i32 = (new_position + 1).try_into().unwrap();
                    sqlx::query!(
                        "UPDATE queue SET position = ? WHERE twitch_name = ? AND channel_id = ?",
                        new_position,
                        entry.twitch_name,
                        channel
                    ).execute(&mut *tx).await?;
                }

                tx.commit().await?;
                if !config.random_queue {
                    format!("BigSad {name} has left the queue.")
                } else {
                    format!("BigSad {name} has left the raffle.")
                }
            }
        } else {
            format!("You were already free, {name}")
        };
        send_message(&msg, client.lock().await.borrow_mut(), &reply).await?;
        Ok(())
    }
    

    //Shows whole queue
    pub async fn handle_queue(&self, msg: &tmi::Privmsg<'_>, client: Arc<tokio::sync::Mutex<tmi::Client>>, pool: &SqlitePool) -> Result<(), BotError> {
        let config = if let Some(config) = self.config.get_channel_config(msg.channel()) {
            config
        } else {
            return Ok(())
        };
        let channel = config.queue_channel.clone();
        let teamsize = config.teamsize as usize;
    
        // 🔹 Fetch queue data
        let queue_entries = sqlx::query!(
            "SELECT twitch_name, bungie_name FROM queue WHERE channel_id = ? ORDER BY position ASC, locked_first DESC, group_priority ASC",
            channel
        ).fetch_all(pool).await?;
    
        if queue_entries.is_empty() {
            send_message(msg, client.lock().await.borrow_mut(), "Queue is empty!").await?;
            return Ok(());
        }
        
        //Convert queue into formatted strings
        let queue_msg: Vec<String> = queue_entries.iter().enumerate().map(|(i, q)| format!("{}. {} ({})", i + 1, q.twitch_name, q.bungie_name)).collect();
        let format_group = |group: &[String]| group.join(", ");
        if config.random_queue {
            let live_group = if queue_msg.len() > 0 { &queue_msg[..queue_msg.len().min(teamsize)] } else { &[] };
            let rest_group = if queue_msg.len() > teamsize { &queue_msg[teamsize ..] } else { &[] };
            reply_to_message(&msg, client.lock().await.borrow_mut(), &format!("Chosen: {} // Entered people: {}", format_group(live_group), format_group(rest_group))).await?;
            return Ok(());
        }
        let reply = if queue_msg.iter().map(|s| s.len()).sum::<usize>() < 400 {
            let live_group = if queue_msg.len() > 0 { &queue_msg[..queue_msg.len().min(teamsize)] } else { &[] };
            let next_group = if queue_msg.len() > teamsize { &queue_msg[teamsize..queue_msg.len().min(teamsize * 2)] } else { &[] };
            let rest_group = if queue_msg.len() > teamsize * 2 { &queue_msg[teamsize * 2..] } else { &[] };
    
            format!(
                "LIVE: {} || NEXT: {} || QUEUE: {}",
                format_group(live_group), format_group(next_group), format_group(rest_group)
            )
        } else {
            format!("You can find queue here: https://krapmatt.bounceme.net/queue.html?streamer={}", channel.strip_prefix("#").unwrap_or(&channel))
        };
        
        reply_to_message(msg, client.lock().await.borrow_mut(), &reply).await?;
        
        Ok(())
    }

    //random fireteam
    pub async fn random(&mut self, msg: &tmi::Privmsg<'_>, client: Arc<tokio::sync::Mutex<tmi::Client>>) -> Result<(), BotError>{
        let channel = msg.channel().to_string();
        let config = &mut self.config;
        let bla = config.get_channel_config(msg.channel()).unwrap();
        let random_queue = bla.random_queue;
        let changed_channels = config.channels.iter_mut().filter_map(|(channel_id, channel_config)| {
            // Check if the channel matches the `queue_channel`
            if channel_config.queue_channel == channel {
                channel_config.random_queue = !random_queue;
                Some(channel_id.to_owned())
            } else {
                None
            }
        }).collect::<Vec<_>>();
        config.save_config();
        if random_queue {
            client.lock().await.privmsg(msg.channel(), &format!("Random queue has been disabled for these channels: {}", changed_channels.join(", "))).send().await?;
        } else {
            client.lock().await.privmsg(msg.channel(), &format!("Random queue has been enabled for these channels: {}", changed_channels.join(", "))).send().await?;
        }

        Ok(())
    }

    pub async fn move_groups(&self, msg: &tmi::Privmsg<'_>, client: Arc<tokio::sync::Mutex<tmi::Client>>, pool: &SqlitePool) -> Result<(), BotError> {
        let words: Vec<&str> = msg.text().split_ascii_whitespace().collect();
        if words.len() < 2 {
            send_message(msg, client.lock().await.borrow_mut(), "Usage: !move <twitch_name>").await?;
            return Ok(());
        }
    
        let twitch_name = words[1].strip_prefix("@").unwrap_or(words[1]).to_string();
    
        let config = if let Some(config) = self.config.get_channel_config(msg.channel()) {
            config
        } else {
            return Ok(());
        };
        let teamsize = config.teamsize as i64;
        let channel = &config.queue_channel;
    
        let mut tx = pool.begin().await?;
    
        // 🔹 Find the user's current position
        let position = sqlx::query_scalar!(
            "SELECT position FROM queue WHERE twitch_name = ? AND channel_id = ?",
            twitch_name, channel
        ).fetch_optional(&mut *tx).await?;
    
        let position = match position {
            Some(pos) => pos,
            None => {
                send_message(msg, client.lock().await.borrow_mut(), &format!("User {} isn’t in the queue!", twitch_name)).await?;
                return Ok(());
            }
        };
    
        // 🔹 Find the last position in the queue
        let max_pos = sqlx::query_scalar!(
            "SELECT COALESCE(MAX(position), 0) FROM queue WHERE channel_id = ?",
            channel
        ).fetch_one(&mut *tx).await?;
    
        let new_position = position + teamsize;
        if new_position > max_pos {
            send_message(msg, client.lock().await.borrow_mut(), &format!("User {} is already in the last group.", twitch_name)).await?;
            return Ok(());
        }
    
        // 🔹 Step 3: Temporarily move the user to a high out-of-the-way position
        let temp_position = max_pos + 1000;  // Safe position far from conflicts
        sqlx::query!(
            "UPDATE queue SET position = ? WHERE channel_id = ? AND twitch_name = ?",
            temp_position, channel, twitch_name
        ).execute(&mut *tx).await?;

        // 🔹 Step 4: Shift all affected users down
        let position = position + 1;
        sqlx::query!(
            "UPDATE queue SET position = position - 1 WHERE channel_id = ? AND position BETWEEN ? AND ?",
            channel, position, new_position
        ).execute(&mut *tx).await?;

        // 🔹 Step 5: Move the user to the correct new position
        sqlx::query!(
            "UPDATE queue SET position = ? WHERE channel_id = ? AND twitch_name = ?",
            new_position, channel, twitch_name
        ).execute(&mut *tx).await?;

        tx.commit().await?;

        send_message(msg, client.lock().await.borrow_mut(), &format!("User {} has been moved to the next group.", twitch_name)).await?;
        Ok(())
    }
    
    pub async fn toggle_combined_queue(&mut self, msg: &tmi::Privmsg<'_>, client: Arc<tokio::sync::Mutex<tmi::Client>>) -> Result<(), BotError> {
        let channel = msg.channel();
        let mut config = self.config.clone();
        // Get the shared group this channel belongs to
        let group = match self.get_group_for_channel_mut(&format!("#{}", channel)) {
            Some(g) => g,
            None => {
                send_message(msg, client.lock().await.borrow_mut(), "This channel is not in any shared streaming group.").await?;
                return Ok(());
            }
        };

        // Toggle combined state in the group
        let new_combined = group.toggle_combined();

        // Sync the queue_length and team_size from main channel config, if needed
        if let Some(main_config) = config.get_channel_config(&group.main_channel) {
            group.queue_length = main_config.len;
            group.team_size = main_config.teamsize;
        }

        // Update each channel's config to reflect combined queue state
        for chan in group.all_channels() {
            let chan_config = config.get_channel_config_mut(&chan);
            chan_config.combined = new_combined;
            chan_config.open = new_combined;
            chan_config.len = group.queue_length;
            chan_config.teamsize = group.team_size;
            chan_config.queue_channel = if new_combined {
                group.main_channel.clone()
            } else {
                chan.clone()
            };
            
        }

        // Save config persistently if you want
        config.save_config();

        // Reply to chat
        let reply = if new_combined {
            "Combined Queue activated"
        } else {
            "Combined Queue deactivated"
        };

        send_message(msg, client.lock().await.borrow_mut(), reply).await?;

        Ok(())
    }
}

pub fn is_valid_bungie_name(name: &str) -> Option<String> {
    BUNGIE_REGEX.captures(name).map(|caps| format!("{}#{}", &caps["name"].trim(), &caps["digits"]))
}

async fn add_to_queue(queue_len: usize, pool: &SqlitePool, user: &TwitchUser, channel_id: &str, join_type: Queue, raffle: bool) -> Result<String, BotError> {
    match join_type {
        Queue::Join => {
            let count: i64 = sqlx::query_scalar!(
                "SELECT COUNT(*) FROM queue WHERE channel_id = ?",
                channel_id
            ).fetch_one(pool).await.unwrap_or(0);
        
            if count >= queue_len as i64 {
                if !raffle {
                    return Ok(format!("❌ {}, you can't enter the queue, it is full", user.twitch_name));
                } else {
                    return Ok(format!("❌ {}, you can't enter the raffle, it is full", user.twitch_name));
                }
            }
            if bungie_name_exists_in_queue(pool, &user.bungie_name, channel_id).await? {
                return Ok(format!("❌ {}, wishes for some jail time ⛓", user.twitch_name))
            }
        },
        Queue::ForceJoin => {}
    }
    
    let next_position: i64 = sqlx::query_scalar!(
        "SELECT COALESCE(MAX(position), 0) + 1 FROM queue WHERE channel_id = ?",
        channel_id
    ).fetch_one(pool).await.unwrap_or(1);

    let result = sqlx::query!(
        "INSERT INTO queue (position, twitch_name, bungie_name, channel_id) VALUES (?, ?, ?, ?)",
        next_position, user.twitch_name, user.bungie_name, channel_id
    ).execute(pool).await;
    match result {
        Ok(_) => {
            if !raffle {
                Ok(format!("✅ {} entered the queue at position #{next_position}", user.twitch_name))
            } else {
                Ok(format!("✅ {} entered the raffle", user.twitch_name))
            }
        },
        Err(sqlx::Error::Database(err)) if err.is_unique_violation() => {
            Ok(format!("❌ Error Occured entering! {}", user.bungie_name))
        }
        Err(e) => Err(e.into())
    }
    
}

async fn send_invalid_name_reply(msg: &tmi::Privmsg<'_>, client: &mut Client, alias_config: Arc<AliasConfig>) -> Result<(), BotError> {
    println!("{:?}", alias_config.get_aliases("join"));
    let mut aliases = alias_config.get_aliases("join");
    let join = alias_config.get_removed_aliases("join");
    let j =alias_config.get_removed_aliases("j");

    if !join {
        aliases.push("join".to_string());
    }
    if !j {
        aliases.push("j".to_string());
    }  
    let reply1 = format!("❌ To join use: {}", aliases.join(" // "));
    let reply2 = format!("❌ If your bungiename is correct make sure your crosssave is on! Here is video to help: https://www.youtube.com/watch?v=2nncg_QYXPM");
    send_message(msg, client, &reply1).await?;
    send_message(msg, client, &reply2).await?;
    Ok(())
}

pub async fn process_queue_entry(msg: &tmi::Privmsg<'_>, client: &mut Client, queue_len: usize, pool: &SqlitePool, user: TwitchUser, channel_id: &str, queue_join: Queue, raffle: bool) -> Result<(), BotError> {
   
    let reply = if twitchuser_exists_in_queue(&pool, &user.clone().twitch_name, channel_id).await? {
        update_queue(&pool, &user).await?;
        format!("{} updated their Bungie name to {}", user.clone().twitch_name, user.clone().bungie_name)
    } else {
        add_to_queue(queue_len, &pool, &user, channel_id, queue_join, raffle).await?
    };
    send_message(msg, client, &reply).await?;
    Ok(())
}

async fn twitchuser_exists_in_queue(pool: &SqlitePool, twitch_name: &str, channel_id: &str) -> Result<bool, BotError> {
    let exists = sqlx::query_scalar!(
        "SELECT EXISTS(SELECT 1 FROM queue WHERE twitch_name = ? AND channel_id = ?)",
        twitch_name,
        channel_id
    ).fetch_one(pool).await.unwrap_or(0);
    Ok(exists == 1)
}

async fn bungie_name_exists_in_queue(pool: &SqlitePool, bungie_name: &str, channel_id: &str) -> Result<bool, BotError> {
    let exists = sqlx::query_scalar!(
        "SELECT EXISTS(SELECT 1 FROM queue WHERE bungie_name = ? AND channel_id = ?)",
        bungie_name,
        channel_id
    ).fetch_one(pool).await.unwrap_or(0);
    Ok(exists == 1)
}

async fn update_queue(pool: &SqlitePool, user: &TwitchUser) -> Result<(), BotError> {
    sqlx::query!(
        "UPDATE queue SET bungie_name = ? WHERE twitch_name = ?",
        user.bungie_name, user.twitch_name
    ).execute(pool).await?;
    Ok(())
}

async fn randomize_queue(pool: &SqlitePool, channel: &str, teamsize: i32) -> Result<String, BotError> {
    let mut tx = pool.begin().await?;
    let entries = sqlx::query!(
        "SELECT twitch_name FROM queue WHERE channel_id = ? ORDER BY position LIMIT ?",
        channel, teamsize
    ).fetch_all(&mut *tx).await?;
    for entry in entries {
        sqlx::query!(
            "DELETE FROM queue WHERE channel_id = ? AND twitch_name = ?",
            channel, entry.twitch_name
        ).fetch_all(&mut *tx).await?;
    }
    sqlx::query!(
        "UPDATE queue
         SET position = position + 10000
         WHERE channel_id = ?;",
        channel
    ).execute(&mut *tx).await?;

    // Step 2: Randomly assign new sequential positions
    sqlx::query!(
        "WITH shuffled AS (
            SELECT position, twitch_name, bungie_name, 
                   ROW_NUMBER() OVER () AS new_position
            FROM (SELECT * FROM queue WHERE channel_id = ? ORDER BY RANDOM())
        )
        UPDATE queue
        SET position = (SELECT new_position FROM shuffled WHERE shuffled.position = queue.position)
        WHERE channel_id = ?;",
        channel, channel
    ).execute(&mut *tx).await?;

    let next_group = sqlx::query!(
        "SELECT twitch_name, bungie_name FROM queue WHERE channel_id = ? ORDER BY position ASC LIMIT ?",
        channel, teamsize
    ).fetch_all(&mut *tx).await?;
    

    tx.commit().await?;
    let selected_team = next_group.iter().map(|q| format!("@{} ({})", q.twitch_name, q.bungie_name)).collect::<Vec<String>>().join(", ");
        // 🔹 Announce the random selection
    let announcement = format!("🎲 Randomly selected team: {}", selected_team);
    Ok(announcement)
}

async fn next_handler(channel: &str, teamsize: i32, pool: &SqlitePool) -> Result<String, BotError> {
    let mut tx = pool.begin().await?;

    // Step 1: Fetch current group
    let queue_entries = sqlx::query!(
        "SELECT twitch_name, priority_runs_left, locked_first 
         FROM queue 
         WHERE channel_id = ? 
         ORDER BY position ASC 
         LIMIT ?",
        channel, teamsize
    ).fetch_all(&mut *tx).await?;

    // Step 2: Check if any prio'd user is active in this group
    let has_active_prio = queue_entries.iter().any(|entry| {
        entry.locked_first.unwrap_or(false) && entry.priority_runs_left.unwrap_or(0) > 0
    });

    for entry in &queue_entries {
        match (entry.locked_first.unwrap_or(false), entry.priority_runs_left.unwrap_or(0)) {
            (true, runs_left) if runs_left > 0 => {
                let new_count = runs_left - 1;
                if new_count == 0 {
                    // Priority user done
                    sqlx::query!(
                        "DELETE FROM queue WHERE twitch_name = ? AND channel_id = ?",
                        entry.twitch_name,
                        channel
                    ).execute(&mut *tx).await?;
                } else {
                    // Decrement runs
                    sqlx::query!(
                        "UPDATE queue SET priority_runs_left = ? WHERE twitch_name = ? AND channel_id = ?",
                        new_count,
                        entry.twitch_name,
                        channel
                    ).execute(&mut *tx).await?;
                }
            }
            _ => {
                // Not a prio user — remove immediately
                sqlx::query!(
                    "DELETE FROM queue WHERE twitch_name = ? AND channel_id = ?",
                    entry.twitch_name,
                    channel
                ).execute(&mut *tx).await?;
            }
        }
    }

    // Step 3: Lock next group of priority users (if any)
    sqlx::query!(
        "UPDATE queue SET locked_first = TRUE
         WHERE channel_id = ? AND group_priority = 1 AND locked_first = FALSE 
         AND twitch_name IN (
             SELECT twitch_name FROM queue 
             WHERE channel_id = ? 
             ORDER BY position ASC 
             LIMIT ?
         )",
        channel, channel, teamsize
    ).execute(&mut *tx).await?;

    // Step 4: Get the new top of the queue for response
    let remaining_queue = sqlx::query!(
        "SELECT twitch_name, bungie_name FROM queue 
         WHERE channel_id = ? 
         ORDER BY position ASC 
         LIMIT ?",
        channel, teamsize
    ).fetch_all(&mut *tx).await?;

    let result: Vec<_> = remaining_queue
        .into_iter()
        .map(|row| format!("@{} ({})", row.twitch_name, row.bungie_name))
        .collect();

    // Step 5: Recalculate positions
    let rows = sqlx::query!(
        "SELECT rowid FROM queue WHERE channel_id = ? ORDER BY position ASC",
        channel
    ).fetch_all(&mut *tx).await?;

    for (index, row) in rows.iter().enumerate() {
        let new_pos = index as i32 + 1;
        sqlx::query!(
            "UPDATE queue SET position = ? WHERE rowid = ?",
            new_pos, row.rowid
        ).execute(&mut *tx).await?;
    }

    tx.commit().await?;

    Ok(if result.is_empty() {
        "Queue is empty".to_string()
    } else {
        format!("Next Group: {}", result.join(", "))
    })
}

async fn is_banned_from_queue(msg: &tmi::Privmsg<'_>, pool: &SqlitePool, client: &mut Client, bungie_name: &str, x_api_key: &str) -> Result<bool, BotError> {
    
    let membership_id = if let Some(id) = sqlx::query!("SELECT membership_id FROM user WHERE bungie_name = ?1",
        bungie_name
    ).fetch_optional(pool).await? {
        id.membership_id
    } else {
        Some(get_membershipid(bungie_name, x_api_key).await?.id)
    };

    if membership_id.is_none() {
        reply_to_message(&msg, client, "Please do !register <bungiename#0000>. You membership has not been registered!").await?;
        return Ok(true);
    }
    let membership_id = membership_id.unwrap();
    // Query for the ban reason
    let result: Option<(String,)> = sqlx::query_as::<_, (String,)>(
        "SELECT membership_id FROM banlist 
         WHERE membership_id = ? 
         AND (banned_until IS NULL OR banned_until > datetime('now'))"
    ).bind(&membership_id).fetch_optional(pool).await?;

    if let Some(_id) = result {
        let record = sqlx::query!("SELECT reason, banned_until FROM banlist WHERE membership_id = ?1", membership_id).fetch_one(pool).await?;
        let reply = if record.banned_until.is_none() {
            format!("You are banned from entering queue || Reason: {}", record.reason.into_iter().collect::<Vec<String>>().join(" "))
        } else {
            format!("You are timed out from entering queue || Reason: {} || Time left: {} hours", record.reason.into_iter().collect::<Vec<String>>().join(" "), hours_until(&record.banned_until.unwrap()).unwrap_or(0))
        };
        reply_to_message(&msg, client, &reply).await?;
        return Ok(true);
    } else {
        return Ok(false);
    }
}

fn hours_until(timestamp: &str) -> Option<i64> {
    let naive_datetime = NaiveDateTime::parse_from_str(timestamp, "%Y-%m-%d %H:%M:%S").unwrap();
    let datetime = DateTime::<Utc>::from_naive_utc_and_offset(naive_datetime, Utc);
    let now = Utc::now();
    let duration = datetime.signed_duration_since(now);
    Some(duration.num_hours())
}
