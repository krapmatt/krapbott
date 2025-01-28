use async_sqlite::rusqlite::params;
use regex::Regex;
use serde::Serialize;
use std::{borrow::BorrowMut, cmp::min, collections::HashSet, sync::{Arc, Mutex}};
use async_sqlite::{rusqlite::OptionalExtension, Client as SqliteClient};
use dotenv::var;


use serde_json::Value;
use tmi::Client;

use crate::database::user_exists_in_database;
use crate::{api::{get_membershipid, get_users_clears, MemberShip}, bot::BotState, database::{load_membership, pick_random, remove_command, save_command, save_to_user_database}, models::{BotError, CommandAction, TwitchUser}};

pub const ADMINS: &[&str] = &["KrapMatt", "ThatJK"];

pub async fn is_broadcaster(msg: &tmi::Privmsg<'_>, client: Arc<tokio::sync::Mutex<tmi::Client>>) -> bool {
    if msg.badges().into_iter().any(|badge| badge.as_badge_data().name() == "broadcaster") || ADMINS.contains(&&*msg.sender().name().to_string()) {
        return true;
    } else {
        _ = client.lock().await.privmsg(msg.channel(), "You are not a moderator/broadcaster. You can't use this command").send().await;
        return false;
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
    if msg.badges().into_iter().any(|badge| badge.as_badge_data().name() == "moderator" || badge.as_badge_data().name() == "broadcaster" || badge.as_badge_data().name() == "vip") | ADMINS.contains(&&*msg.sender().name().to_string()) {
        return true;
    } else {
        _ = client.lock().await.privmsg(msg.channel(), "You are not a VIP/Moderator. You can't use this command").send().await;
        return false;
    }
    
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
    dotenv::dotenv().ok();
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

async fn is_banned_from_queue(msg: &tmi::Privmsg<'_>, conn: &SqliteClient, client: &mut Client) -> Result<bool, BotError> {
    let twitch_name = msg.sender().name().to_string();
    if let Ok(reason) = conn.conn( move |conn| {
        conn.query_row("SELECT reason FROM banlist WHERE twitch_name = ?1", params![twitch_name], |row| row.get::<_, String>(0))
    }).await {
        send_message(msg, client, &format!("You are banned from entering queue || Reason: {} || You can try to contact Streamer or MODS on discord for a solution", reason)).await?;
        Ok(true)
    } else {
        Ok(false)
    }
    
    
}

impl BotState {
    //User can join into queue
    pub async fn handle_join(&mut self, msg: &tmi::Privmsg<'_>, client: Arc<tokio::sync::Mutex<tmi::Client>>, conn: &SqliteClient) -> Result<(), BotError> {
        let mut client = client.lock().await;

        let config = self.config.get_channel_config(&msg.channel());
        let channel = config.clone().queue_channel;

        if config.open {
            if !is_banned_from_queue(msg, conn, &mut client).await? {
                if let Some((_join, name)) = msg.text().split_once(" ") {
                    if let Some(name) = is_valid_bungie_name(name) {
                        let mut new_user = TwitchUser {
                            twitch_name: msg.sender().name().to_string(),
                            bungie_name: name.trim().to_string(),
                        };
                        if let Some(bungie_name) = user_exists_in_database(conn, new_user.twitch_name.clone()).await {
                            new_user.bungie_name = bungie_name;
                        }

                        process_queue_entry(msg, &mut client, config.len, conn, new_user, Some(channel)).await?;
                    
                    } else {
                        send_invalid_name_reply(msg, &mut client).await?;
                    }
                } else {
                    if let Some(bungie_name) = user_exists_in_database(conn, msg.sender().name().to_string()).await {
                        let new_user = TwitchUser {
                            twitch_name: msg.sender().name().to_string(),
                            bungie_name: bungie_name
                        };
                        process_queue_entry(msg, &mut client, config.len, conn, new_user, Some(channel)).await?;
                    } else {
                        send_invalid_name_reply(msg, &mut client).await?;
                    }
                    
                }
            }
            Ok(())
        } else {
            client.privmsg(msg.channel(), "Queue is closed").send().await?;
            Ok(())
        }
    }
    
    pub async fn handle_next(&mut self, channel_id: String, conn: &SqliteClient) -> Result<String, BotError> {
        
        let mut bot_state = self.clone();
        let config = bot_state.config.get_channel_config(&channel_id);
        let channel = config.clone().queue_channel;

        let teamsize = config.teamsize;

        let queue_msg = conn.conn(move |conn| {
            
            
            // Query priority and non-priority in a single operation
            let mut stmt = conn.prepare("
                SELECT twitch_name, priority_runs_left
                FROM queue
                WHERE channel_id = ?1
                ORDER BY CASE
                        WHEN priority_runs_left > 0 THEN 0
                        ELSE 1
                    END,
                    position ASC
                LIMIT ?2
            ")?;
    
            let queue_entries: Vec<(String, i32)> = stmt
                .query_map(params![channel, teamsize], |row| {
                    Ok((
                        row.get(0)?, // twitch_name
                        row.get(1)?, // priority_runs_left
                    ))
                })?
                .filter_map(Result::ok)
                .collect();
    
            // Process entries
            let mut result = Vec::new();
            for (twitch_name, priority_runs_left) in queue_entries {
                if priority_runs_left > 0 {
                    conn.execute(
                        "UPDATE queue SET priority_runs_left = priority_runs_left - 1 WHERE twitch_name = ?1 AND channel_id = ?2",
                        params![twitch_name, channel],
                    )?;
                } else {
                    conn.execute(
                        "DELETE FROM queue WHERE twitch_name = ?1 AND channel_id = ?2",
                        params![twitch_name, channel],
                    )?;
                }
                
            }
            let mut stmt = conn.prepare("SELECT twitch_name, bungie_name FROM queue WHERE channel_id = ?1 ORDER BY position ASC LIMIT ?2")?;
            let queue_iter = stmt.query_map(params![channel, teamsize], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?.filter_map(Result::ok);

            for user in queue_iter {
                result.push(format!("@{} ({})", user.0, user.1));
            }
            // Recalculate positions
            let mut stmt = conn.prepare("SELECT rowid FROM queue WHERE channel_id = ?1 ORDER BY position ASC")?;
            let rows: Vec<i64> = stmt.query_map(params![channel], |row| row.get(0))?
                .filter_map(Result::ok)
                .collect();
    
            for (new_position, rowid) in rows.into_iter().enumerate() {
                conn.execute(
                    "UPDATE queue SET position = ?1 WHERE rowid = ?2",
                    params![new_position as i32 + 1, rowid],
                )?;
            }
    
            Ok(result)
        }).await?;
        config.runs += 1;
        bot_state.config.save_config();
        let reply = if queue_msg.is_empty() {
            "Queue is empty".to_string()
        } else {
            format!("Next Group: {}", queue_msg.join(", "))
        };
        return Ok(reply);
    }

    pub async fn prio(&mut self, msg: &tmi::Privmsg<'_>, client: Arc<tokio::sync::Mutex<tmi::Client>>, conn: &SqliteClient) -> Result<(), BotError> {

        let text = msg.text().to_owned();
        
        let config = self.config.get_channel_config(msg.channel());
        let channel = config.clone().queue_channel;
        let teamsize = config.teamsize;
        
        let reply = conn.conn(move |conn| {
            let words: Vec<&str> = text.split_ascii_whitespace().to_owned().collect();
            let mut twitch_name = words[1].to_string();
            if twitch_name.starts_with("@") {
                twitch_name.remove(0);
            }
            if words.len() == 2 {
                let second_group = teamsize + 1;
    
                let mut stmt = conn.prepare(
                    "SELECT position FROM queue WHERE twitch_name = ?1 AND channel_id = ?2")?;
                if stmt.query_row(params![twitch_name, channel], |row| row.get::<_, i32>(0))
                    .optional()?.is_some() {
                        conn.execute(
                            "UPDATE queue SET position = position + 10000 WHERE channel_id = ?1 AND position >= ?2", 
                            params![channel, second_group]
                        )?;
            
                        conn.execute(
                            "UPDATE queue SET position = ?1 WHERE twitch_name = ?2 AND channel_id = ?3",
                            params![second_group, twitch_name, channel]
                        )?;

                        let mut new_position = second_group + 1;
                        let mut reorder_stmt = conn.prepare("SELECT twitch_name FROM queue WHERE channel_id = ?1 AND position > 10000 ORDER BY position ASC")?;
                        let queue_iter = reorder_stmt.query_map(params![channel], |row| row.get::<_, String>(0))?;
            
                        for name in queue_iter {
                            let name = name?;
                            conn.execute(
                                "UPDATE queue SET position = ?1 WHERE twitch_name = ?2 AND channel_id = ?3",
                                params![new_position, name, channel],
                            )?;
                            new_position += 1;
                        }
    
                    Ok(format!("{} has been pushed to the second group", twitch_name))
                } else {
                    Ok(format!("User {} not found in the queue", twitch_name))
                }
            } else if words.len() == 3 {
                let runs = words[2].to_string();
                let mut stmt = conn.prepare("SELECT position FROM queue WHERE twitch_name = ?1 AND channel_id = ?2")?;
                if let Some(_current_position) = stmt.query_row(params![twitch_name, channel], |row| row.get::<_, i32>(0)).optional()? {
                    
                    conn.execute(
                        "UPDATE queue SET position = position + 10000 WHERE channel_id = ?1",
                        params![channel],
                    )?;
        
                    conn.execute(
                        "UPDATE queue SET position = 1, locked_first = TRUE, group_priority = 1, priority_runs_left = ?1
                        WHERE twitch_name = ?2 AND channel_id = ?3",
                        params![runs, twitch_name, channel],
                    )?;
        
                    let mut new_position = 2;
                    let mut reorder_stmt = conn.prepare("SELECT twitch_name FROM queue WHERE channel_id = ?1 AND position > 10000 ORDER BY position ASC")?;
                    let queue_iter = reorder_stmt.query_map(params![channel], |row| row.get::<_, String>(0))?;
        
                    for name in queue_iter {
                        let name = name?;
                        conn.execute(
                            "UPDATE queue SET position = ?1 WHERE twitch_name = ?2 AND channel_id = ?3",
                            params![new_position, name, channel],
                        )?;
                        new_position += 1;
                    }
                    Ok(format!("{} has been promoted to priority for {} runs", twitch_name, runs))
                } else {
                    Ok(format!("User {} not found in the queue", twitch_name))
                }
            } else {
                Ok(format!("Wrong usage! !prio twitch_name"))
            }
        }).await?;

        
        send_message(msg, client.lock().await.borrow_mut(), &reply).await?;

        Ok(())
    }

    //Moderator can remove player from queue
    pub async fn handle_remove(&mut self, msg: &tmi::Privmsg<'_>, client: Arc<tokio::sync::Mutex<tmi::Client>>, conn: &SqliteClient) -> Result<(), BotError> {
        let parts: Vec<&str> = msg.text().split_whitespace().collect();
        let mut client = client.lock().await;
        
        let config = self.config.get_channel_config(&msg.channel());
        let channel = config.clone().queue_channel;
        
        if parts.len() == 2 {
            let mut twitch_name = parts[1].to_string().to_owned();
            if twitch_name.starts_with("@") {
                twitch_name.remove(0);
            }

            let reply = conn.conn(move |conn| {
                match conn.query_row(
                    "SELECT position FROM queue WHERE twitch_name = ?1 AND channel_id = ?2", 
                    params![twitch_name, channel], |row| row.get::<_, i32>(0)
                ) {
                    Ok(position) => {
                        conn.execute("DELETE FROM queue WHERE twitch_name = ?1 AND channel_id = ?2", params![twitch_name, channel])?;
                        let mut stmt = conn.prepare(
                            "SELECT twitch_name FROM queue WHERE channel_id = ?1 ORDER BY position ASC",
                        )?;
                        let mut rows = stmt.query(params![channel])?;
                        let mut new_position = 1;
                        while let Some(row) = rows.next()? {
                            let name: String = row.get(0)?;
                            conn.execute(
                                "UPDATE queue SET position = ?1 WHERE twitch_name = ?2 AND channel_id = ?3",
                                params![new_position, name, channel],
                            )?;
                            new_position += 1;
                        }
                        Ok(format!("{} has been removed from the queue.", twitch_name))
                    }

                    Err(_err) => {
                        Ok(format!("User {} not found in the queue. FailFish ", twitch_name))
                    }
                }
            }).await?;
            send_message(msg, &mut client, &reply).await?;
        }
    
        
        Ok(())
    }

    //Show the user where he is in queue
    pub async fn handle_pos(&mut self, msg: &tmi::Privmsg<'_>, client: Arc<tokio::sync::Mutex<tmi::Client>>, conn: &SqliteClient) -> Result<(), BotError> {
        let reply = Arc::new(tokio::sync::Mutex::new(String::new()));
        let reply_clone = Arc::clone(&reply);
        let mut client = client.lock().await;
        
        let sender_name = msg.sender().name().to_string();
        let config = self.config.get_channel_config(msg.channel()); 
        let teamsize = config.teamsize as i64; 
        let channel = config.clone().queue_channel;
        // Perform the database operation inside an async context
        conn.conn(move |conn| {
            let mut stmt = conn.prepare(
                "WITH RankedQueue AS (
                    SELECT twitch_name, ROW_NUMBER() OVER (ORDER BY position) AS position
                    FROM queue WHERE channel_id = ?1)
                    SELECT position
                    FROM RankedQueue
                    WHERE twitch_name = ?2"
            )?;

            let result = stmt.query_row(params![channel, sender_name], |row| row.get::<_, i64>(0)).optional()?;
            let message = match result {
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
            let mut reply_lock = reply_clone.blocking_lock();
            *reply_lock = message;

            Ok(())
        }).await?;

        client.privmsg(msg.channel(), &reply.lock().await).send().await?;
        
        Ok(())
    }

    //User leaves queue
    pub async fn handle_leave(&mut self, msg: &tmi::Privmsg<'_>, client: Arc<tokio::sync::Mutex<tmi::Client>>, conn: &SqliteClient) -> Result<(), BotError> {
        let name = msg.sender().name().to_string();
        let config = self.config.get_channel_config(msg.channel());
        let teamsize = config.teamsize.try_into().unwrap();
        let channel = config.clone().queue_channel;

        let reply = conn.conn(move |conn| {
            if let Ok(position_to_leave) = conn.query_row("SELECT position FROM queue WHERE twitch_name = ?1 AND channel_id = ?2", 
            params![&name, &channel], |row| row.get::<_, i32>(0)) {
                if position_to_leave <=  teamsize {
                    Ok(format!("You cannot leave the live group! If you want to be removed ask streamer or wait for !next"))
                } else {
                    conn.execute(
                        "DELETE FROM queue WHERE twitch_name = ?1 AND channel_id = ?2", 
                        params![&name, &channel],
                    )?;
                    conn.execute("UPDATE queue SET position = position - 1 WHERE channel_id = ?1 AND position > ?2", 
                        params![&channel, position_to_leave]
                    )?;
                    Ok(format!("{} has been removed from queue.", &name))
                }
            } else {
                Ok(format!("You are not in queue, {}.", &name))
            }
        }).await?;
        client.lock().await.privmsg(msg.channel(), &reply).send().await?;
        Ok(())
    }

    //Shows whole queue
    //UNWRAPS ON VEC????
    //TODO! COMBINED/SINGLE
    pub async fn handle_queue(&mut self, msg: &tmi::Privmsg<'_>, client: Arc<tokio::sync::Mutex<tmi::Client>>, conn: &SqliteClient) -> Result<(), BotError> {
        let queue_msg = Arc::new(Mutex::new(Vec::new()));
        let queue_msg_clone = Arc::clone(&queue_msg);

        
        let config = self.config.get_channel_config(msg.channel());
        let channel = config.clone().queue_channel;
        let teamsize = config.teamsize;
        let reply = {
            conn.conn(move |conn| {
                let mut stmt = conn.prepare("SELECT * FROM queue WHERE channel_id = ?1 ORDER BY position ASC, locked_first DESC, group_priority ASC")?;
                let queue_iter = stmt.query_map(params![channel], |row| {
                    Ok((row.get::<_, String>(1)?, row.get::<_, String>(2)?))
                })?;
                for entry in queue_iter {
                    queue_msg_clone.lock().unwrap().push(entry?);
                };
                Ok(())
            }).await?;

            let queue_msg = queue_msg.lock().unwrap();
   
            let queue_msg: Vec<String> = queue_msg.iter().enumerate().map(|(i, q)| format!("{}. {} ({})", i + 1, q.0, q.1)).collect();
            let format_group = |group: &Vec<String>| group.join(", ");

            
            if queue_msg.is_empty() {
                vec![format!("Queue is empty!")]
            } else {
                let mut vec = vec![];
                let mut count: usize = 0;
                for name in queue_msg.clone() {
                    count += name.len();
                }
                println!("{}", count);
                if count < 400 {
                    let mut live_group: Vec<String> = Vec::new();
                    let mut next_group: Vec<String> = Vec::new();
           
                    let queue_msg: Vec<String> = queue_msg.iter().map(|x| format!("{}", x)).collect();
                    for name in &queue_msg[0..min(teamsize, queue_msg.len())] {
                        live_group.push(name.to_string());
                    }
        
                    if queue_msg.len() > teamsize {
                        for name in &queue_msg[teamsize..min(teamsize * 2, queue_msg.len())] {
                            next_group.push(name.to_string());
                        }
                    }
                    
                    let rest_group: Vec<String> = if queue_msg.len() > teamsize * 2 {
                        queue_msg[teamsize * 2..].iter().map(|x| x.to_string()).collect()
                    } else {
                        Vec::new()
                    };
                    vec.push(format!( "LIVE: {} || NEXT: {} || QUEUE: {}", format_group(&live_group), format_group(&next_group), format_group(&rest_group)));

                } else {
                    let mut start = 0;
                    let queue_len = queue_msg.len();
                    for group in 1..=((queue_len + teamsize - 1) / teamsize) {
                        let end = (start + teamsize).min(queue_msg.len());
                        
                        if start < end {
                            // Collect items for this group
                            let group_items: Vec<_> = queue_msg[start..end].to_vec();
                            vec.push(format!("🛡️ GROUP {} || {}", group, format_group(&group_items)));
                        }
                        start = end;
                    }
                }

                vec
            }
        };
        for reply in reply {
            send_message(&msg, client.lock().await.borrow_mut(), &reply).await?;
        }
        Ok(())
    }

    //random fireteam
    pub async fn random(&mut self, msg: &tmi::Privmsg<'_>, client: Arc<tokio::sync::Mutex<tmi::Client>>, conn: &SqliteClient) -> Result<(), BotError>{
        let channel = msg.channel().to_string();
        let positions = pick_random(conn.clone(), self.config.get_channel_config(&channel).teamsize).await?;
        
        let live_names = Arc::new(Mutex::new(Vec::new()));
        let live_names_clone = Arc::clone(&live_names);
        conn.conn(move |conn| {
            let mut stmt = conn.prepare("SELECT twitch_name, bungie_name from queue WHERE position = ?1 AND channel_id = ?2")?;
            
            for position in positions {
                let name = stmt.query_row(params![position, channel], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?;
                live_names_clone.lock().unwrap().push(name);
            }
            
            Ok(())
        }).await?;
        let names = live_names.lock().unwrap().to_vec();

        client.lock().await.privmsg(msg.channel(), &format!("Randomly selected: {:?}", names)).send().await?;
        Ok(())
    }
    //Get total clears of raid of a player
    pub async fn total_raid_clears(&mut self, msg: &tmi::Privmsg<'_>, client: &mut Client, conn: &SqliteClient) -> Result<(), BotError> {
        let mut membership = MemberShip { id: String::new(), type_m: -1 };
        let words: Vec<&str> = msg.text().split_ascii_whitespace().collect();
        
        let reply = if words.len() > 1 {
        let mut name = words[1..].to_vec().join(" ").to_string();
        
        if let Some(name) = is_valid_bungie_name(&name) {
            match get_membershipid(name.clone(), self.x_api_key.clone()).await {
                Ok(ship) => membership = ship,
                Err(err) => client.privmsg(msg.channel(), &format!("Error: {}", err)).send().await?,
            }
        } else {
            if name.starts_with("@") {
                name.remove(0); 
            }
            if let Some(ship) = load_membership(&conn, name.clone()).await {
                membership = ship;
            } else {
                client.privmsg(msg.channel(), "Twitch user isn't registered in the database! Use their Bungie name!").send().await?;
                return Ok(());
            }
        }
        let clears = get_users_clears(membership.id, membership.type_m, self.x_api_key.clone()).await? as i32;
        format!("{} has total {} raid clears", name, clears)
    } else {
        if let Some(membership) = load_membership(&conn, msg.sender().name().to_string()).await {
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

    pub async fn move_groups(&mut self, msg: &tmi::Privmsg<'_>, client: Arc<tokio::sync::Mutex<tmi::Client>>, conn: &SqliteClient) -> Result<(), BotError> {
        let words: Vec<&str> = msg.text().split_ascii_whitespace().collect();
        let mut twitch_name = words[1..].join(" ").to_string();
        let mut bot_state = self.clone();
        let message = msg.clone().into_owned();
        if twitch_name.starts_with("@") {
            twitch_name.remove(0);
        }

        let reply = conn.conn(move |conn|  {
            let config = bot_state.config.get_channel_config(message.channel());
            let channel = config.clone().queue_channel;

            if let Ok(position) = conn.query_row("SELECT position FROM queue WHERE twitch_name = ?1 AND channel_id = ?2", params![twitch_name, channel], |row| row.get::<_, usize>(0)) {
                let max_pos = conn.query_row("SELECT MAX(position) FROM queue WHERE channel_id = ?1", params![channel], |row| row.get::<_, usize>(0))?;
                let new_position = position + config.teamsize;
                
                if max_pos < new_position {
                    return Ok(format!("User {} is in the last group.", twitch_name));
                } else {
                    let tx = conn.unchecked_transaction()?;
                    tx.execute("UPDATE queue SET position = -1 WHERE channel_id = ?1 AND position = ?2", params![channel, position])?;
                    tx.execute("UPDATE queue SET position = position - 1 WHERE channel_id = ?1 AND position BETWEEN ?2 AND ?3", params![channel, position, new_position])?;
                    tx.execute("UPDATE queue SET position = ?1 WHERE channel_id = ?2 AND position = -1",params![new_position, channel])?;
                    tx.commit()?;
                    
                    return Ok(format!("User {} has been moved to the next group.", twitch_name));
                }
            } else {
                return Ok(format!("User {} isn´t in queue!", twitch_name));
            }
            
            
            
        }).await?;
        send_message(msg, client.lock().await.borrow_mut(), &reply).await?;
        
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

pub async fn process_queue_entry(msg: &tmi::Privmsg<'_>, client: &mut Client, queue_len: usize, conn: &SqliteClient, user: TwitchUser, channel_id: Option<String>) -> Result<(), BotError> {
   
    let reply = if user_exists_in_queue(&conn, user.clone().twitch_name, channel_id.clone()).await {
        update_queue(&conn, user.clone()).await?;
        format!("{} updated their Bungie name to {}", msg.sender().name(), user.clone().bungie_name)
    } else {
        add_to_queue(msg, queue_len, &conn, user, channel_id).await?
    };
    send_message(msg, client, &reply).await?;
    Ok(())
}

async fn user_exists_in_queue(conn: &SqliteClient, twitch_name: String, channel_id: Option<String>) -> bool {
    let res = conn.conn(move |conn| {
        let mut stmt = conn.prepare("SELECT 1 FROM queue WHERE twitch_name = ?1 AND channel_id = ?2")?;
        let params = params![twitch_name, channel_id];
        let exists: Result<Option<i64>, _> = stmt.query_row(params, |row| row.get(0));
        Ok(exists.is_ok())
    }).await.unwrap();
    return res
}

async fn update_queue(conn: &SqliteClient, user: TwitchUser) -> Result<(), BotError> {
    conn.conn(move |conn| 
        Ok(conn.execute("UPDATE queue SET bungie_name = ?1 WHERE twitch_name = ?2", params![user.bungie_name, user.twitch_name])?)
    ).await?;
    Ok(())
}
//TODO! redo the queue so that combined/solo settings
async fn add_to_queue<'a>(msg: &tmi::Privmsg<'_>, queue_len: usize, conn: &SqliteClient, user: TwitchUser, channel_id: Option<String>) -> Result<String, BotError> {
    let reply;
    let channel_id_clone = channel_id.clone();
    let count: i64 = conn.conn(move |conn| Ok(conn.query_row("SELECT COUNT(*) FROM queue WHERE channel_id = ?1", params![channel_id_clone.unwrap()], |row| row.get::<_,i64>(0))?)).await?;
    if count < queue_len as i64 {
        let channel_id_clone = channel_id.clone();
        let next_position: i32 = conn.conn(move |conn| {
            Ok(conn.query_row(
                "SELECT COALESCE(MAX(position), 0) + 1 FROM queue WHERE channel_id = ?1", 
                params![channel_id_clone], 
                |row| row.get(0)
            )?)
            
        }).await?;
        
        
        conn.conn(move |conn| Ok(conn.execute(
            "INSERT INTO queue (position, twitch_name, bungie_name, channel_id) VALUES (?1, ?2, ?3, ?4)",
            params![next_position, user.twitch_name, user.bungie_name, channel_id.unwrap()],
        )?)).await?;
        reply = format!("✅ {} entered the queue at position #{}", msg.sender().name(), next_position);
    } else {
        //Queue is full
        reply = "❌ You can't enter queue, it is full".to_string();
    }
    Ok(reply)
}

pub async fn register_user(conn: &SqliteClient, twitch_name: &str, bungie_name: &str) -> Result<String, BotError> {
    dotenv::dotenv().ok();
    let x_api_key = var("XAPIKEY").expect("No bungie api key");
    let reply = if let Some(bungie_name) = is_valid_bungie_name(bungie_name) {
        let new_user = TwitchUser {
            twitch_name: twitch_name.to_string(),
            bungie_name: bungie_name.to_string()
        };
        save_to_user_database(conn, new_user, x_api_key).await?
    } else {
        "❌ You have typed invalid format of bungiename, make sure it looks like -> bungiename#0000".to_string()
    };
    Ok(reply)
    
}
//if is/not in database
pub async fn bungiename(msg: &tmi::Privmsg<'_>, client: &mut Client, conn: &SqliteClient, twitch_name: String) -> Result<(), BotError> {
    let reply = conn.conn(move |conn| {
        let mut stmt = conn.prepare("SELECT rowid, * FROM user WHERE twitch_name = ?1")?;
        let reply = if let Some(bungie_name) = stmt.query_row(params![twitch_name], |row| {
            Ok(row.get::<_, String>(3)?)
        }).optional()? {
            format!("@{} || BungieName: {} ||", twitch_name, bungie_name).to_string()
        } else {
            format!("{}, you are not registered", twitch_name).to_string()
        };
        Ok(reply)
    }).await?;

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

pub async fn unban_player_from_queue(msg: &tmi::Privmsg<'_>, client:Arc<tokio::sync::Mutex<Client>>, conn: &SqliteClient) -> Result<(), BotError> {
    let words: Vec<&str> = msg.text().split_ascii_whitespace().collect();
    let reply;
    if words.len() == 1 {
        reply = "Maybe try to add a twitch name. Somebody deserves the unban. :krapmaStare:".to_string();
    } else {
        let mut twitch_name = words[1].to_string();
        if twitch_name.starts_with("@") {
            twitch_name.remove(0);
        }
        let name_clone = twitch_name.clone();
        conn.conn(move |conn| Ok(conn.execute("DELETE FROM banlist WHERE twitch_name = ?1", params![twitch_name])?)).await?;
        reply = format!("User {} has been unbanned from queue! They are free to enter queue again. :krapmaHeart: ", name_clone);
    }
    client.lock().await.privmsg(msg.channel(), &reply).send().await?;
    Ok(())
}

pub async fn ban_player_from_queue(msg: &tmi::Privmsg<'_>, client:Arc<tokio::sync::Mutex<Client>>, conn: &SqliteClient) -> Result<(), BotError> {
    let words: Vec<&str> = msg.text().split_ascii_whitespace().collect();
    let mut client = client.lock().await;
    
    if words.len() < 2 {
        client.privmsg(msg.channel(), "Usage: !mod_ban <twitch name> Optional(reason)").send().await?;
        return Ok(());
    }
    let mut twitch_name = words[1].to_string();

    if twitch_name.starts_with("@") {
        twitch_name.remove(0);
    }

    let twitch_name_clone = twitch_name.clone();
    let mut reason = String::new();
    if words.len() > 3 {
        reason = words[2..].join(" ").to_string();
    }
    
    

    if conn.conn(move |conn| Ok(conn.execute("INSERT INTO banlist (twitch_name, reason) VALUES (?1, ?2)", params![twitch_name, reason])?)).await? > 0 {
        client.privmsg(msg.channel(),&format!("User {} has been banned from entering queue.", twitch_name_clone)).reply_to(msg.id()).send().await?;
    }

    Ok(())
}


pub async fn modify_command(msg: &tmi::Privmsg<'_>, client:Arc<tokio::sync::Mutex<Client>>, conn: SqliteClient, action: CommandAction, channel: Option<String>) -> Result<(), BotError> {
    let words: Vec<&str> = msg.text().split_whitespace().collect();
    let mut client = client.lock().await;
    let mut reply;
    if words.len() < 2 {
        reply = "Usage: !removecommand <command>".to_string();
    }
    
    let command = words[1].to_string().to_ascii_lowercase();
    let reply_to_command = words[2..].join(" ").to_string();
    
    match action {
        CommandAction::Add => {
            reply = save(&conn, command, reply_to_command, channel, "Usage: !addcommand <command> <response>").await?;
        }
        CommandAction::Remove => {
            if remove_command(&conn, &command).await {
                reply = format!("Command !{} removed.", command)
            } else {
                reply = format!("Command !{} doesn't exist.", command)
            }
        }
        CommandAction::AddGlobal => {
            reply = save(&conn, command, reply_to_command, None, "Usage: !addcommand <command> <response>").await?;
            
        } 
    };
    send_message(msg, &mut client, &reply).await?;
    Ok(())
}

async fn save(conn: &SqliteClient, command: String, reply: String, channel: Option<String>, error_mess: &str) -> Result<String, BotError> {
    if !reply.is_empty() {
        save_command(&conn, command.clone(), reply, channel).await;
        Ok(format!("Command !{} added.", command))
    } else {
        Ok(error_mess.to_string())
    }
}

