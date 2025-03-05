use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::{mpsc::UnboundedSender, Mutex};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use warp::{http::StatusCode, reject, reply::json, Filter};

use crate::models::BotConfig;

#[derive(Serialize, Clone)]
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

pub async fn get_queue_handler(
    channel_id: String,
    pool: Arc<SqlitePool>,
) -> Result<impl warp::Reply, warp::Rejection> {
    // Simulate fetching queue data¨
    let channel_id = format!("#{}", channel_id.to_ascii_lowercase());
    let queue = sqlx::query_as!(
        QueueEntry,
        "SELECT position, twitch_name, bungie_name FROM queue WHERE channel_id = ? ORDER BY position ASC",
        channel_id
    )
    .fetch_all(&*pool).await
    .map_err(|_| warp::reject::reject())?;

    let mut config = BotConfig::load_config();
    let config = config.get_channel_config_mut(&channel_id);

    if queue.is_empty() {
        return Ok(warp::reply::json(&queue));
    }
    let grouped_queue: Vec<Vec<QueueEntry>> = queue
        .chunks(config.teamsize)
        .map(|chunk| chunk.to_vec())
        .collect();
    Ok(warp::reply::json(&grouped_queue))
}

pub async fn remove_from_queue_handler(
    body: serde_json::Value,
    pool: Arc<SqlitePool>,
) -> Result<impl warp::Reply, warp::Rejection> {
    let twitch_name = body["twitch_name"].as_str().ok_or_else(|| reject())?;
    let channel_name = body["channel_id"].as_str().ok_or_else(|| reject())?;
    let channel_id = format!("#{}", channel_name.to_ascii_lowercase());

    let position = sqlx::query!(
        "SELECT position FROM queue WHERE twitch_name = ? AND channel_id = ?",
        twitch_name,
        channel_id
    )
    .fetch_optional(&*pool)
    .await
    .map_err(|_| warp::reject())?;

    if position.is_some() {
        // Remove the user from queue
        sqlx::query!(
            "DELETE FROM queue WHERE twitch_name = ? AND channel_id = ?",
            twitch_name,
            channel_id
        )
        .execute(&*pool)
        .await
        .map_err(|_| warp::reject())?;

        let queue_entries = sqlx::query!(
            "SELECT twitch_name FROM queue WHERE channel_id = ? ORDER BY position ASC",
            channel_id
        )
        .fetch_all(&*pool)
        .await
        .map_err(|_| warp::reject())?;

        let mut new_position = 1;
        for entry in queue_entries {
            sqlx::query!(
                "UPDATE queue SET position = ? WHERE twitch_name = ? AND channel_id = ?",
                new_position,
                entry.twitch_name,
                channel_id
            )
            .execute(&*pool)
            .await
            .map_err(|_| warp::reject())?;
            new_position += 1;
        }
        return Ok(warp::reply::with_status("Removed", StatusCode::OK));
    }

    Ok(warp::reply::with_status(
        "Failed to remove",
        StatusCode::INTERNAL_SERVER_ERROR,
    ))
}

#[derive(Deserialize, Debug)]
pub struct UpdateQueueOrderRequest {
    pub channel_id: String,
    pub new_order: Vec<QueueUpdate>,
}

#[derive(Deserialize, Debug)]
pub struct QueueUpdate {
    pub twitch_name: String,
    pub position: i64,
}

pub async fn update_queue_order(
    data: UpdateQueueOrderRequest,
    pool: Arc<SqlitePool>,
) -> Result<impl warp::Reply, warp::Rejection> {
    let channel_id = format!("#{}", &data.channel_id.to_lowercase());
    sqlx::query!(
        "UPDATE queue SET position = position + 10000 WHERE channel_id = ?",
        channel_id
    )
    .execute(&*pool)
    .await
    .map_err(|_| warp::reject())?;
    for entry in &data.new_order {
        sqlx::query!(
            "UPDATE queue SET position = ? WHERE twitch_name = ? AND channel_id = ?",
            entry.position,
            entry.twitch_name,
            channel_id
        )
        .execute(&*pool)
        .await
        .map_err(|_| warp::reject())?;
        println!("{}", entry.twitch_name);
        println!("Updated data {:?}", entry);
    }

    Ok(warp::reply::json(&"Queue order updated"))
}

pub async fn next_queue_handler(
    channel_id: String,
    sender: Arc<UnboundedSender<(String, String)>>,
) -> Result<impl warp::Reply, warp::Rejection> {
    if let Err(err) = sender.send((channel_id.clone(), "next".to_string())) {
        eprintln!("Failed to send message: {:?}", err);
        return Ok(warp::reply::with_status(
            "Failed to send message",
            StatusCode::INTERNAL_SERVER_ERROR,
        ));
    }

    Ok(warp::reply::with_status(
        "Next group message sent",
        StatusCode::OK,
    ))
}
pub async fn keep_channel_alive(
    sender: Arc<Mutex<UnboundedSender<(String, String)>>>,
) -> Result<impl warp::Reply, warp::Rejection> {
    if let Err(err) = sender
        .lock()
        .await
        .send(("keep".to_string(), "alive".to_string()))
    {
        eprintln!("Failed to send message: {:?}", err);
        return Ok(warp::reply::with_status(
            "Failed to send message",
            StatusCode::INTERNAL_SERVER_ERROR,
        ));
    }

    Ok(warp::reply::with_status(
        "Next group message sent",
        StatusCode::OK,
    ))
}

pub async fn get_run_counter_handler(
    channel_id: String,
) -> Result<impl warp::Reply, warp::Rejection> {
    let mut config = BotConfig::load_config();
    let channel_id = format!("#{}", channel_id);
    if config.channels.contains_key(&channel_id) {
        let channel_config = config.get_channel_config_mut(&channel_id);
        let runs = channel_config.runs;
        return Ok(json(&json!({"run_counter": runs})));
    } else {
        Err(reject())
    }
}

pub async fn get_queue_state_handler(
    channel_id: String,
) -> Result<impl warp::Reply, warp::Rejection> {
    let mut config = BotConfig::load_config();
    let channel_id = format!("#{}", channel_id);
    let config = config.get_channel_config_mut(&channel_id);
    let is_open = config.open;
    println!("{}", is_open);
    Ok(warp::reply::json(
        &serde_json::json!({ "is_open": is_open }),
    ))
}
pub async fn toggle_queue_handler(
    toggle_action: String,
    channel_id: String,
) -> Result<impl warp::Reply, warp::Rejection> {
    let mut config = BotConfig::load_config();
    let channel_id = format!("#{}", channel_id);
    let channel_config = config.get_channel_config_mut(&channel_id);
    match toggle_action.as_str() {
        "open" => {
            channel_config.open = true;
            config.save_config();
            Ok(warp::reply::json(
                &serde_json::json!({ "success": true, "state": "open" }),
            ))
        }
        "close" => {
            channel_config.open = false;
            config.save_config();
            Ok(warp::reply::json(
                &serde_json::json!({ "success": true, "state": "closed" }),
            ))
        }
        _ => Err(warp::reject()),
    }
}

pub async fn sabotage_truck_queue() -> Result<impl warp::Reply, warp::Rejection> {
    let mut config = BotConfig::load_config();
    config.get_channel_config_mut("#nyc62truck").open =
        !config.get_channel_config_mut("#nyc62truck").open;
    config.save_config();
    Ok(warp::reply::with_status("Sabotaged", StatusCode::OK))
}
