
use dotenv::dotenv;
use rand::{random, Rng};
use regex::Regex;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::json;
use twitch_irc::{login::{LoginCredentials, StaticLoginCredentials}, message::{AsRawIRC, PrivmsgMessage, ServerMessage}, transport::Transport, ClientConfig, SecureTCPTransport, TwitchIRCClient};

use std::{env::var, fs::File, io::{Read, Write}, sync::Arc, time::{self, SystemTime}};
use tokio::sync::Mutex;

use crate::{bot_commands::{announcment, ban_bots, bungiename, in_right_chat, is_follower, is_moderator, register_user}, database::{get_command_response, initialize_database, remove_command, save_command}, models::{BotError, ChatMessage, SharedState}};

pub const CHANNELS: &[&str] = &["krapmatt"];

#[derive(Serialize, Deserialize)]
pub struct BotConfig {
    pub open: bool,
    pub len: usize,
    pub teamsize: usize,
}

impl BotConfig {
    pub fn new() -> Self {
        BotConfig {
            open: false,
            len: 0,
            teamsize: 0,
        }
    }
    
    pub fn load_config() -> Self {
        let mut file = File::open("Config.json").expect("Failed to load config. Create file Config.json");
        let mut string = String::new();
        let _ = file.read_to_string(&mut string);
        let bot_config: BotConfig = serde_json::from_str(&string).expect("Always will be correct format");
        bot_config
    }

    pub fn save_config(&self) {
        let content = serde_json::to_string_pretty(self).expect("Json serialization is wrong? Check save_config function");
        let mut file = File::create("Config.json").expect("Still the config file doesnt exist?");
        file.write_all(content.as_bytes());
        
    }
}
pub struct BotState {
    oauth_token_bot: String,
    pub nickname: String,
    bot_id: String,
    pub x_api_key: String,
    pub conn: Mutex<Connection>,
    pub queue_config: BotConfig
}

impl BotState {
    pub fn new() -> BotState {
        dotenv().ok();
        let oauth_token_bot = var("TWITCH_OAUTH_TOKEN_BOTT").expect("No bot oauth token"); 
        let nickname = var("TWITCH_BOT_NICK").expect("No bot name");   
        let bot_id = var("TWITCH_CLIENT_ID_BOT").expect("msg");
        let x_api_key = var("XAPIKEY").expect("No bungie api key");
        let conn = Mutex::new(initialize_database());
        
        

        BotState { 
            oauth_token_bot: oauth_token_bot,
            nickname: nickname,
            bot_id: bot_id,
            conn: conn,
            x_api_key: x_api_key,
            queue_config: BotConfig::load_config()
        }
    }

    async fn non_queue_comms<T, L>(&mut self, client: TwitchIRCClient<T, L>, msg: PrivmsgMessage) -> Result<(), BotError> where T: Transport, L: LoginCredentials{
        let message_text = msg.message_text.clone();
        let channel_login = msg.channel_login.clone();
        match message_text.to_ascii_lowercase().as_str() {   
            text if text.to_ascii_lowercase().starts_with("!connect") && is_moderator(msg.clone(), &client).await => {
                if let Some((_, channel)) = msg.message_text.split_once(" ") {
                    client.join(channel.to_string());
                } else {
                    client.say(channel_login, "You didn't write the channel to connect to".to_string()).await;
                }
            }
            
            text if text.to_ascii_lowercase().starts_with("!lurk") && in_right_chat(&msg).await => {
                client.say(msg.channel_login, format!("Thanks for the krapmaLurk {}! Be sure to leave the tab on low volume, or mute tab, to support stream krapmaHeart", msg.sender.name)).await;
            }
            text if text.to_ascii_lowercase().starts_with("!so") && is_moderator(msg.clone(), &client).await && in_right_chat(&msg).await => {
                let words:Vec<&str> = msg.message_text.split_ascii_whitespace().collect();
                let mut twitch_name = words[1].to_string();
                if twitch_name.starts_with("@") {
                    twitch_name.remove(0);
                }
                client.say(channel_login.clone(), format!("Let's give a big Shoutout to https://www.twitch.tv/{}!. Make sure to check them out and give them a FOLLOW krapmaHeart", twitch_name)).await;
                client.privmsg(channel_login, format!("/shoutout {}", twitch_name)).await;
            }
            text if text.to_ascii_lowercase().starts_with("!total") => {
                self.total_raid_clears(msg, client).await?;
            }
            text if text.to_ascii_lowercase().starts_with("!register") => {
                let reply;
                if let Some((_, bungie_name)) = msg.message_text.split_once(" ") {
                    reply = register_user(&self.conn, &msg.sender.name, bungie_name).await?;
                } else {
                    reply = "Invalid command format! Use: !register bungiename#1234".to_string();
                }
                client.say(msg.channel_login, reply).await;
            }
            text if text.to_ascii_lowercase().starts_with("!mod_register") && is_moderator(msg.clone(), &client).await => {
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
                client.say(msg.clone().channel_login, reply).await;
            }
            text if is_bannable_link(&text) => {
                ban_bots(msg.clone(), &self.oauth_token_bot, self.bot_id.clone()).await;
                client.say(msg.clone().channel_login, "We don't want cheap viewers, only expensive ones <3".to_string()).await;
            }
            text if text.starts_with("!bungiename") => {
                if text.trim_end().len() == 11 {
                    bungiename(msg.clone(), client, &self.conn, &msg.clone().sender.name).await?;
                } else {
                    let (_, twitch_name) = text.split_once(" ").expect("How did it panic, what happened? //Always is something here");
                    let mut twitch_name = twitch_name.to_string();
                    if twitch_name.starts_with("@") {
                        twitch_name.remove(0);
                    }
                    bungiename(msg, client, &self.conn, &twitch_name).await?;
                }
                
            }
            text if text.starts_with("!mod_addglobalcommand") => {
                let words: Vec<&str> = text.split_whitespace().collect();
                let message;
                if words.len() > 2 {
                    let command = words[1];
                    let reply = words[2..].join(" ");
                    save_command(&self.conn, command, &reply, None).await;
                    message = format!("Global Command !{} added.", command);
                    
                } else {
                    message = "Usage: !mod_addglobalcommand <command> <response>".to_string();
                }
                client.say(msg.clone().channel_login, message).await;
            }
            text if text.starts_with("!mod_addcommand") && is_moderator(msg.clone(), &client).await => {
                self.mod_addcommand(&msg, client).await?;
            }
            text if text.starts_with("!mod_removecommand") && is_moderator(msg.clone(), &client).await => {
                self.mod_removecommand(msg, client).await?;
            }
            text if text == "!mod_config" && is_moderator(msg.clone(), &client).await => {
                let a = BotConfig::load_config();
                let reply = format!("Queue: {} || Lenght: {} || Fireteam size: {}", a.open, a.len, a.teamsize);
                client.say(msg.clone().channel_login, reply).await;
            }
            text if text.starts_with("!") => {
                if let Ok(Some(reply)) = get_command_response(&self.conn, &text, Some(&msg.clone().channel_login)).await {
                    client.say(msg.clone().channel_login, reply).await;
                }
            }
            
            _ => {},
        } 
        Ok(())
    }

    async fn mod_addcommand<T, L>(&mut self, msg: &PrivmsgMessage, client: TwitchIRCClient<T, L>) -> Result<(), BotError> where T: Transport, L: LoginCredentials {
        let words: Vec<&str> = msg.message_text.split_whitespace().collect();
        let message;
        if words.len() > 2 {
            let channel = &msg.channel_login;
            let command = words[1];
            let reply = words[2..].join(" ");
            save_command(&self.conn, command, &reply, Some(&channel)).await;
            message= format!("Command !{} added.", command);
        } else {
            message = "Usage: !mod_addcommand <command> <response>".to_string();
        }
        client.say(msg.channel_login.clone(), message).await;
        Ok(())    
    }

    async fn mod_removecommand<T, L> (&mut self, msg: PrivmsgMessage, client: TwitchIRCClient<T, L>) -> Result<(), BotError> where T: Transport, L: LoginCredentials {
        let words: Vec<&str> = msg.channel_login.split_whitespace().collect();
        let command = words[1];
        let reply;
        if remove_command(&self.conn, command).await {
            reply = format!("Command !{} removed.", command);
        } else {
            reply = format!("Command !{} doesn't exist.", command);
        }
        client.say(msg.channel_login, reply).await;
        Ok(())
    }
}
//Timers/Counters
//Bungie api stuff - evade it

pub async fn run_chat_bot(shared_state: Arc<std::sync::Mutex<SharedState>>) -> Result<(), BotError> {
    let bot_state = Mutex::new(BotState::new());


    let config = ClientConfig::new_simple(StaticLoginCredentials::new(bot_state.lock().await.nickname, Some(bot_state.lock().await.oauth_token_bot)));
    let (mut msg, client) = TwitchIRCClient::<SecureTCPTransport, StaticLoginCredentials>::new(config);

    client.join("krapmatt".to_string()).unwrap();

    let mut run_count = 0;
    let mut messeges = 0;
    
    let mut start_time = SystemTime::now();
    let mut time: SystemTime;

    
    
        loop {
            match msg.recv().await {
                ServerMessage::Privmsg(msg) => {
                    println!("#{} {} {}", msg.channel_login, msg.sender.name, msg.message_text);
                    let chat_message = ChatMessage {
                        channel: msg.channel_login,
                        user: msg.sender.name,
                        text: msg.message_text,
                    };
                    
                    shared_state.lock().unwrap().add_stats(chat_message, run_count);
                    let mut bot_state = bot_state.lock().await;
                    match msg.message_text {
                        text if text.to_ascii_lowercase() == "!open_queue" && is_moderator(msg, &client).await => {
                            bot_state.conn.lock().await.execute("DELETE from queue", []);
                            bot_state.queue_config.open = true;
                            client.say(msg.channel_login, "The queue is now open!".to_string()).await;
                        }
                        text if text.to_ascii_lowercase() == "!close_queue" && is_moderator(msg, &client).await => {
                            bot_state.queue_config.open = false;
                            client.say(msg.channel_login, "The queue is now closed!".to_string()).await;
                        }
                        text if text.starts_with("!queue_len") && is_moderator(msg, &client).await => {
                            let reply;
                            let words:Vec<&str> = text.split_whitespace().collect();
                            if words.len() == 2 {
                                let lenght = words[1].to_owned();
                                bot_state.queue_config.len = lenght.parse().unwrap();
                                reply = format!("Queue lenght has been changed to {}", lenght);
                            } else {
                                reply = "Are you sure you had the right command? In case !queue_len <queue lenght>".to_owned();
                            }
                            client.say(msg.channel_login, reply).await;
                        }
                        text if text.starts_with("!queue_size") && is_moderator(msg, &client).await => {
                            let words:Vec<&str> = text.split_whitespace().collect();
                            let reply;
                            if words.len() == 2 {
                                let lenght = words[1].to_owned();
                                bot_state.queue_config.teamsize = lenght.parse().unwrap();
                                reply = format!("Queue fireteam size has been changed to {}", lenght);
                            } else {
                                reply = "Are you sure you had the right command? In case !queue_size <fireteam size>".to_owned();
                            }
                            client.say(msg.channel_login, reply);
                        }
                        text if text.to_ascii_lowercase().starts_with("!join") && is_follower(msg, client, &bot_state.oauth_token_bot, bot_state.bot_id.clone()).await.unwrap() => {
                            bot_state.handle_join(msg, client).await;
                        }
                        text if text.to_ascii_lowercase().starts_with("!next") && is_moderator(msg, &client).await => {
                            run_count += 1;
                            bot_state.handle_next(msg, client).await;
                        }
                        text if text.to_ascii_lowercase().starts_with("!remove") && is_moderator(msg, &client).await => {
                            bot_state.handle_remove(msg, client).await;
                        }
                        text if text.to_ascii_lowercase().starts_with("!pos") => {
                            bot_state.handle_pos(msg, client).await;
                        }
                        text if text.to_ascii_lowercase().starts_with("!leave") => {
                            bot_state.handle_leave(msg, client).await;
                        }
                        text if text.to_ascii_lowercase().starts_with("!queue") || text.starts_with("!list")=> {
                            bot_state.handle_queue(msg, client).await;
                        }
                        text if text.to_ascii_lowercase().starts_with("!random") && is_moderator(msg, &client).await => {
                            bot_state.random(msg, client).await;
                        }
                        _ => match bot_state.non_queue_comms(client, msg).await {
                            Ok(_) => return Ok(()),
                            Err(e) => client.say(msg.channel_login, e.to_string()).await.expect("Huh?")
                        },
                    }
                    messeges += 1;
                }

               _ => {}
            }
        }
    
            
    bot_state.lock().await.queue_config.save_config();
    
    //rendom choose from preset messages
    //Add those messages into a database in future
    println!("Differenct in time {:?}", start_time.elapsed());
    if start_time.elapsed().unwrap() > time::Duration::from_secs(800) && messeges >= 10 {
        let bot_state = bot_state.lock().await;
        let mut rand = rand::thread_rng();
        let ch = rand.gen_range(1..4);
        let mut mess = String::new(); 
        
        if ch == 1 {
            mess = "Join my discord krapmaStare : https://discord.gg/jJMwaetjeu".to_string();
        } else if ch == 2 {
            mess = "Don't forget to follow, if you enjoy it here! Also krapmaLurk is greatly appreciated krapmaHeart".to_string();
        } else if ch == 3 {
            mess = "If you need any help with dungeons. Try asking in chat! I just might be able to help you krapmaHeart".to_string();
        } else if ch == 4 {
            mess = "If you need any crota yeets just let me know. If I have a cp i will get it for you. <3".to_string();
        }
        
        announcment("216105918", "1091219021",&bot_state.oauth_token_bot , bot_state.bot_id.to_string(), mess).await?;
        
        messeges = 0;
        start_time = SystemTime::now();
    }


        
    
    
    Ok(())
}
//Find first message
fn is_bannable_link(text: &str) -> bool {
    // Remove non-alphanumeric characters and convert to lowercase
    let cleaned_text: String = text.chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();

    // Compile regex for matching phrases
    let cheap_viewers_re = Regex::new(r"cheap\s*viewers\s*on").unwrap();
    let best_viewers_re = Regex::new(r"best\s*viewers\s*on").unwrap();
    let promo_re = Regex::new(r"hello\s*sorry\s*for\s*bothering\s*you\s*i\s*want\s*to\s*offer\s*promotion\s*of\s*your\s*channel\s*viewers\s*followers\s*views\s*chat\s*bots\s*etc\s*the\s*price\s*is\s*lower\s*than\s*any\s*competitor\s*the\s*quality\s*is\s*guaranteed\s*to\s*be\s*the\s*best\s*flexible\s*and\s*convenient\s*order\s*management\s*panel\s*chat\s*panel\s*everything\s*is\s*in\s*your\s*hands\s*a\s*huge\s*number\s*of\s*custom\s*settings").unwrap();

    // Check the conditions
    if (cheap_viewers_re.is_match(&cleaned_text) || best_viewers_re.is_match(&cleaned_text) && text.contains(".")) ||
        promo_re.is_match(&cleaned_text) {
        true
    } else {
        false
    }
}


