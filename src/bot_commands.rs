use std::{borrow::BorrowMut, cmp::min, sync::{Arc, Mutex}};
use async_sqlite::{rusqlite::{params, OptionalExtension}, Client as SqliteClient};
use dotenv::var;

use serde::{Deserialize, Serialize};

use tmi::Client;

use crate::{api::{get_membershipid, get_users_clears, MemberShip}, bot::{BotState, CHANNELS}, database::{load_membership, pick_random, save_to_user_database}, models::{BotError, TwitchUser}};

pub async fn is_moderator(msg: &tmi::Privmsg<'_>, client: Arc<tokio::sync::Mutex<tmi::Client>>) -> bool {
    if msg.badges().into_iter().any(|badge| badge.as_badge_data().name() == "moderator" || badge.as_badge_data().name() == "broadcaster") {
        return true;
    } else {
        _ = client.lock().await.privmsg(msg.channel(), "You are not a moderator/broadcaster. You can't use this command").send().await;
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

pub async fn shoutout(oauth_token: &str, client_id: String, to_broadcaster_id: &str) {
    let url = format!("https://api.twitch.tv/helix/chat/shoutouts?from_broadcaster_id=216105918&to_broadcaster_id={}&moderator_id=1091219021", to_broadcaster_id);
    let res = reqwest::Client::new()
        .post(url)
        .bearer_auth(oauth_token)
        .header("Client-Id", client_id)
        .send()
        .await.expect("Bad reqwest");
    println!("{:?}", res.text().await);
}

#[derive(Deserialize, Debug)]
struct data_follow {
    total: usize,
    data: String,
    pagination: String
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

pub fn is_valid_bungie_name(name: &str) -> bool {
    name.contains('#') && name.split_once('#').unwrap().1.len() == 4
}

async fn get_bungie_name_from_db(twitch_name: String, conn: &SqliteClient) -> Option<String> {
    if let Ok(bungie_name) = conn.conn(move |conn| {
        conn.query_row("SELECT bungie_name FROM user WHERE twitch_name = ?1", params![twitch_name],|row| row.get(0))}).await {
            bungie_name    
    } else {
            None
    }
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
        if self.queue_config.open {
            if !is_banned_from_queue(msg, conn, &mut client).await? {
                if let Some((_join, name)) = msg.text().split_once(" ") {
                    if is_valid_bungie_name(name.trim()) {
                        let new_user = TwitchUser {
                            twitch_name: msg.sender().name().to_string(),
                            bungie_name: name.trim().to_string(),
                        };
                        process_queue_entry(msg, &mut client, self.queue_config.len, conn, new_user, self.queue_config.channel_id.clone()).await?;
                    
                    } else {
                        send_invalid_name_reply(msg, &mut client).await?;
                    }
                } else {
                    if let Some(bungie_name) = get_bungie_name_from_db(msg.sender().name().to_string(), conn).await {
                        let new_user = TwitchUser {
                            twitch_name: msg.sender().name().to_string(),
                            bungie_name: bungie_name
                        };
                        process_queue_entry(msg, &mut client, self.queue_config.len, conn, new_user, self.queue_config.channel_id.clone()).await?;
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

    //Kicks out users that were in game
    pub async fn handle_next(&mut self, msg: &tmi::Privmsg<'_>, client: Arc<tokio::sync::Mutex<tmi::Client>>, conn: &SqliteClient) -> Result<(), BotError> {
        let bot_state = self.clone();
        let mut client = client.lock().await;
        let channel = msg.channel().clone().to_string();

        conn.conn(move |conn| {
            Ok(conn.execute("DELETE FROM queue WHERE channel_id = ?2 AND id IN (SELECT id FROM queue WHERE channel_id = ?2 LIMIT ?1)", params![bot_state.queue_config.teamsize, channel])?)
        }).await?;
        
        let queue_msg = Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let queue_msg_clone = Arc::clone(&queue_msg);
        let channel = msg.channel().clone().to_string();

        conn.conn(move |conn| {
            let mut stmt = conn.prepare("SELECT bungie_name FROM queue WHERE channel_id = ?2 LIMIT ?1")?;
            let queue_iter = stmt.query_map(params![bot_state.queue_config.teamsize, &channel], |row| row.get::<_, String>(0))?;
            for entry in queue_iter {
                queue_msg_clone.blocking_lock().push(entry?);
            }
            Ok(())
        }).await?;
        
    
        
        let queue_msg = queue_msg.lock().await;
    
        
        let reply = if queue_msg.is_empty() {
            "Queue is empty".to_string()
        } else {
            format!("Next: {:?}", queue_msg.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(", "))
        };
    
        send_message(msg, &mut client, &reply).await?;
        
        //Vymyslet způsob jak vypisovat vždy kde je bot připojen TODO!
        
        Ok(())
    
    }

    //Moderator can remove player from queue
    pub async fn handle_remove(&mut self, msg: &tmi::Privmsg<'_>, client: Arc<tokio::sync::Mutex<tmi::Client>>, conn: &SqliteClient) -> Result<(), BotError> {
        
        if is_moderator(msg, client.clone()).await {
            let parts: Vec<&str> = msg.text().split_whitespace().collect();
            let mut client = client.lock().await;
            if parts.len() == 2 {
                
                let twitch_name = parts[1].to_string();
                let twitch_name_clone = twitch_name.clone();
                
                let rows = match conn.conn(move |conn| conn.execute("DELETE FROM queue WHERE twitch_name = ?1", params![twitch_name_clone])).await {
                    Ok(rows) => rows,
                    Err(err) => return Err(BotError {error_code: 100, string: Some(err.to_string())}),
                };
                
                let reply = if rows > 0 {
                    format!("{} has been removed from the queue.", twitch_name)
                } else {
                    format!("User {} not found in the queue.", twitch_name)
                };
                send_message(msg, &mut client, &reply).await?;
            }
        
        }
        Ok(())
    }

    //Show the user where he is in queue
    pub async fn handle_pos(&mut self, msg: &tmi::Privmsg<'_>, client: Arc<tokio::sync::Mutex<tmi::Client>>, conn: &SqliteClient) -> Result<(), BotError> {
        let reply = Arc::new(tokio::sync::Mutex::new(String::new()));
        let reply_clone = Arc::clone(&reply);
        let mut client = client.lock().await;
        
        let sender_name = msg.sender().name().to_string(); 
        let teamsize = self.queue_config.teamsize as i64; 
        let channel_id = self.queue_config.channel_id.clone();
        // Perform the database operation inside an async context
        conn.conn(move |conn| {
            let mut stmt = conn.prepare(
                "WITH RankedQueue AS (
                    SELECT twitch_name, ROW_NUMBER() OVER (ORDER BY id) AS position
                    FROM queue WHERE channel_id = ?1)
                    SELECT position
                    FROM RankedQueue
                    WHERE twitch_name = ?2"
            )?;

            // Capture necessary data as owned to avoid lifetime issues
            let result = stmt.query_row(params![channel_id.unwrap(), sender_name], |row| row.get::<_, i64>(0)).optional()?;

            // Build the reply message based on the query result
            let message = match result {
                Some(index) => {
                    let group = index as f64 / teamsize as f64;
                    if group <= 1.0 {
                        format!("You are at position {} and in LIVE group krapmaHeart!", index)
                    } else if group <= 2.0 {
                        format!("You are at position {} and in NEXT group!", index)
                    } else {
                        
                        format!("You are at position {} (Group {}) !", index, group as usize + 1)
                    }
                }
                None => format!("You are not in the queue, {}.", sender_name),
            };

            let mut reply_lock = reply_clone.blocking_lock();
            *reply_lock = message;

            Ok(())
        }).await?;

        // Send the reply message using the client
        client.privmsg(msg.channel(), &reply.lock().await).send().await?;
        
        Ok(())
    }

    //User leaves queue
    pub async fn handle_leave(&mut self, msg: &tmi::Privmsg<'_>, client: Arc<tokio::sync::Mutex<tmi::Client>>, conn: &SqliteClient) -> Result<(), BotError> {
        let name = msg.sender().name().to_string();
        let rows = conn.conn(move |conn| Ok(conn.execute("DELETE FROM queue WHERE twitch_name = ?1", params![name])?)).await.unwrap();
        
        let reply = if rows > 0 {
            format!("You have been removed from the queue, {}.", msg.sender().name())
            
        } else {
            format!("You are not in queue, {}.", msg.sender().name())
            
        };

        client.lock().await.privmsg(msg.channel(), &reply).send().await?;
        Ok(())
    }

    //Shows whole queue
    //TODO! COMBINED/SINGLE
    pub async fn handle_queue(&mut self, msg: &tmi::Privmsg<'_>, client: Arc<tokio::sync::Mutex<tmi::Client>>, conn: &SqliteClient) -> Result<(), BotError> {
        let queue_msg = Arc::new(Mutex::new(Vec::new()));
        let queue_msg_clone = Arc::clone(&queue_msg);

        let channel_id = self.queue_config.channel_id.clone();
        let reply = if self.queue_config.open {
            conn.conn(move |conn| {
                let mut stmt = conn.prepare("SELECT twitch_name FROM queue WHERE channel_id = ?1")?;
                let queue_iter = stmt.query_map(params![channel_id.unwrap()], |row| row.get::<_,String>(0))?;
                for entry in queue_iter {
                    queue_msg_clone.lock().unwrap().push(entry?);
                };
                Ok(())
            }).await?;

            let queue_msg = queue_msg.lock().unwrap();
            let mut live_group: Vec<&str> = Vec::new();
            let mut next_group: Vec<&str> = Vec::new();

            
            let queue_msg: Vec<String> = queue_msg.iter().enumerate().map(|(i, q)| format!("{}. {}", i + 1, q)).collect();
            for name in &queue_msg[0..min(self.queue_config.teamsize, queue_msg.len())] {
                live_group.push(name);
            }

            if queue_msg.len() > self.queue_config.teamsize {
                for name in &queue_msg[self.queue_config.teamsize..min(self.queue_config.teamsize * 2, queue_msg.len())] {
                    next_group.push(name);
                }
            }
            
            let rest_group: Vec<&str> = if queue_msg.len() > self.queue_config.teamsize * 2 {
                queue_msg[self.queue_config.teamsize * 2..].iter().map(AsRef::as_ref).collect()
            } else {
                Vec::new()
            };

            let format_group = |group: &Vec<&str>| group.join(", ");

            format!( "LIVE: {} || NEXT: {} || QUEUE: {}", format_group(&live_group), format_group(&next_group), format_group(&rest_group))
            
        } else {
            "Queue is not opened!".to_string()
        };
        
        client.lock().await.privmsg(msg.channel(), &reply).send().await?;
        Ok(())
    }

    //random fireteam
    pub async fn random(&mut self, msg: &tmi::Privmsg<'_>, client: Arc<tokio::sync::Mutex<tmi::Client>>, conn: &SqliteClient) -> Result<(), BotError>{
        //Push the randomly chosen player to first positions
        let bot_state = self.clone();
        pick_random(conn.clone(), self.queue_config.teamsize).await?;
        let live_names = Arc::new(Mutex::new(Vec::new()));
        let live_names_clone = Arc::clone(&live_names);
        conn.conn(move |conn| {
            let mut stmt = conn.prepare("SELECT twitch_name from queue WHERE id <= ?1").unwrap();
            let rows = stmt.query_map(params![bot_state.queue_config.teamsize], |row| row.get::<_,String>(1))?;
            for names in rows {
                live_names_clone.lock().unwrap().push(names?);
            };
            Ok(())
        }).await?;
        
        
        

        client.lock().await.privmsg(msg.channel(), &format!("Randomly selected: {:?}",live_names)).send().await?;
        Ok(())
    }
    //Get total clears of raid of a player
    pub async fn total_raid_clears(&mut self, msg: &tmi::Privmsg<'_>, client: &mut Client, conn: &SqliteClient) -> Result<(), BotError> {
        
        let mut membership = MemberShip { id: String::new(), type_m: -1 };
        let words: Vec<&str> = msg.text().split_ascii_whitespace().collect();
        
        let reply = if words.len() > 1 {
            let mut name = words[1..].to_vec().join(" ").to_string();
    
            if is_valid_bungie_name(&name) {
                match get_membershipid(name.clone(), self.x_api_key.clone()).await {
                    Ok(ship) => membership = ship,
                    Err(err) => client.privmsg(msg.channel(), &format!("Error: {}", err)).send().await?,
                }
            } else {
                if name.starts_with("@") {
                    name.remove(0); 
                }
            
                println!("{:?}", name);
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
                format!("{} is not registered to the database. Use !register <yourbungiename#0000>", msg.sender().name())
            }
        };
        
        client.privmsg(msg.channel(), &reply).send().await?;
        Ok(())
    }
}


async fn send_invalid_name_reply(msg: &tmi::Privmsg<'_>, client: &mut Client) -> Result<(), BotError>{
    let reply = format!("Use !join bungiename#0000, {}!", msg.sender().name());
    send_message(msg, client, &reply).await?;
    Ok(())
}

async fn process_queue_entry(msg: &tmi::Privmsg<'_>, client: &mut Client, queue_len: usize, conn: &SqliteClient, user: TwitchUser, channel_id: Option<String>) -> Result<(), BotError> {
   
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
        let mut stmt = conn.prepare("SELECT 1 FROM queue WHERE twitch_name = ?1 AND channel_id = ?2").unwrap();
        let params = params![twitch_name, channel_id];
        let exists: Result<Option<i64>, _> = stmt.query_row(params, |row| row.get(0));
        Ok(exists.is_ok())
    }).await.unwrap();
    return res
}

async fn update_queue(conn: &SqliteClient, user: TwitchUser) -> Result<(), BotError>{
    conn.conn(move |conn| 
        Ok(conn.execute("UPDATE queue SET bungie_name = ?1 WHERE twitch_name = ?2", params![user.bungie_name, user.twitch_name]).unwrap())
    ).await?;
    Ok(())
}
//TODO! redo the queue so that combined/solo settings
async fn add_to_queue<'a>(msg: &tmi::Privmsg<'_>, queue_len: usize, conn: &SqliteClient, user: TwitchUser, channel_id: Option<String>) -> Result<String, BotError>{
    let reply;
    let channel_id_clone = channel_id.clone();
    let count: i64 = conn.conn(move |conn| Ok(conn.query_row("SELECT COUNT(*) FROM queue WHERE channel_id = ?1", params![channel_id_clone.unwrap()], |row| row.get::<_,i64>(0))?)).await?;
    if count < queue_len as i64 {
        conn.conn(move |conn| Ok(conn.execute(
            "INSERT INTO queue (twitch_name, bungie_name, channel_id) VALUES (?1, ?2, ?3)",
            params![user.twitch_name, user.bungie_name, channel_id.unwrap()],
        )?)).await?;
        reply = format!("{} entered the queue at position #{}", msg.sender().name(), count + 1);
    } else {
        //Queue is full
        reply = "You can't enter queue, it is full".to_string();
    }
    Ok(reply)
}

pub async fn register_user(conn: &SqliteClient, twitch_name: &str, bungie_name: &str) -> Result<String, BotError> {
    dotenv::dotenv().ok();
    let x_api_key = var("XAPIKEY").expect("No bungie api key");
    let reply = if is_valid_bungie_name(bungie_name) {
        let new_user = TwitchUser {
            twitch_name: twitch_name.to_string(),
            bungie_name: bungie_name.to_string()
        };
        save_to_user_database(conn, new_user, x_api_key).await?
    } else {
        "You have typed invalid format of bungiename, make sure it looks like -> bungiename#0000".to_string()
    };
    Ok(reply)
    
}
//if is/not in database
pub async fn bungiename(msg: &tmi::Privmsg<'_>, client: &mut Client, conn: &SqliteClient, twitch_name: String) -> Result<(), BotError> {
    let reply = conn.conn(move |conn| {
        let mut stmt = conn.prepare("SELECT rowid, * FROM user WHERE twitch_name = ?1").unwrap();
        let reply = if let Some(bungie_name) = stmt.query_row(params![twitch_name], |row| {
            Ok(row.get::<_, String>(3)?)
        }).optional()? {
            format!("@{} || BungieName: {}||", twitch_name, bungie_name).to_string()
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
//https://api.twitch.tv/helix/chat/announcements?broadcaster_id={broadcaster_id}&moderator_id={moderator_id}
//Make announcment automatizations!
pub async fn announcment(broadcaster_id: &str, mod_id: &str, oauth_token: &str, client_id: String, message: String) -> Result<(), BotError> {
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

