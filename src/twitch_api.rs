use std::sync::Arc;

use dotenvy::{dotenv, var};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{bot_commands::reply_to_message, models::BotError};

//Ban Bots
#[derive(Serialize)]
struct BanRequest {
    data: BanData,
}
#[derive(Serialize)]
struct BanData {
    user_id: String,
}

//Announcement
#[derive(Serialize)]
struct Data {
    message: String,
    color: String
}

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


// Best viewers on u.to/paq8IA
pub async fn ban_bots(msg: &tmi::Privmsg<'_>, oauth_token: &str, client_id: String) {
    let url = format!("https://api.twitch.tv/helix/moderation/bans?broadcaster_id={}&moderator_id=1091219021", msg.channel_id());
    
    let ban_request = BanRequest {
        data: BanData {
            user_id: msg.sender().id().to_string(),
        },
    };
    let res = reqwest::Client::new()
        .post(&url)
        .bearer_auth(oauth_token)
        .header("Client-Id", client_id)

        .header("Content-Type", "application/json")
        .body(serde_json::to_string(&ban_request).unwrap())
        
        .send()
        .await.expect("Bad reqwest");
    println!("{:?}", res.text().await);
}

pub async fn shoutout(oauth_token: &str, client_id: String, to_broadcaster_id: &str, broadcaster: &str) -> Result<(), BotError> {
    let url = format!("https://api.twitch.tv/helix/chat/shoutouts?from_broadcaster_id={}&to_broadcaster_id={}&moderator_id=1091219021", broadcaster, to_broadcaster_id);
    reqwest::Client::new().post(url).bearer_auth(oauth_token).header("Client-Id", client_id).send().await?;
    Ok(())
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

pub async fn get_twitch_user_id(username: &str) -> Result<String, BotError> {
    let url = format!("https://api.twitch.tv/helix/users?login={}", username);

    let oauth_token = var("TWITCH_OAUTH_TOKEN_BOTT").expect("No oauth token");
    let client_id = var("TWITCH_CLIENT_ID_BOT").expect("No bot id");

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

//Not actually checking follow status
pub async fn is_follower(msg: &tmi::Privmsg<'_>, client: Arc<tokio::sync::Mutex<tmi::Client>>) -> bool {
    dotenv().ok();
    let oauth_token = var("TWITCH_OAUTH_TOKEN_BOTT").expect("No oauth token");
    let client_id = var("TWITCH_CLIENT_ID_BOT").expect("No bot id");

    let url = format!("https://api.twitch.tv/helix/channels/followers?broadcaster_id={}&user_id={}", msg.channel_id(), msg.sender().id());
    let res = reqwest::Client::new()
        .get(&url)
        .header("Client-Id", client_id)
        .bearer_auth(oauth_token)
        .send()
        .await.expect("Bad reqwest");
    //.
    if let Ok(a) = res.text().await  { 
        if a.contains("user_id") || msg.channel_id() == msg.sender().id() {
            true
        } else {
            let mut client = client.lock().await;
            let _ = reply_to_message(msg, &mut client, "You are not a follower!").await;
            false
        }
    } else {
        let mut client = client.lock().await;
        let _ = reply_to_message(msg, &mut client, "Error occured! Tell Matt").await;
        true
    }
}

//Make announcment automatizations!
pub async fn announcement(broadcaster_id: &str, mod_id: &str, oauth_token: &str, client_id: String, message: String) -> Result<(), BotError> {
    let url = format!("https://api.twitch.tv/helix/chat/announcements?broadcaster_id={}&moderator_id={}", broadcaster_id, mod_id);
    let res = reqwest::Client::new()
        .post(&url)
        .header("Client-Id", client_id)
        .bearer_auth(oauth_token)
        .form(&Data {message: message, color: "primary".to_string()})
        .send()
        .await.expect("Bad reqwest");
    println!("{:?}", res.text().await);
    
    Ok(())
}