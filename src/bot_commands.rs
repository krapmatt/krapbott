use std::time::Duration;

use enigo::{Enigo, Keyboard, Mouse, Settings};
use rusqlite::{params, Connection, OptionalExtension};
use tmi::Client;
use tokio::sync::{Mutex, MutexGuard};

use crate::{database::{initialize_database, save_to_user_database, USER_TABLE}, TwitchUser};

pub async fn is_moderator(msg: &tmi::Privmsg<'_>, client: &mut Client, ) -> bool {
    if msg.badges().into_iter().any(|badge| badge.as_badge_data().name() == "mod" || badge.as_badge_data().name() == "broadcaster") {
        return true;
    } else {
        client.privmsg(msg.channel(), "You are not a moderator/broadcaster. You can't use this command").send().await.expect("No connection to channel");
        return false;
    }
}

pub async fn is_follower(oauth_token: &str, from_user_id: &str, to_user_id: &str) -> Result<bool, reqwest::Error> {
    let url = format!("https://api.twitch.tv/helix/users/follows?from_id={}&to_id={}", from_user_id, to_user_id);
    let client = reqwest::Client::new();
todo!();
    let res = client
        .get(&url)
        .bearer_auth(oauth_token)
        .send()
        .await;
    println!("{:?}", res);
    Ok(!res.is_ok())
}

//User can join into queue
pub async fn handle_join(msg: &tmi::Privmsg<'_>, client: &mut Client, queue_len: usize, conn: &Mutex<Connection>) -> anyhow::Result<()> {
    if let Some((_join, name)) = msg.text().split_once(" ") {
        //If name is correct
        if is_valid_bungie_name(name) {
            //Update their name in queue
            let new_queue = TwitchUser {
                twitch_name: msg.sender().name().to_string(),
                bungie_name: name.to_string(),
            };
            
            let conn = conn.lock().await;
            let mut stmt = conn.prepare("SELECT * FROM queue WHERE twitch_name = ?1")?;

            let exists: Result<Option<TwitchUser>, _> = stmt.query_row(params![new_queue.twitch_name], |row| {
                Ok(TwitchUser {
                    twitch_name: row.get(1)?,
                    bungie_name: row.get(2)?,
                })
            }).optional();
            drop(stmt);
            if let Some(_) = exists? {
                conn.execute("UPDATE queue SET bungie_name = ?1 WHERE twitch_name = ?2", params![new_queue.bungie_name, new_queue.twitch_name])?;
                let reply = format!("{} updated their Bungie name to {}", msg.sender().name(), new_queue.bungie_name);
                client.privmsg(msg.channel(), &reply).send().await?;
            //New name in queue - joined
            } else {
                add_to_queue(msg, client, queue_len, conn, new_queue).await?;
            }
        //Name is incorrect
        } else {
            send_invalid_name_reply(msg, client).await?;
        }
    //if command is incorrect or user is registered
    } else {
        let conn_user = initialize_database("user.db", USER_TABLE).unwrap();
        let bungie_name = conn_user.query_row("SELECT * FROM user WHERE twitch_name = ?1", params![msg.sender().name()], |row| row.get::<_, String>(2))?;
        
        let join_queue = TwitchUser {
            twitch_name: msg.sender().name().to_string(),
            bungie_name: bungie_name
        };
        let conn = conn.lock().await;
        add_to_queue(msg, client, queue_len, conn, join_queue).await?;
    }
    Ok(())
}

fn is_valid_bungie_name(name: &str) -> bool {
    name.contains('#') && name.split_once('#').unwrap().1.len() == 4
}

async fn send_invalid_name_reply(msg: &tmi::Privmsg<'_>, client: &mut Client) -> anyhow::Result<()> {
    let reply = format!("Invalid command format or Bungie name, {}!", msg.sender().name());
    client.privmsg(msg.channel(), &reply).send().await?;
    Ok(())
}

async fn process_queue_entry(msg: &tmi::Privmsg<'_>, client: &mut Client, queue_len: usize, conn: &Mutex<Connection>, user: TwitchUser) -> anyhow::Result<()> {
    let conn = conn.lock().await;
    if user_exists_in_queue(&conn, &user.twitch_name)? {
        update_queue(&conn, &user)?;
        let reply = format!("{} updated their Bungie name to {}", msg.sender().name(), user.bungie_name);
        client.privmsg(msg.channel(), &reply).send().await?;
    } else {
        add_to_queue(msg, client, queue_len, conn, user).await?;
    }
    Ok(())
}

fn user_exists_in_queue(conn: &Connection, twitch_name: &str) -> anyhow::Result<bool> {
    let mut stmt = conn.prepare("SELECT 1 FROM queue WHERE twitch_name = ?1")?;
    let exists: Result<Option<i64>, _> = stmt.query_row(params![twitch_name], |row| row.get(0)).optional();
    Ok(exists?.is_some())
}

fn update_queue(conn: &Connection, user: &TwitchUser) -> anyhow::Result<()> {
    conn.execute(
        "UPDATE queue SET bungie_name = ?1 WHERE twitch_name = ?2",
        params![user.bungie_name, user.twitch_name],
    )?;
    Ok(())
}

async fn add_to_queue<'a>(msg: &tmi::Privmsg<'_>, client: &mut Client, queue_len: usize, conn: MutexGuard<'a, Connection>, user: TwitchUser) -> anyhow::Result<()>{
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM queue", [], |row| row.get(0))?;
        if count < queue_len as i64 {
            conn.execute(
                "INSERT INTO queue (twitch_name, bungie_name) VALUES (?1, ?2)",
                params![user.twitch_name, user.bungie_name],
            )?;
            let reply = format!("{} entered the queue at position #{}", msg.sender().name(), count + 1);
            client.privmsg(msg.channel(), &reply).send().await?;
        } else {
            //Queue is full
            client.privmsg(msg.channel(), "You can't enter queue, it is full").send().await?;
        }
    Ok(())
}

//Kicks out users that were in game
pub async fn handle_next(msg: &tmi::Privmsg<'_>, client: &mut Client, queue_len: usize, conn: &Mutex<Connection>) -> anyhow::Result<()> {
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
pub async fn handle_remove(msg: &tmi::Privmsg<'_>, client: &mut Client, conn: &Mutex<Connection>) -> anyhow::Result<()> {
    let parts: Vec<&str> = msg.text().split_whitespace().collect();
    if parts.len() == 2 {
        let twitch_name = parts[1];
        
        let conn = conn.lock().await;
        let rows = conn.execute("DELETE FROM queue WHERE twitch_name = ?1", params![twitch_name])?;
        if rows > 0 {
            let reply = format!("{} has been removed from the queue.", twitch_name);
            client.privmsg(msg.channel(), &reply).send().await?;
        } else {
            let reply = format!("User {} not found in the queue.", twitch_name);
            client.privmsg(msg.channel(), &reply).send().await?;
        }
    }
    Ok(())
}


//Show the user where he is in queue
pub async fn handle_pos(msg: &tmi::Privmsg<'_>, client: &mut Client, queue_len: usize, conn: &Mutex<Connection>) -> anyhow::Result<()> {
    let conn = conn.lock().await;
    let mut stmt = conn.prepare("SELECT rowid, * FROM queue WHERE twitch_name = ?1")?;
    if let Some(index)= stmt.query_row(params![msg.sender().name()], |row| {
        Ok(row.get::<_, i64>(0)?)    
    }).optional()? {
        let group = index / queue_len as i64;
        let reply = format!("You are at position {} and in group {}", index, group + 1);
        client.privmsg(msg.channel(), &reply).send().await?;
    } else {
        let reply = format!("You are not in the queue, {}.", msg.sender().name());
        client.privmsg(msg.channel(), &reply).send().await?;
    }
    
    Ok(())
}

//User leaves queue
pub async fn handle_leave(msg: &tmi::Privmsg<'_>, client: &mut Client, conn: &Mutex<Connection>) -> anyhow::Result<()> {
    let conn = conn.lock().await;
    let rows = conn.execute("DELETE FROM queue WHERE twitch_name = ?1", params![msg.sender().name()])?;
    if rows > 0 {
        let reply = format!("You have been removed from the queue, {}.", msg.sender().name());
        client.privmsg(msg.channel(), &reply).send().await?;
    } else {
        let reply = format!("You are not in queue, {}.", msg.sender().name());
    }
    Ok(())
}

//Shows whole queue
pub async fn handle_queue(msg: &tmi::Privmsg<'_>, client: &mut Client, conn: &Mutex<Connection>) -> anyhow::Result<()> {
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

pub async fn register_user(msg: &tmi::Privmsg<'_>, client: &mut Client) {
    
    if let Some((_, name)) = msg.text().split_once(" ") {
        if is_valid_bungie_name(name) {
            let new_user = TwitchUser {
                twitch_name: msg.sender().name().to_string(),
                bungie_name: name.to_string()
            };
            println!("{} {}" ,new_user.twitch_name, new_user.bungie_name);
            let conn = Mutex::new(initialize_database("user.db", USER_TABLE).unwrap());
            match save_to_user_database(&conn, &new_user).await {
                Ok(_) => client.privmsg(msg.channel(), "Successful registration to database").send().await,
                Err(err) => client.privmsg(msg.channel(), "You are already registered").send().await
            }; 
            
        }
    }

    
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

pub async fn join_on_me(msg: &tmi::Privmsg<'_>, client: &mut Client) -> anyhow::Result<()> {
    client.privmsg(msg.channel(), "Type in game chat: /join KrapMatt#1497").send().await?;
    Ok(())
}

pub async fn id_text(msg: &tmi::Privmsg<'_>, client: &mut Client) -> anyhow::Result<()> {
    client.privmsg(msg.channel(), "KrapMatt#1497").send().await?;
    Ok(())
}

pub async fn discord(msg: &tmi::Privmsg<'_>, client: &mut Client) -> anyhow::Result<()> {
    client.privmsg(msg.channel(), "https://discord.gg/jJMwaetjeu").send().await?;
    Ok(())
}

pub async fn lurk_msg(msg: &tmi::Privmsg<'_>, client: &mut Client) -> anyhow::Result<()> {
    let reply = format!("Thanks for the lurk {}. I'll appreciate if you leave tab open <3", msg.sender().name());
    client.privmsg(msg.channel(), &reply).send().await?;
    Ok(())
}