mod bot_commands;
mod bot;
mod database;
pub mod models;
pub mod api;
pub mod discord_bot;
pub mod commands;
pub mod obs_dock;
use std::{collections::{HashMap, HashSet}, thread::spawn, time::{Duration, Instant, SystemTime}};
use bot::{create_obs_bot, run_chat_bot};
use discord_bot::run_discord_bot;
use models::BotConfig;
use obs_dock::{connect_to_obs, get_queue_handler, get_queue_state_handler, get_run_counter_handler, next_queue_handler, remove_from_queue_handler, toggle_queue_handler};
use tokio::{sync::mpsc::channel, time::sleep};
use warp::{filters::fs::dir, Filter};

#[tokio::main]
async fn main() {

    let (tx, rx) = channel::<(String, String)>(100);

    spawn(move || {
        let tx_filter = warp::any().map(move || tx.clone());
        let rt = tokio::runtime::Runtime::new().unwrap();
        let static_files = dir("./public");
        let get_queue = warp::path("queue").and(warp::get()).and(warp::path::param::<String>()).and_then(get_queue_handler);
        let remove_route = warp::path("remove").and(warp::post()).and(warp::body::json()).and_then(remove_from_queue_handler);
        let next_route = warp::path("next").and(warp::path::param::<String>()).and(tx_filter).and_then(next_queue_handler);
        let run_route = warp::path("run-counter").and(warp::path::param::<String>()).and_then(get_run_counter_handler);
        let toggle_queue_route = warp::path("queue").and(warp::path::param::<String>()).and(warp::path::param::<String>()).and(warp::post()).and_then(toggle_queue_handler);
        let queue_state_route = warp::path("queue").and(warp::path("state")).and(warp::path::param::<String>()).and(warp::get()).and_then(get_queue_state_handler);
        let cors = warp::cors()
        .allow_any_origin()
        .allow_methods(vec!["GET", "POST"])
        .allow_headers(vec!["Content-Type"]);
        let routes = static_files.or(get_queue).or(remove_route).or(next_route).or(toggle_queue_route).or(queue_state_route).or(run_route).with(cors);
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
    spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            create_obs_bot(rx).await;
        });
    });
    
    //Loop for if a error accours bot ,,doesnt" panics

    if let Err(e) = run_chat_bot().await {
        eprintln!("Error running chat bot: {} Time: {:?}", e, SystemTime::now());
    }

    
}
