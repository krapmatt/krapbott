
use std::sync::Arc;

use async_sqlite::rusqlite::params;
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::{mpsc::{self, Sender, UnboundedSender}, Mutex};
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
use warp::{http::StatusCode, reject, reply::json, Filter};

use crate::{database::{initialize_database, initialize_database_async}, models::{BotConfig, BotError}};

pub async fn remove_from_queue_handler(
    body: serde_json::Value,
) -> Result<impl warp::Reply, warp::Rejection> {
    let twitch_name = body["twitch_name"].as_str().ok_or_else(|| {
        reject()
    })?;
    let channel_name = body["channel_id"].as_str().ok_or_else(|| {
        reject()
    })?;
    let channel_id = format!("#{}", channel_name);
    let conn = initialize_database();
    let mut result = Ok(0);
    if let Ok(_pos) = conn.query_row(
        "SELECT position FROM queue WHERE twitch_name = ?1 AND channel_id = ?2",
        params![twitch_name, channel_id],
        |row| row.get::<_, i32>(0),
    ) {
        result = conn.execute(
            "DELETE FROM queue WHERE twitch_name = ?1 AND channel_id = ?2",
            params![twitch_name, channel_id],
        );
        let mut stmt = conn.prepare(
            "SELECT twitch_name FROM queue WHERE channel_id = ?1 ORDER BY position ASC",
        ).unwrap();
        let mut rows = stmt.query(params![channel_id]).unwrap();
        let mut new_position = 1;
        while let Ok(Some(row)) = rows.next() {
            let name: String = row.get(0).unwrap();
            let _ = conn.execute(
                "UPDATE queue SET position = ?1 WHERE twitch_name = ?2 AND channel_id = ?3",
                params![new_position, name, channel_id],
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

pub async fn update_queue_order(data: UpdateQueueOrderRequest) -> Result<impl warp::Reply, warp::Rejection> {
    let mut conn = initialize_database_async().await;
    conn.conn(move |conn| {
        let tx = conn.unchecked_transaction().unwrap();
        for entry in &data.new_order {
            conn.execute(
                "UPDATE queue SET position = ?1 WHERE twitch_name = ?2 AND channel_id = ?3",
                params![entry.position, &entry.twitch_name, "#krapmatt"]
            ).unwrap();
            println!("data {:?}", entry);

        }
        println!("channel {}", data.channel_id);
        tx.commit();
        Ok(())
    }).await;
    println!("here");
    /*for entry in &data.new_order {
        tx.execute(
            "UPDATE queue SET position = ?1 WHERE twitch_name = ?2 AND channel_id = ?3",
            params![entry.position, &entry.twitch_name, format!("#{}", &data.channel_id)]
        );

    }*/
    
    Ok(warp::reply::json(&"Queue order updated"))
}

pub async fn next_queue_handler(channel_id: String, sender: Arc<UnboundedSender<(String, String)>>) -> Result<impl warp::Reply, warp::Rejection> {
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
pub async fn keep_channel_alive(sender: Arc<Mutex<UnboundedSender<(String, String)>>>) -> Result<impl warp::Reply, warp::Rejection> {
    if let Err(err) = sender.lock().await.send(("keep".to_string(), "alive".to_string())) {
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

pub async fn get_run_counter_handler(channel_id: String) -> Result<impl warp::Reply, warp::Rejection> {
    let mut config = BotConfig::load_config();
    let channel_id = format!("#{}", channel_id);
    if config.channels.contains_key(&channel_id) {
        let channel_config = config.get_channel_config(&channel_id);
        let runs = channel_config.runs;
        return Ok(json(&json!({"run_counter": runs})))
    } else {
        Err(reject())
    }
}

pub async fn get_queue_state_handler(channel_id: String) -> Result<impl warp::Reply, warp::Rejection> {
    let mut config = BotConfig::load_config();
    let channel_id = format!("#{}", channel_id);
    let config = config.get_channel_config(&channel_id);
    let is_open = config.open;
    println!("{}", is_open);
    Ok(warp::reply::json(&serde_json::json!({ "is_open": is_open })))
}
pub async fn toggle_queue_handler(toggle_action: String, channel_id: String) -> Result<impl warp::Reply, warp::Rejection> {
    let mut config = BotConfig::load_config();
    let channel_id = format!("#{}", channel_id);
    let channel_config = config.get_channel_config(&channel_id);
    match toggle_action.as_str() {
        "open" => {
            channel_config.open = true;
            config.save_config();
            Ok(warp::reply::json(&serde_json::json!({ "success": true, "state": "open" })))
        }
        "close" => {
            channel_config.open = false;
            config.save_config();
            Ok(warp::reply::json(&serde_json::json!({ "success": true, "state": "closed" })))
        }
        _ => Err(warp::reject()),
    }
}

pub async fn sabotage_truck_queue() -> Result<impl warp::Reply, warp::Rejection> {
    let mut config = BotConfig::load_config();
    config.get_channel_config("#nyc62truck").open = !config.get_channel_config("#nyc62truck").open;
    config.save_config();
    Ok(warp::reply::with_status("Sabotaged", StatusCode::OK))

}

