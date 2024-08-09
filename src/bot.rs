use crate::{ 
    bot_commands::{ban_bots, bungiename, is_follower, is_moderator, register_user, send_message}, database::{get_command_response, remove_command, save_command}, initialize_database, models::{BotError, ChatMessage}, SharedState
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
            queue_len: 20,
            queue_teamsize: 5,
        }
    }

    pub async fn client_builder(&mut self) -> Client {
        let credentials = tmi::Credentials::new(self.nickname.clone(), self.oauth_token_bot.clone());
        let mut client = tmi::Client::builder().credentials(credentials).connect().await.unwrap();
        client.join_all(CHANNELS).await.unwrap();
        client
    }

    async fn non_queue_comms(&mut self, mut client: &mut Client, msg: &tmi::Privmsg<'_>) -> Result<(), BotError> {
        match msg.text() {   
            text if text.to_ascii_lowercase().starts_with("!connect") && is_moderator(msg, client).await => {
                if let Some((_, channel)) = msg.text().split_once(" ") {
                    client.join(format!("#{}", channel)).await?;
                } else {
                    client.privmsg(msg.channel(), "You didn't write the channel to connect to").send().await?;
                }
            }
            text if text.to_ascii_lowercase().starts_with("!lurk") => {
                send_message(&msg, &mut client, &format!("Thanks for the krapmaLurk {}! Be sure to leave the tab on low volume, or mute tab, to support stream krapmaHeart", msg.sender().name())).await?;
            }
            text if text.to_ascii_lowercase().starts_with("!so") && is_moderator(msg, client).await => {
                let words:Vec<&str> = msg.text().split_ascii_whitespace().collect();
                let mut twitch_name = words[1].to_string();
                if twitch_name.starts_with("@") {
                    twitch_name.remove(0);
                }
                send_message(&msg, &mut client, &format!("Let's give a big Shoutout to https://www.twitch.tv/{}!. Make sure to check them out and give them a FOLLOW krapmaHeart", twitch_name)).await?;
                send_message(msg, client, &format!("/shoutout {}", twitch_name)).await?;
            }
            text if text.to_ascii_lowercase().starts_with("!total") => {
                self.total_raid_clears(msg, client).await?;
            }
            text if text.to_ascii_lowercase().starts_with("!register") => {
                let reply;
                if let Some((_, bungie_name)) = msg.text().split_once(" ") {
                    reply = register_user(&self.conn, &msg.sender().name(), bungie_name).await?;
                } else {
                    reply = "Invalid command format! Use: !register bungiename#1234".to_string();
                }
                send_message(msg, client, &reply).await?;
            }
            text if text.to_ascii_lowercase().starts_with("!mod_register") && is_moderator(&msg, &mut client).await => {
                let words: Vec<&str> = text.split_whitespace().collect();
                let reply;
                if words.len() >= 3 {
                    let mut twitch_name = words[1].to_string();
                    let bungie_name = &words[2..].join(" ");
                    if twitch_name.starts_with("@") {
                        twitch_name.remove(0);
                    }
                    reply = register_user(&self.conn, &twitch_name, bungie_name).await?;
                } else {
                    reply = "You are a mod. . . || If you forgot use: !mod_register twitchname bungoname".to_string();
                }
                send_message(msg, client, &reply).await?;
            }
            text if is_bannable_link(text) => {
                ban_bots(&msg, &self.oauth_token_bot, self.bot_id.clone()).await;
                client.privmsg(msg.channel(), "We don't want cheap viewers, only expensive ones <3").send().await?;
            }
            text if text.starts_with("!bungiename") => {
                if text.trim_end().len() == 11 {
                    bungiename(&msg, &mut client, &self.conn, &msg.sender().name()).await?;
                } else {
                    let (_, twitch_name) = text.split_once(" ").expect("How did it panic, what happened? //Always is something here");
                    let mut twitch_name = twitch_name.to_string();
                    if twitch_name.starts_with("@") {
                        twitch_name.remove(0);
                    }
                    bungiename(&msg, &mut client, &self.conn, &twitch_name).await?;
                }
                
            }
            text if text.starts_with("!mod_addglobalcommand") => {
                let words: Vec<&str> = text.split_whitespace().collect();
                if words.len() > 2 {
                    let command = words[1];
                    let reply = words[2..].join(" ");
                    save_command(&self.conn, command, &reply, None).await;
                    client.privmsg(msg.channel(), &format!("Global Command !{} added.", command)).send().await?;
                } else {
                    client.privmsg(msg.channel(), "Usage: !mod_addglobalcommand <command> <response>").send().await?;
                }
            }
            text if text.starts_with("!mod_addcommand") && is_moderator(&msg, &mut client).await => {
                self.mod_addcommand(msg, client).await?;
            }
            text if text.starts_with("!mod_removecommand") && is_moderator(&msg, &mut client).await => {
                self.mod_removecommand(msg, client).await?;
            }
            text if text.starts_with("!") => {
                if let Ok(Some(reply)) = get_command_response(&self.conn, text, Some(msg.channel())).await {
                    send_message(msg, client, &reply).await?;
                }
            }
            &_ => {},
        } 
        Ok(())
    }

    async fn mod_addcommand(&mut self, msg: &tmi::Privmsg<'_>, client: &mut Client) -> Result<(), BotError> {
        let words: Vec<&str> = msg.text().split_whitespace().collect();
        if words.len() > 2 {
            let channel = msg.channel();
            let command = words[1];
            let reply = words[2..].join(" ");
            save_command(&self.conn, command, &reply, Some(channel)).await;
            send_message(msg, client, &format!("Command !{} added.", command)).await?;
        } else {
            send_message(msg, client, "Usage: !mod_addcommand <command> <response>").await?;
        }
        Ok(())    
    }

    async fn mod_removecommand(&mut self, msg: &tmi::Privmsg<'_>, client: &mut Client) -> Result<(), BotError> {
        let words: Vec<&str> = msg.text().split_whitespace().collect();
        let command = words[1];
        let reply;
        if remove_command(&self.conn, command).await {
            reply = format!("Command !{} removed.", command);
        } else {
            reply = format!("Command !{} doesn't exist.", command);
        }
        send_message(msg, client, &reply).await?;
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
                
                shared_state.lock().unwrap().add_stats(chat_message, run_count);
      
                let mut bot_state = bot_state.lock().await;
                match msg.text() {
                    text if text.starts_with("!open_queue") && is_moderator(&msg, &mut client).await => {
                        bot_state.conn.lock().await.execute("DELETE from queue", [])?;
                        bot_state.queue_open = true;
                        send_message(&msg, &mut client, "The queue is now open!").await?
                    }
                    text if text.starts_with("!close_queue") && is_moderator(&msg, &mut client).await => {
                        bot_state.queue_open = false;
                        send_message(&msg, &mut client, "The queue is now closed!").await?;
                    }
                    text if text.starts_with("!queue_len") && is_moderator(&msg, &mut client).await => {
                        let words:Vec<&str> = text.split_whitespace().collect();
                        if words.len() == 2 {
                            let lenght = words[1].to_owned();
                            bot_state.queue_len = lenght.parse().unwrap();
                            client.privmsg(msg.channel(), &format!("Queue lenght has been changed to {}", lenght)).send().await?;
                        } else {
                            client.privmsg(msg.channel(), "Are you sure you had the right command? In case !queue_len <queue lenght>").send().await?;
                        }
                    }
                    text if text.starts_with("!queue_size") && is_moderator(&msg, &mut client).await => {
                        let words:Vec<&str> = text.split_whitespace().collect();
                        if words.len() == 2 {
                            let lenght = words[1].to_owned();
                            bot_state.queue_teamsize = lenght.parse().unwrap();
                            client.privmsg(msg.channel(), &format!("Queue fireteam size has been changed to {}", lenght)).send().await?;
                        } else {
                            client.privmsg(msg.channel(), "Are you sure you had the right command? In case !queue_size <fireteam size>").send().await?;
                        }
                    }
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
                        bot_state.random(&msg, &mut client).await?;
                    }
                    _ => bot_state.non_queue_comms(&mut client, &msg).await?
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
//Find first message
fn is_bannable_link(text: &str) -> bool {
    if (text.to_ascii_lowercase().starts_with("cheap viewers on") || text.to_ascii_lowercase().starts_with("best viewers on") && text.contains(".")) || text.contains("Hello, sorry for bothering you. I want to offer promotion of your channel, viewers, followers, views, chat bots, etc...The price is lower than any competitor, the quality is guaranteed to be the best. Flexible and convenient order management panel, chat panel, everything is in your hands, a huge number of custom settings")  {
        true
    } else {
        false
    }
}


