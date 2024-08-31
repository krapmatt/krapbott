use std::{cmp::min, time::Duration};
use dotenv::var;
use enigo::{Enigo, Keyboard, Mouse, Settings};
use rusqlite::{params, Connection, OptionalExtension, Result};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, MutexGuard};
use twitch_irc::{login::LoginCredentials, message::PrivmsgMessage, transport::Transport, TwitchIRCClient};

use crate::{api::{get_membershipid, get_users_clears, MemberShip}, bot::{BotState, CHANNELS}, database::{load_membership, pick_random, save_to_user_database}, models::{BotError, TwitchUser}};

pub async fn is_moderator<T, L>(msg: PrivmsgMessage, client: &TwitchIRCClient<T, L>) -> bool where T: Transport, L: LoginCredentials {
    if msg.badges.into_iter().any(|badge| badge.name == "moderator" || badge.name == "broadcaster") {
        return true;
    } else {
        client.say(msg.channel_login, "You are not a moderator/broadcaster. You can't use this command".to_string()).await;
        return false;
    }
    
}
pub async fn in_right_chat(msg: &PrivmsgMessage) -> bool {
    if msg.channel_login == CHANNELS[0] {
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
pub async fn ban_bots(msg: PrivmsgMessage, oauth_token: &str, client_id: String) {
    let url = format!("https://api.twitch.tv/helix/moderation/bans?broadcaster_id={}&moderator_id=1091219021", msg.channel_id);
    
    let ban_request = BanRequest {
        data: BanData {
            user_id: msg.sender.id,
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

//Not actually checking follow status
pub async fn is_follower<T, L>(msg: PrivmsgMessage, client: TwitchIRCClient<T, L>, oauth_token: &str, client_id: String) -> Result<bool, BotError> where T: Transport, L: LoginCredentials {
    let url = format!("https://api.twitch.tv/helix/channels/followers?broadcaster_id={}&user_id={}", msg.channel_id, msg.sender.id);
    let res = reqwest::Client::new()
        .get(&url)
        .header("Client-Id", client_id)
        .bearer_auth(oauth_token)
        .send()
        .await.expect("Bad reqwest");
    
    if res.text().await?.contains("user_id") || msg.channel_id == msg.sender.id { 
        Ok(true)
    } else {
        client.say(msg.channel_login, "You are not a follower!".to_string()).await;
        Ok(false)
    }
}

pub fn is_valid_bungie_name(name: &str) -> bool {
    name.contains('#') && name.split_once('#').unwrap().1.len() == 4
}

async fn get_bungie_name_from_db(twitch_name: &str, conn: &Mutex<Connection>) -> Option<String> {
    if let Ok(bungie_name) = conn.lock().await.query_row("SELECT bungie_name FROM user WHERE twitch_name = ?1", params![twitch_name], 
        |row| row.get(0)) {
        bungie_name    
        } else {
            None
        }
    
}
impl BotState {
    //User can join into queue
    pub async fn handle_join<T, L>(&mut self, msg: PrivmsgMessage, client: TwitchIRCClient<T, L>) -> Result<(), BotError> where T: Transport, L: LoginCredentials {
        if self.queue_config.open {
            if let Some((_join, name)) = msg.message_text.split_once(" ") {
                if is_valid_bungie_name(name.trim()) {
                    let new_user = TwitchUser {
                        twitch_name: msg.sender.name.to_string(),
                        bungie_name: name.trim().to_string(),
                    };
                    process_queue_entry(msg, client, self.queue_config.len, &self.conn, new_user).await?;
                
                } else {
                    send_invalid_name_reply(msg, client).await?;
                }
            } else {
                if let Some(bungie_name) = get_bungie_name_from_db(&msg.sender.name, &self.conn).await {
                    let new_user = TwitchUser {
                        twitch_name: msg.clone().sender.name,
                        bungie_name: bungie_name
                    };
                    process_queue_entry(msg, client, self.queue_config.len, &self.conn, new_user).await?;
                } else {
                    send_invalid_name_reply(msg, client).await?;
                }
                
            }
            Ok(())
        } else {
            client.say(msg.channel_login, "Queue is closed".to_string()).await;
            Ok(())
        }
    }

    //Kicks out users that were in game
    pub async fn handle_next<T, L>(&mut self, msg: PrivmsgMessage, client: TwitchIRCClient<T, L>) -> Result<(), BotError> where T: Transport, L: LoginCredentials {
        let conn = self.conn.lock().await;
    
        conn.execute("DELETE FROM queue WHERE id IN (SELECT id FROM queue LIMIT ?1);", params![self.queue_config.teamsize])?;
    
        let mut stmt = conn.prepare("SELECT bungie_name FROM queue LIMIT ?1")?;
        let queue_iter = stmt.query_map(params![self.queue_config.teamsize], |row| row.get::<_, String>(0))?;
    
        let mut queue_msg = Vec::new();
        for entry in queue_iter {
            queue_msg.push(entry?);
        }
    
        let reply;
        if queue_msg.is_empty() {
            reply = "Queue is empty".to_string();
        } else {
            reply = format!("Next: {:?}", queue_msg.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(", "));
            /*let futures: Vec<_> = queue_msg.iter().take(queue_len).map(|q| invite_macro(q)).collect();
            futures::future::join_all(futures).await;*/
        }
    
        client.say(msg.channel_login, reply).await;
        
        //Vymyslet způsob jak vypisovat vždy kde je bot připojen TODO!
        
        Ok(())
    
    }

    //Moderator can remove player from queue
    pub async fn handle_remove<T, L>(&mut self, msg: PrivmsgMessage, client: TwitchIRCClient<T, L>) -> Result<(), BotError> where T: Transport, L: LoginCredentials {
        if is_moderator(msg.clone(), &client).await {
            let parts: Vec<&str> = msg.message_text.split_whitespace().collect();
            if parts.len() == 2 {
                let twitch_name = parts[1];
                let reply;
                let conn = self.conn.lock().await;
                let rows = match conn.execute("DELETE FROM queue WHERE twitch_name = ?1", params![twitch_name]) {
                    Ok(rows) => rows,
                    Err(err) => return Err(BotError {error_code: 100, string: Some(err.to_string())}),
                };
                if rows > 0 {
                    reply = format!("{} has been removed from the queue.", twitch_name);
                } else {
                    reply = format!("User {} not found in the queue.", twitch_name);
                }
                client.say(msg.channel_login, reply).await;
            }
        
        }
        Ok(())
    }

    //Show the user where he is in queue
    pub async fn handle_pos<T, L>(&mut self, msg: PrivmsgMessage, client: TwitchIRCClient<T, L>) -> Result<(), BotError> where T: Transport, L: LoginCredentials {
        let reply;
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare("
            WITH RankedQueue AS (
                SELECT twitch_name, ROW_NUMBER() OVER (ORDER BY id) AS position
                FROM queue
            )
            SELECT position
            FROM RankedQueue
            WHERE twitch_name = ?1").unwrap();
        if let Some(index) = stmt.query_row(params![msg.sender.name], |row| {
            Ok(row.get::<_, i64>(0)?)    
        }).optional()? {
            let group = index / self.queue_config.teamsize as i64;
            
            if group == 0 {
                reply = format!("You are at position {} and in LIVE group krapmaHeart!", index);
            } else if group == 1 {
                reply = format!("You are at position {} and in NEXT group!", index);
            } else {
                reply = format!("You are at position {} (Group {}) !", index, group);
            }
           
        } else {
            reply = format!("You are not in the queue, {}.", msg.sender.name);
        }

        client.privmsg(msg.channel_login, reply).await;
        Ok(())
    }

    //User leaves queue
    pub async fn handle_leave<T, L>(&mut self, msg: PrivmsgMessage, client: TwitchIRCClient<T, L>) -> Result<(), BotError> where T: Transport, L: LoginCredentials {
        let conn = self.conn.lock().await;
        let rows = conn.execute("DELETE FROM queue WHERE twitch_name = ?1", params![msg.sender.name])?;
        let reply;

        if rows > 0 {
            reply = format!("You have been removed from the queue, {}.", msg.sender.name);
            
        } else {
            reply = format!("You are not in queue, {}.", msg.sender.name);
            
        }

        client.privmsg(msg.channel_login, reply).await;
        Ok(())
    }

    //Shows whole queue
    pub async fn handle_queue<T, L>(&mut self, msg: PrivmsgMessage, client: TwitchIRCClient<T, L>) -> Result<(), BotError> where T: Transport, L: LoginCredentials {
        let reply;
        if self.queue_config.open {
            let conn = self.conn.lock().await;
            let mut stmt = conn.prepare("SELECT twitch_name FROM queue")?;
            let queue_iter = stmt.query_map([], |row| row.get::<_,String>(0))?;

            let mut queue_msg: Vec<String> = Vec::new();
            let mut live_group: Vec<&str> = Vec::new();
            let mut next_group: Vec<&str> = Vec::new();

            for entry in queue_iter {
                queue_msg.push(entry?);
            }
            queue_msg = queue_msg.iter().enumerate().map(|(i, q)| format!("{}. {}", i + 1, q)).collect();
            
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

            reply = format!( "LIVE: {} || NEXT: {} || QUEUE: {}", format_group(&live_group), format_group(&next_group), format_group(&rest_group));
            
        } else {
            reply = "Queue is not opened!".to_string();
        }
        
        client.say(msg.channel_login, reply).await;
        Ok(())
    }

    //random fireteam
    pub async fn random<T, L>(&mut self, msg: PrivmsgMessage, client: TwitchIRCClient<T, L>) -> Result<(), BotError> where T: Transport, L: LoginCredentials {
        //Push the randomly chosen player to first positions
        let mut conn = self.conn.lock().await;
        pick_random(&mut conn, self.queue_config.teamsize)?;

        let mut stmt = conn.prepare("SELECT twitch_name from queue WHERE id <= ?1").unwrap();
        let rows = stmt.query_map(params![self.queue_config.teamsize], |row| row.get::<_,String>(1))?;
        let mut live_names = Vec::new();
        for names in rows {
            live_names.push(names?);
        }

        client.say(msg.channel_login, format!("Randomly selected: {:?}",live_names)).await;
        Ok(())
    }
    //Get total clears of raid of a player
    pub async fn total_raid_clears<T, L>(&mut self, msg: PrivmsgMessage, client: TwitchIRCClient<T, L>) -> Result<(), BotError> where T: Transport, L: LoginCredentials {
        let conn = self.conn.lock().await;
        let mut membership = MemberShip { id: String::new(), type_m: -1 };
        let words: Vec<&str> = msg.message_text.split_ascii_whitespace().collect();
        let mut reply = String::new();
        if words.len() > 1 {
            let mut name = words[1..].to_vec().join(" ").to_string();
    
            if is_valid_bungie_name(&name) {
                match get_membershipid(name.clone(), self.x_api_key.clone()).await {
                    Ok(ship) => membership = ship,
                    Err(err) => client.say(msg.clone().channel_login, format!("Error: {}", err)).await?,
                }
            } else {
                if name.starts_with("@") {
                    name.remove(0); 
                }
            
                println!("{:?}", name);
                if let Some(ship) = load_membership(&conn, name.clone()) {
                    membership = ship;
                } else {
                    client.say(msg.channel_login, "Twitch user isn't registered in the database! Use their Bungie name!".to_string()).await;
                    return Ok(());
                }
            }
            let clears = get_users_clears(membership.id, membership.type_m, self.x_api_key.clone()).await? as i32;
            reply = format!("{} has total {} raid clears", name, clears);
        } else {
            if let Some(membership) = load_membership(&conn, msg.clone().sender.name) {
                let clears = get_users_clears(membership.id, membership.type_m, self.x_api_key.clone()).await? as i32;
                reply = format!("You have total {} raid clears", clears);
            } else {
                reply = format!("{} is not registered to the database. Use !register <yourbungiename#0000>", msg.clone().sender.name);
            }
        }
        
        client.say(msg.clone().channel_login, reply).await;
        Ok(())
    }
}


async fn send_invalid_name_reply<T, L>(msg: PrivmsgMessage, client: TwitchIRCClient<T, L>) -> Result<(), BotError> where T: Transport, L: LoginCredentials {
    let reply = format!("Use !join bungiename#0000, {}!", msg.sender.name);
    client.say(msg.channel_login, reply).await;
    
    Ok(())
}

async fn process_queue_entry<T, L>(msg: PrivmsgMessage, client: TwitchIRCClient<T, L>, queue_len: usize, conn: &Mutex<Connection>, user: TwitchUser) -> Result<(), BotError> where T: Transport, L: LoginCredentials {
    let reply;
    let conn = conn.lock().await;
    if user_exists_in_queue(&conn, &user.twitch_name) {
        update_queue(&conn, &user);
        reply = format!("{} updated their Bungie name to {}", msg.sender.name, user.bungie_name);
    } else {
        reply = add_to_queue(msg.clone(), queue_len, conn, user).await?;
    }
    client.say(msg.channel_login, reply).await;
    Ok(())
}

fn user_exists_in_queue(conn: &Connection, twitch_name: &str) -> bool {
    let mut stmt = conn.prepare("SELECT 1 FROM queue WHERE twitch_name = ?1").unwrap();
    let exists: Result<Option<i64>, _> = stmt.query_row(params![twitch_name], |row| row.get(0));
    exists.is_ok()
}

fn update_queue(conn: &Connection, user: &TwitchUser) {
    conn.execute("UPDATE queue SET bungie_name = ?1 WHERE twitch_name = ?2", params![user.bungie_name, user.twitch_name]).unwrap();
}

async fn add_to_queue<'a>(msg: PrivmsgMessage, queue_len: usize, conn: MutexGuard<'a, Connection>, user: TwitchUser) -> Result<String, BotError>{
    let reply;
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM queue", [], |row| row.get(0))?;
    if count < queue_len as i64 {
        conn.execute(
            "INSERT INTO queue (twitch_name, bungie_name) VALUES (?1, ?2)",
            params![user.twitch_name, user.bungie_name],
        ).unwrap();
        reply = format!("{} entered the queue at position #{}", msg.sender.name, count + 1);
    } else {
        //Queue is full
        reply = "You can't enter queue, it is full".to_string();
    }
    Ok(reply)
}

pub async fn register_user(conn: &Mutex<Connection>, twitch_name: &str, bungie_name: &str) -> Result<String, BotError> {
    dotenv::dotenv().ok();
    let x_api_key = var("X-API-KEY").expect("No bungie api key");
    let reply;
    if is_valid_bungie_name(bungie_name) {
        let new_user = TwitchUser {
            twitch_name: twitch_name.to_string(),
            bungie_name: bungie_name.to_string()
        };
        reply = save_to_user_database(&conn, &new_user, x_api_key).await?;
    } else {
        reply = "You have typed invalid format of bungiename, make sure it looks like -> bungiename#0000".to_string();
    }
    Ok(reply)
    
}
//if is/not in database
pub async fn bungiename<T,L>(msg: PrivmsgMessage, client: TwitchIRCClient<T, L>, conn: &Mutex<Connection>, twitch_name: &str) -> Result<(), BotError> where T: Transport, L: LoginCredentials {
    let conn = conn.lock().await;
    let mut stmt = conn.prepare("SELECT rowid, * FROM user WHERE twitch_name = ?1").unwrap();
    let reply;
    if let Some(bungie_name) = stmt.query_row(params![twitch_name], |row| {
        Ok(row.get::<_, String>(3)?)
    }).optional()? {
        reply = format!("@{} || BungieName: {}||", twitch_name, bungie_name);
    } else {
        reply = format!("{}, you are not registered", twitch_name);
    }
    client.say(msg.channel_login, reply).await;
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




//REmade invites twitch
//Still needs a better timing - maybe AHK?

async fn invite_macro(bungie_name: &str) {
    let mut enigo = Enigo::new(&Settings::default()).unwrap();
    let _ = enigo.move_mouse(100, 0, enigo::Coordinate::Abs);
    let _ = enigo.button(enigo::Button::Left, enigo::Direction::Click);
    
    tokio::time::sleep(Duration::from_secs(3)).await;
    
    let _ = enigo.key(enigo::Key::Return, enigo::Direction::Click);
    
    let _ = enigo.text(&format!("/invite {}", bungie_name));
    tokio::time::sleep(Duration::from_secs(3)).await;
    let _ = enigo.key(enigo::Key::Return, enigo::Direction::Click);
    tokio::time::sleep(Duration::from_secs(7)).await;
}

