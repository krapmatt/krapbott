use dotenvy::{dotenv, var};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sqlx::{pool, SqlitePool};
use std::{borrow::BorrowMut, sync::Arc};
use serde_json::Value;
use tmi::{Badge, Client};

use crate::database::{is_bungiename, user_exists_in_database};
use crate::{api::{get_membershipid, get_users_clears, MemberShip}, bot::BotState, database::{load_membership, remove_command, save_command, save_to_user_database}, models::{BotError, CommandAction, TwitchUser}};

pub const ADMINS: &[&str] = &["KrapMatt", "ThatJK"];

pub async fn is_broadcaster(msg: &tmi::Privmsg<'_>, client: Arc<tokio::sync::Mutex<tmi::Client>>) -> bool {
    if msg.badges().into_iter().any(|badge| badge.as_badge_data().name() == "broadcaster") || ADMINS.contains(&&*msg.sender().name().to_string()) {
        return true;
    } else {
        _ = client.lock().await.privmsg(msg.channel(), "You are not a moderator/broadcaster. You can't use this command").send().await;
        return false;
    }
    
}

pub fn is_subscriber(msg: &tmi::Privmsg<'_>) -> bool {
    println!("{:?}", msg.badges().into_iter().collect::<Vec<&Badge<'_>>>());
    if msg.badges().into_iter().any(|badge| badge.as_badge_data().name() == "subscriber" || badge.as_badge_data().name() == "moderator" ) || ADMINS.contains(&&*msg.sender().name().to_string()) {
        true
    } else {
        false
    }
}

pub async fn is_moderator(msg: &tmi::Privmsg<'_>, client: Arc<tokio::sync::Mutex<tmi::Client>>) -> bool {
    if msg.badges().into_iter().any(|badge| badge.as_badge_data().name() == "moderator" || badge.as_badge_data().name() == "broadcaster") || ADMINS.contains(&&*msg.sender().name().to_string()) {
        return true;
    } else {
        _ = client.lock().await.privmsg(msg.channel(), "You are not a moderator/broadcaster. You can't use this command").send().await;
        return false;
    }
    
}
pub async fn is_vip(msg: &tmi::Privmsg<'_>, client: Arc<tokio::sync::Mutex<tmi::Client>>) -> bool {
    if msg.badges().into_iter().any(|badge| badge.as_badge_data().name() == "moderator" || badge.as_badge_data().name() == "broadcaster" || badge.as_badge_data().name() == "vip") || ADMINS.contains(&&*msg.sender().name().to_string()) {
        return true;
    } else {
        _ = client.lock().await.privmsg(msg.channel(), "You are not a VIP/Moderator. You can't use this command").send().await;
        return false;
    }
    
}

#[derive(Deserialize)]
struct ChatterResponse {
    data: Vec<Chatter>,
}

#[derive(Deserialize)]
struct Chatter {
    user_name: String,
}

pub async fn fetch_lurkers(broadcaster_id: &str, token: &str) -> Vec<String> {
    let url = format!(
        "https://api.twitch.tv/helix/chat/chatters?broadcaster_id={}&moderator_id={}",
        broadcaster_id, broadcaster_id
    );

    let res = reqwest::Client::new()
        .get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .header("Client-Id", "your_client_id")
        .send()
        .await
        .unwrap()
        .json::<ChatterResponse>()
        .await
        .unwrap();

    res.data.into_iter().map(|c| c.user_name).collect()
}

//Pro twitch na ban botů
#[derive(Serialize)]
struct BanRequest {
    data: BanData,
}
#[derive(Serialize)]
struct BanData {
    user_id: String,
}
// Best viewers on u.to/paq8IA
pub async fn ban_bots(msg: &tmi::Privmsg<'_>, oauth_token: &str, client_id: String) {
    let url = format!("https://api.twitch.tv/helix/moderation/bans?broadcaster_id={}&moderator_id=1091219021", msg.channel_id());
    
    let ban_request = BanRequest {
        data: BanData {
            user_id: msg.sender().id().to_string(),
        },
    };
    let res = reqwest::Client::new()
        .post(&url)
        .bearer_auth(oauth_token)
        .header("Client-Id", client_id)

        .header("Content-Type", "application/json")
        .body(serde_json::to_string(&ban_request).unwrap())
        
        .send()
        .await.expect("Bad reqwest");
    println!("{:?}", res.text().await);
}

pub async fn shoutout(oauth_token: &str, client_id: String, to_broadcaster_id: &str, broadcaster: &str) {
    let url = format!("https://api.twitch.tv/helix/chat/shoutouts?from_broadcaster_id={}&to_broadcaster_id={}&moderator_id=1091219021", broadcaster, to_broadcaster_id);
    let res = reqwest::Client::new()
    .post(url)
    .bearer_auth(oauth_token)
    .header("Client-Id", client_id)
    .send()
    .await.expect("Bad reqwest");
    println!("{:?}", res.text().await);

}

pub async fn is_channel_live(channel_id: &str, token: &str, client_id: &str) -> Result<bool, reqwest::Error> {
    let url = format!("https://api.twitch.tv/helix/streams?user_login={}", channel_id);
    let response = reqwest::Client::new()
        .get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .header("Client-ID", client_id)
        .send()
        .await?;
    let json: serde_json::Value = response.json().await?;
    Ok(json["data"].as_array().map_or(false, |data| !data.is_empty()))
}

pub async fn get_twitch_user_id(username: &str) -> Result<String, BotError> {
    let url = format!("https://api.twitch.tv/helix/users?login={}", username);

    let oauth_token = var("TWITCH_OAUTH_TOKEN_BOTT").expect("No oauth token");
    let client_id = var("TWITCH_CLIENT_ID_BOT").expect("No bot id");

    let client = reqwest::Client::new();
    let res = client
        .get(&url)
        .header("Client-id", client_id)
        .bearer_auth(oauth_token)
        .send()
        .await?;
    
    let parsed: Value = serde_json::from_str(&res.text().await?)?;
    if let Some(id) = parsed["data"][0]["id"].as_str() {
        return Ok(id.to_string()); 
    } else {
        Err(BotError { error_code: 107, string: None })
    }

}

//Not actually checking follow status
pub async fn is_follower(msg: &tmi::Privmsg<'_>, client: Arc<tokio::sync::Mutex<tmi::Client>>) -> bool {
    dotenv().ok();
    let oauth_token = var("TWITCH_OAUTH_TOKEN_BOTT").expect("No oauth token");
    let client_id = var("TWITCH_CLIENT_ID_BOT").expect("No bot id");

    let url = format!("https://api.twitch.tv/helix/channels/followers?broadcaster_id={}&user_id={}", msg.channel_id(), msg.sender().id());
    let res = reqwest::Client::new()
        .get(&url)
        .header("Client-Id", client_id)
        .bearer_auth(oauth_token)
        .send()
        .await.expect("Bad reqwest");
    //.
    if let Ok(a) = res.text().await  { 
        if a.contains("user_id") || msg.channel_id() == msg.sender().id() {
            true
        } else {
            let mut client = client.lock().await;
            let _ = send_message(msg, &mut client, "You are not a follower!").await;
            false
        }
    } else {
        let mut client = client.lock().await;
        let _ = send_message(msg, &mut client, "Error occured! Tell Matt").await;
        true
    }
}
lazy_static::lazy_static!{
    static ref BUNGIE_REGEX: Regex = Regex::new(r"^(?P<name>.+)#(?P<digits>\d{4})").unwrap();
}
pub fn is_valid_bungie_name(name: &str) -> Option<String> {
    BUNGIE_REGEX.captures(name).map(|caps| format!("{}#{}", &caps["name"].trim(), &caps["digits"]))
}

async fn is_banned_from_queue(msg: &tmi::Privmsg<'_>, pool: &SqlitePool, client: &mut Client) -> Result<bool, BotError> {
    let twitch_name = msg.sender().name().to_string();

    // Query for the ban reason
    let result = sqlx::query!("SELECT reason FROM banlist WHERE twitch_name = ?1",
        twitch_name
    ).fetch_optional(pool).await;

    match result {
        Ok(Some(record)) => {
            send_message(msg, client, &format!(
                    "You are banned from entering queue || Reason: {} || You can try to contact Streamer or MODS on discord for a solution", 
                    record.reason.into_iter().collect::<Vec<String>>().join(" ")
            )).await?;
            Ok(true)
        }
        Ok(None) => Ok(false),  // No entry found in the banlist
        Err(e) => {
            Err(e.into())
        }
    }
    
    
}


impl BotState {
    //User can join into queue
    pub async fn handle_join(&mut self, msg: &tmi::Privmsg<'_>, client: Arc<tokio::sync::Mutex<tmi::Client>>, pool: &SqlitePool) -> Result<(), BotError> {
        let mut client = client.lock().await;

        let config = self.config.get_channel_config(&msg.channel());
        let channel = config.clone().queue_channel;

        if config.open {
            if config.sub_only && !is_subscriber(&msg) {
                send_message(&msg, &mut client, "Only subscribers can enter the queue.").await?;
                return Ok(());
            }

            if !is_banned_from_queue(msg, pool, &mut client).await? {
                let mut bungie_name = user_exists_in_database(pool, msg.sender().name().to_string()).await;
                
                if bungie_name.is_none() {
                    if let Some((_join, name)) = msg.text().split_once(" ") {
                        if let Some(bungie) = is_valid_bungie_name(name) {
                            if is_bungiename(self.x_api_key.clone(), &bungie, &msg.sender().name(), pool).await {
                                bungie_name = Some(name.to_string());
                            }
                        }
                    }
                }

                if let Some(name) = bungie_name {
                    let new_user = TwitchUser {
                        twitch_name: msg.sender().name().to_string(),
                        bungie_name: name.trim().to_string(),
                    };
                    process_queue_entry(msg, &mut client, config.len, pool, new_user, &channel).await?;
                    
                } else {
                    send_invalid_name_reply(msg, &mut client).await?;
                }
            }
        } else {
            client.privmsg(msg.channel(), "Queue is closed").send().await?;
        }
        Ok(())
    }
    
    pub async fn handle_next(&mut self, channel_id: String, pool: &SqlitePool) -> Result<String, BotError> {
        let config = self.config.get_channel_config(&channel_id);
        let channel = config.queue_channel.clone();
        let teamsize: i32 = config.teamsize.try_into().unwrap();
    
        let mut tx = pool.begin().await?; // Start transaction
    
        // Fetch next group (priority first)
        let queue_entries = sqlx::query!(
            "SELECT twitch_name, priority_runs_left 
             FROM queue 
             WHERE channel_id = ? 
             ORDER BY 
                CASE WHEN priority_runs_left > 0 THEN 0 ELSE 1 END, 
                position ASC 
             LIMIT ?",
            channel, teamsize
        )
        .fetch_all(&mut *tx)
        .await?;
    
        let mut result = Vec::new();
    
        for entry in &queue_entries {
            if entry.priority_runs_left > Some(0) {
                // Reduce priority runs
                sqlx::query!(
                    "UPDATE queue SET priority_runs_left = priority_runs_left - 1 WHERE twitch_name = ? AND channel_id = ?",
                    entry.twitch_name,
                    channel
                )
                .execute(&mut *tx)
                .await?;
            } else {
                // Remove non-priority users
                sqlx::query!(
                    "DELETE FROM queue WHERE twitch_name = ? AND channel_id = ?",
                    entry.twitch_name,
                    channel
                )
                .execute(&mut *tx)
                .await?;
            }
        }
    
        // Fetch remaining queue
        let remaining_queue = sqlx::query!(
            "SELECT twitch_name, bungie_name FROM queue WHERE channel_id = ? ORDER BY position ASC LIMIT ?",
            channel, teamsize
        )
        .fetch_all(&mut *tx)
        .await?;
    
        for row in remaining_queue {
            result.push(format!("@{} ({})", row.twitch_name, row.bungie_name));
        }
    
        // Recalculate positions
        let mut rows = sqlx::query!(
            "SELECT rowid FROM queue WHERE channel_id = ? ORDER BY position ASC",
            channel_id
        )
        .fetch_all(&mut *tx)
        .await?;

        // Update positions
        for (index, row) in rows.iter().enumerate() {
            let index = index as i32 + 1;
            sqlx::query!(
                "UPDATE queue SET position = ? WHERE rowid = ?",
                index, // New position
                row.rowid
            )
            .execute(&mut *tx)
            .await?;
        }
    
        tx.commit().await?; // Commit transaction
    
        config.runs += 1;
        self.config.save_config();
    
        Ok(if result.is_empty() {
            "Queue is empty".to_string()
        } else {
            format!("Next Group: {}", result.join(", "))
        })
    }

    pub async fn prio(&mut self, msg: &tmi::Privmsg<'_>, client: Arc<tokio::sync::Mutex<tmi::Client>>, pool: &SqlitePool) -> Result<(), BotError> {
        let text = msg.text().to_owned();
        let words: Vec<&str> = text.split_ascii_whitespace().collect();
    
        if words.len() < 2 {
            let reply = "Wrong usage! Use: !prio <twitch_name> [runs]";
            send_message(msg, client.lock().await.borrow_mut(), reply).await?;
            return Ok(());
        }
    
        let mut twitch_name = words[1].to_string();
        if twitch_name.starts_with("@") {
            twitch_name.remove(0);
        }
    
        let config = self.config.get_channel_config(msg.channel());
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
    
            sqlx::query!(
                "UPDATE queue SET position = position + 10000 WHERE channel_id = ?",
                channel
            ).execute(&mut *tx).await?;
    
            sqlx::query!("UPDATE queue SET position = 1, locked_first = TRUE, group_priority = 1, priority_runs_left = ? 
                WHERE twitch_name = ? AND channel_id = ?",
                runs, twitch_name, channel
            ).execute(&mut *tx).await?;
    
            // 🔹 Reorder the rest of the queue
            let mut new_position = 2;
            let queue_entries = sqlx::query!(
                "SELECT twitch_name FROM queue WHERE channel_id = ? AND position > 10000 ORDER BY position ASC",
                channel
            ).fetch_all(&mut *tx).await?;
    
            for entry in queue_entries {
                sqlx::query!(
                    "UPDATE queue SET position = ? WHERE twitch_name = ? AND channel_id = ?",
                    new_position, entry.twitch_name, channel
                ).execute(&mut *tx).await?;
                new_position += 1;
            }
    
            format!("{} has been promoted to priority for {} runs", twitch_name, runs)
        } else {
            "Wrong usage! Use: !prio <twitch_name> [runs]".to_string()
        };
    
        tx.commit().await?;
    
        send_message(msg, client.lock().await.borrow_mut(), &reply).await?;
        Ok(())
    }

    //Moderator can remove player from queue
    pub async fn handle_remove(&mut self, msg: &tmi::Privmsg<'_>, client: Arc<tokio::sync::Mutex<tmi::Client>>, pool: &SqlitePool) -> Result<(), BotError> {
        let parts: Vec<&str> = msg.text().split_whitespace().collect();
        if parts.len() != 2 {
            return Ok(()); // No valid username provided
        }

        let mut twitch_name = parts[1].to_string();
        if twitch_name.starts_with("@") {
            twitch_name.remove(0);
        }

        let config = self.config.get_channel_config(&msg.channel());
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
    pub async fn handle_pos(&mut self, msg: &tmi::Privmsg<'_>, client: Arc<tokio::sync::Mutex<tmi::Client>>, pool: &SqlitePool) -> Result<(), BotError> {
        let sender_name = msg.sender().name().to_string();
        let config = self.config.get_channel_config(msg.channel());
        let teamsize = config.teamsize as i64;
        let channel = config.queue_channel.clone();

        // 🔹 Fetch position using a ranked query
        let result = sqlx::query_scalar!(
            r#"
            WITH RankedQueue AS (
                SELECT twitch_name, ROW_NUMBER() OVER (ORDER BY position) AS position
                FROM queue WHERE channel_id = ?
            )
            SELECT position FROM RankedQueue WHERE twitch_name = ?
            "#,
            channel, sender_name
        ).fetch_optional(pool).await?;

        let reply = match result {
            Some(index) => {
                let group = (index - 1) / teamsize + 1;
                if group == 1 {
                    format!("You are at position {} and in LIVE group! DinoDance", index)
                } else if group == 2 {
                    format!("You are at position {} and in NEXT group! GoldPLZ", index)
                } else {
                    format!("You are at position {} (Group {}) !", index, group)
                }
            }
            None => format!("You are not in the queue, {}.", sender_name),
        };
        send_message(&msg, client.lock().await.borrow_mut(), &reply).await?;
        Ok(())
    }

    //User leaves queue
    pub async fn handle_leave(&mut self, msg: &tmi::Privmsg<'_>, client: Arc<tokio::sync::Mutex<tmi::Client>>, pool: &SqlitePool) -> Result<(), BotError> {
        let name = msg.sender().name().to_string();
        let config = self.config.get_channel_config(msg.channel());
        let teamsize: i64 = config.teamsize.try_into().unwrap();
        let channel = config.queue_channel.clone();

        // 🔹 Fetch the player's position
        let position_to_leave = sqlx::query_scalar!(
            "SELECT position FROM queue WHERE twitch_name = ? AND channel_id = ?",
            name, channel
        ).fetch_optional(pool).await?;

        let reply = if let Some(position) = position_to_leave {
            if position <= teamsize {
                format!("You cannot leave the live group! If you want to be removed ask streamer or wait for !next")
            } else {
                let mut tx = pool.begin().await?;

                // 🔹 Remove player from the queue
                sqlx::query!(
                    "DELETE FROM queue WHERE twitch_name = ? AND channel_id = ?",
                    name, channel
                ).execute(&mut *tx).await?;

                // 🔹 Shift the positions of remaining players
                sqlx::query!(
                    "UPDATE queue SET position = position - 1 WHERE channel_id = ? AND position > ?",
                    channel, position
                ).execute(&mut *tx).await?;
                tx.commit().await?;
                format!("{} has been removed from queue.", name)
            }
        } else {
            format!("You are not in queue, {}.", name)
        };
        send_message(&msg, client.lock().await.borrow_mut(), &reply).await?;
        Ok(())
    }
    

    //Shows whole queue
    //UNWRAPS ON VEC????
    //TODO! COMBINED/SINGLE
    pub async fn handle_queue(&mut self, msg: &tmi::Privmsg<'_>, client: Arc<tokio::sync::Mutex<tmi::Client>>, pool: &SqlitePool) -> Result<(), BotError> {
        let config = self.config.get_channel_config(msg.channel());
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
    
        // 🔹 Convert queue into formatted strings
        let queue_msg: Vec<String> = queue_entries.iter().enumerate().map(|(i, q)| format!("{}. {} ({})", i + 1, q.twitch_name, q.bungie_name)).collect();
        let format_group = |group: &[String]| group.join(", ");
    
        let reply = if queue_msg.iter().map(|s| s.len()).sum::<usize>() < 400 {
            let live_group = if queue_msg.len() > 0 { &queue_msg[..queue_msg.len().min(teamsize)] } else { &[] };
            let next_group = if queue_msg.len() > teamsize { &queue_msg[teamsize..queue_msg.len().min(teamsize * 2)] } else { &[] };
            let rest_group = if queue_msg.len() > teamsize * 2 { &queue_msg[teamsize * 2..] } else { &[] };
    
            vec![format!(
                "LIVE: {} || NEXT: {} || QUEUE: {}",
                format_group(live_group), format_group(next_group), format_group(rest_group)
            )]
        } else {
            let mut formatted_groups = Vec::new();
            let mut start = 0;
    
            for group_num in 1..=((queue_msg.len() + teamsize - 1) / teamsize) {
                let end = (start + teamsize).min(queue_msg.len());
                if start < end {
                    formatted_groups.push(format!(
                        "🛡️ GROUP {} || {}",
                        group_num,
                        format_group(&queue_msg[start..end])
                    ));
                }
                start = end;
            }
            formatted_groups
        };
        for msg_part in reply {
            send_message(msg, client.lock().await.borrow_mut(), &msg_part).await?;
        }
        Ok(())
    }

    //random fireteam
    pub async fn random(&mut self, msg: &tmi::Privmsg<'_>, client: Arc<tokio::sync::Mutex<tmi::Client>>, pool: &SqlitePool) -> Result<(), BotError>{
        let channel = msg.channel().to_string();
        let teamsize = self.config.get_channel_config(&channel).teamsize;
        let teamsize_i32: i32 = teamsize.try_into().unwrap();
        // 🔹 Fetch all users from queue
        let queue_entries = sqlx::query!(
            "SELECT twitch_name, bungie_name FROM queue WHERE channel_id = ? ORDER BY RANDOM() LIMIT ?",
            channel, teamsize_i32
        ).fetch_all(pool).await?;

        // 🔹 Ensure we have enough players
        if queue_entries.is_empty() {
            send_message(msg, client.lock().await.borrow_mut(), "Queue is empty!").await?;
            return Ok(());
        }
        if queue_entries.len() < teamsize {
            send_message(msg, client.lock().await.borrow_mut(),
                &format!("Not enough players for a full team! Only selected: {}",
                    queue_entries.iter().map(|q| format!("@{} ({})", q.twitch_name, q.bungie_name)).collect::<Vec<String>>().join(", ")
                ),
            ).await?;
            return Ok(());
        }
        // 🔹 Format the selected team
        let selected_team = queue_entries.iter().map(|q| format!("@{} ({})", q.twitch_name, q.bungie_name)).collect::<Vec<String>>().join(", ");
        // 🔹 Announce the random selection
        let announcement = format!("🎲 Randomly selected team: {}", selected_team);
        send_message(msg, client.lock().await.borrow_mut(), &announcement).await?;

        Ok(())
    }
    //Get total clears of raid of a player
    pub async fn total_raid_clears(&mut self, msg: &tmi::Privmsg<'_>, client: &mut Client, pool: &SqlitePool) -> Result<(), BotError> {
        let mut membership = MemberShip { id: String::new(), type_m: -1 };
        let words: Vec<&str> = msg.text().split_ascii_whitespace().collect();
        
        let reply = if words.len() > 1 {
            let mut name = words[1..].to_vec().join(" ").to_string();
            
            if let Some(name) = is_valid_bungie_name(&name) {
                match get_membershipid(&name, self.x_api_key.clone()).await {
                    Ok(ship) => membership = ship,
                    Err(err) => client.privmsg(msg.channel(), &format!("Error: {}", err)).send().await?,
                }
            } else {
                if name.starts_with("@") {
                    name.remove(0); 
                }
                if let Some(ship) = load_membership(&pool, name.clone()).await {
                    membership = ship;
                } else {
                    client.privmsg(msg.channel(), "Twitch user isn't registered in the database! Use their Bungie name!").send().await?;
                    return Ok(());
                }
            }
            let clears = get_users_clears(membership.id, membership.type_m, self.x_api_key.clone()).await? as i32;
            format!("{} has total {} raid clears", name, clears)
        } else {
            if let Some(membership) = load_membership(&pool, msg.sender().name().to_string()).await {
                let clears = get_users_clears(membership.id, membership.type_m, self.x_api_key.clone()).await? as i32;
                format!("You have total {} raid clears", clears)
            } else {
                format!("ItsBoshyTime {} is not registered to the database. Use !register <yourbungiename#0000>", msg.sender().name())
            }
        };
        client.privmsg(msg.channel(), &reply).send().await?;
        Ok(())
    }

    pub async fn add_package(&mut self, msg: &tmi::Privmsg<'_>, client: Arc<tokio::sync::Mutex<tmi::Client>>) -> Result<(), BotError> {
        let config = self.config.get_channel_config(msg.channel());
        let words: Vec<&str> = msg.text().split_ascii_whitespace().collect();

        if words.len() <= 1 {
            send_message(&msg, client.lock().await.borrow_mut(), "You didnt mention name of the package!").await?;
            return Ok(());
        }

        let package_name = words[1..].join(" ").to_string();

        config.packages.push(package_name.clone());
        self.config.save_config();
        send_message(&msg, client.lock().await.borrow_mut(), &format!("Package {} has been added", package_name)).await?;

        Ok(())
    }

    pub async fn move_groups(&mut self, msg: &tmi::Privmsg<'_>, client: Arc<tokio::sync::Mutex<tmi::Client>>, pool: &SqlitePool) -> Result<(), BotError> {
        let words: Vec<&str> = msg.text().split_ascii_whitespace().collect();
        if words.len() < 2 {
            send_message(msg, client.lock().await.borrow_mut(), "Usage: !move <twitch_name>").await?;
            return Ok(());
        }
    
        let mut twitch_name = words[1].strip_prefix("@").unwrap_or(words[1]).to_string();
    
        let config = self.config.get_channel_config(msg.channel());
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
            "SELECT MAX(position) FROM queue WHERE channel_id = ?",
            channel
        ).fetch_one(&mut *tx).await?.unwrap_or(0);
    
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
        // Update the bot confi
        let mut config = self.clone().config;
        
        if let Some(shared_chats) = self.streaming_together.get(msg.channel()) {
            let source_config = self.config.get_channel_config(msg.channel());
            let mut all_channels = shared_chats.clone();
            all_channels.insert(msg.channel().to_string());
            for channel in all_channels {
                let channel_config = config.get_channel_config(&channel);
                channel_config.combined = !channel_config.combined;
                channel_config.open = !channel_config.open;
                channel_config.len = source_config.len;
                channel_config.teamsize = source_config.teamsize;
                if channel_config.combined {
                    channel_config.queue_channel = msg.channel().to_string();
                } else {
                    channel_config.queue_channel = channel.clone();
                }
            }
            let reply = if !source_config.combined {
                "Combined Queue activated"
            } else {
                "Combined Queue deactivated"
            };
            send_message(&msg, client.lock().await.borrow_mut(), reply).await?;

            config.save_config();
        }
    
        Ok(())
    }
}

async fn send_invalid_name_reply(msg: &tmi::Privmsg<'_>, client: &mut Client) -> Result<(), BotError>{
    let reply = format!("❌ Use !join bungiename#0000, {}!", msg.sender().name());
    send_message(msg, client, &reply).await?;
    Ok(())
}

pub async fn process_queue_entry(msg: &tmi::Privmsg<'_>, client: &mut Client, queue_len: usize, pool: &SqlitePool, user: TwitchUser, channel_id: &str) -> Result<(), BotError> {
   
    let reply = if user_exists_in_queue(&pool, &user.clone().twitch_name, channel_id).await? {
        update_queue(&pool, &user).await?;
        format!("{} updated their Bungie name to {}", msg.sender().name(), user.clone().bungie_name)
    } else {
        add_to_queue(msg, queue_len, &pool, &user, channel_id).await?
    };
    send_message(msg, client, &reply).await?;
    Ok(())
}

async fn user_exists_in_queue(pool: &SqlitePool, twitch_name: &str, channel_id: &str) -> Result<bool, BotError> {
    let exists = sqlx::query_scalar!(
        "SELECT EXISTS(SELECT 1 FROM queue WHERE twitch_name = ? AND channel_id = ?)",
        twitch_name,
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
//TODO! redo the queue so that combined/solo settings
async fn add_to_queue<'a>(msg: &tmi::Privmsg<'_>, queue_len: usize, pool: &SqlitePool, user: &TwitchUser, channel_id: &str) -> Result<String, BotError> {
    let count: i64 = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM queue WHERE channel_id = ?",
        channel_id
    ).fetch_one(pool).await.unwrap_or(0);

    if count >= queue_len as i64 {
        return Ok("❌ You can't enter queue, it is full".to_string());
    }

    let next_position: i64 = sqlx::query_scalar!(
        "SELECT COALESCE(MAX(position), 0) + 1 FROM queue WHERE channel_id = ?",
        channel_id
    ).fetch_one(pool).await.unwrap_or(1);

    sqlx::query!(
        "INSERT INTO queue (position, twitch_name, bungie_name, channel_id) VALUES (?, ?, ?, ?)",
        next_position, user.twitch_name, user.bungie_name, channel_id
    ).execute(pool).await?;

    Ok(format!("✅ {} entered the queue at position #{}", msg.sender().name(), next_position))
}

pub async fn register_user(pool: &SqlitePool, twitch_name: &str, bungie_name: &str) -> Result<String, BotError> {
    dotenv().ok();
    let x_api_key = var("XAPIKEY").expect("No bungie api key");
    let reply = if let Some(bungie_name) = is_valid_bungie_name(bungie_name) {
        let new_user = TwitchUser {
            twitch_name: twitch_name.to_string(),
            bungie_name: bungie_name.to_string()
        };
        save_to_user_database(&pool, new_user, x_api_key).await?
    } else {
        "❌ You have typed invalid format of bungiename, make sure it looks like -> bungiename#0000".to_string()
    };
    Ok(reply)
    
}
//if is/not in database
pub async fn bungiename(msg: &tmi::Privmsg<'_>, client: &mut Client, pool: &SqlitePool, twitch_name: String) -> Result<(), BotError> {
    let result = sqlx::query_scalar!(
        "SELECT bungie_name FROM user WHERE twitch_name = ?",
        twitch_name
    ).fetch_optional(pool).await?;

    let reply = match result {
        Some(bungie_name) => format!("@{} || BungieName: {} ||", twitch_name, bungie_name),
        None => format!("{}, you are not registered", twitch_name),
    };

    send_message(msg, client, &reply).await?;

    Ok(())
}
#[derive(Serialize)]
struct Data {
    message: String,
    color: String
}
//Make announcment automatizations!
pub async fn announcement(broadcaster_id: &str, mod_id: &str, oauth_token: &str, client_id: String, message: String) -> Result<(), BotError> {
    let url = format!("https://api.twitch.tv/helix/chat/announcements?broadcaster_id={}&moderator_id={}", broadcaster_id, mod_id);
    let res = reqwest::Client::new()
        .post(&url)
        .header("Client-Id", client_id)
        .bearer_auth(oauth_token)
        .form(&Data {message: message, color: "primary".to_string()})
        .send()
        .await.expect("Bad reqwest");
    println!("{:?}", res.text().await);
    
    Ok(())
}

pub async fn send_message(msg: &tmi::Privmsg<'_>, client: &mut Client, reply: &str) -> Result<(), BotError> {
    client.privmsg(msg.channel(), &reply).send().await?;
    Ok(())
}

pub async fn unban_player_from_queue(msg: &tmi::Privmsg<'_>, client:Arc<tokio::sync::Mutex<Client>>, pool: &SqlitePool) -> Result<(), BotError> {
    let words: Vec<&str> = msg.text().split_ascii_whitespace().collect();
    let reply = if words.len() < 2 {
        "Maybe try to add a Twitch name. Somebody deserves the unban. :krapmaStare:".to_string()
    } else {
        let twitch_name = words[1].strip_prefix("@").unwrap_or(words[1]).to_string();

        let affected_rows = sqlx::query!(
            "DELETE FROM banlist WHERE twitch_name = ?",
            twitch_name
        ).execute(pool).await?.rows_affected();

        if affected_rows > 0 {
            format!("User {} has been unbanned from queue! They are free to enter again. :krapmaHeart:", twitch_name)
        } else {
            format!("User {} was not found in the banlist.", twitch_name)
        }
    };
    send_message(msg, client.lock().await.borrow_mut(), &reply).await?;
    Ok(())
}

pub async fn ban_player_from_queue(msg: &tmi::Privmsg<'_>, client:Arc<tokio::sync::Mutex<Client>>, pool: &SqlitePool) -> Result<(), BotError> {
    let words: Vec<&str> = msg.text().split_ascii_whitespace().collect();
    if words.len() < 2 {
        send_message(msg, client.lock().await.borrow_mut(), "Usage: !mod_ban <twitch name> Optional(reason)").await?;
        return Ok(());
    }

    let twitch_name = words[1].strip_prefix("@").unwrap_or(words[1]).to_string();
    let twitch_name_clone = twitch_name.clone();

    let reason = if words.len() > 2 { words[2..].join(" ") } else { String::new() };

    // Insert into the banlist using sqlx
    let result = sqlx::query!(
        "INSERT INTO banlist (twitch_name, reason) VALUES (?, ?)",
        twitch_name, reason
    ).execute(pool).await;

    match result {
        Ok(_) => {
            send_message(msg, client.lock().await.borrow_mut(), &format!("User {} has been banned from entering queue.", twitch_name_clone)).await?;
        }
        Err(_) => {
            send_message(msg, client.lock().await.borrow_mut(), "An error occurred while banning the user.").await?;
        }
    }

    Ok(())
}



pub async fn modify_command(msg: &tmi::Privmsg<'_>, client:Arc<tokio::sync::Mutex<Client>>, pool: &SqlitePool, action: CommandAction, channel: Option<String>) -> Result<(), BotError> {
    let words: Vec<&str> = msg.text().split_whitespace().collect();
    let mut reply;
    if words.len() < 2 {
        reply = "Use: !help (and your desired command)".to_string();
    }
    
    let command = words[1].to_string().to_ascii_lowercase();
    let reply_to_command = words[2..].join(" ").to_string();
    
    match action {
        CommandAction::Add => {
            reply = save(&pool, command, reply_to_command, channel, "Use: !help !addcommand").await?;
        }
        CommandAction::Remove => {
            if remove_command(&pool, &command).await {
                reply = format!("Command !{} removed.", command)
            } else {
                reply = format!("Command !{} doesn't exist.", command)
            }
        }
        CommandAction::AddGlobal => {
            reply = save(&pool, command, reply_to_command, None, "Use: !help !addglobalcommand").await?;
            
        } 
    };
    send_message(msg, client.lock().await.borrow_mut(), &reply).await?;
    Ok(())
}

async fn save(pool: &SqlitePool, command: String, reply: String, channel: Option<String>, error_mess: &str) -> Result<String, BotError> {
    if !reply.is_empty() {
        save_command(&pool, command.clone(), reply, channel).await;
        Ok(format!("Command !{} added.", command))
    } else {
        Ok(error_mess.to_string())
    }
}

