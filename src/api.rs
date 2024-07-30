use futures::TryFutureExt;
use serde::{Deserialize, Serialize};
use serde_json::{from_str, json, Value};

use crate::models::BotError;

#[derive(Serialize)]
struct BungieName {
    displayName: String,
    displayNameCode: String
}

#[derive(Deserialize, Debug, Clone)]
pub struct MemberShip {
    membershipId: String,
    membershipType: i32,
}

#[derive(Deserialize, Debug)]
struct ApiResponse {
    Response: Vec<MemberShip>,
    ErrorCode: i32,
    ThrottleSeconds: i32,
    ErrorStatus: String,
    Message: String,
    MessageData: serde_json::Value,
}

pub async fn get_membershipid() -> Result<MemberShip, BotError> {
    let bungiename = BungieName {
        displayName: "KrapMatt".to_string(),
        displayNameCode: "1497".to_string(),
    };
    let res = reqwest::Client::new()
        .post("https://www.bungie.net/Platform/Destiny2/SearchDestinyPlayerByBungieName/All/")
        .header("X-API-Key", "7423a7dc6a504e8685ab412f17a4477f")
        .json(&bungiename)
        .send()
        .await?;

    if res.status().is_success() {
        let body = res.text().await.unwrap();
        let body:ApiResponse = from_str(&body).unwrap();
        // Print membershipId and membershipType for each user
        let mut users: Vec<MemberShip> = vec![];
        for user in body.Response {
            //println!("membershipId: {}, membershipType: {}", user.membershipId, user.membershipType);
            users.push(user);
            
        }
        Ok(users[0].clone())
    } else {
        println!("Request failed with status: {}", res.status());
        Err(BotError{error_code: 106, string: Some("dojebal jsi to".to_string())})
    }
}

pub async fn get_character_act() {
    get_character_ids().await;
    /*let res = reqwest::Client::new()
    .get("https://www.bungie.net/Platform/Destiny2/3/Profile/4611686018493345248/?components=204")
    .header("X-API-Key", "7423a7dc6a504e8685ab412f17a4477f")
    .send()
    .await.unwrap();
    println!("{:?}", res.text().await);*/
}

pub async fn get_character_ids() -> Result<Vec<String>, BotError> {
    let url = format!("https://www.bungie.net/Platform/Destiny2/3/Profile/4611686018493345248/?components=200");
    let res = reqwest::Client::new()
        .get(&url)
        .header("X-API-Key", "7423a7dc6a504e8685ab412f17a4477f")
        .send()
        .await?;

    if res.status().is_success() {
        let body: Vec<String> = res.text().await.into_iter().collect();
        println!("{:?}", body);
    }
    Err(BotError{error_code: 111, string: Some("Failed to get character IDs".to_string())})
}


