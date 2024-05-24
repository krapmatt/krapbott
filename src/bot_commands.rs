use tmi::Client;
use tokio::sync::Mutex;

use crate::{save_to_file, Queue, CHANNELS, FILENAME};

pub async fn handle_join(msg: &tmi::Privmsg<'_>, client: &mut Client, queue: &Mutex<Vec<Queue>>) -> anyhow::Result<()> {
    let parts: Vec<&str> = msg.text().split_whitespace().collect();
    if parts.len() == 2 && parts[1].contains('#') {
        let new_queue = Queue {
            twitch_name: msg.sender().name().to_string(),
            bungie_name: parts[1].to_string(),
        };
        
        let mut queue_guard = queue.lock().await;
        if !queue_guard.contains(&new_queue) {
            queue_guard.push(new_queue);
            save_to_file(&queue_guard, FILENAME)?;

            let reply = format!("{} entered the queue at position #{}", msg.sender().name(), queue_guard.len());
            client.privmsg(CHANNELS[0], &reply).send().await?;
        } else {
            let reply = format!("You are already in the queue, {}!", msg.sender().name());
            client.privmsg(CHANNELS[0], &reply).send().await?;
        }
    } else {
        let reply = format!("Invalid command format or Bungie name, {}!", msg.sender().name());
        client.privmsg(CHANNELS[0], &reply).send().await?;
    }
    Ok(())
}

pub async fn handle_next(client: &mut Client, queue: &Mutex<Vec<Queue>>) -> anyhow::Result<()> {
    let mut queue_guard = queue.lock().await;
    for _ in 0..5 {
        if !queue_guard.is_empty() {
            queue_guard.remove(0);
        }
    }

    let queue_msg: Vec<String> = queue_guard.iter().enumerate().map(|(i, q)| format!("{}. {}", i + 1, q.twitch_name)).collect();
    let reply = format!("Next: {:?}", queue_msg);
    client.privmsg(CHANNELS[0], &reply).send().await?;
    
    save_to_file(&queue_guard, FILENAME)?;
    Ok(())
}

pub async fn handle_remove(msg: &tmi::Privmsg<'_>, client: &mut Client, queue: &Mutex<Vec<Queue>>) -> anyhow::Result<()> {
    let parts: Vec<&str> = msg.text().split_whitespace().collect();
    if parts.len() == 2 {
        let twitch_name = parts[1];
        
        let mut queue_guard = queue.lock().await;
        if let Some(index) = queue_guard.iter().position(|q| q.twitch_name == twitch_name) {
            queue_guard.remove(index);
            save_to_file(&queue_guard, FILENAME)?;
            let reply = format!("{} has been removed from the queue.", twitch_name);
            client.privmsg(CHANNELS[0], &reply).send().await?;
        } else {
            let reply = format!("User {} not found in the queue.", twitch_name);
            client.privmsg(CHANNELS[0], &reply).send().await?;
        }
    }
    Ok(())
}

pub async fn handle_pos(msg: &tmi::Privmsg<'_>, client: &mut Client, queue: &Mutex<Vec<Queue>>) -> anyhow::Result<()> {
    let queue_guard = queue.lock().await;
    if let Some((index, _)) = queue_guard.iter().enumerate().find(|(_, q)| q.twitch_name == msg.sender().name()) {
        let group = (index + 1) / 5;
        let reply = format!("You are at position {} and in group {}", index + 1, group);
        client.privmsg(CHANNELS[0], &reply).send().await?;
    } else {
        let reply = format!("You are not in the queue, {}.", msg.sender().name());
        client.privmsg(CHANNELS[0], &reply).send().await?;
    }
    save_to_file(&queue_guard, FILENAME)?;
    Ok(())
}

pub async fn handle_leave(msg: &tmi::Privmsg<'_>, client: &mut Client, queue: &Mutex<Vec<Queue>>) -> anyhow::Result<()> {
    let mut queue_guard = queue.lock().await;
    if let Some(index) = queue_guard.iter().position(|q| q.twitch_name == msg.sender().name()) {
        queue_guard.remove(index);
        save_to_file(&queue_guard, FILENAME)?;
        let reply = format!("You have been removed from the queue, {}.", msg.sender().name());
        client.privmsg(CHANNELS[0], &reply).send().await?;
    }
    Ok(())
}

pub async fn handle_queue(client: &mut Client, queue: &Mutex<Vec<Queue>>) -> anyhow::Result<()> {
    let queue_guard = queue.lock().await;
    let queue_str: Vec<String> = queue_guard.iter().enumerate().map(|(i, q)| format!("{}. {}", i + 1, q.twitch_name)).collect();
    let reply = format!("Queue: {:?}", queue_str);
    client.privmsg(CHANNELS[0], &reply).send().await?;
    save_to_file(&queue_guard, FILENAME)?;
    Ok(())
}