use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::Value;
use tracing::info;

use crate::bot::{chat_event::chat_event::ChatEvent, commands::commands::BotResult, state::def::{BotError, BotSecrets, TwitchAppToken}};


//Fetch Lurkers
#[derive(Deserialize, Debug)]
struct ChatterResponse {
    data: Vec<Chatter>,
}

#[derive(Deserialize, Debug)]
struct Chatter {
    user_name: String,
}

pub async fn fetch_lurkers(broadcaster_id: &str, token: &str, client_id: &str) -> Vec<String> {
    let url = format!(
        "https://api.twitch.tv/helix/chat/chatters?broadcaster_id={}&moderator_id=1091219021",
        broadcaster_id
    );

    let res = reqwest::Client::new()
        .get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .header("Client-Id", client_id)
        .send()
        .await.unwrap().json::<ChatterResponse>().await;
    res.unwrap().data.into_iter().map(|c| c.user_name).collect()
}

pub async fn is_channel_live(channel_id: &str, token: &str, client_id: &str) -> Result<bool, reqwest::Error> {
    let url = format!("https://api.twitch.tv/helix/streams?user_login={}", channel_id);
    let response = reqwest::Client::new()
        .get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .header("Client-ID", client_id)
        .send()
        .await?;
    let json: serde_json::Value = response.json().await?;
    Ok(json["data"].as_array().map_or(false, |data| !data.is_empty()))
}



#[derive(Deserialize)]
struct UsersResponse {
    data: Vec<TwitchUser>,
}

#[derive(Deserialize)]
struct TwitchUser {
    id: String,
}

//Not actually checking follow status
pub async fn is_follower(event: &ChatEvent, oauth_token: &str, client_id: &str) -> bool {
    let client = reqwest::Client::new();

    let broadcaster_id = match &event.broadcaster_id {
        Some(id) => id,
        None => return true, // non-Twitch platforms
    };

    let url = format!("https://api.twitch.tv/helix/channels/followers?broadcaster_id={}&user_id={}", broadcaster_id, event.user.as_ref().unwrap().identity.platform_user_id);
    let follow_res = client
        .get(&url)
        .header("Client-Id", client_id)
        .bearer_auth(oauth_token)
        .send()
        .await;

    match follow_res {
        Ok(res) => {
            let text = res.text().await.unwrap_or_default();
            text.contains("user_id")
        }
        Err(_) => true, // fail-open (important)
    }
}

pub async fn resolve_twitch_user_id(login: &str, secrets: &BotSecrets, token: &str) -> BotResult<(String, String)> {
    let url = format!("https://api.twitch.tv/helix/users?login={}", login);
    let res = reqwest::Client::new()
        .get(url)
        .header("Client-Id", &secrets.bot_id)
        .bearer_auth(token)
        .send()
        .await?;

    let parsed: Value = serde_json::from_str(&res.text().await?)?;
    info!("{:?}", parsed);
    let data = parsed["data"].as_array().ok_or_else(|| BotError::Custom("Invalid Twitch response".into()))?;

    let user = data.get(0).ok_or_else(|| BotError::Custom("Twitch user not found".into()))?;

    let id = user["id"].as_str().ok_or_else(|| BotError::Custom("Can't parse ID".to_string()))?;

    let display = user["display_name"].as_str().ok_or_else(|| BotError::Custom("Can't parse display name".to_string()))?;

    Ok((id.to_string(), display.to_string()))   
}

#[derive(Deserialize)]
struct TwitchTokenResponse {
    access_token: String,
    expires_in: u64,
    token_type: String,
}

pub async fn create_twitch_app_token(secrets: &BotSecrets) -> BotResult<TwitchAppToken> {
    let body = format!(
        "client_id={}&client_secret={}&grant_type=client_credentials",
        secrets.bot_id,
        secrets.client_secret,
    );
    
    let res = reqwest::Client::new()
        .post("https://id.twitch.tv/oauth2/token")
        .body(body).send().await?;

    if !res.status().is_success() {
        return Err(BotError::Custom("Failed to acquire Twitch app token".into()));
    }

    let token: TwitchTokenResponse = res.json().await?;

    Ok(TwitchAppToken {
        access_token: token.access_token,
        expires_at: Instant::now() + Duration::from_secs(token.expires_in),
    })
}

