use crate::{ 
    bot_commands::{ban_bots, bungiename, handle_join, handle_leave, handle_next, handle_pos, 
        handle_queue, handle_remove, is_follower, is_moderator, is_valid_bungie_name, register_user, simple_command}, 
    database::{get_command_response, remove_command, save_command, COMMAND_TABLE, QUEUE_TABLE}, initialize_database, ChatMessage, SharedState
};
use dotenv::dotenv;
use egui::TextBuffer;
use rusqlite::Connection;
use tmi::Client;

use std::{env::var, sync::Arc};
use tokio::sync::Mutex;

pub const CHANNELS: &[&str] = &["#krapmatt"];

pub struct BotState {
    queue_open: bool,
    oauth_token_bot: String,
    nickname: String,
    bot_id: String,
    conn_command: Connection,
    queue_len: usize,
    queue_teamsize: usize,

}

impl BotState {
    pub fn new() -> BotState {
        dotenv().ok();
        let oauth_token_bot = var("TWITCH_OAUTH_TOKEN_BOTT").expect("No bot oauth token"); 
        let nickname = var("TWITCH_BOT_NICK").expect("No bot name");   
        let bot_id = var("TWITCH_CLIENT_ID_BOT").expect("msg");
        let conn_command = initialize_database("commands.db", COMMAND_TABLE).unwrap();
        BotState { 
            queue_open: false,
            oauth_token_bot: oauth_token_bot,
            nickname: nickname,
            bot_id: bot_id,
            conn_command: conn_command,
            queue_len: 30,
            queue_teamsize: 5,
        }
    }

    pub async fn client_builder(&mut self) -> Client {
        let credentials = tmi::Credentials::new(self.nickname.clone(), self.oauth_token_bot.clone());
        let mut client = tmi::Client::builder().credentials(credentials).connect().await.unwrap();
        client.join_all(CHANNELS).await;
        client
    }
}
//Add command command for mods
//Timers/Counters
// !bungiename
//Bungie api stuff - evade it
pub async fn run_chat_bot(bot_state: Arc<Mutex<BotState>>, shared_state: Arc<std::sync::Mutex<SharedState>>,) -> anyhow::Result<()> {
    let mut client = bot_state.lock().await.client_builder().await;

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
                  
                let conn_queue = Mutex::new(initialize_database("queue.db", QUEUE_TABLE).unwrap());
      
                let mut bot_state = bot_state.lock().await;
                match msg.text() {
                    text if text.starts_with("!open_queue") && is_moderator(&msg, &mut client).await => {
                        conn_queue.lock().await.execute("DELETE from queue", [])?;
                        bot_state.queue_open = true;
                        client.privmsg(msg.channel(), "The queue is now open!").send().await?;
                    }
                    text if text.starts_with("!close_queue") && is_moderator(&msg, &mut client).await => {
                        bot_state.queue_open = false;
                        client.privmsg(msg.channel(), "The queue is now closed!").send().await?;
                    }
                    text if bot_state.queue_open => match text {
                        text if text.starts_with("!join") && is_follower(&msg, &mut client, &bot_state.oauth_token_bot, bot_state.bot_id.clone()).await => {
                            handle_join(&msg, &mut client, bot_state.queue_len, &conn_queue).await?;
                        }
                        text if text.starts_with("!next") && is_moderator(&msg, &mut client).await => {
                            handle_next(&msg, &mut client, bot_state.queue_teamsize, &conn_queue).await?;
                        }
                        text if text.starts_with("!remove") && is_moderator(&msg, &mut client).await => {
                            handle_remove(&msg, &mut client, &conn_queue).await?;
                        }
                        text if text.starts_with("!pos") => {
                            handle_pos(&msg, &mut client, bot_state.queue_len, &conn_queue).await?;
                        }
                        text if text.starts_with("!leave") => {
                            handle_leave(&msg, &mut client, &conn_queue).await?;
                        }
                        text if text.starts_with("!queue") => {
                            handle_queue(&msg, &mut client, &conn_queue).await?;
                        }
                        _ => {}
                    }
                    text if text.starts_with("!join") || text.starts_with("!next") || text.starts_with("!remove") || text.starts_with("!pos") || text.starts_with("!leave") || text.starts_with("!queue") => {
                        client.privmsg(msg.channel(), "The queue is currently closed!").send().await?;
                    }
                    text if text.starts_with("!lurk") => {
                        simple_command(&msg, &mut client, &format!("Thanks for the lurk {}. I'll appreciate if you leave tab open <3", msg.sender().name())).await?;
                    }
                    text if text.starts_with("!register") => {
                        register_user(&msg, &mut client).await;
                    }
                    text if text.starts_with("Cheap viewers on u.to/") || text.starts_with("Best viewers on cutt.ly/") => {
                        ban_bots(&msg, &mut client, &bot_state.oauth_token_bot, bot_state.bot_id.clone()).await;
                        client.privmsg(msg.channel(), "We don't want cheap viewers, only expensive ones <3").send().await?;
                    }
                    text if text.starts_with("!bungiename") => {
                        if text.trim_end().len() == 11 {
                            bungiename(&msg, &mut client, &msg.sender().name()).await?;
                        } else {
                            let (_, twitch_name) = text.split_once(" ").expect("How did it panic, what happened? //Always is something here");
                            let mut twitch_name = twitch_name.to_string();
                            if twitch_name.starts_with("@") {
                                twitch_name.remove(0);
                            }
                            bungiename(&msg, &mut client, &twitch_name).await?;
                        }

                    }
                    text if text.starts_with("!mod_addcommand") && is_moderator(&msg, &mut client).await => {
                        let words: Vec<&str> = text.split_whitespace().collect();
                        if words.len() > 2 {
                            let command = words[1];
                            let reply = words[2..].join(" ");
                            save_command(&bot_state.conn_command, command, &reply);
                            client.privmsg(msg.channel(), &format!("Command !{} added.", command)).send().await?;
                        } else {
                            client.privmsg(msg.channel(), "Usage: !addcommand <command> <response>").send().await?;
                        }
                    }
                    text if text.starts_with("!mod_removecommand") && is_moderator(&msg, &mut client).await => {
                        let words: Vec<&str> = text.split_whitespace().collect();
                        let command = words[1];
                        if remove_command(&bot_state.conn_command, command) {
                            client.privmsg(msg.channel(), &format!("Command !{} removed.", command)).send().await?;
                        } else {
                            client.privmsg(msg.channel(), &format!("Command !{} doesn't exist.", command)).send().await?;
                        }
                    }
                    text if text.starts_with("!") => {
                        if let Ok(Some(reply)) = get_command_response(&bot_state.conn_command, text) {
                            client.privmsg(msg.channel(), &reply).send().await?;
                        }
                    }
                    _ => {}
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

