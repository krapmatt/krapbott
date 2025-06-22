use crate::{
    bot_commands::send_message, commands::create_command_dispatcher, database::get_command_response, models::{has_permission, AnnouncementState, BotConfig, BotError}, twitch_api::{announcement, ban_bots, fetch_lurkers, get_twitch_user_id, is_channel_live}
};
use dotenvy::dotenv;
use rand::{rng, Rng};
use regex::Regex;
use sqlx::SqlitePool;
use tmi::{Client, Tag};
use unicode_general_category::{get_general_category, GeneralCategory};
use unicode_normalization::UnicodeNormalization;

use std::{
    borrow::BorrowMut,
    collections::{HashMap, HashSet},
    env::var,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    sync::{
        Mutex, RwLock,
    },
    time::interval,
};

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

    pub async fn client_builder(&mut self) -> Result<Client, BotError> {
        let credentials =
            tmi::Credentials::new(self.nickname.clone(), self.oauth_token_bot.clone());
        let mut client = tmi::Client::builder()
            .credentials(credentials)
            .connect()
            .await?;
        client.join_all(self.config.channels.keys().cloned().collect::<Vec<String>>()).await?;
        Ok(client)
    }
    pub async fn client_channel(&mut self, channel: String) -> Result<Client, BotError> {
        let credentials =
            tmi::Credentials::new(self.nickname.clone(), self.oauth_token_bot.clone());
        let mut client = tmi::Client::builder().credentials(credentials).connect().await?;
        client.join(channel).await?;
        Ok(client)
    }
}

pub async fn bot_task(channel: String, state: Arc<RwLock<BotState>>, pool: Arc<SqlitePool>) -> Result<(), BotError> {
    let mut retry_attempts = 0;
    let channel_clone = channel.clone();
    println!("Starting bot for channel: {}", channel);
    
    let mut channel_id_map: HashMap<String, String> = HashMap::new();
    //ADD CHANNEL ID MAP TO BOTSTATE
    let channels = BotConfig::load_config()
        .channels
        .into_keys()
        .collect::<Vec<String>>();
    for mut channel in channels {
        channel.remove(0);
        let id = get_twitch_user_id(&channel).await?;
        channel_id_map.insert(id, format!("#{}", channel));
    }
    {
        let bot_state_clone = Arc::clone(&state);

        let channel_clone = channel_clone.clone();
        let pool_clone = pool.clone();
        tokio::spawn(async move {
            let channel = channel_clone
                .strip_prefix("#")
                .unwrap_or(&channel_clone)
                .to_string();
            if let Ok(id) = get_twitch_user_id(&channel).await {
                if let Err(e) = start_annnouncement_scheduler(bot_state_clone, id, channel.clone(), &*pool_clone).await {
                    eprintln!("Announcement error: {}", e);
                }
            }
        });
    }

    loop {
        println!("(Re)connecting client for {}", channel);
        let client = {
            let mut bot_state = state.write().await;
            Arc::new(Mutex::new(
                bot_state.client_channel(channel_clone.clone()).await?,
            ))
        };

        match run_bot_loop(Arc::clone(&state), client, &*pool, channel_id_map.clone()).await {
            Ok(_) => {
                // If bot exits cleanly, break out of loop
                break Ok(());
            }
            Err(e) => {
                println!("Em here");
                println!(
                    "Bot task for {} failed: {}. Restarting",
                    channel, e
                );
                retry_attempts += 1;
                let backoff = 2u64.pow(retry_attempts.min(6)) * 1000;
                tokio::time::sleep(Duration::from_millis(backoff)).await;
            }
        }
        if retry_attempts >= 25 {
            println!("Bot task for {} has failed too many times. Giving up.", channel);
            break Ok(());
        }
    }
}

async fn run_bot_loop(state: Arc<RwLock<BotState>>, client: Arc<Mutex<Client>>, pool: &SqlitePool, channel_id_map: HashMap<String, String>) -> Result<(), BotError> {
    loop {
        let irc_msg_result = client.lock().await.recv().await;

        let irc_msg = match irc_msg_result  {
            Ok(msg) => msg,
            Err(e) => {
                eprintln!("Connection error: {}. Attempting reconnect", e);
                return Err(BotError::RecvError(e));
            }
        };
        if let Some(source_room_id) = irc_msg
            .tags()
            .find(|(key, _)| *key == "source-room-id")
            .map(|(_, value)| value)
        {
            //Streamer doesnt own krapbott, skip messages from that channel
            if !channel_id_map.contains_key(source_room_id) {
                continue;
            }
            if let Some(room_id) = irc_msg
                .tags()
                .find(|(key, _)| *key == "room-id")
                .map(|(_, value)| value) {
                if room_id != source_room_id {
                    continue;
                }
            }
        }
        match irc_msg.as_ref().as_typed()? {
            tmi::Message::Privmsg(msg) => {
                let first_time = irc_msg.tag(Tag::FirstMsg).map(|x| x.to_string());
                if is_bannable_link(msg.text()) && first_time == Some("1".to_string()) {
                    let state = state.read().await;
                    ban_bots(&msg, &state.oauth_token_bot, state.bot_id.clone()).await;
                    client
                        .lock()
                        .await.privmsg(msg.channel(),"Kr4pTr4p is the last bot this channel needed.",).send().await?;
                }
                handle_privmsg(
                    msg.clone(),
                    Arc::clone(&state),
                    pool.clone(),
                    Arc::clone(&client),
                )
                .await?;
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

pub async fn manage_channels(state: Arc<RwLock<BotState>>, pool: Arc<SqlitePool>) {
    let mut tasks: HashMap<String, tokio::task::JoinHandle<()>> = HashMap::new();
    loop {
        // Periodically check for new channels in the configuration
        let bot_config = BotConfig::load_config();

        for (channel, _) in &bot_config.channels {
            if !tasks.contains_key(channel) {
                // Spawn a new bot task for this channel
                let channel_clone = channel.clone();
                let state_clone = Arc::clone(&state);
                let pool_clone = Arc::clone(&pool);

                let handle = tokio::spawn(async move {
                    bot_task(channel_clone, state_clone, pool_clone).await;
                });

                tasks.insert(channel.clone(), handle);
            }
        }
        for (channel, _) in &bot_config.channels {
            let should_restart = match tasks.get(channel) {
                Some(handle) => handle.is_finished(), // If finished (crashed/exited), restart
                None => false,
            };

            if should_restart {
                println!("Restarting bot task for channel: {}", channel);
                let channel_clone = channel.clone();
                let state_clone = Arc::clone(&state);
                let pool_clone = pool.clone();

                let handle = tokio::spawn(async move {
                    if let Err(e) = bot_task(channel_clone, state_clone, pool_clone).await {
                        eprintln!("Bot task for channel failed: {}", e);
                    }
                });

                tasks.insert(channel.clone(), handle);
            }
        }
        let existing_channels: Vec<String> = tasks.keys().cloned().collect();
        for channel in existing_channels {
            if !bot_config.channels.contains_key(&channel) {
                if let Some(handle) = tasks.remove(&channel) {
                    handle.abort();
                }
            }
        }
        drop(bot_config);
        tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
    }
}

pub async fn run_chat_bot(pool: Arc<SqlitePool>) -> Result<(), BotError> {
    manage_channels(Arc::new(RwLock::new(BotState::new())), pool).await;

    Ok(())
}

pub async fn handle_obs_message(
    channel_id: String,
    command: String,
    pool: Arc<SqlitePool>,
) -> Result<(), BotError> {
    let mut bot_state = BotState::new();
    let mut client = bot_state.client_builder().await?;

    println!("Processing OBS message: {} -> {}", channel_id, command);
    if command == "next" {
        let reply = bot_state
            .handle_next(format!("#{}", channel_id), &*pool)
            .await
            .unwrap_or_else(|e| format!("Next failed: {}", e));

        client
            .privmsg(&format!("#{}", channel_id), &reply)
            .send()
            .await?;
    }

    Ok(())
}

async fn handle_privmsg(
    msg: tmi::Privmsg<'_>,
    bot_state: Arc<RwLock<BotState>>,
    pool: SqlitePool,
    client: Arc<Mutex<Client>>,
) -> Result<(), BotError> {
    let mut locked_state = bot_state.write().await;

    locked_state.config = BotConfig::load_config();
    if msg.text().starts_with("!pos")
        && (msg.sender().login().to_ascii_lowercase() == "thatjk" || msg.channel() == "#samoan_317")
    {
        let number = rng().random_range(1..1000);
        send_message(
            &msg,
            client.lock().await.borrow_mut(),
            &format!(
                "{} you are {}% PoS <3",
                msg.sender().name().to_string(),
                number
            ),
        )
        .await?;
    }
    let command_dispatcher = create_command_dispatcher(&locked_state.config, msg.channel(), None);
    let prefix = locked_state.config.get_channel_config(msg.channel()).unwrap().prefix.clone();
    drop(locked_state);
    if msg.text().starts_with("!") {
        if let Ok(Some(reply)) = get_command_response(&pool, msg.text().to_string().to_ascii_lowercase(), Some(msg.channel().to_string())).await {
            send_message(&msg, client.lock().await.borrow_mut(), &reply).await?;
        }
    }
    if msg.text().starts_with(&prefix) {
        let without_prefix = msg.text().strip_prefix(&prefix).unwrap();
        let mut parts = without_prefix.split_whitespace();

        if let Some(command) = parts.next() {
            let command = command.to_lowercase();
            if let Some(cmd) = command_dispatcher.get(&command) {
                let msg = msg.clone();
                if has_permission(&msg, Arc::clone(&client), cmd.permission).await {
                    (cmd.handler)(msg.into_owned().clone(), Arc::clone(&client), pool, Arc::clone(&bot_state)).await?;
                }
            }
            return Ok(());
        }
    }

    
    Ok(())
}

#[warn(dead_code)]
async fn handle_usernotice(notice: tmi::UserNotice<'_>) -> Result<(), BotError> {
    todo!()
}

lazy_static::lazy_static! {
    static ref CHEAP_VIEWERS_RE: Regex = Regex::new(r"cheap\s*viewers\s*on\s*\w*\.?\w*").unwrap();
    static ref BEST_VIEWERS_RE: Regex = Regex::new(r"best\s*viewers\s*on\s*\w*\.?\w*").unwrap();
    static ref PROMO_RE: Regex = Regex::new(r"hello\s*sorry\s*for\s*bothering\s*you\s*i\s*want\s*to\s*offer\s*promotion\s*of\s*your\s*channel\s*viewers\s*followers\s*views\s*chat\s*bots\s*etc\s*the\s*price\s*is\s*lower\s*than\s*any\s*competitor\s*the\s*quality\s*is\s*guaranteed\s*to\s*be\s*the\s*best\s*flexible\s*and\s*convenient\s*order\s*management\s*panel\s*chat\s*panel\s*everything\s*is\s*in\s*your\s*hands\s*a\s*huge\s*number\s*of\s*custom\s*settings").unwrap();
    static ref DISCORD_RE: Regex = Regex::new(r"hey\s*friend,\s*you\s*stream").unwrap();
}

// Normalize text by removing diacritics
fn normalize_text(text: &str) -> String {
    text.nfd() // decompose characters
        .filter(|c| {
            !matches!(get_general_category(*c), GeneralCategory::NonspacingMark)
        }) // remove combining marks
        .collect::<String>()
        .to_ascii_lowercase()
}

fn is_bannable_link(text: &str) -> bool {
    let cleaned_text = normalize_text(text);

    let contains_dot = text.contains('.');

    // Use correct parenthesis to ensure correct logic
    (CHEAP_VIEWERS_RE.is_match(&cleaned_text)
        || (BEST_VIEWERS_RE.is_match(&cleaned_text) && contains_dot))
        || PROMO_RE.is_match(&cleaned_text)
        || DISCORD_RE.is_match(&cleaned_text)
}

async fn start_annnouncement_scheduler(bot_state: Arc<RwLock<BotState>>, channel_id: String, channel_name: String, pool: &SqlitePool) -> Result<(), BotError> {
    let mut sleep_duration = Duration::from_secs(60);
    let channel = format!("#{}", channel_name);

    loop {
        let id_clone = channel_id.clone();

        let is_live = {
            let bot_state = bot_state.read().await;
            is_channel_live(&channel_name, &bot_state.oauth_token_bot, &bot_state.bot_id).await
        };

        if let Ok(is_live) = is_live {
            if is_live {
                let (state, last_sent, interval) = {
                    let bot_state = bot_state.read().await;
                    let config = bot_state.config.get_channel_config(&channel).unwrap();
                    (
                        config.announcement_config.state.clone(),
                        config.announcement_config.last_sent,
                        config.announcement_config.interval,
                    )
                };
                let now = SystemTime::now().duration_since(UNIX_EPOCH).expect("Time went backwards").as_millis();
                if now - last_sent > interval.as_millis() {
                    let message: Option<String> = match state {
                        AnnouncementState::Active => {
                            sqlx::query_scalar!(
                                "SELECT announcement FROM announcements 
                                WHERE state = 'active' AND channel = ? 
                                ORDER BY RANDOM() LIMIT 1",
                                id_clone
                            ).fetch_optional(pool).await?
                        }
                        AnnouncementState::Custom(activity) => {
                            sqlx::query_scalar!(
                                "SELECT announcement FROM announcements 
                                WHERE (state = 'active' OR state = ?) AND channel = ? 
                                ORDER BY RANDOM() LIMIT 1",
                                activity, id_clone
                            ).fetch_optional(pool).await?
                        }
                        AnnouncementState::Paused => None,
                    };

                    if let Some(message) = message {
                        let bot_state = bot_state.read().await;
                        announcement(&channel_id, "1091219021", &bot_state.oauth_token_bot, bot_state.bot_id.clone(), message).await?;
                    }

                    {
                        let mut bot_state_config = bot_state.write().await;
                        let config = bot_state_config.config.get_channel_config_mut(&channel);
                        config.announcement_config.last_sent = SystemTime::now().duration_since(UNIX_EPOCH).expect("Time went backwards").as_millis();
                        sleep_duration = interval;
                        bot_state_config.config.save_config();
                    }
                }
            }
        }

        tokio::time::sleep(sleep_duration).await;
    }
}

pub async fn grant_points_task(broadcaster_id: &str, pool: Arc<SqlitePool>) -> Result<(), BotError> {
    dotenv().ok();
    let oauth_token = var("TWITCH_OAUTH_TOKEN_BOTT").expect("No bot oauth token");
    let bot_id = var("TWITCH_CLIENT_ID_BOT").expect("msg");
    let active_viewers = Arc::new(Mutex::new(HashSet::new()));
    let mut interval = interval(Duration::from_secs(600)); // Every 10 min
    loop {
        if is_channel_live("krapmatt", &oauth_token, &bot_id).await? {
            interval.tick().await;

            let mut viewers = active_viewers.lock().await.clone(); // Get chatters

            // Fetch Lurkers from Twitch API
            let lurkers = fetch_lurkers(broadcaster_id, &oauth_token, &bot_id).await;

            // Combine chatters and lurkers
            viewers.extend(lurkers);

            // Grant points
            for viewer in viewers.iter() {
                sqlx::query!(
                    "INSERT INTO currency (twitch_name, points, channel) VALUES (?, ?, ?) 
                    ON CONFLICT(twitch_name, channel) DO UPDATE SET points = points + 10",
                    viewer,
                    10,
                    broadcaster_id
                )
                .execute(&*pool)
                .await?;
            }

            active_viewers.lock().await.clear(); // Reset chatters after granting points
        }
    }
}
