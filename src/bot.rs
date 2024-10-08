use crate::{ 
    bot_commands::{announcment, ban_bots, bungiename, in_right_chat, is_follower, is_moderator, register_user, send_message, shoutout}, 
    database::{get_command_response, initialize_database_async, remove_command, save_command}, 
    models::{BotConfig, BotError, ChatMessage}, SharedState
};
use async_sqlite::Client as SqliteClient;
use dotenv::dotenv;
use rand::Rng;
use regex::Regex;
use serde::{Deserialize, Serialize};
use tmi::{Client, Event, SubOrResub, Tag};




use std::{env::var, sync::Arc, time::{self, SystemTime}};
use tokio::sync::Mutex;

pub const CHANNELS: &[&str] = &["#krapmatt,#nyc62truck"];

#[derive(Clone)]
pub struct BotState {
    oauth_token_bot: String,
    pub nickname: String,
    bot_id: String,
    pub x_api_key: String,
    pub queue_config: BotConfig
}

impl BotState {
    pub fn new() -> BotState {
        dotenv().ok();
        let oauth_token_bot = var("TWITCH_OAUTH_TOKEN_BOTT").expect("No bot oauth token"); 
        let nickname = var("TWITCH_BOT_NICK").expect("No bot name");   
        let bot_id = var("TWITCH_CLIENT_ID_BOT").expect("msg");
        let x_api_key = var("XAPIKEY").expect("No bungie api key");
        

        BotState { 
            oauth_token_bot: oauth_token_bot,
            nickname: nickname,
            bot_id: bot_id,
            x_api_key: x_api_key,
            queue_config: BotConfig::new()
        }
    }

    pub async fn client_builder(&mut self) -> Client {
        let credentials = tmi::Credentials::new(self.nickname.clone(), self.oauth_token_bot.clone());
        let mut client = tmi::Client::builder().credentials(credentials).connect().await.unwrap();
        client.join_all(CHANNELS).await.unwrap();
        client
    }
    
    async fn non_queue_comms(&mut self, mut client: &mut Client, msg: &tmi::Privmsg<'_>, conn: &SqliteClient, first_time: Option<&str>) -> Result<(), BotError> {
        match msg.text() {   
            
            text if text.to_ascii_lowercase().starts_with("!connect") && is_moderator(msg, client).await => {
                if let Some((_, channel)) = msg.text().split_once(" ") {
                    client.join(format!("#{}", channel)).await?;
                } else {
                    client.privmsg(msg.channel(), "You didn't write the channel to connect to").send().await?;
                }
            }
            
            text if text.to_ascii_lowercase().starts_with("!lurk") && in_right_chat(&msg).await => {
                send_message(&msg, &mut client, &format!("Thanks for the krapmaLurk {}! Be sure to leave the tab on low volume, or mute tab, to support stream krapmaHeart", msg.sender().name())).await?;
            }
            text if text.to_ascii_lowercase().contains("!so") && text.len() > 6 && in_right_chat(&msg).await => {
                so(msg, client).await?;
            }
            text if text.to_ascii_lowercase().starts_with("!total") => {
                self.total_raid_clears(msg, client, conn).await?;
            }
            text if text.to_ascii_lowercase().starts_with("!register") => {
                let reply;
                if let Some((_, bungie_name)) = msg.text().split_once(" ") {
                    reply = register_user(conn, &msg.sender().name(), bungie_name).await?;
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
                    reply = register_user(conn, &twitch_name, bungie_name).await?;
                } else {
                    reply = "You are a mod. . . || If you forgot use: !mod_register twitchname bungoname".to_string();
                }
                send_message(msg, client, &reply).await?;
            }
            text if is_bannable_link(text) && first_time == Some("1") => {
                ban_bots(&msg, &self.oauth_token_bot, self.bot_id.clone()).await;
                client.privmsg(msg.channel(), "We don't want cheap viewers, only expensive ones <3").send().await?;
            }
            text if text.starts_with("!bungiename") => {
                if text.trim_end().len() == 11 {
                    bungiename(&msg, &mut client, conn, msg.sender().name().to_string()).await?;
                } else {
                    let (_, twitch_name) = text.split_once(" ").expect("How did it panic, what happened? //Always is something here");
                    let mut twitch_name = twitch_name.to_string();
                    if twitch_name.starts_with("@") {
                        twitch_name.remove(0);
                    }
                    bungiename(&msg, &mut client, conn, twitch_name).await?;
                }
                
            }
            text if text.starts_with("!mod_addglobalcommand") => {
                let words: Vec<&str> = text.split_whitespace().collect();
                let reply = if words.len() > 2 {
                    let command = words[1].to_string();
                    let reply = words[2..].join(" ").to_string();
                    save_command(conn, command.clone(), reply, None).await;
                    format!("Global Command !{} added.", command)
                } else {
                    "Usage: !mod_addglobalcommand <command> <response>".to_string()
                };
                send_message(msg, client, &reply).await?;
            }
            text if text.starts_with("!mod_addcommand") && is_moderator(&msg, &mut client).await => {
                self.mod_addcommand(msg, client, conn).await?;
            }
            text if text.starts_with("!mod_removecommand") && is_moderator(&msg, &mut client).await => {
                self.mod_removecommand(msg, client, conn).await?;
            }
            text if text == "!mod_config" && is_moderator(msg, client).await => {
                let channel_name = msg.channel().replace("#", "");
                let config = BotConfig::load_config(&channel_name);
                let reply = format!("Queue: {} || Length: {} || Fireteam size: {}", config.open, config.len, config.teamsize);
                send_message(msg, client, &reply).await?;
            }
            text if text.starts_with("!") => {
                if let Ok(Some(reply)) = get_command_response(conn, text.to_string(), Some(msg.channel().to_string())).await {
                    send_message(msg, client, &reply).await?;
                }
            }
            
            &_ => {},
        } 
        Ok(())
    }

    async fn mod_addcommand(&mut self, msg: &tmi::Privmsg<'_>, client: &mut Client, conn: &SqliteClient) -> Result<(), BotError> {
        let words: Vec<&str> = msg.text().split_whitespace().collect();
        let reply = if words.len() > 2 {
            let channel = msg.channel().to_string();
            let command = words[1].to_string();
            let reply = words[2..].join(" ").to_string();
            save_command(conn, command.clone(), reply, Some(channel)).await;
            format!("Command !{} added.", command)
        } else {
            "Usage: !mod_addcommand <command> <response>".to_string()
        };
        send_message(msg, client, &reply).await?;
        Ok(())    
    }

    async fn mod_removecommand(&mut self, msg: &tmi::Privmsg<'_>, client: &mut Client, conn: &SqliteClient) -> Result<(), BotError> {
        let words: Vec<&str> = msg.text().split_whitespace().collect();
        let command = words[1];
        let reply;
        if remove_command(conn, command).await {
            reply = format!("Command !{} removed.", command);
        } else {
            reply = format!("Command !{} doesn't exist.", command);
        }
        send_message(msg, client, &reply).await?;
        Ok(())
    }
}

async fn so(msg: &tmi::Privmsg<'_>, client: &mut Client) -> Result<(), BotError> {
    if is_moderator(msg, client).await {
        let words:Vec<&str> = msg.text().split_ascii_whitespace().collect();
        let mut twitch_name = words[1].to_string();
        if twitch_name.starts_with("@") {
            twitch_name.remove(0);
        }
        send_message(&msg, client, &format!("Let's give a big Shoutout to https://www.twitch.tv/{} ! Make sure to check them out and give them a FOLLOW krapmaHeart", twitch_name)).await?;
    }
    Ok(())
}
//Timers/Counters
//Bungie api stuff - evade it
//TODO Match ! as the first thing so that ! is just for commands now could clean up code

//if begins with !
// if with queue
//  if with queue && moderator
//   if with queue && moderator && _len
//
pub async fn run_chat_bot(shared_state: Arc<std::sync::Mutex<SharedState>>) -> Result<(), BotError> {
    let bot_state = Mutex::new(BotState::new());

    let mut client = bot_state.lock().await.client_builder().await;
    
    let mut run_count = 0;
    let mut messeges = 0;
    
    let mut start_time = SystemTime::now();

    client.privmsg("#krapmatt", "Krapbott connected!").send().await?;
    let conn = initialize_database_async().await;
    loop {
        let msg = client.recv().await?;
        let first_time = msg.tag(Tag::FirstMsg);
        let a = msg.tags().map(|x| println!("Tag: {:?} str: {:?}", x.0, x.1));
        match msg.as_typed()? {
            tmi::Message::Privmsg(msg) => {
                
                let chat_message = ChatMessage {
                    channel: msg.channel().to_string(),
                    user: msg.sender().name().to_string(),
                    text: msg.text().to_string(),
                };
                
                shared_state.lock().unwrap().add_stats(chat_message, run_count);
      
                let mut bot_state = bot_state.lock().await;
                bot_state.queue_config  = BotConfig::load_config(&msg.channel().replace("#", ""));
                
                let mut config = bot_state.queue_config.clone();
                config.channel_id = Some(msg.channel().to_string());

                match msg.text() {
                    text if text.to_ascii_lowercase() == "!open_queue" => {
                        if is_moderator(&msg, &mut client).await {
                            config.open = true;
                            send_message(&msg, &mut client, "The queue is now open!").await?
                        }
                    }
                    text if text.to_ascii_lowercase() == "!close_queue" => {
                        if is_moderator(&msg, &mut client).await {
                            config.open = false;
                            send_message(&msg, &mut client, "The queue is now closed!").await?;
                        }
                    }
                    text if text.to_ascii_lowercase() == "!clear" => {
                        let channel = msg.channel().replace("#", "");
                        conn.conn(move |conn| Ok(conn.execute("DELETE from queue WHERE channel_id", [channel])?)).await?;
                    }
                    text if text.starts_with("!queue_len") => {
                        if is_moderator(&msg, &mut client).await {
                            let words:Vec<&str> = text.split_whitespace().collect();
                            if words.len() == 2 {
                                let length = words[1].to_owned();
                                config.len = length.parse().unwrap();
                                client.privmsg(msg.channel(), &format!("Queue length has been changed to {}", length)).send().await?;
                            } else {
                                client.privmsg(msg.channel(), "Are you sure you had the right command? In case !queue_len <queue length>").send().await?;
                            } 
                        }
                    }
                    text if text.starts_with("!queue_size") && is_moderator(&msg, &mut client).await => {
                        let words:Vec<&str> = text.split_whitespace().collect();
                        if words.len() == 2 {
                            let length = words[1].to_owned();
                            config.teamsize = length.parse().unwrap();
                            client.privmsg(msg.channel(), &format!("Queue fireteam size has been changed to {}", length)).send().await?;
                        } else {
                            client.privmsg(msg.channel(), "Are you sure you had the right command? In case !queue_size <fireteam size>").send().await?;
                        }
                    }
                    text if text.to_ascii_lowercase().starts_with("!join") && is_follower(&msg, &mut client, &bot_state.oauth_token_bot, bot_state.bot_id.clone()).await? => {
                        println!("{:?}", msg.text());
                        bot_state.handle_join(&msg, &mut client, &conn).await?;
                    }
                    text if text.starts_with("!test") => {
                        client.privmsg("#krapmatt", "!so krapmatt").send().await?;
                        
                    }
                    text if text.to_ascii_lowercase().starts_with("!next") && is_moderator(&msg, &mut client).await => {
                        run_count += 1;
                        bot_state.handle_next(&msg, &mut client, &conn).await?;
                    }
                    text if text.to_ascii_lowercase().starts_with("!remove") && is_moderator(&msg, &mut client).await => {
                        bot_state.handle_remove(&msg, &mut client, &conn).await?;
                    }
                    text if text.to_ascii_lowercase().starts_with("!pos") => {
                        bot_state.handle_pos(&msg, &mut client, &conn).await?;
                    }
                    text if text.to_ascii_lowercase().starts_with("!leave") => {
                        bot_state.handle_leave(&msg, &mut client, &conn).await?;
                    }
                    text if text.to_ascii_lowercase().starts_with("!queue") || text.starts_with("!list")=> {
                        bot_state.handle_queue(&msg, &mut client, &conn).await?;
                    }
                    text if text.to_ascii_lowercase().starts_with("!random") && is_moderator(&msg, &mut client).await => {
                        bot_state.random(&msg, &mut client, &conn).await?;
                    }
                    _ => bot_state.non_queue_comms(&mut client, &msg, &conn, first_time).await?
                    
                }
                if msg.channel() == "#krapmatt" {
                    messeges += 1;
                }
                config.save_config(&msg.channel().replace("#", ""));
            }
            tmi::Message::Reconnect => {
                client.reconnect().await?;
                client.join_all(CHANNELS).await?;
            }
            tmi::Message::Ping(ping) => {
                client.pong(&ping).await?;
            }
            tmi::Message::UserNotice(notice) => {
                println!("{:?}", notice);
                if notice.channel() == "#krapmatt" {
                    match notice.event() {
                        Event::Raid(raid) => {
                            if let Some(raider) = notice.sender() {
                                let bot_state = bot_state.lock().await;
                                shoutout(&bot_state.oauth_token_bot, bot_state.clone().bot_id, raider.id()).await;
                                client.privmsg("#krapmatt", 
                                    &format!("Let's give a BIG shoutout to https://www.twitch.tv/{:?} :krapmaHeart: They have raided us with {} people from their community! :krapmaStare: Please welcome them in :krapmaHeart:",
                                    raider.login(), raid.viewer_count())).send().await?;
                                
                                client.privmsg("#krapmatt", 
                                    &format!("They have raided us with {} people from their community! :krapmaStare: Please welcome them in :krapmaHeart:", 
                                    raid.viewer_count())).send().await?;
                            }
                            
                        }
                        Event::SubOrResub(sub) => {
                            let mut answer = String::new();

                            answer = format!("A new sub alert! :krapmaHeart: GOAT {:?} has just subbed", notice.sender());

                            if sub.is_resub() {
                                answer = format!("Thank you {:?} for the resub! :krapmaHeart: I appreciate your support! You've been supporting for {} months! :krapmaStare:", notice.sender(), sub.cumulative_months())
                            }

                            client.privmsg("#krapmatt", &answer).send().await?;
                        }
                        Event::SubGift(gift) => {
                            client.privmsg("#krapmatt", &format!("{:?} has gifted a sub to {:?}", notice.sender().unwrap(), gift.recipient())).send().await?;
                        }
                        _ => {}
                        
                    }
                }
            }
            _ => {}
            
        }
        
        //rendom choose from preset messages
        //Add those messages into a database in future

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

        
    }
}

lazy_static::lazy_static! {
    static ref CHEAP_VIEWERS_RE: Regex = Regex::new(r"cheap\s*viewers\s*on").unwrap();
    static ref BEST_VIEWERS_RE: Regex = Regex::new(r"best\s*viewers\s*on").unwrap();
    static ref PROMO_RE: Regex = Regex::new(r"hello\s*sorry\s*for\s*bothering\s*you\s*i\s*want\s*to\s*offer\s*promotion\s*of\s*your\s*channel\s*viewers\s*followers\s*views\s*chat\s*bots\s*etc\s*the\s*price\s*is\s*lower\s*than\s*any\s*competitor\s*the\s*quality\s*is\s*guaranteed\s*to\s*be\s*the\s*best\s*flexible\s*and\s*convenient\s*order\s*management\s*panel\s*chat\s*panel\s*everything\s*is\s*in\s*your\s*hands\s*a\s*huge\s*number\s*of\s*custom\s*settings").unwrap();
}

//Find first message
fn is_bannable_link(text: &str) -> bool {
    // Remove non-alphanumeric characters and convert to lowercase
    let cleaned_text: String = text.chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();

    // Check the conditions using precompiled regexes
    (CHEAP_VIEWERS_RE.is_match(&cleaned_text) || BEST_VIEWERS_RE.is_match(&cleaned_text) && text.contains(".")) ||
        PROMO_RE.is_match(&cleaned_text)
}


