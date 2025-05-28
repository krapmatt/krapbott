pub mod api;
mod bot;
mod bot_commands;
pub mod commands;
mod database;
pub mod discord_bot;
pub mod models;
pub mod obs_dock;
pub mod twitch_api;
pub mod queue;
mod giveaway;
use bot::{grant_points_task, handle_obs_message, run_chat_bot};
use database::initialize_currency_database;
use discord_bot::run_discord_bot;
use models::{BotConfig, BotError};
use obs_dock::{
    check_session, get_public_queue, get_queue_handler, get_queue_state_handler, get_run_counter_handler, next_queue_handler, remove_from_queue_handler, toggle_queue_handler, twitch_callback, update_queue_order, with_authorization, AuthCallbackQuery
};
use std::{sync::Arc, time::Duration};
use tokio::{sync::mpsc, time};
use warp::{filters::{fs::dir, log}, Filter};

#[tokio::main]
async fn main() -> Result<(), BotError> {
    
    let pool = initialize_currency_database().await?;

    let (tx, mut rx) = mpsc::unbounded_channel::<(String, String)>();
    let tx_arc = Arc::new(tx);

    // HTTP Server
    let tx_filter = warp::any().map({
        let tx_arc = Arc::clone(&tx_arc);
        move || Arc::clone(&tx_arc)
    });

    let pool_filter = warp::any().map({
        let pool = Arc::clone(&pool);
        move || Arc::clone(&pool)
    });

    let static_files = dir("./public");
    let session_route = warp::path!("auth" / "session")
        .and(warp::header::optional("cookie"))
        .and(pool_filter.clone())
        .and_then(check_session);
    let auth_routes = warp::path!("auth" / "callback")
        .and(warp::query::<AuthCallbackQuery>())
        .and(pool_filter.clone())
        .and_then(twitch_callback);
    let get_queue = warp::path("queue")
        .and(warp::get())
        .and(warp::header::optional("cookie"))
        .and(pool_filter.clone())
        .and_then(get_queue_handler);
    let remove_route = warp::path("remove").and(warp::post())
        .and(warp::header::optional("cookie"))
        .and(warp::body::json())
        .and(pool_filter.clone())
        .and_then(remove_from_queue_handler);

    let queue_drag_drop = warp::path("reorder")
        .and(warp::post())
        .and(warp::header::optional("cookie"))
        .and(warp::body::json())
        .and(pool_filter.clone())
        .and_then(update_queue_order);
    let next_route = warp::path("next")
        .and(warp::header::optional("cookie"))
        .and(tx_filter)
        .and_then(next_queue_handler);
    let run_route = warp::path("run-counter")
        .and(warp::get())
        .and(warp::header::optional("cookie"))
        .and_then(get_run_counter_handler);
    let toggle_queue_route = warp::path("queue")
        .and(warp::path::param::<String>())
        .and(warp::post())
        .and(warp::header::optional("cookie"))
        .and_then(toggle_queue_handler);
    let queue_state_route = warp::path("queue")
        .and(warp::path("state"))
        .and(warp::path::param::<String>())
        .and(warp::get())
        .and_then(get_queue_state_handler);
    let cors = warp::cors()
        .allow_any_origin()
        .allow_methods(vec!["GET", "POST"])
        .allow_headers(vec!["Content-Type", "Authorization", "Cookie"]).allow_credentials(true);
    let public_queue_api = warp::path!("queue" / String)
        .and(warp::get())
        .and(pool_filter.clone())
        .and_then(get_public_queue);
    let routes = static_files
        .or(get_queue)
        .or(remove_route)
        .or(next_route)
        .or(toggle_queue_route)
        .or(queue_state_route)
        .or(queue_drag_drop)
        .or(run_route).or(auth_routes).or(session_route).or(with_authorization()).with(cors);
    let queue_page = dir("./public/queue.html");
    let main_route = queue_page.or(public_queue_api).or(routes);

    tokio::spawn(async move {
        warp::serve(main_route)
            .tls()
            .cert_path("ssl/cert.pem")
            .key_path("ssl/key_pkcs8.pem")
            .run(([0, 0, 0, 0], 443))
            .await;
    });
    // OBS Bot Task
    let pool_clone = Arc::clone(&pool);
    tokio::spawn(async move {
        while let Some((channel_id, command)) = rx.recv().await {
            if let Err(e) = handle_obs_message(channel_id, command, Arc::clone(&pool_clone)).await {
                eprintln!("OBS bot error: {}", e);
            }
        }
    });
    let pool_clone = Arc::clone(&pool);
    tokio::spawn(async move {
        grant_points_task("216105918", Arc::clone(&pool_clone)).await;
    });

    // Discord Bot Task
    tokio::spawn(async {
        loop {
            run_discord_bot().await;
            time::sleep(Duration::from_secs(5)).await;
        }
    });

    // Chat Bot Task
    if let Err(e) = run_chat_bot(Arc::clone(&pool)).await {
        eprintln!("Chat bot error: {}", e);
    }

    Ok(())
}
/*
Giveaway -> Config - start time & keyword // Database - entries
Command start -> while function -> announcements for in how long it ends -> last 5 seconds countdown

*/