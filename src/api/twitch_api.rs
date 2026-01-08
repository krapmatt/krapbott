use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::bot::{chat_event::chat_event::ChatEvent, commands::commands::BotResult, state::def::{BotError, BotSecrets}};


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

pub async fn get_twitch_user_id(username: &str, oauth_token: &str, client_id: &str) -> Result<String, BotError> {
    let url = format!("https://api.twitch.tv/helix/users?login={}", username);

    let client = reqwest::Client::new();
    let res = client
        .get(&url)
        .header("Client-id", client_id)
        .bearer_auth(oauth_token)
        .send()
        .await?;
    
    let parsed: Value = serde_json::from_str(&res.text().await?)?;
    if let Some(id) = parsed["data"][0]["id"].as_str() {
        return Ok(id.to_string()); 
    } else {
        Err(BotError::Custom("Failed to parse twitch id".to_string()))
    }

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

pub async fn resolve_twitch_user_id(login: &str, secrets: &BotSecrets) -> BotResult<(String, String)> {
    #[derive(Deserialize)]
    struct TwitchUser {
        id: String,
        display_name: String,
    }

    #[derive(Deserialize)]
    struct TwitchResponse {
        data: Vec<TwitchUser>,
    }
    let url = format!("https://api.twitch.tv/helix/users?login={}", login);
    let res = reqwest::Client::new()
        .get(url)
        .header("Client-Id", &secrets.bot_id)
        .bearer_auth(&secrets.oauth_token_bot)
        .send()
        .await?
        .json::<TwitchResponse>()
        .await?;

    let user = res.data.into_iter().next()
        .ok_or_else(|| BotError::Custom("User not found".into()))?;

    Ok((user.id, user.display_name))
}