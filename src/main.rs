mod bot_commands;
mod bot;
mod database;
pub mod models;
pub mod api;
pub mod discord_bot;
pub mod commands;
pub mod obs_dock;
use std::{collections::{HashMap, HashSet}, thread::spawn, time::{Duration, Instant, SystemTime}};
use bot::run_chat_bot;
use discord_bot::run_discord_bot;
use models::BotConfig;
use obs_dock::{connect_to_obs, get_queue_handler, remove_from_queue_handler};
use tokio::time::sleep;
use warp::{filters::fs::dir, Filter};

#[tokio::main]
async fn main() {
    spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let static_files = dir("./public");
        let get_queue = warp::path("queue").and(warp::get()).and(warp::path::param::<String>()).and_then(get_queue_handler);
        let remove_route = warp::path("remove").and(warp::post()).and(warp::body::json()).and_then(remove_from_queue_handler);
        let cors = warp::cors()
        .allow_any_origin()
        .allow_methods(vec!["GET", "POST"])
        .allow_headers(vec!["Content-Type"]);
        let routes = static_files.or(get_queue).or(remove_route).with(cors);
        rt.block_on(async {
            warp::serve(routes).run(([0, 0, 0, 0], 8080)).await;
        })
    });
    spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            loop {
                connect_to_obs().await;
            }
        });
    });
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
    
    
    //Loop for if a error accours bot ,,doesnt" panics

    if let Err(e) = run_chat_bot().await {
        eprintln!("Error running chat bot: {} Time: {:?}", e, SystemTime::now());
    }

    
}
