use std::time::Duration;

use enigo::{Enigo, Keyboard, Mouse, Settings};
use rusqlite::Connection;
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
pub async fn handle_join(msg: &tmi::Privmsg<'_>, client: &mut Client, queue: &Mutex<Vec<Queue>>, queue_len: usize) -> anyhow::Result<()> {
    if let Some((_join, part)) = msg.text().split_once(" ") {
        if part.contains('#') && part.split_once("#").unwrap().1.len() == 4 {
            let new_queue = Queue {
                twitch_name: msg.sender().name().to_string(),
                bungie_name: part.to_string(),
            };
            
            let mut queue_guard = queue.lock().await;
            if let Some(existing_queue) = queue_guard.iter_mut().find(|q| q.twitch_name == new_queue.twitch_name) {
                existing_queue.bungie_name = new_queue.bungie_name.clone();
                save_to_file(&queue_guard, FILENAME)?;

                let reply = format!("{} updated their Bungie name to {}", msg.sender().name(), new_queue.bungie_name);
                client.privmsg(msg.channel(), &reply).send().await?;
            } else if queue_guard.len() < queue_len{
                queue_guard.push(new_queue);
                
                save_to_file(&queue_guard, FILENAME)?;

                let reply = format!("{} entered the queue at position #{}", msg.sender().name(), queue_guard.len());
                client.privmsg(msg.channel(), &reply).send().await?;
            }
        } else {
            let reply = format!("Invalid command format or Bungie name, {}!", msg.sender().name());
            client.privmsg(msg.channel(), &reply).send().await?;
        }
    } else {
        let reply = format!("Invalid command format, {}! Use: !join <BungieName#1234>", msg.sender().name());
        client.privmsg(msg.channel(), &reply).send().await?;
    }
    Ok(())
}

//Kicks out users that were in game
pub async fn handle_next(msg: &tmi::Privmsg<'_>, client: &mut Client, queue: &Mutex<Vec<Queue>>, queue_len: usize) -> anyhow::Result<()> {
    
    let mut queue_guard = queue.lock().await;
    for _ in 0..queue_len {
        if !queue_guard.is_empty() {
            queue_guard.remove(0);
        }
    }

    let queue_msg: Vec<String> = queue_guard.iter().enumerate().take(queue_len).map(|(i, q)| format!("{}. {}", i + 1, q.twitch_name)).collect();
    let reply;
    if queue_msg.is_empty() {
        reply = "Queue is empty".to_string();
    } else {
        reply = format!("Next: {:?}", queue_msg);
        let futures: Vec<_> = queue_guard.iter().take(queue_len).map(|q| invite_macro(&q.bungie_name)).collect();
        futures::future::join_all(futures).await;
    };
        
    client.privmsg(msg.channel(), &reply).send().await?;

    
    
    save_to_file(&queue_guard, FILENAME)?;
    Ok(())
}

//Moderator can remove player from queue
pub async fn handle_remove(msg: &tmi::Privmsg<'_>, client: &mut Client, queue: &Mutex<Vec<Queue>>) -> anyhow::Result<()> {
    let parts: Vec<&str> = msg.text().split_whitespace().collect();
    if parts.len() == 2 {
        let twitch_name = parts[1];
        
        let mut queue_guard = queue.lock().await;
        if let Some(index) = queue_guard.iter().position(|q| q.twitch_name == twitch_name) {
            queue_guard.remove(index);
            save_to_file(&queue_guard, FILENAME)?;
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
pub async fn handle_pos(msg: &tmi::Privmsg<'_>, client: &mut Client, queue: &Mutex<Vec<Queue>>) -> anyhow::Result<()> {
    let queue_guard = queue.lock().await;
    if let Some((index, _)) = queue_guard.iter().enumerate().find(|(_, q)| q.twitch_name == msg.sender().name()) {
        let group = (index + 1) / 5;
        let reply = format!("You are at position {} and in group {}", index + 1, group);
        client.privmsg(msg.channel(), &reply).send().await?;
    } else {
        let reply = format!("You are not in the queue, {}.", msg.sender().name());
        client.privmsg(msg.channel(), &reply).send().await?;
    }
    save_to_file(&queue_guard, FILENAME)?;
    Ok(())
}

//User leaves queue
pub async fn handle_leave(msg: &tmi::Privmsg<'_>, client: &mut Client, queue: &Mutex<Vec<Queue>>) -> anyhow::Result<()> {
    let mut queue_guard = queue.lock().await;
    if let Some(index) = queue_guard.iter().position(|q| q.twitch_name == msg.sender().name()) {
        queue_guard.remove(index);
        save_to_file(&queue_guard, FILENAME)?;
        let reply = format!("You have been removed from the queue, {}.", msg.sender().name());
        client.privmsg(msg.channel(), &reply).send().await?;
    }
    Ok(())
}

//Shows whole queue
pub async fn handle_queue(msg: &tmi::Privmsg<'_>, client: &mut Client, queue: &Mutex<Vec<Queue>>) -> anyhow::Result<()> {
    let queue_guard = queue.lock().await;
    let queue_str: Vec<String> = queue_guard.iter().enumerate().map(|(i, q)| format!("{}. {}", i + 1, q.twitch_name)).collect();
    let reply = format!("Queue: {:?}", queue_str);
    client.privmsg(msg.channel(), &reply).send().await?;
    save_to_file(&queue_guard, FILENAME)?;
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