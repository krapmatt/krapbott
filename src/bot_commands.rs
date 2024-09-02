use std::{cmp::min, sync::{Arc, Mutex}};
use async_sqlite::{rusqlite::{params, OptionalExtension}, Client as SqliteClient};
use dotenv::var;

use serde::{Deserialize, Serialize};
use tmi::Client;

use crate::{api::{get_membershipid, get_users_clears, MemberShip}, bot::{BotState, CHANNELS}, database::{load_membership, pick_random, save_to_user_database}, models::{BotError, TwitchUser}};

pub async fn is_moderator(msg: &tmi::Privmsg<'_>, client: &mut Client) -> bool {
    if msg.badges().into_iter().any(|badge| badge.as_badge_data().name() == "moderator" || badge.as_badge_data().name() == "broadcaster") {
        return true;
    } else {
        _ = client.privmsg(msg.channel(), "You are not a moderator/broadcaster. You can't use this command").send().await;
        return false;
    }
    
}
pub async fn in_right_chat(msg: &tmi::Privmsg<'_>) -> bool {
    if msg.channel() == CHANNELS[0] {
        return true
    } else {
        return false
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
#[derive(Deserialize, Debug)]
struct data_follow {
    total: usize,
    data: String,
    pagination: String
}
//Not actually checking follow status
pub async fn is_follower(msg: &tmi::Privmsg<'_>, client: &mut Client, oauth_token: &str, client_id: String) -> Result<bool, BotError> {
    let url = format!("https://api.twitch.tv/helix/channels/followers?broadcaster_id={}&user_id={}", msg.channel_id(), msg.sender().id());
    let res = reqwest::Client::new()
        .get(&url)
        .header("Client-Id", client_id)
        .bearer_auth(oauth_token)
        .send()
        .await.expect("Bad reqwest");
    
    if res.text().await?.contains("user_id") || msg.channel_id() == msg.sender().id() { 
        Ok(true)
    } else {
        send_message(msg, client, "You are not a follower!").await?;
        Ok(false)
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
impl BotState {
    //User can join into queue
    pub async fn handle_join(&mut self, msg: &tmi::Privmsg<'_>, client: &mut Client, conn: &SqliteClient) -> Result<(), BotError> {
        if self.queue_config.open {
            if let Some((_join, name)) = msg.text().split_once(" ") {
                if is_valid_bungie_name(name.trim()) {
                    let new_user = TwitchUser {
                        twitch_name: msg.sender().name().to_string(),
                        bungie_name: name.trim().to_string(),
                    };
                    process_queue_entry(msg, client, self.queue_config.len, conn, new_user).await?;
                
                } else {
                    send_invalid_name_reply(msg, client).await?;
                }
            } else {
                if let Some(bungie_name) = get_bungie_name_from_db(msg.sender().name().to_string(), conn).await {
                    let new_user = TwitchUser {
                        twitch_name: msg.sender().name().to_string(),
                        bungie_name: bungie_name
                    };
                    process_queue_entry(msg, client, self.queue_config.len, conn, new_user).await?;
                } else {
                    send_invalid_name_reply(msg, client).await?;
                }
                
            }
            Ok(())
        } else {
            client.privmsg(msg.channel(), "Queue is closed").send().await?;
            Ok(())
        }
    }

    //Kicks out users that were in game
    pub async fn handle_next(&mut self, msg: &tmi::Privmsg<'_>, client: &mut Client, conn: &SqliteClient) -> Result<(), BotError> {
        let bot_state = self.clone();
        conn.conn(move |conn| {
            Ok(conn.execute("DELETE FROM queue WHERE id IN (SELECT id FROM queue LIMIT ?1);", params![bot_state.queue_config.teamsize])?)
        
        }).await?;
        
        let queue_msg = Arc::new(Mutex::new(Vec::new()));
        let queue_msg_clone = Arc::clone(&queue_msg);

        conn.conn(move |conn| {
            let mut stmt = conn.prepare("SELECT bungie_name FROM queue LIMIT ?1")?;
            let queue_iter = stmt.query_map(params![bot_state.queue_config.teamsize], |row| row.get::<_, String>(0))?;
            for entry in queue_iter {
                queue_msg_clone.lock().unwrap().push(entry?);
            }
            Ok(())
        }).await?;
        
    
        
        let queue_msg = queue_msg.lock().unwrap();
    
        
        let reply = if queue_msg.is_empty() {
            "Queue is empty".to_string()
        } else {
            format!("Next: {:?}", queue_msg.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(", "))
        };
    
        send_message(msg, client, &reply).await?;
        
        //Vymyslet způsob jak vypisovat vždy kde je bot připojen TODO!
        
        Ok(())
    
    }

    //Moderator can remove player from queue
    pub async fn handle_remove(&mut self, msg: &tmi::Privmsg<'_>, client: &mut Client, conn: &SqliteClient) -> Result<(), BotError> {
        if is_moderator(msg, client).await {
            let parts: Vec<&str> = msg.text().split_whitespace().collect();
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
                send_message(msg, client, &reply).await?;
            }
        
        }
        Ok(())
    }

    //Show the user where he is in queue
    pub async fn handle_pos(&mut self, msg: &tmi::Privmsg<'_>, client: &mut Client, conn: &SqliteClient) -> Result<(), BotError> {
        let reply = Arc::new(Mutex::new(String::new()));
        let reply_clone = Arc::clone(&reply);

        // Clone the necessary data to avoid borrowing issues
        let sender_name = msg.sender().name().to_string(); // Convert to owned String
        let queue_len = self.queue_config.len as i64; // Copy the value from self

        // Perform the database operation inside an async context
        conn.conn(move |conn| {
            let mut stmt = conn.prepare(
                "WITH RankedQueue AS (
                    SELECT twitch_name, ROW_NUMBER() OVER (ORDER BY id) AS position
                    FROM queue)
                    SELECT position
                    FROM RankedQueue
                    WHERE twitch_name = ?1",
            )?;

            // Capture necessary data as owned to avoid lifetime issues
            let result = stmt.query_row(params![sender_name], |row| row.get::<_, i64>(0)).optional()?;

            // Build the reply message based on the query result
            let message = match result {
                Some(index) => {
                    let group = index / queue_len;
                    if group == 0 {
                        format!("You are at position {} and in LIVE group krapmaHeart!", index)
                    } else if group == 1 {
                        format!("You are at position {} and in NEXT group!", index)
                    } else {
                        format!("You are at position {} (Group {}) !", index, group)
                    }
                }
                None => format!("You are not in the queue, {}.", sender_name),
            };

            // Safely modify the shared reply variable
            *reply_clone.lock().unwrap() = message;

            Ok(())
        })
        .await?;

        // Send the reply message using the client
        client.privmsg(msg.channel(), &reply.lock().unwrap()).send().await?;
        Ok(())
    }

    //User leaves queue
    pub async fn handle_leave(&mut self, msg: &tmi::Privmsg<'_>, client: &mut Client, conn: &SqliteClient) -> Result<(), BotError> {
        let name = msg.sender().name().to_string();
        let rows = conn.conn(move |conn| Ok(conn.execute("DELETE FROM queue WHERE twitch_name = ?1", params![name])?)).await.unwrap();
        
        let reply = if rows > 0 {
            format!("You have been removed from the queue, {}.", msg.sender().name())
            
        } else {
            format!("You are not in queue, {}.", msg.sender().name())
            
        };

        client.privmsg(msg.channel(), &reply).send().await?;
        Ok(())
    }

    //Shows whole queue
    pub async fn handle_queue(&mut self, msg: &tmi::Privmsg<'_>, client: &mut Client, conn: &SqliteClient) -> Result<(), BotError> {
        let queue_msg = Arc::new(Mutex::new(Vec::new()));
        let queue_msg_clone = Arc::clone(&queue_msg);


        let reply = if self.queue_config.open {
            conn.conn(move |conn| {
                let mut stmt = conn.prepare("SELECT twitch_name FROM queue")?;
                let queue_iter = stmt.query_map([], |row| row.get::<_,String>(0))?;
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
        
        client.privmsg(msg.channel(), &reply).send().await?;
        Ok(())
    }

    //random fireteam
    pub async fn random(&mut self, msg: &tmi::Privmsg<'_>, client: &mut Client, conn: &SqliteClient) -> Result<(), BotError>{
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
        
        
        

        client.privmsg(msg.channel(), &format!("Randomly selected: {:?}",live_names)).send().await?;
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

async fn process_queue_entry(msg: &tmi::Privmsg<'_>, client: &mut Client, queue_len: usize, conn: &SqliteClient, user: TwitchUser) -> Result<(), BotError> {
   
    let reply = if user_exists_in_queue(&conn, user.clone().twitch_name).await {
        update_queue(&conn, user.clone()).await?;
        format!("{} updated their Bungie name to {}", msg.sender().name(), user.clone().bungie_name)
    } else {
        add_to_queue(msg, queue_len, &conn, user).await?
    };
    send_message(msg, client, &reply).await?;
    Ok(())
}

async fn user_exists_in_queue(conn: &SqliteClient, twitch_name: String) -> bool {
    let res = conn.conn(move |conn| {
        let mut stmt = conn.prepare("SELECT 1 FROM queue WHERE twitch_name = ?1").unwrap();
        let exists: Result<Option<i64>, _> = stmt.query_row(params![twitch_name], |row| row.get(0));
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

async fn add_to_queue<'a>(msg: &tmi::Privmsg<'_>, queue_len: usize, conn: &SqliteClient, user: TwitchUser) -> Result<String, BotError>{
    let reply;
    let count: i64 = conn.conn(move |conn| Ok(conn.query_row("SELECT COUNT(*) FROM queue", [], |row| row.get::<_,i64>(0))?)).await?;
    if count < queue_len as i64 {
        conn.conn(move |conn| Ok(conn.execute(
            "INSERT INTO queue (twitch_name, bungie_name) VALUES (?1, ?2)",
            params![user.twitch_name, user.bungie_name],
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
    let x_api_key = var("X-API-KEY").expect("No bungie api key");
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