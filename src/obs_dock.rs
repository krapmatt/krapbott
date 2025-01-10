
use async_sqlite::rusqlite::params;
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[derive(Serialize, Clone)]
struct QueueEntry {
    position: i32,
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

    ws_stream.feed(Message::Text(auth_message.to_string().into())).await?;
    // Listen for incoming messages
    while let Some(msg) = ws_stream.next().await {
        let msg = msg?;
        if let Message::Text(text) = msg {
            println!("OBS Message: {}", text);
        }
    }

    Ok(())
}

pub async fn get_queue_handler(channel_id: String) -> Result<impl warp::Reply, warp::Rejection> {
    // Simulate fetching queue data¨
    let channel_id = format!("#{}", channel_id.to_ascii_lowercase());
    let conn = initialize_database();
    let mut stmt = conn.prepare("SELECT position, twitch_name, bungie_name FROM queue WHERE channel_id = ? ORDER BY position ASC").unwrap();
    
    let mut config = BotConfig::load_config();
    let config = config.get_channel_config(&channel_id);

    let rows = stmt.query_map(params![channel_id], |row| {
            Ok(QueueEntry{
                position: row.get(0)?,
                twitch_name: row.get(1)?,
                bungie_name: row.get(2)?,
            })
        }).unwrap();
    let queue: Vec<QueueEntry> = rows.filter_map(Result::ok).collect();
    if queue.is_empty() {
        return Ok(warp::reply::json(&queue));
    }
    let grouped_queue: Vec<Vec<QueueEntry>> = queue.chunks(config.teamsize).map(|chunk| chunk.to_vec()).collect();

    Ok(warp::reply::json(&grouped_queue))
    
}
use warp::{http::StatusCode, reject};

use crate::{database::initialize_database, models::{BotConfig, BotError}};

pub async fn remove_from_queue_handler(
    body: serde_json::Value,
) -> Result<impl warp::Reply, warp::Rejection> {
    let twitch_name = body["twitch_name"].as_str().ok_or_else(|| {
        reject()
    })?;

    let conn = initialize_database();
    let mut result = Ok(0);
    if let Ok(_pos) = conn.query_row(
        "SELECT position FROM queue WHERE twitch_name = ?1 AND channel_id = ?2",
        params![twitch_name, "#krapmatt"],
        |row| row.get::<_, i32>(0),
    ) {
        result = conn.execute(
            "DELETE FROM queue WHERE twitch_name = ?1 AND channel_id = ?2",
            params![twitch_name, "#krapmatt"],
        );
        let mut stmt = conn.prepare(
            "SELECT twitch_name FROM queue WHERE channel_id = ?1 ORDER BY position ASC",
        ).unwrap();
        let mut rows = stmt.query(params!["#krapmatt"]).unwrap();
        let mut new_position = 1;
        while let Ok(Some(row)) = rows.next() {
            let name: String = row.get(0).unwrap();
            let _ = conn.execute(
                "UPDATE queue SET position = ?1 WHERE twitch_name = ?2 AND channel_id = ?3",
                params![new_position, name, "#krapmatt"],
            ); 
            new_position += 1;
        }
    }

    match result {
        Ok(_) => Ok(warp::reply::with_status("Removed", StatusCode::OK)),
        Err(_) => Ok(warp::reply::with_status(
            "Failed to remove",
            StatusCode::INTERNAL_SERVER_ERROR,
        )),
    }
}

#[derive(Deserialize)]
struct UpdateQueueOrderRequest {
    channel_id: String,
    new_order: Vec<QueueUpdate>,
}

#[derive(Deserialize)]
struct QueueUpdate {
    twitch_name: String,
    position: i64,
}

async fn update_queue_order(data: UpdateQueueOrderRequest) -> Result<(), BotError> {
    let mut conn = initialize_database();
    let transaction = conn.transaction()?;

    for entry in &data.new_order {
        transaction.execute(
            "UPDATE queue SET position = ? WHERE twitch_name = ? AND channel_id = ?",
            (entry.position, &entry.twitch_name, &data.channel_id),
        )?;
    }

    transaction.commit()?;
    Ok(())
}