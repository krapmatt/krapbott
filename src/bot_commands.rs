use std::time::Duration;

use egui::emath::smart_aim;
use enigo::{Enigo, Keyboard, Mouse, Settings};
use rusqlite::{params, Connection, OptionalExtension};
use tmi::{msg, Client};
use tokio::sync::Mutex;

use crate::{bot::FILENAME, initialize_database, save_to_database, save_to_file, Queue};

pub async fn is_moderator(msg: &tmi::Privmsg<'_>, client: &mut Client, ) -> bool {
    //předělat píše i když je success)
    
    if msg.badges().into_iter().any(|badge| badge.as_badge_data().name() == "mod" || badge.as_badge_data().name() == "broadcaster") {
        return true;
    } else {
        client.privmsg(msg.channel(), "You are not a moderator/broadcaster. You can't use this command").send().await;
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
    if let Some((_join, part)) = msg.text().split_once(" ") {
        //If name is correct
        if part.contains('#') && part.split_once("#").unwrap().1.len() == 4 {
            //Update their name in queue
            let new_queue = Queue {
                twitch_name: msg.sender().name().to_string(),
                bungie_name: part.to_string(),
            };
            
            let conn = conn.lock().await;
            let mut stmt = conn.prepare("SELECT * FROM queue WHERE twitch_name = ?1")?;

            let exists: Result<Option<Queue>, _> = stmt.query_row(params![new_queue.twitch_name], |row| {
                Ok(Queue {
                    twitch_name: row.get(0)?,
                    bungie_name: row.get(1)?,
                })
            }).optional();

            if let Some(existing_queue) = exists? {
                conn.execute("UPDATE queue SET bungie_name = ?1 WHERE twitch_name = ?2", params![new_queue.bungie_name, new_queue.twitch_name])?;
                let reply = format!("{} updated their Bungie name to {}", msg.sender().name(), new_queue.bungie_name);
                client.privmsg(msg.channel(), &reply).send().await?;
            //New name in queue - joined
            } else {
                let count: i64 = conn.query_row("SELECT COUNT(*) FROM queue", [], |row| row.get(0))?;
                if count < queue_len as i64 {
                    conn.execute(
                        "INSERT INTO queue (twitch_name, bungie_name) VALUES (?1, ?2)",
                        params![new_queue.twitch_name, new_queue.bungie_name],
                    )?;
                    let reply = format!("{} entered the queue at position #{}", msg.sender().name(), count + 1);
                    client.privmsg(msg.channel(), &reply).send().await?;
                //Queue is full
                } else {
                    client.privmsg(msg.channel(), "You can't enter queue if full").send().await?;
                }
            }
        //Name is incorrect
        } else {
            let reply = format!("Invalid command format or Bungie name, {}!", msg.sender().name());
            client.privmsg(msg.channel(), &reply).send().await?;
        }
    //if command is incorrect
    } else {
        let reply = format!("Invalid command format, {}! Use: !join <BungieName#1234>", msg.sender().name());
        client.privmsg(msg.channel(), &reply).send().await?;
    }
    Ok(())
}

//Kicks out users that were in game
pub async fn handle_next(msg: &tmi::Privmsg<'_>, client: &mut Client, queue_len: usize, conn: &Mutex<Connection>) -> anyhow::Result<()> {
    let mut conn = conn.lock().await;

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
    if let Some((index, _)) = stmt.query_row(params![msg.sender().name()], |row| {
        Ok((row.get::<_, i64>(0)? - 1, row.get::<_, String>(1)?))    
    }).optional()? {
        let group = (index + 1) / queue_len as i64;
        let reply = format!("You are at position {} and in group {}", index + 1, group);
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
    let queue_iter = stmt.query_map([], |row| row.get::<_,String>(1))?;

    let mut queue_msg = Vec::new();
    for entry in queue_iter {
        queue_msg.push(entry?);
    }
    let queue_str: Vec<String> = queue_msg.iter().enumerate().map(|(i, q)| format!("{}. {}", i + 1, q)).collect();
    let reply = format!("Queue: {:?}", queue_str);
    client.privmsg(msg.channel(), &reply).send().await?;
    
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