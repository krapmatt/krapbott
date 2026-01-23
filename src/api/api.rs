use reqwest::{header::{HeaderMap, HeaderValue}, Client};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashMap, fs::{self, File}, io::Write};

use crate::bot::state::def::BotError;



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



pub async fn get_users_clears(
    membership_id: String,
    membership_type: i32,
    x_api_key: String,
) -> Result<f64, BotError> {
    let mut headers = HeaderMap::new();
    headers.insert("X-API-Key", HeaderValue::from_str(&x_api_key).unwrap());
    headers.insert("User-Agent", HeaderValue::from_str("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/112.0.0.0 Safari/537.36").unwrap());
    let res = reqwest::Client::new()
    .get(format!("https://www.bungie.net/Platform/Destiny2/{}/Account/{}/Character/0/Stats/?groups=&modes=4", membership_type, membership_id))
    .headers(headers)
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
    Err(BotError::Custom("Failed to get activities cleared".to_string()))
}

pub async fn get_character_ids(membership_id: String, membership_type: i32, x_api_key: String) -> Result<Vec<String>, BotError> {
    let url = format!(
        "https://www.bungie.net/Platform/Destiny2/{}/Profile/{}/?components=200",
        membership_type, membership_id
    );
    let mut headers = HeaderMap::new();
    headers.insert("X-API-Key", HeaderValue::from_str(&x_api_key).unwrap());
    headers.insert("User-Agent", HeaderValue::from_str("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/112.0.0.0 Safari/537.36").unwrap());
    let res = reqwest::Client::new()
        .get(&url).headers(headers)
        .send()
        .await?;

    if res.status().is_success() {
        let body = res.text().await.unwrap();

        let api_response: CharacterIdResponse = serde_json::from_str(&body).unwrap();
        let mut character_id_string: Vec<String> = vec![];
        for (character_id, _character_data) in api_response.response.characters.data.iter() {
            println!("Character ID: {}", character_id);
            character_id_string.push(character_id.to_string());
        }
        println!("{:?}", character_id_string);
    }
    Err(BotError::Custom("Failed to get character IDs".to_string()))
}
// https://www.bungie.net/Platform/Destiny2/3/Profile/4611686018493345248/?components=204
// https://www.bungie.net/Platform/GroupV2/User/254/23506163/0/1/
// https://www.bungie.net/Platform/Destiny2/3/Profile/4611686018493345248/?components=Profiles,Characters,CharacterProgressions,CharacterActivities,CharacterEquipment,ItemInstances,CharacterInventories,ProfileInventories,ProfileProgression,ItemObjectives,PresentationNodes,Records,Collectibles,ItemSockets,ItemPlugObjectives,StringVariables
// https://www.bungie.net/Platform/Destiny2/Milestones/
pub async fn get_master_challenges(membership_type: i32, membership_id: String, x_api_key: &str, activity: String) -> Result<Vec<String>, BotError> {
    let url = format!("https://www.bungie.net/Platform/Destiny2/{}/Profile/{}/?components=Records", membership_type, membership_id);
    // IR YUT - 3256765903
    // crota - 3256765902
    // abyss - 3256765901
    // bridge - 3256765900
    //Conquest by virtue - 295018272

    let mut headers = HeaderMap::new();
    headers.insert("X-API-Key", HeaderValue::from_str(x_api_key).unwrap());
    headers.insert("User-Agent", HeaderValue::from_str("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/112.0.0.0 Safari/537.36").unwrap());

    let response = reqwest::Client::new().get(url).headers(headers.clone()).send().await?;
    let res: Value = response.json().await?;
    
    let mut hash = String::new();
    let activity = &activity.to_lowercase();
    if activity == "vog" {
        hash = get_record_name("Maestro Glasser", headers.clone()).await?;
    } else if activity == "vow" {
        hash = get_record_name("Pyramid Conqueror", headers.clone()).await?;
    } else if activity == "ron" {
        hash = get_record_name("Final Nightmare", headers.clone()).await?;
    } else if activity == "se" {
        hash = get_record_name("Ignited Light", headers.clone()).await?;
    } else if activity == "kf" {
        hash = get_record_name("King of Kings", headers.clone()).await?;
    } else if activity == "ce" {
        hash = "295018272".to_string()
    }

    let mut result: Vec<String> = vec![];
    let mut triumph: Value = Value::Null;
    if activity == "ce" {
            if let Some(records) = res["Response"]["characterRecords"]["data"].as_object().and_then(|map| map.values().next()).and_then(|char_data| char_data.get("records")) {
                if let Some(trium) = records.get(&hash) {
                    triumph = trium.clone()
                }
            }
    } else {
        if let Some(records) = res["Response"]["profileRecords"]["data"]["records"].as_object() {
            if let Some(trium) = records.get(&hash) {
                triumph = trium.clone()
            }
        }
    }
    if let Some(objectives) = triumph["objectives"].as_array() {
        for objective in objectives {
            if let (Some(objective_hash), Some(progress)) = (
                objective["objectiveHash"].as_u64(),
                objective["progress"].as_u64(),
            ) {
                let name = get_name_by_hash(objective_hash, headers.clone()).await?;
                result.push(format!("{}: {}", name.strip_suffix(" completed").unwrap_or(&name), progress));
            }
        }
    }
    Ok(result)
}

async fn fetch_objective_manifest(headers: HeaderMap) -> Result<String, BotError> {
    // Step 1: Get the Manifest
    let manifest_url = "https://www.bungie.net/Platform/Destiny2/Manifest/";
    let client = reqwest::Client::new();

    let response = client.get(manifest_url).headers(headers.clone()).send().await?;
    let manifest: Value = response.json().await?;

    let activity_def_url = format!(
        "https://www.bungie.net{}",
        manifest["Response"]["jsonWorldComponentContentPaths"]["en"]["DestinyObjectiveDefinition"]
            .as_str()
            .unwrap()
    );

    // Step 2: Fetch DestinyActivityDefinition
    let response = client.get(&activity_def_url).headers(headers).send().await?;
    let content = response.text().await?;

    let mut file = File::create("objective_manifest_cache.json").unwrap();
    file.write_all(content.as_bytes());

    
    Ok(content)
}

fn load_objective_manifest() -> Result<String, BotError> {
    match fs::read_to_string("objective_manifest_cache.json") {
        Ok(content) => Ok(content),
        Err(_) => Err(BotError::IoError(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Manifest not found",
        ))),
    }
}

async fn get_name_by_hash(hash_number: u64, headers: HeaderMap) -> Result<String, BotError> {
    // Try loading the cached manifest
    let json_data = match load_objective_manifest() {
        Ok(data) => data,
        Err(_) => fetch_objective_manifest(headers).await?,
    };

    let record_json: HashMap<String, Value> = serde_json::from_str(&json_data)?;

    for record in record_json.values() {
        if let Some(hash) = record["hash"].as_u64() {
            if hash == hash_number {
                return Ok(record["progressDescription"]
                    .as_str()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "Unknown".to_string()));
            }
        }
    }

    Ok("None".to_string())
}




async fn fetch_record_manifest(headers: HeaderMap) -> Result<String, BotError> {
    let client = Client::new();
    
    let manifest_res = client
        .get("https://www.bungie.net/Platform/Destiny2/Manifest/").headers(headers.clone())
        .send()
        .await?;
    let manifest_json: Value = manifest_res.json().await?;
    let manifest_path = manifest_json["Response"]["jsonWorldComponentContentPaths"]["en"]["DestinyRecordDefinition"]
        .as_str()
        .unwrap();

    let manifest_url = format!("https://www.bungie.net{}", manifest_path);

    // Download DestinyRecordDefinition JSON
    let record_res = client.get(&manifest_url).headers(headers).send().await?;
    let content = record_res.text().await?;

    let mut file = File::create("record_manifest_cache.json").unwrap();
    file.write_all(content.as_bytes());

    
    Ok(content)
}

fn load_record_manifest() -> Result<String, BotError> {
    match fs::read_to_string("record_manifest_cache.json") {
        Ok(content) => Ok(content),
        Err(_) => Err(BotError::IoError(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Manifest not found",
        ))),
    }
}

/*"displayProperties": Object {"description": String("Acquire Major Boons or Corrupted Boons in the Nether activity."), "hasIcon": Bool(true), "icon": String("/common/destiny2_content/icons/bd92acccf9eafddf15512b496e15ec94.png"), "iconSequences": Array [Object {"frames": Array [String("/common/destiny2_content/icons/bd92acccf9eafddf15512b496e15ec94.png")]}, Object {"frames": Array [String("/common/destiny2_content/icons/814207426a3ba44b8a3f1eb2606a544a.png")]}], "name": String("Major Boon Collector")}, "expirationInfo": Object {"description": String(""), "hasExpiration": Bool(false)}, "forTitleGilding": Bool(false), "hash": Number(1541333176), "index": Number(4501), "intervalInfo": Object {"intervalObjectives": Array [Object {"intervalObjectiveHash": Number(3821016597), "intervalScoreValue": Number(10)}, Object {"intervalObjectiveHash": Number(3821016596), "intervalScoreValue": Number(8)}, Object {"intervalObjectiveHash": Number(3821016599), "intervalScoreValue": Number(6)}, Object {"intervalObjectiveHash": Number(3821016598), "intervalScoreValue": Number(4)}, Object {"intervalObjectiveHash": Number(3821016593), "intervalScoreValue": Number(2)}], "intervalRewards": Array [Object {"intervalRewardItems": Array []}, Object {"intervalRewardItems": Array []}, Object {"intervalRewardItems": Array []}, Object {"intervalRewardItems": Array []}, Object {"intervalRewardItems": Array []}], "isIntervalVersionedFromNormalRecord": Bool(false), "originalObjectiveArrayInsertionIndex": Number(0)}, "objectiveHashes": Array [], "parentNodeHashes": Array [Number(1093550159)], "presentationNodeType": Number(3), "recordTypeName": String("Triumphs"), "recordValueStyle": Number(0), "redacted": Bool(false), "requirements": Object {"entitlementUnavailableMessage": String("")}, "rewardItems": Array [], "scope": Number(0), "shouldShowLargeIcons": Bool(false), "stateInfo": Object {"claimedUnlockHash": Number(0), "completeUnlockHash": Number(0), "completedCounterUnlockValueHash": Number(0), "featuredPriority": Number(2147483647), "obscuredDescription": String(""), "obscuredName": String("")}, "titleInfo": Object {"hasTitle": Bool(false)}, "traitHashes": Array [], "traitIds": Array []}, */
pub async fn get_record_name(record_name: &str, headers: HeaderMap) -> Result<String, BotError> {
    //Get manifest definations
    let json_data = match load_record_manifest() {
        Ok(data) => data,
        Err(_) => fetch_record_manifest(headers).await?,
    };
    //Make string into hashmap
    let record_json: HashMap<String, Value> = serde_json::from_str(&json_data)?;
    //Get Hash for name
    for (hash, record) in record_json {
        if let Some(name) = record["displayProperties"]["name"].as_str() {
            if name.eq_ignore_ascii_case(record_name) {
                return Ok(hash.clone()); // Return the hash if found
            }
        }
    }
    Ok("Unknown Record".to_string())
}