use futures::{SinkExt, StreamExt};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{query, SqlitePool};
use uuid::Uuid;
use std::{collections::{HashMap, HashSet}, sync::Arc};
use tokio::sync::{mpsc::UnboundedSender, RwLock};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use warp::{http::StatusCode, reject::Rejection, reply::{json, Reply}, Filter};

use crate::{bot::BotState, commands::{update_dispatcher_if_needed, COMMAND_GROUPS}, models::BotConfig};

#[derive(Serialize, Clone, Debug)]
struct QueueEntry {
    position: i64,
    twitch_name: String,
    bungie_name: String,
}

const OBS_WEBSOCKET_URL: &str = "ws://localhost:4455"; // Default OBS WebSocket URL
const OBS_PASSWORD: &str = "dPCfXN8kulIb496b"; // Replace with your OBS WebSocket password

pub async fn connect_to_obs() -> Result<(), Box<dyn std::error::Error>> {
    let (mut ws_stream, _) = connect_async(OBS_WEBSOCKET_URL).await?;
    println!("Connected to OBS WebSocket!");

    // Authenticate with OBS WebSocket
    let auth_message = json!({
        "op": 1,
        "d": {
            "rpcVersion": 1,
            "authentication": OBS_PASSWORD
        }
    });

    ws_stream
        .feed(Message::Text(auth_message.to_string().into()))
        .await?;
    // Listen for incoming messages
    while let Some(msg) = ws_stream.next().await {
        let msg = msg?;
        if let Message::Text(text) = msg {
            println!("OBS Message: {}", text);
        }
    }

    Ok(())
}

pub async fn get_queue_handler(cookies: Option<String>, pool: Arc<SqlitePool>) -> Result<impl warp::Reply, warp::Rejection> {
    if let Some(twitch_name) = get_twitch_name_from_cookie(cookies, &*pool).await {
        let channel_id = format!("#{}", twitch_name.to_ascii_lowercase());
        let mut config = BotConfig::load_config();
        let config = config.get_channel_config_mut(&channel_id);
        let queue_channel = &config.queue_channel;

        let queue = sqlx::query_as!(
            QueueEntry,
            "SELECT position, twitch_name, bungie_name FROM queue WHERE channel_id = ? ORDER BY position ASC",
            queue_channel
        ).fetch_all(&*pool).await.map_err(|_| warp::reject::reject())?;

        if queue.is_empty() {
            return Ok(warp::reply::json(&queue));
        }
        let grouped_queue: Vec<Vec<QueueEntry>> = queue
            .chunks(config.teamsize)
            .map(|chunk| chunk.to_vec())
            .collect();
        return Ok(warp::reply::json(&grouped_queue));
        
    }
    Ok(warp::reply::json(&serde_json::json!({ "error": "Not logged in" })))
}

pub async fn remove_from_queue_handler(cookies: Option<String>, body: serde_json::Value, pool: Arc<SqlitePool>) -> Result<impl warp::Reply, warp::Rejection> {
    let twitch_name = body["twitch_name"].as_str().ok_or_else(|| warp::reject())?;

    if let Some(name) = get_twitch_name_from_cookie(cookies, &*pool).await {
        let channel_id = format!("#{}", name.to_ascii_lowercase());
        let mut queue_channel = String::new();
        if let Some(config)  = BotConfig::load_config().get_channel_config(&channel_id) {
            queue_channel = config.queue_channel.clone()
        }
        let position = sqlx::query!(
            "SELECT position FROM queue WHERE twitch_name = ? AND channel_id = ?",
            twitch_name, queue_channel
        ).fetch_optional(&*pool).await.map_err(|_| warp::reject())?;
    
        if position.is_some() {
            // Remove the user from queue
            sqlx::query!(
                "DELETE FROM queue WHERE twitch_name = ? AND channel_id = ?",
                twitch_name, queue_channel
            ).execute(&*pool).await.map_err(|_| warp::reject())?;
    
            let queue_entries = sqlx::query!(
                "SELECT twitch_name FROM queue WHERE channel_id = ? ORDER BY position ASC",
                queue_channel
            ).fetch_all(&*pool).await.map_err(|_| warp::reject())?;
    
            let mut new_position = 1;
            for entry in queue_entries {
                sqlx::query!(
                    "UPDATE queue SET position = ? WHERE twitch_name = ? AND channel_id = ?",
                    new_position, entry.twitch_name, queue_channel
                ).execute(&*pool).await.map_err(|_| warp::reject())?;
                new_position += 1;
            }
            return Ok(warp::reply::with_status("Removed", StatusCode::OK));
        }
    }
    Ok(warp::reply::with_status(
        "Failed to remove",
        StatusCode::INTERNAL_SERVER_ERROR,
    ))
}

#[derive(Deserialize, Debug)]
pub struct UpdateQueueOrderRequest {
    pub new_order: Vec<QueueUpdate>,
}

#[derive(Deserialize, Debug)]
pub struct QueueUpdate {
    pub twitch_name: String,
    pub position: i64,
}

pub async fn update_queue_order(cookies: Option<String>, data: UpdateQueueOrderRequest, pool: Arc<SqlitePool>) -> Result<impl warp::Reply, warp::Rejection> {
    if let Some(twitch_name) = get_twitch_name_from_cookie(cookies, &*pool).await {
        let channel_id = format!("#{}", twitch_name.to_ascii_lowercase());
        let mut queue_channel = String::new();
        if let Some(config)  = BotConfig::load_config().get_channel_config(&channel_id) {
            queue_channel = config.queue_channel.clone()
        }
        sqlx::query!(
            "UPDATE queue SET position = position + 10000 WHERE channel_id = ?",
            queue_channel
        ).execute(&*pool).await.map_err(|_| warp::reject())?;
        for entry in &data.new_order {
            sqlx::query!(
                "UPDATE queue SET position = ? WHERE twitch_name = ? AND channel_id = ?",
                entry.position, entry.twitch_name, queue_channel
            ).execute(&*pool).await.map_err(|_| warp::reject())?;
        }
    
        return Ok(warp::reply::json(&"Queue order updated"));
    }
    return Err(warp::reject())
}

pub async fn next_queue_handler(cookies: Option<String>, sender: Arc<UnboundedSender<(String, String)>>, pool: Arc<SqlitePool>) -> Result<impl warp::Reply, warp::Rejection> {
    if let Some(twitch_name) = get_twitch_name_from_cookie(cookies, &*pool).await {
        if let Err(err) = sender.send((twitch_name, "next".to_string())) {
            eprintln!("Failed to send message: {:?}", err);
            return Ok(warp::reply::with_status(
                "Failed to send message",
                StatusCode::INTERNAL_SERVER_ERROR,
            ));
        }
        return Ok(warp::reply::with_status(
            "Next group message sent",
            StatusCode::OK,
        ));
    }
    return Ok(warp::reply::with_status(
        "Failed to send message",
        StatusCode::INTERNAL_SERVER_ERROR,
    ));
}

pub async fn get_run_counter_handler(cookies: Option<String>, pool: Arc<SqlitePool>) -> Result<impl warp::Reply, warp::Rejection> {
    if let Some(twitch_name) = get_twitch_name_from_cookie(cookies, &*pool).await {
        let channel_id = format!("#{}", twitch_name.to_ascii_lowercase());
        let config = BotConfig::load_config();
        if let Some(channel_config) = config.get_channel_config(&channel_id) {
            if let Some(config) = config.get_channel_config(&channel_config.queue_channel) {
                let runs = config.runs;
                return Ok(json(&json!({"run_counter": runs})));
            }
        }
    }
    Err(warp::reject())
    
}
pub async fn get_queue_state_handler(cookies: Option<String>, pool: Arc<SqlitePool>) -> Result<impl warp::Reply, warp::Rejection> {
    if let Some(twitch_name) = get_twitch_name_from_cookie(cookies, &*pool).await {
        let config = BotConfig::load_config();
        let channel_id = format!("#{}", twitch_name.to_ascii_lowercase());
        let config = config.get_channel_config(&channel_id).unwrap();
        return Ok(warp::reply::json(
        &serde_json::json!({ "is_open": config.open }),
        ));
    }
    Err(warp::reject())
}
pub async fn toggle_queue_handler(toggle_action: String, cookies: Option<String>, pool: Arc<SqlitePool>) -> Result<impl warp::Reply, warp::Rejection> {
    if let Some(twitch_name) = get_twitch_name_from_cookie(cookies, &*pool).await {
        let mut channel_name = format!("#{}", twitch_name.to_ascii_lowercase());
        let mut config = BotConfig::load_config();
        if let Some(name) = config.get_channel_config(&channel_name) {
            channel_name = name.queue_channel.clone();
        }
        let updated_channels = config.channels.iter_mut().filter_map(|(channel_id, channel_config)| {
            // Check if the channel matches the `queue_channel`
            if channel_config.queue_channel == channel_name {
                match toggle_action.as_str() {
                    "open" => {
                        channel_config.open = true;
                        Some((channel_id.clone(), "open"))
                    }
                    "close" => {
                        channel_config.open = false;
                        Some((channel_id.clone(), "closed"))
                    }
                    _ => None,
                }
            } else {
                None
            }
        }).collect::<Vec<_>>();
        config.save_config();
        
        return Ok(warp::reply::json(
            &serde_json::json!({
                "success": true,
                "state": toggle_action,
                "updated_channels": updated_channels,
            }),
        ));
    }
    Err(warp::reject())
}

const TWITCH_CLIENT_ID: &str = "mtcgb9falyzs4n3j7x3rqr51ho9gxr";
const TWITCH_CLIENT_SECRET: &str = "jqky6ivqcn83kx6u1nlkx29hsgirjw";
const TWITCH_REDIRECT_URI: &str = "https://krapmatt.bounceme.net/auth/callback";

#[derive(Debug, Deserialize)]
pub struct AuthCallbackQuery {
    code: String
}


#[derive(Debug, Deserialize)]
struct TwitchTokenResponse {
    access_token: String,
    expires_in: i64,
    refresh_token: String,
    token_type: String,
}

#[derive(Serialize)]
struct TokenRequest<'a> {
    client_id: &'a str,
    client_secret: &'a str,
    code: &'a str,
    grant_type: &'a str,
    redirect_uri: &'a str,
}

/// Exchanges the code for an access token and fetches Twitch user info
pub async fn twitch_callback(query: AuthCallbackQuery, pool: Arc<SqlitePool>) -> Result<impl Reply, Rejection> {
    let client = Client::new();
    let token_request = TokenRequest {
        client_id: &TWITCH_CLIENT_ID,
        client_secret: &TWITCH_CLIENT_SECRET,
        code: &query.code,
        grant_type: "authorization_code",
        redirect_uri: &TWITCH_REDIRECT_URI,
    };

    let token_response = client
        .post("https://id.twitch.tv/oauth2/token")
        .form(&token_request)
        .send()
        .await.map_err(|e| {
            eprintln!("Token request failed: {:?}", e);
            warp::reject::not_found()
        })?;
    
        let token_data: TwitchTokenResponse = token_response.json().await.map_err(|e| {
            eprintln!("Token request failed: {:?}", e);
            warp::reject::not_found()
        })?;
        let twitch_user = get_user_info(&token_data.access_token).await.map_err(|e| {
            eprintln!("Token request failed: {:?}", e);
            warp::reject::not_found()
        })?;

        if let Err(e) = save_user_tokens(&twitch_user, &token_data, pool.clone()).await {
            eprintln!("Failed to save tokens: {:?}", e);
        }

        let session_token = Uuid::new_v4().to_string();
        let expires_at = chrono::Utc::now().timestamp() + 604800;

        sqlx::query!(
            "INSERT INTO sessions (session_token, twitch_id, expires_at) VALUES (?, ?, ?)",
            session_token, twitch_user.id, expires_at
        ).execute(&*pool).await.map_err(|e| {
            eprintln!("Failed to store session: {:?}", e);
            warp::reject::not_found()
        })?;

        let session_cookie = format!(
            "session={}; Path=/; HttpOnly; Secure; SameSite=None; Max-Age=604800",
            session_token
        );
        let response = warp::reply::with_header(warp::reply::html("Login successful. Redirecting..."), "Set-Cookie", session_cookie);
        let res = warp::reply::with_header(response, "Location", "/queue_dock.html");

        Ok(warp::reply::with_status(res, warp::http::StatusCode::FOUND))
}

#[derive(Debug, Deserialize)]
struct TwitchUser {
    id: String,
    login: String,
    display_name: String,
    profile_image_url: String
}

async fn get_user_info(access_token: &str) -> Result<TwitchUser, reqwest::Error> {
    let client = reqwest::Client::new();
    let response = client.get("https://api.twitch.tv/helix/users")
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Client-Id", TWITCH_CLIENT_ID).send().await?;

    let json: serde_json::Value = response.json().await?;
    let user = json["data"][0].clone();

    Ok(TwitchUser {
        id: user["id"].as_str().unwrap().to_string(),
        login: user["login"].as_str().unwrap().to_string(),
        display_name: user["display_name"].as_str().unwrap().to_string(),
        profile_image_url: user["profile_image_url"].as_str().unwrap().to_string()
    })
}

async fn save_user_tokens(user: &TwitchUser, tokens: &TwitchTokenResponse, pool: Arc<SqlitePool>) -> Result<(), sqlx::Error> {
    let expires_at = chrono::Utc::now().timestamp() + tokens.expires_in;

    sqlx::query!(
        "INSERT INTO users (id, twitch_name, access_token, refresh_token, expires_at, profile_pp) 
         VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT (id) DO UPDATE 
         SET access_token = $3, refresh_token = $4, expires_at = $5, profile_pp = $6",
        user.id, user.login, tokens.access_token, tokens.refresh_token, expires_at, user.profile_image_url
    ).execute(&*pool).await?;
    
    Ok(())
}

pub async fn check_session(cookies: Option<String>, pool: Arc<SqlitePool>) -> Result<impl Reply, Rejection> {
    if let Some(cookies) = cookies {
        // Extract session ID (Twitch user ID)
        if let Some(session_token) = cookies
            .split(';')
            .find(|c| c.trim().starts_with("session="))
            .map(|c| c.trim().strip_prefix("session=").unwrap_or(""))
        {
            let session = sqlx::query!(
                "SELECT users.twitch_name, users.profile_pp, sessions.expires_at 
                 FROM sessions 
                 JOIN users ON users.id = sessions.twitch_id 
                 WHERE sessions.session_token = ?1",
                session_token
            ).fetch_optional(&*pool).await.map_err(|e| {
                eprintln!("DB Error: {:?}", e);
                warp::reject::not_found()
            })?;

            if let Some(session) = session {
                let now = chrono::Utc::now().timestamp();
                if session.expires_at > now {
                    return Ok(warp::reply::json(&serde_json::json!({
                        "logged_in": true,
                        "username": session.twitch_name,
                        "profile_pp": session.profile_pp
                    })));
                }
            }
        }
    }
    // No valid session found
    Ok(warp::reply::json(&serde_json::json!({ "logged_in": false })))
}

pub async fn check_authorization(cookie: Option<String>, pool: Arc<SqlitePool>) -> Result<String, Rejection> {
    if let Some(cookies) = cookie {
        if let Some(session_token) = cookies
            .split(';')
            .find(|c| c.trim().starts_with("session="))
            .and_then(|c| c.trim().strip_prefix("session="))
        {
            let session = sqlx::query!(
                "SELECT users.twitch_name, sessions.expires_at 
                 FROM sessions 
                 JOIN users ON users.id = sessions.twitch_id 
                 WHERE sessions.session_token = ?1",
                session_token
            ).fetch_optional(&*pool).await.map_err(|_| warp::reject::not_found())?;

            if let Some(session) = session {
                let expire= chrono::Utc::now().timestamp() + 604800;
                let _ = sqlx::query!(
                    "UPDATE sessions SET expires_at = ?1 WHERE session_token = ?2",
                    expire, session_token
                ).execute(&*pool).await;
                if session.expires_at > chrono::Utc::now().timestamp() {
                    let name = format!("#{}", session.twitch_name.unwrap());
                    let config = BotConfig::load_config();
                    if config.channels.contains_key(&name) {
                        return Ok(name);
                    }
                }
            }
        }
    }

    Err(warp::reject::custom(NotAuthorizedError))
}

pub fn with_authorization(pool: Arc<SqlitePool>) -> impl Filter<Extract = (String,), Error = Rejection> + Clone {
    warp::header::optional("cookie")
        .and(warp::any().map(move || pool.clone()))
        .and_then(check_authorization)
}

// Error handling for unauthorized access
#[derive(Debug)]
struct NotAuthorizedError;

impl warp::reject::Reject for NotAuthorizedError {}

pub async fn get_public_queue(streamer: String, pool: Arc<SqlitePool>) -> Result<impl Reply, warp::Rejection> {
    let channel_id = format!("#{}", streamer.to_ascii_lowercase());
    let mut config = BotConfig::load_config();
    let config = config.get_channel_config_mut(&channel_id);
    let queue_channel = &config.queue_channel;
    let queue = sqlx::query_as!(
        QueueEntry,
        "SELECT position, twitch_name, bungie_name FROM queue WHERE channel_id = ? ORDER BY position ASC",
        queue_channel
    ).fetch_all(&*pool).await.map_err(|_| warp::reject::reject())?;

    if queue.is_empty() {
        return Ok(warp::reply::json(&queue));
    }
    let grouped_queue: Vec<Vec<QueueEntry>> = queue
        .chunks(config.teamsize)
        .map(|chunk| chunk.to_vec())
        .collect();
    Ok(warp::reply::json(&grouped_queue))
}

async fn get_twitch_name_from_cookie(cookies: Option<String>, pool: &SqlitePool) -> Option<String> {
    let cookie = cookies?;
    let session_token = cookie.split(';').find(|c| c.trim().starts_with("session="))?.trim().strip_prefix("session=")?;

    let row = sqlx::query!(
        "SELECT users.twitch_name, sessions.expires_at 
         FROM sessions 
         JOIN users ON users.id = sessions.twitch_id 
         WHERE sessions.session_token = ?",
        session_token
    ).fetch_optional(pool).await.ok()??;

    if row.expires_at > chrono::Utc::now().timestamp() {
        row.twitch_name
    } else {
        None
    }
}

pub async fn get_aliases_handler(pool: Arc<SqlitePool>, cookies: Option<String>) -> Result<impl warp::Reply, warp::Rejection> {
    if let Some(twitch_name) = get_twitch_name_from_cookie(cookies, &*pool).await {
        let channel = format!("#{twitch_name}").to_ascii_lowercase();
        let rows = sqlx::query!(
            "SELECT alias, command FROM command_aliases WHERE channel = ?",
            channel
        ).fetch_all(&*pool).await.map_err(|_| warp::reject::not_found())?;

        let aliases: HashMap<String, String> = rows
            .into_iter()
            .map(|row| (row.alias, row.command))
            .collect();

        return Ok(warp::reply::json(&aliases));
    }
    
    Err(warp::reject())
}

#[derive(Deserialize, Debug)]
pub struct AliasUpdate {
    alias: String,
    command: String,
}

pub async fn set_alias_handler(pool: Arc<SqlitePool>, cookies: Option<String>, bot_state: Arc<RwLock<BotState>>, body: AliasUpdate) -> Result<impl warp::Reply, warp::Rejection> {
    if let Some(twitch_name) = get_twitch_name_from_cookie(cookies, &*pool).await {
        let channel = format!("#{twitch_name}").to_ascii_lowercase();
        let alias = body.alias.to_ascii_lowercase();
        let command = body.command.to_ascii_lowercase();
        println!("{alias} {command} {:?}", body);
        sqlx::query!(
            "INSERT OR REPLACE INTO command_aliases (channel, alias, command) VALUES (?, ?, ?)",
            channel, alias, command
        ).execute(&*pool).await.map_err(|_| warp::reject::reject())?;
        let bot_state = bot_state.write().await;
        update_dispatcher_if_needed(&channel, &bot_state.config, &pool, Arc::clone(&bot_state.dispatchers)).await
            .map_err(|_| warp::reject())?;
        return Ok(warp::reply::with_status("Alias updated", warp::http::StatusCode::OK));
    }
    Err(warp::reject())
}

#[derive(Deserialize)]
pub struct AliasDelete {
    alias: String,
}

pub async fn delete_alias_handler(pool: Arc<SqlitePool>, cookies: Option<String>, bot_state: Arc<RwLock<BotState>>, body: AliasDelete) -> Result<impl warp::Reply, warp::Rejection> {
    if let Some(twitch_name) = get_twitch_name_from_cookie(cookies, &*pool).await {
        let channel = format!("#{twitch_name}").to_ascii_lowercase();
        let alias = body.alias.to_ascii_lowercase();
        sqlx::query!(
            "DELETE FROM command_aliases WHERE channel = ? AND alias = ?",
            channel, alias
        ).execute(&*pool).await.map_err(|_| warp::reject::reject())?;
        let bot_state = bot_state.write().await;
        update_dispatcher_if_needed(&channel, &bot_state.config, &pool, Arc::clone(&bot_state.dispatchers)).await
            .map_err(|_| warp::reject())?;
        return Ok(warp::reply::with_status("Alias removed", warp::http::StatusCode::OK));
    }
    Err(warp::reject())
}

#[derive(Serialize)]
pub struct CommandAliasView {
    pub command: String,
    pub default_aliases: Vec<String>,
    pub removed_default_aliases: Vec<String>,
    pub default_disabled: bool,
    pub custom_aliases: Vec<String>,
}

pub async fn get_all_command_aliases(cookies: Option<String>, pool: Arc<SqlitePool>) -> Result<impl warp::Reply, warp::Rejection> {
    if let Some(twitch_name) = get_twitch_name_from_cookie(cookies, &*pool).await {
        let channel = format!("#{twitch_name}").to_ascii_lowercase();
        // Get default alias removals
        let removed_aliases: HashSet<String> = query!(
            "SELECT alias FROM command_alias_removals WHERE channel = ?",
            channel
        ).fetch_all(&*pool).await.map_err(|_| warp::reject::reject())?.into_iter().map(|row| row.alias.to_lowercase()).collect();

        // Get disabled commands
        let disabled_commands: HashSet<String> = query!(
            "SELECT command FROM command_disabled WHERE channel = ?",
            channel
        ).fetch_all(&*pool).await.map_err(|_| warp::reject::reject())?.into_iter().map(|row| row.command.to_lowercase()).collect();

        // Get custom aliases
        let custom_aliases: Vec<(String, String)> = query!(
            "SELECT alias, command FROM command_aliases WHERE channel = ?",
            channel
        ).fetch_all(&*pool).await.map_err(|_| warp::reject::reject())?.into_iter().map(|row| (row.command.to_lowercase(), row.alias.to_lowercase())).collect();

        let mut commands: HashMap<String, CommandAliasView> = HashMap::new();

        for (_pkg, group) in COMMAND_GROUPS.iter() {
            for reg in &group.commands {
                let cmd = reg.command.name().to_string();
                let mut active = Vec::new();
                let mut removed = Vec::new();

                for alias in &reg.aliases {
                    let lower = alias.to_lowercase();
                    if removed_aliases.contains(&lower) {
                        removed.push(lower);
                    } else {
                        active.push(lower);
                    }
                }

                let custom = custom_aliases.iter()
                    .filter_map(|(c, a)| if c == &cmd.to_lowercase() { Some(a.clone()) } else { None })
                    .collect();

                commands.insert(cmd.clone(), CommandAliasView {
                    command: cmd.clone(),
                    default_aliases: active,
                    removed_default_aliases: removed,
                    default_disabled: disabled_commands.contains(&cmd),
                    custom_aliases: custom,
                });
            }
        }

        let mut result: Vec<_> = commands.into_values().collect();
        result.sort_by(|a, b| a.command.cmp(&b.command));
        return Ok(warp::reply::json(&result));
    }
    Err(warp::reject())
}

#[derive(Deserialize)]
pub struct DisableToggleRequest {
    command: String,
}

pub async fn toggle_default_command_handler(pool: Arc<SqlitePool>, cookies: Option<String>, bot_state: Arc<RwLock<BotState>>, body: DisableToggleRequest) -> Result<impl warp::Reply, warp::Rejection> {
    if let Some(twitch_name) = get_twitch_name_from_cookie(cookies, &*pool).await {
        let channel = format!("#{twitch_name}").to_ascii_lowercase();
        let command = body.command.to_ascii_lowercase();

        let existing = sqlx::query!(
            "SELECT * FROM command_disabled WHERE channel = ? AND command = ?",
            channel, command
        ).fetch_optional(&*pool).await.map_err(|_| warp::reject())?;

        if existing.is_some() {
            sqlx::query!(
                "DELETE FROM command_disabled WHERE channel = ? AND command = ?",
                channel, command
            ).execute(&*pool).await.map_err(|_| warp::reject())?;
        } else {
            sqlx::query!(
                "INSERT INTO command_disabled (channel, command) VALUES (?, ?)",
                channel, command
            ).execute(&*pool).await.map_err(|_| warp::reject())?;
        }
        let bot_state = bot_state.write().await;
        update_dispatcher_if_needed(&channel, &bot_state.config, &pool, Arc::clone(&bot_state.dispatchers)).await
            .map_err(|_| warp::reject())?;

        return Ok(warp::reply::with_status("Toggle success", warp::http::StatusCode::OK));
    }
    Err(warp::reject())
}

#[derive(Deserialize)]
pub struct DefaultAliasRemoval {
    command: String,
    alias: String,
}

pub async fn remove_default_alias_handler(pool: Arc<SqlitePool>, cookies: Option<String>, bot_state: Arc<RwLock<BotState>>, body: DefaultAliasRemoval) -> Result<impl warp::Reply, warp::Rejection> {
    if let Some(twitch_name) = get_twitch_name_from_cookie(cookies, &*pool).await {
        let channel = format!("#{twitch_name}").to_ascii_lowercase();
        let alias = body.alias.to_ascii_lowercase();

        sqlx::query!(
            "INSERT OR IGNORE INTO command_alias_removals (channel, alias) VALUES (?, ?)",
            channel,alias
        ).execute(&*pool).await.map_err(|_| warp::reject())?;
        let bot_state = bot_state.write().await;
        update_dispatcher_if_needed(&channel, &bot_state.config, &pool, Arc::clone(&bot_state.dispatchers))
            .await
            .map_err(|_| warp::reject())?;

        Ok(warp::reply::with_status("Alias removed", warp::http::StatusCode::OK))
    } else {
        Err(warp::reject())
    }
}

pub async fn restore_default_alias_handler(pool: Arc<SqlitePool>, cookies: Option<String>, bot_state: Arc<RwLock<BotState>>, body: DefaultAliasRemoval,) -> Result<impl warp::Reply, warp::Rejection> {
    if let Some(twitch_name) = get_twitch_name_from_cookie(cookies, &*pool).await {
        let channel = format!("#{twitch_name}").to_ascii_lowercase();
        let alias = body.alias.to_ascii_lowercase();

        sqlx::query!(
            "DELETE FROM command_alias_removals WHERE channel = ? AND alias = ?",
            channel, alias
        ).execute(&*pool).await.map_err(|_| warp::reject())?;
        let bot_state = bot_state.write().await;
        update_dispatcher_if_needed(&channel, &bot_state.config, &pool, bot_state.dispatchers.clone())
            .await
            .map_err(|_| warp::reject())?;

        Ok(warp::reply::with_status("Alias restored", warp::http::StatusCode::OK))
    } else {
        Err(warp::reject())
    }
}