mod bot_commands;
use std::{fs::{remove_file, File}, io::{self, BufRead, BufReader, Read, Write}, vec};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::bot_commands::{handle_join, handle_leave, handle_next, handle_pos, handle_queue, handle_remove, is_moderator};

const FILENAME: &str = "queue.json";
const CHANNELS: &[&str] = &["#krapmatt"];

#[derive(Debug, Deserialize, Serialize)]
struct Queue {
    twitch_name: String,
    bungie_name: String,
}

impl Default for Queue {
    fn default() -> Self {
        Queue { twitch_name: "Empty".to_string(), bungie_name: "Empty".to_string() }
    }
}
impl PartialEq for Queue {
    fn eq(&self, other: &Self) -> bool {
        if self.twitch_name == other.twitch_name {
            return true
        } else {
            return false
        }
    }
}

fn save_to_file(data: &Vec<Queue>, filename: &str) -> io::Result<()> {
    let mut file = File::create(filename)?;
    for entry in data {
        let json = serde_json::to_string(entry)?;
        writeln!(file, "{}", json)?;
    }
    //let json = serde_json::to_string(data)?;
    //let mut file = File::create(filename)?;
    //file.write_all(json.as_bytes())?;
    Ok(())
}

fn load_from_file(filename: &str) -> io::Result<Vec<Queue>> {
    /*let mut file = File::open(filename)?;
    let mut json = String::new();
    file.read_to_string(&mut json)?;
    let data: Vec<Queue> = serde_json::from_str(&json)?;
    Ok(data)*/

    let file = File::open(filename)?;
    let reader = BufReader::new(file);
    let mut data = Vec::new();
    for line in reader.lines() {
        let line = line?;
        let entry: Queue = serde_json::from_str(&line)?;
        data.push(entry);
    }
    Ok(data)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let oauth_token = "gbmw5v39axrv7q2xdzyhneph5hi7vi"; 
    let nickname = "krapbott";   
    
    let queue: Vec<Queue> = vec![];
    remove_file(FILENAME)?;
    save_to_file(&queue, FILENAME)?;

    let credentials = tmi::Credentials::new(nickname, oauth_token);
    let mut client = tmi::Client::builder().credentials(credentials).connect().await.unwrap();
    

    client.join_all(CHANNELS).await?;

    loop {
        let msg = client.recv().await?;
        match msg.as_typed()? {
            tmi::Message::Privmsg(msg) => {
                //TODO better queue save to file and load
                let queue_mutex = Mutex::new(load_from_file(FILENAME)?);
                println!("{}: {}", msg.sender().name(), msg.text());
                if msg.text().starts_with("!join") {
                    handle_join(&msg, &mut client, &queue_mutex).await?;
                } else if msg.text().starts_with("!next") && is_moderator(&msg, &mut client).await {
                    handle_next(&mut client, &queue_mutex).await?;
                } else if msg.text().starts_with("!remove") && is_moderator(&msg, &mut client).await {
                    handle_remove(&msg, &mut client, &queue_mutex).await?;
                } else if msg.text().starts_with("!pos") {
                    handle_pos(&msg, &mut client, &queue_mutex).await?;
                } else if msg.text().starts_with("!leave") {
                    handle_leave(&msg, &mut client, &queue_mutex).await?;
                } else if msg.text().starts_with("!queue") {
                    handle_queue(&mut client, &queue_mutex).await?;
                }
                
            }
            tmi::Message::Reconnect => {
                client.reconnect().await?;
                client.join_all(CHANNELS).await?;
            }
            tmi::Message::Ping(ping) => {
                client.pong(&ping).await?;
            }
            _ => {}
        }
    }
    
}


