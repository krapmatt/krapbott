use crate::{bot, commands::{create_dispatcher, traits::CommandT}, database::{fetch_aliases_from_db, get_command_response}, models::{has_permission, AnnouncementState, BotConfig, BotResult, SharedQueueGroup}, twitch_api::{announcement, ban_bots, fetch_lurkers, get_twitch_user_id, is_channel_live}};
use dashmap::DashMap;
use dotenvy::dotenv;
use rand::{rng, Rng};
use regex::Regex;
use shuttle_runtime::SecretStore;
use sqlx::PgPool;
use tracing::{error, info};
use twitch_irc::{login::StaticLoginCredentials, message::{PrivmsgMessage, ServerMessage}, ClientConfig, SecureTCPTransport, TwitchIRCClient};
use unicode_general_category::{get_general_category, GeneralCategory};
use unicode_normalization::UnicodeNormalization;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    sync::{
        mpsc, Mutex, RwLock
    }, task::JoinHandle, time::sleep
};

pub type CommandMap = HashMap<String, Arc<dyn CommandT + Send + Sync>>;

pub type DispatcherCache = HashMap<String, CommandMap>;

pub type TwitchClient = TwitchIRCClient<SecureTCPTransport, StaticLoginCredentials>;

#[derive(Clone)]
pub struct BotState {
    pub oauth_token_bot: String,
    pub nickname: String,
    pub bot_id: String,
    pub x_api_key: String,
    pub first_time_tag: Option<String>,

    // Immutable once loaded
    pub config: BotConfig,

    // Mutable concurrent maps
    pub streaming_together: HashMap<String, HashSet<String>>,
    pub shared_groups: HashMap<String, SharedQueueGroup>,
    pub channel_to_main: HashMap<String, String>,

    pub dispatchers: Arc<RwLock<DispatcherCache>>,
    pub active_point_loops: Arc<RwLock<HashMap<String, JoinHandle<()>>>>,

    pub obs_client: Option<TwitchClient>,
    pub obs_sender: Option<mpsc::UnboundedSender<ServerMessage>>,

    // Fine-grained per-channel locks for critical sections
    pub per_channel_locks: DashMap<String, Arc<Mutex<()>>>,
}

impl BotState {
    pub async fn new(pool: &PgPool, secrets: SecretStore) -> BotState {
        let oauth_token_bot = secrets.get("TWITCH_OAUTH_TOKEN_BOTT").expect("No bot oauth token");
        let nickname = secrets.get("TWITCH_BOT_NICK").expect("No bot name");
        let bot_id = secrets.get("TWITCH_CLIENT_ID_BOT").expect("msg");
        let x_api_key = secrets.get("XAPIKEY").expect("No bungie api key");
        info!("{}", oauth_token_bot);
        BotState {
            oauth_token_bot: oauth_token_bot,
            nickname: nickname,
            bot_id: bot_id,
            x_api_key: x_api_key,
            first_time_tag: None,
            config: BotConfig::load_from_db(pool).await.expect("Config has to be in db"),
            streaming_together: HashMap::new(),
            shared_groups: HashMap::new(),
            channel_to_main: HashMap::new(),
            dispatchers: Arc::new(HashMap::new().into()),
            active_point_loops: Arc::new(HashMap::new().into()),
            obs_client: None,
            obs_sender: None,
            per_channel_locks: DashMap::new()
        }
        
    }

     pub async fn init_obs_client(state: Arc<RwLock<Self>>) {
        let (nick, oauth) = {
            let s = state.read().await;
            (s.nickname.clone(), s.oauth_token_bot.clone())
        };

        let creds = StaticLoginCredentials::new(nick, Some(oauth));
        let config = ClientConfig::new_simple(creds);
        let (mut incoming, client) =
            TwitchClient::new(config);

        {
            let mut s = state.write().await;
            s.obs_client = Some(client.clone());
        }

        // join all channels
        let channels: Vec<String> = {
            let s = state.read().await;
            s.config.channels.keys().cloned().collect()
        };
        for ch in channels {
            client.join(ch.clone());
        }

        // background task just discards messages
        tokio::spawn(async move {
            while let Some(msg) = incoming.recv().await {
                if let ServerMessage::Notice(n) = msg {
                    info!("OBS client notice: {:?}", n);
                }
            }
        });
    }

    pub fn streaming_group(&self, channel: &str) -> Vec<String> {
        let binding = channel.to_string();
        let main = self.channel_to_main.get(channel).unwrap_or(&binding);
        if let Some(group) = self.shared_groups.get(main) {
            let mut group_channels: Vec<String> = group.member_channels.iter().cloned().collect();
            group_channels.push(main.clone());
            group_channels.sort();
            group_channels.dedup();
            group_channels
        } else {
            vec![main.clone()]
        }
    }

    /// Create twitch-irc client and message receiver
    pub fn client_builder(&self) -> (tokio::sync::mpsc::UnboundedReceiver<ServerMessage>, TwitchClient) {
        let credentials = StaticLoginCredentials::new(
            self.nickname.clone(),
            Some(self.oauth_token_bot.clone()),
        );
        let config = ClientConfig::new_simple(credentials);
        TwitchClient::new(config)
    }
}

pub async fn run_chat_bot(pool: Arc<PgPool>, bot_state: Arc<RwLock<BotState>>) -> BotResult<()> {
    manage_channels(bot_state, pool).await;
    Ok(())
}

pub async fn manage_channels(state: Arc<RwLock<BotState>>, pool: Arc<PgPool>) {
    let mut tasks: HashMap<String, tokio::task::JoinHandle<()>> = HashMap::new();
    BotState::init_obs_client(Arc::clone(&state)).await;
    loop {
        // Periodically check for new channels in the configuration
        let bot_config = BotConfig::load_from_db(&pool).await.expect("No errors");
        for (channel, _) in &bot_config.channels {
            if !tasks.contains_key(channel) {
            // Spawn a new bot task for this channel
                let channel_clone = channel.clone();
                let state_clone = Arc::clone(&state);
                let pool_clone = Arc::clone(&pool);

                let handle = tokio::spawn(async move {
                    
                    if let Err(e) = bot_task(channel_clone.clone(), state_clone, pool_clone).await {
                        error!("Bot task has failed for channel {}: {}",channel_clone, e)
                    }
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
                info!("Restarting bot task for channel: {}", channel);
                let channel_clone = channel.clone();
                let state_clone = Arc::clone(&state);
                let pool_clone = pool.clone();

                let handle = tokio::spawn(async move {
                    if let Err(e) = bot_task(channel_clone, state_clone, pool_clone).await {
                        error!("Bot task for channel failed: {}", e);
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

pub async fn bot_task(channel: String, state: Arc<RwLock<BotState>>, pool: Arc<PgPool>) -> BotResult<()> {
    let mut retry_attempts = 0;
    let channel_clone = channel.clone();
    info!("Starting bot for channel: {}", channel);
    {
    let bot_state_guard = state.write().await;
    let aliases = fetch_aliases_from_db(&channel, &pool).await.unwrap();
    let dispatcher = create_dispatcher(&bot_state_guard.config, &channel, &aliases);
    
        bot_state_guard.dispatchers.write().await.insert(channel.clone(), dispatcher);
    }
    let bot_state = state.read().await;
    let id = get_twitch_user_id(&channel, &bot_state.oauth_token_bot, &bot_state.bot_id).await?;
    let channel_id_map: HashMap<String, String> = HashMap::new();
    //ADD CHANNEL ID MAP TO BOTSTATE
    //TODO REMAKE THIS MESS PLEASE
    /*let channels = BotConfig::load_config().channels.into_keys().collect::<Vec<String>>();
    for mut channel in channels {
        channel.remove(0);
        channel_id_map.insert(id.clone(), channel);
    }*/
    drop(bot_state);


    {
        let bot_state_clone = Arc::clone(&state);

        let channel_clone = channel_clone.clone();
        let pool_clone = pool.clone();
        tokio::spawn(async move {
            let state = bot_state_clone.read().await.clone();
            if let Ok(id) = get_twitch_user_id(&channel_clone, &state.oauth_token_bot, &state.bot_id).await {
                if let Err(e) = start_annnouncement_scheduler(bot_state_clone, id, channel_clone.clone(), &*pool_clone).await {
                    error!("Announcement error: {}", e);
                }
            }
        });
    }
    
    {
        let (should_have_points, loops_arc, oauth_token_bot, bot_id_clone) = {
            let bot_state = state.read().await;
            let config = bot_state.config.get_channel_config(&channel).unwrap();

            let should = config
                .packages
                .iter()
                .any(|p| p.eq_ignore_ascii_case("points")); // tolerate case differences

            // clone the Arc so we can lock it without holding `state`.
            let loops_arc = bot_state.active_point_loops.clone();

            // clone oauth & bot id so we don't need to take another lock later
            let oauth_token_bot = bot_state.oauth_token_bot.clone();
            let bot_id_clone = bot_state.bot_id.clone();

            (should, loops_arc, oauth_token_bot, bot_id_clone)
        };
        
        if should_have_points {
            let mut loops = loops_arc.write().await;
            if !loops.contains_key(&channel) {
                let pool_clone = pool.clone();
            let channel_for_task = channel.clone();
            let id_for_task = id.clone(); // id was computed earlier in bot_task
            let oauth_for_task = oauth_token_bot;
            let bot_id_for_task = bot_id_clone;
                let handle = tokio::spawn(async move {
                    if let Err(e) = grant_points_task(&id_for_task, pool_clone, &channel_for_task, &oauth_for_task, &bot_id_for_task).await {
                        error!("Points loop crashed for {}: {}", &channel_clone, e);
                    }
                });
                loops.insert(channel.clone(), handle);
            }
        }
    }

    loop {
        info!("(Re)connecting client for {}", &channel);
        let (mut incoming, client) = {
            let creds = {
                let s = state.read().await;
                StaticLoginCredentials::new(s.nickname.clone(), Some(s.oauth_token_bot.clone()))
            };
            let config = ClientConfig::new_simple(creds);
            TwitchIRCClient::<SecureTCPTransport, StaticLoginCredentials>::new(config)
        };
        client.join(channel.to_string())?;
        match run_bot_loop(Arc::clone(&state), incoming, client.clone(), &pool).await {
            Ok(_) => break Ok(()),
            Err(e) => {
                error!("Bot task for {} failed: {}. Restarting", channel, e);
                retry_attempts += 1;
                let backoff = 2u64.pow(retry_attempts.min(6)) * 1000;
                tokio::time::sleep(Duration::from_millis(backoff)).await;
            }
        }

        if retry_attempts >= 25 {
            error!("Bot task for {} failed too many times, giving up", channel);
            break Ok(());
        }
    }
}

async fn run_bot_loop(state: Arc<RwLock<BotState>>, mut incoming: mpsc::UnboundedReceiver<ServerMessage>, client: TwitchClient, pool: &PgPool) -> BotResult<()> {
    while let Some(message) = incoming.recv().await {
        match &message {
            ServerMessage::Privmsg(msg) => {
                // Shared Chat filtering
                let room_id = msg.source.tags.0.get("room-id");
                let source_room_id = msg.source.tags.0.get("source-room-id");
                if let (Some(rid), Some(srid)) = (room_id, source_room_id) {
                    if rid != srid {
                        // message is forwarded from another chat → ignore
                        continue;
                    }
                }
                let mut is_first = false;
                if let Some(a) = msg.source.tags.0.get("first-msg") {
                    if *a == Some(1.to_string()) {
                        is_first = true
                    }
                }

                // bannable links check
                if is_bannable_link(&msg.message_text) && is_first {
                    let s = state.read().await;
                    ban_bots(&msg, &s.oauth_token_bot, s.bot_id.clone()).await;
                    client.say(msg.channel_login.clone(), "Kr4pTr4p is the last bot this channel needed.".to_string()).await?;
                }

                // normal command/dispatcher handling
                handle_privmsg(msg.clone(), state.clone(), pool.clone(), client.clone()).await?;
            }
            _ => {}
        }
    }
    Ok(())
}





pub async fn handle_obs_message(channel_id: String, command: String, pool: Arc<PgPool>, state: Arc<RwLock<BotState>>,) -> BotResult<()> {
    info!("Processing OBS message: {} -> {}", channel_id, command);
    if command == "next" {
        let reply = {
            let mut s = state.write().await;
            s.handle_next(channel_id.clone(), &*pool)
                .await
                .unwrap_or_else(|e| format!("Next failed: {}", e))
        };

        if let Some(client) = &state.read().await.obs_client {
            client.say(channel_id.clone(), reply).await?;
        }
    }

    Ok(())
}

async fn handle_privmsg(msg: PrivmsgMessage, bot_state: Arc<RwLock<BotState>>, pool: PgPool, client: TwitchClient) -> BotResult<()> {
    let channel = msg.channel_login.clone();
    let msg_clone = msg.clone();
    info!("Here");
    if msg_clone.message_text.starts_with("!pos")
        && (msg_clone.sender.login.to_ascii_lowercase() == "thatjk" || msg_clone.channel_login == "samoan_317")
    {
        let number = rng().random_range(1..1000);
        client.say(msg_clone.channel_login, format!("{} you are {}% PoS <3", msg_clone.sender.name.to_string(), number)).await?;
    } 

    let prefix = {
        let s = bot_state.read().await;
        s.config.get_channel_config(&channel).unwrap().prefix.clone()
    };
    
    if msg.message_text.starts_with("!") {
        if let Ok(Some(reply)) = get_command_response(&pool, msg.message_text.to_string().to_ascii_lowercase(), Some(channel.clone())).await {
            client.say(channel.clone(), reply).await?;
        }
    }
    if msg.message_text.starts_with(&prefix) {
        let without_prefix = msg.message_text.strip_prefix(&prefix).unwrap();
        let mut parts = without_prefix.split_whitespace();

        if let Some(command) = parts.next() {
            let command = command.to_lowercase();
            let dispatcher = {
                let s = bot_state.read().await;
                let cache = s.dispatchers.read().await;
                cache.get(&channel).cloned()
            };
            if let Some(dispatcher) = dispatcher {
                if let Some(cmd) = dispatcher.get(&command) {
                    let (oauth_token, bot_id) = {
                        let s = bot_state.read().await;
                        (s.oauth_token_bot.clone(), s.bot_id.clone())
                    };
                    info!("Here");
                    if has_permission(&msg, client.clone(), cmd.permission(), &oauth_token, &bot_id).await {
                        let aliases = fetch_aliases_from_db(&channel, &pool).await?;
                        cmd.execute(msg.clone(), client.clone(), pool.clone(), Arc::clone(&bot_state), Arc::new(aliases)).await?;
                    }
                }
            }
        }
    }

    
    Ok(())
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

async fn start_annnouncement_scheduler(bot_state: Arc<RwLock<BotState>>, channel_id: String, channel_name: String, pool: &PgPool) -> BotResult<()> {
    let mut sleep_duration = Duration::from_secs(60);
    let channel = format!("{}", channel_name);

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
                                WHERE state = 'active' AND channel = $1 
                                ORDER BY RANDOM() LIMIT 1",
                                id_clone
                            ).fetch_optional(pool).await?
                        }
                        AnnouncementState::Custom(activity) => {
                            sqlx::query_scalar!(
                                "SELECT announcement FROM announcements 
                                WHERE (state = 'active' OR state = $1) AND channel = $2 
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
                        bot_state_config.config.save_all(&pool).await?;
                    }
                }
            }
        }

        tokio::time::sleep(sleep_duration).await;
    }
}

pub async fn grant_points_task(broadcaster_id: &str, pool: Arc<PgPool>, name: &str, oauth_token: &str, bot_id: &str) -> BotResult<()> {
    let active_viewers = Arc::new(Mutex::new(HashSet::new()));

    loop {
        if is_channel_live(name, &oauth_token, &bot_id).await? {
            let bot_config = BotConfig::load_from_db(&pool).await?;
            let config = bot_config.get_channel_config(name).unwrap();
            info!("{}", name);
            let points = config.points_config.points_per_time;
            let mut viewers = active_viewers.lock().await.clone(); // Get chatters

            // Fetch Lurkers from Twitch API
            let lurkers = fetch_lurkers(broadcaster_id, &oauth_token, &bot_id).await;
            info!("{:?}", lurkers);
            // Combine chatters and lurkers
            viewers.extend(lurkers);

            // Grant points
            for viewer in viewers.iter() {
                sqlx::query!(
                    "INSERT INTO currency (twitch_name, points, channel) VALUES ($1, $2, $3) 
                    ON CONFLICT(twitch_name, channel) DO UPDATE SET points = currency.points + $4",
                    viewer, points, broadcaster_id, points
                ).execute(&*pool).await?;
            }

            active_viewers.lock().await.clear(); // Reset chatters after granting points

            sleep(Duration::from_secs(config.points_config.interval)).await;
        }
        sleep(Duration::from_secs(10)).await;
    }
}
