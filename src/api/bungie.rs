use reqwest::header::{HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::from_str;

use crate::bot::{commands::commands::BotResult, state::def::BotError};

#[derive(Serialize)]
struct BungieName {
    #[serde(rename = "displayName")]
    name: String,
    #[serde(rename = "displayNameCode")]
    code: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct MemberShip {
    #[serde(rename = "membershipId")]
    pub id: String,
    #[serde(rename = "membershipType")]
    pub type_m: i32,
}

#[derive(Deserialize, Debug)]
struct MembershipIdResponse {
    Response: Vec<MemberShip>,
    ErrorCode: i32,
    ThrottleSeconds: i32,
    ErrorStatus: String,
    Message: String,
    MessageData: serde_json::Value,
}

//https://www.bungie.net/Platform/Destiny2/ {MembershipType} /Account/ {MembershipId} /Character/0/Stats/?groups=&modes=4 and ['Response']['raid']['allTime']['activitiesCleared']['basic']['displayValue']
pub async fn get_membershipid(bungie_name: &str, x_api_key: &str) -> BotResult<MemberShip> {
    let bungie_name = bungie_name.to_string();
    let (display_name, display_name_code) = bungie_name.split_once("#").unwrap();
    
    let bungie_name = BungieName {
        name: display_name.to_string(),
        code: display_name_code.to_string(),
    };

    let mut headers = HeaderMap::new();
    headers.insert("X-API-Key", HeaderValue::from_str(x_api_key).unwrap());
    //headers.insert("User-Agent", HeaderValue::from_str("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/112.0.0.0 Safari/537.36").unwrap());

    let res = reqwest::Client::new()
        .post("https://www.bungie.net/Platform/Destiny2/SearchDestinyPlayerByBungieName/All/")
        .headers(headers)
        .json(&bungie_name)
        .send()
        .await?;

    if res.status().is_success() {
        let body = res.text().await.unwrap();
        let body: MembershipIdResponse = from_str(&body).unwrap();

        let mut users: Vec<MemberShip> = vec![];
        for user in body.Response {
            println!("membershipId: {}, membershipType: {}", user.id, user.type_m);
            users.push(user);
        }
        if users.len() == 0 {
            Ok(MemberShip {
                id: String::new(),
                type_m: -1,
            })
        } else {
            Ok(users[0].clone())
        }
    } else {
        println!("Request failed with status: {}", res.status());
        Err(BotError::Custom("Poopoo".to_string()))
    }
}

pub struct BungieUser {
    pub bungie_name: String,
    pub membership_id: String,
    pub membership_type: i32,
}

pub async fn is_real_bungiename(
    x_api_key: &str,
    bungie_name: &str,
) -> Result<BungieUser, ()> {
    match get_membershipid(bungie_name, x_api_key).await {
        Ok(info) if info.type_m != -1 => Ok(BungieUser {
            bungie_name: bungie_name.to_string(),
            membership_id: info.id.to_string(),
            membership_type: info.type_m,
        }),
        _ => Err(()),
    }
}