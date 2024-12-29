mod bot_commands;
mod bot;
mod database;
mod gui;
pub mod models;
pub mod api;
pub mod discord_bot;
pub mod commands;
use std::{sync::Arc, thread::spawn, time::{Duration, SystemTime}};

use async_sqlite::rusqlite::params;
use bot::run_chat_bot;
use database::initialize_database;
use discord_bot::run_discord_bot;
use egui::ViewportBuilder;
use futures::{SinkExt, StreamExt};
use gui::{load_icon, AppState};
use models::{BotConfig, SharedState, TwitchUser};
use serde::Serialize;
use serde_json::json;
use tokio::time::sleep;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use warp::{filters::fs::dir, Filter};

#[derive(Serialize)]
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

    //ws_stream.send(Message::Text(auth_message.to_string())).await?;
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

async fn get_queue_handler(channel_id: String) -> Result<impl warp::Reply, warp::Rejection> {
    // Simulate fetching queue data
    let conn = initialize_database();
    let mut stmt = conn.prepare("SELECT position, twitch_name, bungie_name FROM queue WHERE channel_id = ? ORDER BY position ASC").unwrap();
    
    let queue_entries: Result<Vec<QueueEntry>, _> = stmt
        .query_map(params!["#krapmatt"], |row| {
            Ok(QueueEntry{
                position: row.get(0)?,
                twitch_name: row.get(1)?,
                bungie_name: row.get(2)?,
            })
        })
        .expect("Failed to execute query")
        .collect();
        
    match queue_entries {
        Ok(entries) => Ok(warp::reply::json(&entries)),
        Err(_) => Ok(warp::reply::json(&Vec::<QueueEntry>::new())),
    }
    
}

#[tokio::main]
async fn main() {
    
    let static_files = dir("./static"); // Assuming queue_dock.html is in the ./static folder
    let get_queue = warp::path("queue").and(warp::get()).and(warp::path::param()).and_then(get_queue_handler);
    let routes = static_files.or(get_queue);
    
    
    /*spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            if let Err(err) = connect_to_obs().await {
                eprintln!("OBS WebSocket Error: {}", err);
            }
        })
    });
    
    warp::serve(routes)
    .run(([127, 0, 0, 1], 8080))
    .await;*/
    




    spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            loop {
                run_discord_bot().await;
                println!("Restarting Discord Krapbott!");
                sleep(Duration::from_secs(5)).await;
            }
        });
    });
    
    


    let shared_state = Arc::new(std::sync::Mutex::new(SharedState::new()));
    let shared_state_clone = Arc::clone(&shared_state);
    
    spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            //Loop for if a error accours bot ,,doesnt" panics
            loop {   
                if let Err(e) = run_chat_bot(shared_state_clone.clone()).await {
                    eprintln!("Error running chat bot: {} Time: {:?}", e, SystemTime::now());
                }
                //Pokud error je nevyhnutelný, nezaloopování
                sleep(Duration::from_secs(5)).await;
                println!("Restarting Twitch Krapbott!");
            }
        });
    });
    
    //Run the GUI
    let app_state = AppState::new(shared_state);
    let native_options = eframe::NativeOptions {
        viewport: ViewportBuilder::default().with_icon(load_icon("pictures/pp.webp")).with_title("Kr4pTr4p").with_inner_size([480.0, 560.0]),
        ..Default::default()
    };
    let _ = eframe::run_native("Twitch Queue Manager", native_options, Box::new(|_cc| Ok(Box::new(app_state))));
    
}
