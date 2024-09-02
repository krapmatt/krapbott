use std::{collections::HashMap, env::var};
use serde::{Deserialize, Serialize};
use serde_json::{from_str, json, Value};

use crate::models::BotError;

#[derive(Serialize)]
struct BungieName {
    #[serde(rename = "displayName")]
    name: String,
    #[serde(rename = "displayNameCode")]
    code: String
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
#[derive(Serialize, Deserialize, Debug)]
pub struct CharacterIdResponse {
    #[serde(rename = "Response")]
    response: Response,
    #[serde(rename = "ErrorCode")]
    error_code: i32,
    #[serde(rename = "ThrottleSeconds")]
    throttle_seconds: i32,
    #[serde(rename = "ErrorStatus")]
    error_status: String,
    #[serde(rename = "Message")]
    message: String,
    #[serde(rename = "MessageData")]
    message_data: HashMap<String, String>,
}
#[derive(Serialize, Deserialize, Debug)]
pub struct Response {
    #[serde(rename = "responseMintedTimestamp")]
    response_minted_timestamp: String,
    #[serde(rename = "secondaryComponentsMintedTimestamp")]
    secondary_components_minted_timestamp: String,
    #[serde(rename = "characters")]
    characters: Characters,
    #[serde(rename = "privacy")]
    privacy: Option<i32>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Characters {
    #[serde(rename = "data")]
    data: HashMap<String, CharacterData>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CharacterData {
    #[serde(rename = "membershipId")]
    membership_id: String,
    #[serde(rename = "membershipType")]
    membership_type: i32,
    #[serde(rename = "characterId")]
    character_id: String,
    #[serde(rename = "dateLastPlayed")]
    date_last_played: String,
    #[serde(rename = "minutesPlayedThisSession")]
    minutes_played_this_session: String,
    #[serde(rename = "minutesPlayedTotal")]
    minutes_played_total: String,
    #[serde(rename = "light")]
    light: i32,
    #[serde(rename = "stats")]
    stats: HashMap<String, i32>,
    #[serde(rename = "raceHash")]
    race_hash: i64,
    #[serde(rename = "genderHash")]
    gender_hash: i64,
    #[serde(rename = "classHash")]
    class_hash: i64,
    #[serde(rename = "raceType")]
    race_type: i32,
    #[serde(rename = "classType")]
    class_type: i32,
    #[serde(rename = "genderType")]
    gender_type: i32,
    #[serde(rename = "emblemPath")]
    emblem_path: String,
    #[serde(rename = "emblemBackgroundPath")]
    emblem_background_path: String,
    #[serde(rename = "emblemHash")]
    emblem_hash: i64,
    #[serde(rename = "emblemColor")]
    emblem_color: EmblemColor,
    #[serde(rename = "levelProgression")]
    level_progression: LevelProgression,
    #[serde(rename = "baseCharacterLevel")]
    base_character_level: i32,
    #[serde(rename = "percentToNextLevel")]
    percent_to_next_level: f64,
    #[serde(rename = "titleRecordHash")]
    title_record_hash: i64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct EmblemColor {
    red: i32,
    green: i32,
    blue: i32,
    alpha: i32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct LevelProgression {
    #[serde(rename = "progressionHash")]
    progression_hash: i64,
    #[serde(rename = "dailyProgress")]
    daily_progress: i32,
    #[serde(rename = "dailyLimit")]
    daily_limit: i32,
    #[serde(rename = "weeklyProgress")]
    weekly_progress: i32,
    #[serde(rename = "weeklyLimit")]
    weekly_limit: i32,
    #[serde(rename = "currentProgress")]
    current_progress: i32,
    #[serde(rename = "level")]
    level: i32,
    #[serde(rename = "levelCap")]
    level_cap: i32,
    #[serde(rename = "stepIndex")]
    step_index: i32,
    #[serde(rename = "progressToNextLevel")]
    progress_to_next_level: i32,
    #[serde(rename = "nextLevelAt")]
    next_level_at: i32,
}

#[derive(Deserialize, Debug)]
struct ActivitiesClearedResponse {
    Response: ResponseActivities,
    ErrorCode: i32,
    ThrottleSeconds: i32,
    ErrorStatus: String,
    Message: String,
    MessageData: serde_json::Value,
}

#[derive(Deserialize, Debug)]
struct ResponseActivities {
    raid: Option<RaidStats>,
}

#[derive(Deserialize, Debug)]
struct RaidStats {
    allTime: Option<AllTimeStats>,
}

#[derive(Deserialize, Debug)]
struct AllTimeStats {
    activitiesCleared: Option<ActivitiesCleared>,
}

#[derive(Deserialize, Debug)]
struct ActivitiesCleared {
    basic: BasicStats,
}

#[derive(Deserialize, Debug)]
struct BasicStats {
    value: f64,
    displayValue: String,
}
/*then use GetProfile
    get their characters
    then use GetActivityHistory
    to get the activities
    https://data.destinysets.com/api
    use this
    mode=4 is Raid */

    //https://www.bungie.net/Platform/Destiny2/ {MembershipType} /Account/ {MembershipId} /Character/0/Stats/?groups=&modes=4 and ['Response']['raid']['allTime']['activitiesCleared']['basic']['displayValue']
pub async fn get_membershipid(bungie_name: String, x_api_key: String) -> Result<MemberShip, BotError> {
    let (display_name, display_name_code) = bungie_name.split_once("#").unwrap();
    
    let bungie_name = BungieName {
        name: display_name.to_string(),
        code: display_name_code.to_string(),
    };
    let res = reqwest::Client::new()
        .post("https://www.bungie.net/Platform/Destiny2/SearchDestinyPlayerByBungieName/All/")
        .header("X-API-Key", x_api_key)
        .json(&bungie_name)
        .send()
        .await?;

    if res.status().is_success() {
        let body = res.text().await.unwrap();
        let body:MembershipIdResponse = from_str(&body).unwrap();
        
        let mut users: Vec<MemberShip> = vec![];
        for user in body.Response {
            println!("membershipId: {}, membershipType: {}", user.id, user.type_m);
            users.push(user);
        }
        if users.len() == 0 {
            Ok(MemberShip{id: String::new(), type_m: -1})
        } else {
            Ok(users[0].clone())
        }
        
    } else {
        println!("Request failed with status: {}", res.status());
        Err(BotError{error_code: 106, string: Some("dojebal jsi to".to_string())})
    }
}



pub async fn get_users_clears(membership_id: String, membership_type: i32, x_api_key: String) -> Result<f64, BotError> {
    let res = reqwest::Client::new()
    .get(format!("https://www.bungie.net/Platform/Destiny2/{}/Account/{}/Character/0/Stats/?groups=&modes=4", membership_type, membership_id))
    .header("X-API-Key", x_api_key)
    .send()
    .await?;
    if res.status().is_success() {
        let body = res.text().await.unwrap();
        let api_response: ActivitiesClearedResponse = serde_json::from_str(&body).unwrap();
        if let Some(raid) = api_response.Response.raid {
            if let Some(all_time) = raid.allTime {
                if let Some(activities_cleared) = all_time.activitiesCleared {
                    return Ok(activities_cleared.basic.value);
                }
            }
        }
    }
    Err(BotError{error_code: 112, string: Some("Failed to get activities cleared".to_string())})
}

pub async fn get_character_ids(membership_id: String, membership_type: i32, x_api_key: String) -> Result<Vec<String>, BotError> {
    let url = format!("https://www.bungie.net/Platform/Destiny2/{}/Profile/{}/?components=200", membership_type, membership_id);
    let res = reqwest::Client::new()
        .get(&url)
        .header("X-API-Key", x_api_key)
        .send()
        .await?;

    if res.status().is_success() {
        let body = res.text().await.unwrap();
        
        let api_response: CharacterIdResponse = serde_json::from_str(&body).unwrap();
        let mut character_id_string:Vec<String> = vec![];
        for (character_id, _character_data) in api_response.response.characters.data.iter() {
            println!("Character ID: {}", character_id);
            character_id_string.push(character_id.to_string());
        
        }
        println!("{:?}", character_id_string);
    }
    Err(BotError{error_code: 111, string: Some("Failed to get character IDs".to_string())})
}


