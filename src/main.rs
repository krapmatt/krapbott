mod bot_commands;
mod bot;
mod database;
pub mod models;
pub mod api;
pub mod discord_bot;
pub mod commands;
pub mod obs_dock;
use std::{sync::Arc, thread::spawn, time::{Duration, SystemTime}};
use bot::{handle_obs_message, run_chat_bot};
use discord_bot::run_discord_bot;
use models::{BotConfig, BotError};
use obs_dock::{connect_to_obs, get_queue_handler, get_queue_state_handler, get_run_counter_handler, next_queue_handler, remove_from_queue_handler, toggle_queue_handler, update_queue_order};
use tokio::{sync::mpsc, time};
use warp::{filters::fs::dir, Filter};

/*#[tokio::main]
async fn main() {
    let (tx, rx) = unbounded_channel::<(String, String)>();
    let sender = Arc::new(tx);
    let receiver = Arc::new(Mutex::new(rx));

    spawn(move || {        
        let tx_filter = warp::any().map(move || Arc::clone(&sender));
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
            warp::serve(routes).tls().cert_path("D:/program/krapbott/ssl/cert.pem").key_path("D:/program/krapbott/ssl/key_pkcs8.pem").run(([0, 0, 0, 0], 8080)).await;
            
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

    let handle = task::spawn(async move {
        create_obs_bot(receiver).await;
    });
    //Loop for if a error accours bot ,,doesnt" panics
    let handle1 = task::spawn(async move {
        if let Err(e) = run_chat_bot().await {
            eprintln!("Error running chat bot: {} Time: {:?}", e, SystemTime::now());
        }
    });
    let _ = join!(handle, handle1);

    
}*/

#[tokio::main]
async fn main() -> Result<(), BotError> {
    let (tx, mut rx) = mpsc::unbounded_channel::<(String, String)>();
    let tx_arc = Arc::new(tx);

    // HTTP Server
    let tx_filter = warp::any().map({
        let tx_arc = Arc::clone(&tx_arc);
        move || Arc::clone(&tx_arc)
    });
    let static_files = dir("./public");
    let get_queue = warp::path("queue").and(warp::get()).and(warp::path::param::<String>()).and_then(get_queue_handler);
    let remove_route = warp::path("remove").and(warp::post()).and(warp::body::json()).and_then(remove_from_queue_handler);
    let queue_drag_drop = warp::path("queue").and(warp::path("reorder")).and(warp::post()).and(warp::body::json()).and_then(update_queue_order);
    let next_route = warp::path("next").and(warp::path::param::<String>()).and(tx_filter).and_then(next_queue_handler);
    let run_route = warp::path("run-counter").and(warp::path::param::<String>()).and_then(get_run_counter_handler);
    let toggle_queue_route = warp::path("queue").and(warp::path::param::<String>()).and(warp::path::param::<String>()).and(warp::post()).and_then(toggle_queue_handler);
    let queue_state_route = warp::path("queue").and(warp::path("state")).and(warp::path::param::<String>()).and(warp::get()).and_then(get_queue_state_handler);
    let cors = warp::cors()
        .allow_any_origin()
        .allow_methods(vec!["GET", "POST"])
        .allow_headers(vec!["Content-Type"]);
    
    let routes = static_files.or(get_queue).or(remove_route).or(next_route).or(toggle_queue_route).or(queue_state_route).or(queue_drag_drop).or(run_route).with(cors);


    tokio::spawn(async move {
        warp::serve(routes)
            .tls()
            .cert_path("D:/program/krapbott/ssl/cert.pem")
            .key_path("D:/program/krapbott/ssl/key_pkcs8.pem")
            .run(([0, 0, 0, 0], 8080))
            .await;
    });

    // OBS Bot Task
    tokio::spawn(async move {
        while let Some((channel_id, command)) = rx.recv().await {
            if let Err(e) = handle_obs_message(channel_id, command).await {
                eprintln!("OBS bot error: {}", e);
            }
        }
    });

    // Discord Bot Task
    tokio::spawn(async {
        loop {
            run_discord_bot().await;
            time::sleep(Duration::from_secs(5)).await;
            
        }
    });

    // Chat Bot Task
    if let Err(e) = run_chat_bot().await {
        eprintln!("Chat bot error: {}", e);
    }

    Ok(())
}
