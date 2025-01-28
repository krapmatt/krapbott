use crate::{ 
    bot_commands::{announcement, ban_bots, get_twitch_user_id, is_channel_live, send_message, shoutout}, commands::create_command_dispatcher, 
    database::{get_command_response, initialize_database_async}, models::{has_permission, AnnouncementState, BotConfig, BotError},
};
use async_sqlite::rusqlite::{params, OptionalExtension};
use dotenv::dotenv;
use rand::{thread_rng, Rng};
use regex::Regex;
use tmi::{Client, Tag};

use std::{borrow::BorrowMut, collections::{HashMap, HashSet}, env::var, sync::Arc, time::{self, Duration, Instant, SystemTime}};
use tokio::sync::{mpsc::{self, Receiver, UnboundedReceiver}, Mutex};


#[derive(Clone)]
pub struct BotState {
    pub oauth_token_bot: String,
    pub nickname: String,
    pub bot_id: String,
    pub x_api_key: String,
    pub first_time_tag: Option<String>,
    pub config: BotConfig,
    pub streaming_together: HashMap<String, HashSet<String>>,
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
            config: BotConfig::load_config(),
            streaming_together: HashMap::new(),
        }
    }

    pub async fn client_builder(&mut self) -> Client {
        let credentials = tmi::Credentials::new(self.nickname.clone(), self.oauth_token_bot.clone());
        let mut client = tmi::Client::builder().credentials(credentials).connect().await.unwrap();
        println!("{:?}", self.clone().config.channels.into_keys().collect::<Vec<String>>());
        client.join_all(self.clone().config.channels.into_keys().collect::<Vec<String>>()).await.unwrap();
        client
    }
    pub async fn client_channel(&mut self, channel: String) -> Client {
        let credentials = tmi::Credentials::new(self.nickname.clone(), self.oauth_token_bot.clone());
        let mut client = tmi::Client::builder().credentials(credentials).connect().await.unwrap();
        client.join(channel).await.unwrap();
        client
    }
}

pub async fn bot_task(channel: String, state: Arc<Mutex<BotState>>, conn: async_sqlite::Client) -> Result<(), BotError> {
    let client = {
        let mut bot_state = state.lock().await;
        Arc::new(Mutex::new(bot_state.client_channel(channel.clone()).await))
    };
    let mut channel_id_map: HashMap<String, String> = HashMap::new();
     //ADD CHANNEL ID MAP TO BOTSTATE       
    let channels = BotConfig::load_config().channels.into_keys().collect::<Vec<String>>();
    
    for mut channel in channels {
        channel.remove(0);
        let id = get_twitch_user_id(&channel).await?;
        channel_id_map.insert(id, format!("#{}", channel));
    }
    let bot_state_clone = Arc::clone(&state);
    let conn_clone = conn.clone();
    tokio::spawn(async move {
        let channel = channel.strip_prefix("#").unwrap_or(&channel).to_string();
        let id = get_twitch_user_id(&channel).await.unwrap();
        if let Err(e) = start_annnouncement_scheduler(bot_state_clone, id, channel.clone(), conn_clone).await {
            eprintln!("Announcement error: {}", e);
        }
    });

    loop {
        let irc_msg = client.lock().await.recv().await?;
        if let Some(source_room_id) = irc_msg.tags().find(|(key, _)| *key == "source-room-id").map(|(_, value)| value) {
            //Streamer doesnt own krapbott, skip messages from that channel
            if !channel_id_map.contains_key(source_room_id) {
                continue;
            }
            if let Some(room_id) = irc_msg.tags().find(|(key, _)| *key == "room-id").map(|(_, value)| value) {
                if room_id != source_room_id {
                    continue;
                }
            }
        }
        match irc_msg.as_ref().as_typed()? {
            tmi::Message::Privmsg(msg) => {
                let first_time = irc_msg.tag(Tag::FirstMsg).map(|x| x.to_string());
                if is_bannable_link(msg.text()) && first_time == Some("1".to_string()) {
                    ban_bots(&msg, &state.lock().await.oauth_token_bot, state.lock().await.bot_id.clone()).await;
                    client.lock().await.privmsg(msg.channel(), "We don't want cheap viewers, only expensive ones <3").send().await?;
                }
                handle_privmsg(msg.clone(), Arc::clone(&state), conn.clone(), Arc::clone(&client)).await?;
            }
            tmi::Message::Reconnect => {
                let mut client = client.lock().await;
                client.reconnect().await?;
            }
            tmi::Message::Ping(ping) => {
                client.lock().await.pong(&ping).await?;
            }
            _ => {}
        }
    }
}


pub async fn manage_channels(state: Arc<Mutex<BotState>>, config: Arc<Mutex<BotConfig>>, conn: async_sqlite::Client) {
    let mut tasks: HashMap<String, tokio::task::JoinHandle<()>> = HashMap::new();
    loop {
        // Periodically check for new channels in the configuration
        let bot_config = config.lock().await;

        for (channel, _) in &bot_config.channels {
            if !tasks.contains_key(channel) {
                // Spawn a new bot task for this channel
                let channel_clone = channel.clone();
                let state_clone = Arc::clone(&state);
                let conn_clone = conn.clone();

                let handle = tokio::spawn(async move {
                    bot_task(channel_clone, state_clone, conn_clone).await;
                });

                tasks.insert(channel.clone(), handle);
            }
        }
        
        // Clean up tasks for removed channels
        tasks.retain(|channel, handle| {
            if !bot_config.channels.contains_key(channel) {
                handle.abort();
                return false;
            }
            true
        });
        tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
    }
}

pub async fn run_chat_bot() -> Result<(), BotError> {    
    let config = Arc::new(Mutex::new(BotConfig::load_config()));
    let bot_state = Arc::new(Mutex::new(BotState::new()));
    let conn = initialize_database_async().await;

    manage_channels(Arc::clone(&bot_state), config, conn).await;
        
    Ok(())
}

pub async fn handle_obs_message(channel_id: String, command: String) -> Result<(), BotError> {
    let conn = initialize_database_async().await;
    let mut bot_state = BotState::new();
    let mut client = bot_state.client_builder().await;

    println!("Processing OBS message: {} -> {}", channel_id, command);
    if command == "next" {
        let reply = bot_state
            .handle_next(format!("#{}", channel_id), &conn)
            .await
            .unwrap_or_else(|e| format!("Next failed: {}", e));

        client.privmsg(&format!("#{}", channel_id), &reply).send().await?;
    }

    Ok(())
}

async fn handle_privmsg(msg: tmi::Privmsg<'_>, bot_state: Arc<Mutex<BotState>>, conn: async_sqlite::Client, client: Arc<Mutex<Client>>) -> Result<(), BotError> {
    let mut locked_state = bot_state.lock().await;
    
    locked_state.config = BotConfig::load_config();
    if msg.text().starts_with("!pos") && (msg.sender().login().to_ascii_lowercase() == "thatjk" || msg.channel() == "#samoan_317") {
        let number = thread_rng().gen_range(1..1000);
        send_message(&msg, client.lock().await.borrow_mut(), &format!("{} you are {}% PoS krapmaHeart",msg.sender().name().to_string(), number)).await?;
    }
    let command_dispatcher = create_command_dispatcher(&locked_state.config, msg.channel());
    
    drop(locked_state);
    if msg.text().starts_with("!") {
        let command = msg.text().split_whitespace().next().unwrap_or_default().to_string().to_lowercase();
        if let Some(cmd) = command_dispatcher.get(&command) {
            let msg = msg.clone();
            if has_permission(&msg, Arc::clone(&client), cmd.permission).await {
                (cmd.handler)(msg.into_owned().clone(), Arc::clone(&client), conn.clone(), Arc::clone(&bot_state)).await?;
            }
        } else {
            if let Ok(Some(reply)) = get_command_response(&conn, msg.text().to_string().to_ascii_lowercase(), Some(msg.channel().to_string())).await {
                send_message(&msg, client.lock().await.borrow_mut(), &reply).await?;
            }
        }
    }
    Ok(())
}

#[warn(dead_code)]
async fn handle_usernotice(notice: tmi::UserNotice<'_>) -> Result<(), BotError> {
    todo!()
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



async fn start_annnouncement_scheduler(bot_state: Arc<Mutex<BotState>>, channel_id: String, channel_name: String, conn: async_sqlite::Client) -> Result<(), BotError> { 
    let mut sleep_duration = Duration::from_secs(60);
    let channel = format!("#{}", channel_name);

    loop {
        let id_clone = channel_id.clone();

        let is_live = {
            let bot_state = bot_state.lock().await;
            is_channel_live(&channel_name, &bot_state.oauth_token_bot, &bot_state.bot_id).await
        };

        if let Ok(is_live) = is_live {
            if is_live {
                let (state, last_sent, interval) = {
                    let mut bot_config = BotConfig::load_config();
                    let config = bot_config.get_channel_config(&channel);
                    (
                        config.announcement_config.state.clone(),
                        config.announcement_config.last_sent,
                        config.announcement_config.interval,
                    )
                };
    
                match state {
                    AnnouncementState::Paused => {
                        // Do nothing while paused
                    }
                    AnnouncementState::Active | AnnouncementState::Custom(_) => {
                        if last_sent.map_or(true, |last| last.elapsed() > interval) {
                            let message = match state {
                                AnnouncementState::Active => {
                                    conn.conn( move |conn| {
                                        conn.query_row("SELECT announcement FROM announcements WHERE state = 'Active' AND channel = ?1 ORDER BY RANDOM() LIMIT 1", params![id_clone.clone()], |row| {
                                            Ok(row.get::<_, String>(0).optional())
                                        })?
                                    }).await?
                                }
                                AnnouncementState::Custom(activity) => {
                                    conn.conn( move |conn| {
                                        conn.query_row("SELECT announcement FROM announcements WHERE (state = 'Active' OR state = ?1) AND channel = ?2 ORDER BY RANDOM() LIMIT 1", params![activity, id_clone.clone()], |row| {
                                            Ok(row.get::<_, String>(0).optional())
                                        })?
                                    }).await?
                                },
                                _ => unreachable!(),
                            };
                            if let Some(message) = message {
                                let bot_state = bot_state.lock().await;
                                announcement(&channel_id.clone(), "1091219021", &bot_state.oauth_token_bot, bot_state.clone().bot_id, message).await?;
                            }
                            {
                                let bot_state_config = &mut bot_state.lock().await.config;
                                let config = bot_state_config.get_channel_config(&channel.clone());
                                config.announcement_config.last_sent = Some(Instant::now());
                                sleep_duration = interval;
                                bot_state_config.save_config();
                            }
                            
                            
                        }
                    }
                }
            }
            
        }
        tokio::time::sleep(sleep_duration).await;
    }
    Ok(())
}



#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[tokio::test]
    async fn test_streaming_together() {
        // Create test data
        let mut channel_id_map: HashMap<String, String> = HashMap::new();
        channel_id_map.insert("216105918".to_string(), "#hostchannel".to_string());
        channel_id_map.insert("123456789".to_string(), "#sharedchannel".to_string());

        let mut streaming_together: HashMap<String, HashSet<String>> = HashMap::new();

        // Simulate source_room_id and room_id from IRC message
        let source_room_id = "216105918";
        let room_id = "123456789";

        // Check that we can create the relationship
        if let (Some(host_channel), Some(shared_channel)) = (
            channel_id_map.get(source_room_id),
            channel_id_map.get(room_id),
        ) {
            streaming_together
                .entry(host_channel.clone())
                .or_insert_with(HashSet::new)
                .insert(shared_channel.clone());
        }

        // Assert the relationship was created
        assert_eq!(streaming_together.len(), 1);
        assert!(streaming_together.contains_key("#hostchannel"));
        assert!(streaming_together["#hostchannel"].contains("#sharedchannel"));
    }
}
