use crate::{ 
    bot_commands::{announcment, ban_bots, send_message, shoutout}, commands::create_command_dispatcher, database::{get_command_response, initialize_database_async}, models::{has_permission, BotConfig, BotError}, SharedState
};
use dotenv::dotenv;
use rand::Rng;
use regex::Regex;
use tmi::{Client, Event, Ritual, Tag};

use std::{borrow::BorrowMut, env::var, sync::Arc, time::{self, SystemTime}};
use tokio::sync::Mutex;

pub const CHANNELS: &[&str] = &["#krapmatt,#nyc62truck,#therayii,#samoan_317"];


#[derive(Clone)]
pub struct BotState {
    pub oauth_token_bot: String,
    pub nickname: String,
    pub bot_id: String,
    pub x_api_key: String,
    pub first_time_tag: Option<String>,
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
            first_time_tag: None,
            queue_config: BotConfig::new()
        }
    }

    pub async fn client_builder(&mut self) -> Client {
        let credentials = tmi::Credentials::new(self.nickname.clone(), self.oauth_token_bot.clone());
        let mut client = tmi::Client::builder().credentials(credentials).connect().await.unwrap();
        client.join_all(CHANNELS).await.unwrap();
        client
    }
}







//Timers/Counters
//Bungie api stuff - evade it
pub async fn run_chat_bot(shared_state: Arc<std::sync::Mutex<SharedState>>) -> Result<(), BotError> {
    let bot_state = Mutex::new(BotState::new());

    let mut messeges = 0;
    let mut run_count = 0;

    let mut start_time = SystemTime::now();
    let client = Arc::new(Mutex::new(bot_state.lock().await.client_builder().await));

    let conn = initialize_database_async().await;
    let command_dispatcher = create_command_dispatcher();
    loop {
        let irc_msg = client.lock().await.recv().await?;
        let first_time = irc_msg.tag(Tag::FirstMsg).map(|x| x.to_string());
    
        match irc_msg.as_ref().as_typed()? {
            tmi::Message::Privmsg(msg) => {
                let mut bot_state = bot_state.lock().await;
                bot_state.first_time_tag = first_time.clone();
                bot_state.queue_config = BotConfig::load_config(&msg.channel().replace("#", ""));
                
                let mut config = bot_state.queue_config.clone();
                config.channel_id = Some(msg.to_owned().channel().to_string());

                let bot_state_arc = Arc::new(Mutex::new(bot_state.to_owned()));
                
                if msg.text().starts_with("!next") {
                    run_count += 1;
                    shared_state.lock().unwrap().add_stats(run_count);
                }
                
                if is_bannable_link(msg.text()) && first_time == Some("1".to_string()) {
                    ban_bots(&msg, &bot_state.oauth_token_bot, bot_state.bot_id.clone()).await;
                    client.lock().await.privmsg(msg.channel(), "We don't want cheap viewers, only expensive ones <3").send().await?;
                }

                if msg.text().starts_with("!") {
                    let command = msg.text().split_whitespace().next().unwrap_or_default().to_string().to_lowercase();
                    if let Some(cmd) = command_dispatcher.get(&command) {
                        if cmd.channels.is_empty() || cmd.channels.contains(&msg.channel().to_string()) {
                            let msg_clone = msg.clone().into_owned(); 
                            let conn_clone = conn.clone(); 
                            let client_arc = Arc::clone(&client);

                            if has_permission(&msg, Arc::clone(&client), cmd.permission).await {
                                // Execute the command handler if permission is granted
                                (cmd.handler)(msg_clone, client_arc, conn_clone, bot_state_arc).await?;
                            }
                        }
                    } else {
                        if let Ok(Some(reply)) = get_command_response(&conn, msg.text().to_string().to_ascii_lowercase(), Some(msg.channel().to_string())).await {
                            send_message(&msg, client.lock().await.borrow_mut(), &reply).await?;
                        }
                    }
                }
                if msg.channel() == "#krapmatt" {
                    messeges += 1;
                }
            }
            tmi::Message::Reconnect => {
                let mut client = client.lock().await;
                client.reconnect().await?;
                client.join_all(CHANNELS).await?;
            }
            tmi::Message::Ping(ping) => {
                client.lock().await.pong(&ping).await?;
            }
            tmi::Message::UserNotice(notice) => {
                if notice.channel() == "#krapmatt" {
                    match notice.event() {
                        Event::Raid(raid) => {
                            if let Some(raider) = notice.sender() {
                                let bot_state = bot_state.lock().await;
                                shoutout(&bot_state.oauth_token_bot, bot_state.clone().bot_id, raider.id()).await;
                                let mut client = client.lock().await;
                                client.privmsg("#krapmatt", 
                                    &format!("Let's give a BIG shoutout to https://www.twitch.tv/{:?} krapmaHeart",
                                    raider.login())).send().await?;
                                
                                client.privmsg("#krapmatt", 
                                    &format!("They have raided us with {} people from their community! krapmaStare Please welcome them in krapmaHeart", 
                                    raid.viewer_count())).send().await?;
                            }
                        }
                        Event::SubOrResub(sub) => {
                            let mut answer = String::new();

                            answer = format!("A new sub alert! krapmaHeart GOAT {:?} has just subbed", notice.sender().unwrap().name());

                            if sub.is_resub() {
                                answer = format!("Thank you {:?} for the resub! krapmaHeart I appreciate your support! You've been supporting for {} months! krapmaStare", notice.sender().unwrap().name(), sub.cumulative_months())
                            }

                            client.lock().await.borrow_mut().privmsg("#krapmatt", &answer).send().await?;
                        }
                        Event::SubGift(gift) => {
                            client.lock().await.borrow_mut().privmsg("#krapmatt", &format!("{:?} has gifted a sub to {:?}", notice.sender().unwrap().name(), gift.recipient())).send().await?;
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


