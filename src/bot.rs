use crate::{ bot_commands::{discord, handle_join, handle_leave, handle_next, handle_pos, handle_queue, handle_remove, id_text, is_moderator, join_on_me, lurk_msg}, initialize_database, load_from_file, save_to_file, Queue};
use dotenv::dotenv;

use std::{env::var, sync::Arc};
use tokio::sync::Mutex;

pub const CHANNELS: &[&str] = &["#krapmatt"];

pub struct BotState {
    queue_open: bool,
}

impl BotState {
    pub fn new() -> BotState {
        BotState { 
            queue_open: false, 
        }
    }
}


pub async fn run_chat_bot(bot_state: Arc<Mutex<BotState>>) -> anyhow::Result<()> {
    dotenv().ok();
    let oauth_token = var("TWITCH_OAUTH_TOKEN_BOTT").expect("No oauth token"); 
    let nickname = var("TWITCH_BOT_NICK").expect("No bot name");   

    let credentials = tmi::Credentials::new(nickname, oauth_token);
    let mut client = tmi::Client::builder().credentials(credentials).connect().await.unwrap();

    client.join_all(CHANNELS).await?;
    
    loop {
        let msg = client.recv().await?;
        match msg.as_typed()? {
            tmi::Message::Privmsg(msg) => {
                //temp
                //set size of queue
                let queue_len = 30;
                //set size of fireteam
                let queue_drop = 5;
                
                println!("Channel: {}, {}: {}", msg.channel() ,msg.sender().name(), msg.text());
                if msg.text().starts_with("!open_queue") && is_moderator(&msg, &mut client).await {
                    bot_state.lock().await.queue_open = true;
                    client.privmsg(msg.channel(), "The queue is now open!").send().await?;
                } else if msg.text().starts_with("!close_queue") && is_moderator(&msg, &mut client).await {
                    bot_state.lock().await.queue_open = false;
                    client.privmsg(msg.channel(), "The queue is now closed!").send().await?;
                } else if bot_state.lock().await.queue_open {
                    let conn = Arc::new(Mutex::new(initialize_database().unwrap()));
                    if msg.text().starts_with("!join") {
                        handle_join(&msg, &mut client, queue_len, &conn).await?;
                    } else if msg.text().starts_with("!next") && is_moderator(&msg, &mut client).await {
                        handle_next(&msg, &mut client,queue_drop, &conn).await?;
                    } else if msg.text().starts_with("!remove") && is_moderator(&msg, &mut client).await {
                        handle_remove(&msg, &mut client, &conn).await?;
                    } else if msg.text().starts_with("!pos") {
                        handle_pos(&msg, &mut client, queue_len, &conn).await?;
                    } else if msg.text().starts_with("!leave") {
                        handle_leave(&msg, &mut client, &conn).await?;
                    } else if msg.text().starts_with("!queue") {
                        handle_queue(&msg, &mut client, &conn).await?;
                    }
                } else {
                    if msg.text().starts_with("!join") || msg.text().starts_with("!next") || msg.text().starts_with("!remove") || msg.text().starts_with("!pos") || msg.text().starts_with("!leave") || msg.text().starts_with("!queue") {
                        client.privmsg(msg.channel(), "The queue is currently closed!").send().await?;
                    }
                }
                
                if msg.text().starts_with("!id") {
                    id_text(&msg, &mut client).await?;
                    join_on_me(&msg, &mut client).await?;
                } else if msg.text().starts_with("!discord") {
                    discord(&msg, &mut client).await?;
                } else if msg.text().starts_with("!lurk") {
                    lurk_msg(&msg, &mut client).await?;
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