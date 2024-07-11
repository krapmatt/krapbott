use std::time::Duration;
use enigo::{Enigo, Keyboard, Mouse, Settings};
use rusqlite::{params, Connection, OptionalExtension, Result};
use serde::Serialize;
use tmi::Client;
use tokio::sync::{Mutex, MutexGuard};

use crate::{database::{pick_random, save_to_user_database}, models::{BotError, TwitchUser}};

pub async fn is_moderator(msg: &tmi::Privmsg<'_>, client: &mut Client) -> bool {
    if msg.badges().into_iter().any(|badge| badge.as_badge_data().name() == "moderator" || badge.as_badge_data().name() == "broadcaster") {
        return true;
    } else {
        _ = client.privmsg(msg.channel(), "You are not a moderator/broadcaster. You can't use this command").send().await;
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

pub async fn is_follower(msg: &tmi::Privmsg<'_>, client: &mut Client, oauth_token: &str, client_id: String) -> Result<bool, BotError> {
    let url = format!("https://api.twitch.tv/helix/channels/followers?broadcaster_id={}&user_id={}", msg.channel_id(), msg.sender().id());
    let res = reqwest::Client::new()
        .get(&url)
        .header("Client-Id", client_id)
        .bearer_auth(oauth_token)
        .send()
        .await.expect("Bad reqwest");
    println!("{:?}", res);

    if res.status().is_success() {
        println!("{:?}", res.text().await);
        Ok(true)
    } else {
        println!("{:?}", res.text().await);
        client.privmsg(msg.channel(), "You are not a follower!").send().await?;
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

async fn send_invalid_name_reply(msg: &tmi::Privmsg<'_>, client: &mut Client) {
    let reply = format!("Invalid command format or Bungie name, {}!", msg.sender().name());
    _ = client.privmsg(msg.channel(), &reply).send().await;
}

async fn process_queue_entry(msg: &tmi::Privmsg<'_>, client: &mut Client, queue_len: usize, conn: &Mutex<Connection>, user: TwitchUser) -> Result<(), BotError> {
    let conn = conn.lock().await;
    if user_exists_in_queue(&conn, &user.twitch_name) {
        update_queue(&conn, &user);
        let reply = format!("{} updated their Bungie name to {}", msg.sender().name(), user.bungie_name);
        client.privmsg(msg.channel(), &reply).send().await?;
    } else {
        add_to_queue(msg, client, queue_len, conn, user).await?;
    }
    Ok(())
}

fn user_exists_in_queue(conn: &Connection, twitch_name: &str) -> bool {
    let mut stmt = conn.prepare("SELECT 1 FROM queue WHERE twitch_name = ?1").unwrap();
    let exists: Result<Option<i64>, _> = stmt.query_row(params![twitch_name], |row| row.get(0));
    exists.is_ok()
}

fn update_queue(conn: &Connection, user: &TwitchUser) {
    conn.execute(
        "UPDATE queue SET bungie_name = ?1 WHERE twitch_name = ?2",
        params![user.bungie_name, user.twitch_name],
    ).unwrap();
}

async fn add_to_queue<'a>(msg: &tmi::Privmsg<'_>, client: &mut Client, queue_len: usize, conn: MutexGuard<'a, Connection>, user: TwitchUser) -> Result<(), BotError>{
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM queue", [], |row| row.get(0))?;
        if count < queue_len as i64 {
            conn.execute(
                "INSERT INTO queue (twitch_name, bungie_name) VALUES (?1, ?2)",
                params![user.twitch_name, user.bungie_name],
            ).unwrap();
            let reply = format!("{} entered the queue at position #{}", msg.sender().name(), count + 1);
            client.privmsg(msg.channel(), &reply).send().await?;
        } else {
            //Queue is full
            client.privmsg(msg.channel(), "You can't enter queue, it is full").send().await?;
        }
        Ok(())
}

//User can join into queue
pub async fn handle_join(msg: &tmi::Privmsg<'_>, client: &mut Client, queue_len: usize, conn: &Mutex<Connection>) -> Result<(), BotError> {
    
    if let Some((_join, name)) = msg.text().split_once(" ") {
        if is_valid_bungie_name(name) {
            let new_user = TwitchUser {
                twitch_name: msg.sender().name().to_string(),
                bungie_name: name.to_string(),
            };
            process_queue_entry(msg, client, queue_len, conn, new_user).await?;
        
        } else {
            send_invalid_name_reply(msg, client).await;
        }
    } else {
        if let Some(bungie_name) = get_bungie_name_from_db(&msg.sender().name(), &conn).await {
            let new_user = TwitchUser {
                twitch_name: msg.sender().name().to_string(),
                bungie_name: bungie_name
            };
            process_queue_entry(msg, client, queue_len, conn, new_user).await?;
        } else {
            send_invalid_name_reply(msg, client).await;
        }
        
    }
    Ok(())
}

//Kicks out users that were in game
pub async fn handle_next(msg: &tmi::Privmsg<'_>, client: &mut Client, queue_len: usize, conn: &Mutex<Connection>) -> Result<(), BotError> {
    let conn = conn.lock().await;

    for _ in 0..queue_len {
        conn.execute("DELETE FROM queue WHERE id IN (SELECT id FROM queue LIMIT 1)", params![])?;
    }

    
    let mut stmt = conn.prepare("SELECT twitch_name FROM queue ?1")?;
    let queue_iter = stmt.query_map(params![queue_len], |row| row.get::<_, String>(1))?;

    let mut queue_msg = Vec::new();
    for entry in queue_iter {
        queue_msg.push(entry?);
    }

    let reply;
    if queue_msg.is_empty() {
        reply = "Queue is empty".to_string();
    } else {
        reply = format!("Next: {:?}", queue_msg);
        let futures: Vec<_> = queue_msg.iter().take(queue_len).map(|q| invite_macro(q)).collect();
        futures::future::join_all(futures).await;
    }

    client.privmsg(msg.channel(), &reply).send().await?;
    Ok(())

}

//Moderator can remove player from queue
pub async fn handle_remove(msg: &tmi::Privmsg<'_>, client: &mut Client, conn: &Mutex<Connection>) -> Result<(), BotError> {
    let parts: Vec<&str> = msg.text().split_whitespace().collect();
    if parts.len() == 2 {
        let twitch_name = parts[1];
        
        let conn = conn.lock().await;
        let rows = match conn.execute("DELETE FROM queue WHERE twitch_name = ?1", params![twitch_name]) {
            Ok(rows) => rows,
            Err(err) => return Err(BotError {error_code: 100, string: Some(err.to_string())}),
        };
        if rows > 0 {
            let reply = format!("{} has been removed from the queue.", twitch_name);
            _ = client.privmsg(msg.channel(), &reply).send().await;
        } else {
            let reply = format!("User {} not found in the queue.", twitch_name);
            _ = client.privmsg(msg.channel(), &reply).send().await;
        }
    }
    Ok(())
}


//Show the user where he is in queue
pub async fn handle_pos(msg: &tmi::Privmsg<'_>, client: &mut Client, queue_len: usize, conn: &Mutex<Connection>) -> Result<(), BotError> {

    let conn = conn.lock().await;
    let mut stmt = conn.prepare("SELECT rowid, * FROM queue WHERE twitch_name = ?1").unwrap();
    if let Some(index) = stmt.query_row(params![msg.sender().name()], |row| {
        Ok(row.get::<_, i64>(0)?)    
    }).optional()? {
        let group = index / queue_len as i64;
        let reply = format!("You are at position {} and in group {}", index, group + 1);
        _ = client.privmsg(msg.channel(), &reply).send().await;
    } else {
        let reply = format!("You are not in the queue, {}.", msg.sender().name());
        _ = client.privmsg(msg.channel(), &reply).send().await;
    }
    
    Ok(())
}

//User leaves queue
pub async fn handle_leave(msg: &tmi::Privmsg<'_>, client: &mut Client, conn: &Mutex<Connection>) -> Result<(), BotError> {
    let conn = conn.lock().await;
    let rows = conn.execute("DELETE FROM queue WHERE twitch_name = ?1", params![msg.sender().name()])?;
    if rows > 0 {
        let reply = format!("You have been removed from the queue, {}.", msg.sender().name());
        client.privmsg(msg.channel(), &reply).send().await?;
    } else {
        let reply = format!("You are not in queue, {}.", msg.sender().name());
        client.privmsg(msg.channel(), &reply).send().await?;
    }
    Ok(())
}

//Shows whole queue
pub async fn handle_queue(msg: &tmi::Privmsg<'_>, client: &mut Client, conn: &Mutex<Connection>) -> Result<(), BotError> {
    let conn = conn.lock().await;
    let mut stmt = conn.prepare("SELECT twitch_name FROM queue")?;
    let queue_iter = stmt.query_map([], |row| row.get::<_,String>(0))?;

    let mut queue_msg: Vec<String> = Vec::new();
    for entry in queue_iter {
        queue_msg.push(entry?);
    }
    let queue_str: Vec<String> = queue_msg.iter().enumerate().map(|(i, q)| format!("{}. {}", i + 1, q)).collect();
    let reply = format!("Queue: {:?}", queue_str);
    client.privmsg(msg.channel(), &reply).send().await?;
    
    Ok(())
}

pub async fn register_user(msg: &tmi::Privmsg<'_>, client: &mut Client, conn: &Mutex<Connection>) -> Result<(), BotError> {
    
    if let Some((_, name)) = msg.text().split_once(" ") {
        if is_valid_bungie_name(name) {
            let new_user = TwitchUser {
                twitch_name: msg.sender().name().to_string(),
                bungie_name: name.to_string()
            };
            
            match save_to_user_database(&conn, &new_user).await {
                Ok(_) => client.privmsg(msg.channel(), &format!("Registered to database as {}", new_user.bungie_name)).send().await,
                Err(_err) => {
                    client.privmsg(msg.channel(), "You are already registered").send().await
                }
            }?;
            
        } else {
            client.privmsg(msg.channel(), "You have typed invalid bungiename").send().await?;
        }
    }
    Ok(())
    
}
//if is/not in database
pub async fn bungiename(msg: &tmi::Privmsg<'_>, client: &mut Client, conn: &Mutex<Connection>, twitch_name: &str) -> Result<(), BotError> {
    let conn = conn.lock().await;
    let mut stmt = conn.prepare("SELECT rowid, * FROM user WHERE twitch_name = ?1").unwrap();

    if let Some(bungie_name) = stmt.query_row(params![twitch_name], |row| {
        Ok(row.get::<_, String>(3)?)
    }).optional()? {
        client.privmsg(msg.channel(), &format!("@{} || BungieName: {}||", twitch_name, bungie_name)).send().await?;
    } else {
        client.privmsg(msg.channel(), &format!("{}, you are not registered", twitch_name)).send().await?;
    }
    Ok(())
}

//random fireteam
pub async fn random(msg: &tmi::Privmsg<'_>, client: &mut Client, mutex_conn: &Mutex<Connection>, teamsize: usize) -> Result<(), BotError>{
    //Push the randomly chosen player to first positions
    let mut conn = mutex_conn.lock().await;
    pick_random(&mut conn, teamsize)?;

    let mut stmt = conn.prepare("SELECT twitch_name from queue WHERE id <= ?1").unwrap();
    let rows = stmt.query_map(params![teamsize], |row| row.get::<_,String>(1))?;
    let mut live_names = Vec::new();
    for names in rows {
        live_names.push(names?);
    }

    client.privmsg(msg.channel(), &format!("Randomly selected: {:?}",live_names)).send().await?;
    Ok(())
}




async fn invite_macro(bungie_name: &str) {
    let mut enigo = Enigo::new(&Settings::default()).unwrap();
    let _ = enigo.move_mouse(100, 0, enigo::Coordinate::Abs);
    let _ = enigo.button(enigo::Button::Left, enigo::Direction::Click);
    
    tokio::time::sleep(Duration::from_secs(1)).await;
    
    let _ = enigo.key(enigo::Key::Return, enigo::Direction::Click);
    
    let _ = enigo.text(&format!("/invite {}", bungie_name));
    tokio::time::sleep(Duration::from_secs(3)).await;
    let _ = enigo.key(enigo::Key::Return, enigo::Direction::Click);
}

pub async fn simple_command(msg: &tmi::Privmsg<'_>, client: &mut Client, reply: &str) -> Result<(), BotError> {
    client.privmsg(msg.channel(), &reply).send().await?;
    Ok(())
}