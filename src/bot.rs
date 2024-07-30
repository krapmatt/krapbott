use crate::{ 
    bot_commands::{ban_bots, bungiename, is_follower, is_moderator, random, register_user, simple_command}, database::{get_command_response, remove_command, save_command}, initialize_database, models::{BotError, ChatMessage}, SharedState
};
use dotenv::dotenv;
use rusqlite::Connection;
use tmi::Client;

use std::{env::var, sync::Arc};
use tokio::sync::Mutex;

pub const CHANNELS: &[&str] = &["#krapmatt"];

pub struct BotState {
    pub queue_open: bool,
    oauth_token_bot: String,
    pub nickname: String,
    bot_id: String,
    pub conn: Mutex<Connection>,
    pub queue_len: usize,
    pub queue_teamsize: usize,

}

impl BotState {
    pub fn new() -> BotState {
        dotenv().ok();
        let oauth_token_bot = var("TWITCH_OAUTH_TOKEN_BOTT").expect("No bot oauth token"); 
        let nickname = var("TWITCH_BOT_NICK").expect("No bot name");   
        let bot_id = var("TWITCH_CLIENT_ID_BOT").expect("msg");
        let conn = Mutex::new(initialize_database());
        BotState { 
            queue_open: false,
            oauth_token_bot: oauth_token_bot,
            nickname: nickname,
            bot_id: bot_id,
            conn: conn,
            queue_len: 30,
            queue_teamsize: 5,
        }
    }

    pub async fn client_builder(&mut self) -> Client {
        let credentials = tmi::Credentials::new(self.nickname.clone(), self.oauth_token_bot.clone());
        let mut client = tmi::Client::builder().credentials(credentials).connect().await.unwrap();
        client.join_all(CHANNELS).await.unwrap();
        client
    }

    async fn non_queue_comms(&self, text: &str, mut client: &mut Client, msg: &tmi::Privmsg<'_>) -> Result<(), BotError> {
        match text {   
            text if text.to_ascii_lowercase().starts_with("!connect") && is_moderator(msg, client).await => {
                if let Some((_, channel)) = text.split_once(" ") {
                    client.join(format!("#{}", channel)).await?;
                } else {
                    client.privmsg(msg.channel(), "You didn't write the channel to connect to").send().await?;
                }
            }
            text if text.to_ascii_lowercase().starts_with("!lurk") => {
                simple_command(&msg, &mut client, &format!("Thanks for the krapmaLurk {}! Be sure to leave the tab on low volume, or mute tab, to support stream krapmaHeart", msg.sender().name())).await?;
            }

            text if text.starts_with("!register") => {
                if let Some((_, bungie_name)) = text.split_once(" ") {
                    register_user(&msg, &mut client, &self.conn, &msg.sender().name(), bungie_name).await?;
                } else {
                    client.privmsg(msg.channel(), "Invalid command format! Use: !register bungiename#1234").send().await?;
                }
            }
            text if text.to_ascii_lowercase().starts_with("!mod_register") && is_moderator(&msg, &mut client).await => {
                let words: Vec<&str> = text.split_whitespace().collect();
                if words.len() == 3 {
                    let mut twitch_name = words[1].to_string();
                    let bungie_name = words[2];
                    if twitch_name.starts_with("@") {
                        twitch_name.remove(0);
                    }
                    register_user(&msg, &mut client, &self.conn, &twitch_name, bungie_name).await?;
                } else {
                    client.privmsg(msg.channel(), "You are a mod. . . || If you forgot use: !mod_register twitchname bungoname").send().await?;
                }
            }
            text if is_bannable_link(text) => {
                ban_bots(&msg, &self.oauth_token_bot, self.bot_id.clone()).await;
                client.privmsg(msg.channel(), "We don't want cheap viewers, only expensive ones <3").send().await?;
            }
            text if text.starts_with("!bungiename") => {
                if text.trim_end().len() == 11 {
                    bungiename(&msg, &mut client, &self.conn, &msg.sender().name(), ).await?;
                } else {
                    let (_, twitch_name) = text.split_once(" ").expect("How did it panic, what happened? //Always is something here");
                    let mut twitch_name = twitch_name.to_string();
                    if twitch_name.starts_with("@") {
                        twitch_name.remove(0);
                    }
                    bungiename(&msg, &mut client, &self.conn, &twitch_name).await?;
                }
    
            }
            text if text.starts_with("!mod_addcommand") && is_moderator(&msg, &mut client).await => {
                let words: Vec<&str> = text.split_whitespace().collect();
                if words.len() > 2 {
                    let channel = msg.channel();
                    let command = words[1];
                    let reply = words[2..].join(" ");
                    save_command(&self.conn, command, &reply, channel).await;
                    client.privmsg(msg.channel(), &format!("Command !{} added.", command)).send().await?;
                } else {
                    client.privmsg(msg.channel(), "Usage: !addcommand <command> <response>").send().await?;
                }
            }
            text if text.starts_with("!mod_removecommand") && is_moderator(&msg, &mut client).await => {
                let words: Vec<&str> = text.split_whitespace().collect();
                let command = words[1];
                if remove_command(&self.conn, command).await {
                    client.privmsg(msg.channel(), &format!("Command !{} removed.", command)).send().await?;
                } else {
                    client.privmsg(msg.channel(), &format!("Command !{} doesn't exist.", command)).send().await?;
                }
            }
            text if text.starts_with("!") => {
                if let Ok(Some(reply)) = get_command_response(&self.conn, text, msg.channel()).await {
                    client.privmsg(msg.channel(), &reply).send().await?;
                }
            }
            &_ => {},
        } 
        Ok(())
    }
}
//Timers/Counters
//Bungie api stuff - evade it
pub async fn run_chat_bot(bot_state: Arc<Mutex<BotState>>, shared_state: Arc<std::sync::Mutex<SharedState>>,) -> Result<(), BotError> {
    let mut client = bot_state.lock().await.client_builder().await;
    let mut run_count = 0;
    loop {
        let msg = client.recv().await?;
        match msg.as_typed()? {
            tmi::Message::Privmsg(msg) => {
                
                let chat_message = ChatMessage {
                    channel: msg.channel().to_string(),
                    user: msg.sender().name().to_string(),
                    text: msg.text().to_string(),
                };
                shared_state.lock().unwrap().add_message(chat_message);
      
                let mut bot_state = bot_state.lock().await;
                match msg.text() {
                    text if text.starts_with("!open_queue") && is_moderator(&msg, &mut client).await => {
                        bot_state.conn.lock().await.execute("DELETE from queue", [])?;
                        bot_state.queue_open = true;
                        client.privmsg(msg.channel(), "The queue is now open!").send().await?;
                    }
                    text if text.starts_with("!close_queue") && is_moderator(&msg, &mut client).await => {
                        bot_state.queue_open = false;
                        client.privmsg(msg.channel(), "The queue is now closed!").send().await?;
                    }
                    //TODO! add take any lowerupper case
                    
                    text if text.to_ascii_lowercase().starts_with("!join") && is_follower(&msg, &mut client, &bot_state.oauth_token_bot, bot_state.bot_id.clone()).await? => {
                        bot_state.handle_join(&msg, &mut client).await?;
                    }
                    text if text.to_ascii_lowercase().starts_with("!next") && is_moderator(&msg, &mut client).await => {
                        run_count += 1;
                        bot_state.handle_next(&msg, &mut client).await?;
                    }
                    text if text.to_ascii_lowercase().starts_with("!remove") && is_moderator(&msg, &mut client).await => {
                        bot_state.handle_remove(&msg, &mut client).await?;
                    }
                    text if text.to_ascii_lowercase().starts_with("!pos") => {
                        bot_state.handle_pos(&msg, &mut client).await?;
                    }
                    text if text.to_ascii_lowercase().starts_with("!leave") => {
                        bot_state.handle_leave(&msg, &mut client).await?;
                    }
                    text if text.to_ascii_lowercase().starts_with("!queue") || text.starts_with("!list")=> {
                        bot_state.handle_queue(&msg, &mut client).await?;
                    }
                    text if text.to_ascii_lowercase().starts_with("!random") && is_moderator(&msg, &mut client).await => {
                        random(&msg, &mut client, &bot_state.conn, bot_state.queue_teamsize).await?;
                    }
                    text => {
                        bot_state.non_queue_comms(&text, &mut client, &msg).await?;
                    }
                    
                    
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

fn is_bannable_link(text: &str) -> bool {
    if (text.to_ascii_lowercase().starts_with("cheap viewers on") || text.to_ascii_lowercase().starts_with("best viewers on") && text.contains(".")) || text.contains("Hello, sorry for bothering you. I want to offer promotion of your channel, viewers, followers, views, chat bots, etc...The price is lower than any competitor, the quality is guaranteed to be the best. Flexible and convenient order management panel, chat panel, everything is in your hands, a huge number of custom settings")  {
        true
    } else {
        false
    }
}

