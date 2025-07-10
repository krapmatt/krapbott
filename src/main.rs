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
use database::initialize_database;
use models::{BotConfig, BotError};
use obs_dock::{
    check_session, get_public_queue, get_queue_handler, get_queue_state_handler, get_run_counter_handler, next_queue_handler, remove_from_queue_handler, toggle_queue_handler, twitch_callback, update_queue_order, with_authorization, AuthCallbackQuery
};
use std::sync::Arc;
use tokio::{sync::{mpsc, RwLock}};
use warp::{filters::{fs::dir}, Filter};

use crate::{bot::BotState, obs_dock::{delete_alias_handler, get_aliases_handler, get_all_command_aliases, remove_default_alias_handler, restore_default_alias_handler, set_alias_handler, toggle_default_command_handler}};

#[tokio::main]
async fn main() -> Result<(), BotError> {
    let bot_state = Arc::new(RwLock::new(BotState::new()));
    let pool = initialize_database().await?;

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

    let state_filter = warp::any().map({
        let state = Arc::clone(&bot_state);
        move || Arc::clone(&state)
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
        .and(tx_filter).and(pool_filter.clone())
        .and_then(next_queue_handler);
    let run_route = warp::path("run-counter")
        .and(warp::get())
        .and(warp::header::optional("cookie"))
        .and(pool_filter.clone())
        .and_then(get_run_counter_handler);
    let toggle_queue_route = warp::path("queue")
        .and(warp::path::param::<String>())
        .and(warp::post())
        .and(warp::header::optional("cookie"))
        .and(pool_filter.clone())
        .and_then(toggle_queue_handler);
    let queue_state_route = warp::path("queue")
        .and(warp::path("state"))
        .and(warp::header::optional("cookie"))
        .and(pool_filter.clone())
        .and(warp::get())
        .and_then(get_queue_state_handler);
    let cors = warp::cors()
        .allow_any_origin()
        .allow_methods(vec!["GET", "POST", "DELETE"])
        .allow_headers(vec!["Content-Type", "Authorization", "Cookie"]).allow_credentials(true);
    let public_queue_api = warp::path!("queue" / String)
        .and(warp::get())
        .and(pool_filter.clone())
        .and_then(get_public_queue);
    
    let alias_page = warp::path("alias")
    .and(warp::fs::file("./public/alias.html"));
    let alias_api = warp::path("api")
    .and(warp::path("aliases"))
    .and(pool_filter.clone())
    .and(warp::header::optional("cookie"));
    let alias_get = alias_api.clone()
        .and(warp::get())
        .and_then(get_aliases_handler);
    let alias_post = alias_api.clone()
        .and(warp::post())
        .and(state_filter.clone())
        .and(warp::body::json())
        .and_then(set_alias_handler);
    let alias_delete = alias_api
        .and(warp::delete())
        .and(state_filter.clone())
        .and(warp::body::json())
        .and_then(delete_alias_handler);    
    let alias_ui_route = warp::path!("api" / "aliases")
        .and(warp::get())
        .and(warp::header::optional("cookie"))
        .and(pool_filter.clone())
        .and_then(get_all_command_aliases);
    let alias_toggle = warp::path!("api" / "aliases" / "disable")
        .and(warp::post())
        .and(pool_filter.clone())
        .and(warp::header::optional("cookie"))
        .and(state_filter.clone())
        .and(warp::body::json())
        .and_then(toggle_default_command_handler);
    let alias_default_removal = warp::path!("api" / "aliases" / "remove-default-alias")
        .and(warp::post())
        .and(pool_filter.clone())
        .and(warp::header::optional("cookie"))
        .and(state_filter.clone())
        .and(warp::body::json())
        .and_then(remove_default_alias_handler);
    let alias_default_restore = warp::path!("api" / "aliases" / "restore-default-alias")
        .and(warp::post())
        .and(pool_filter.clone())
        .and(warp::header::optional("cookie"))
        .and(state_filter.clone())
        .and(warp::body::json())
        .and_then(restore_default_alias_handler);

    let routes = static_files
        .or(alias_ui_route)
        .or(alias_get)
        .or(alias_toggle)
        .or(alias_default_removal)
        .or(alias_default_restore)
        .or(alias_post)
        .or(alias_delete)
        .or(get_queue)
        .or(remove_route)
        .or(next_route)
        .or(toggle_queue_route)
        .or(queue_state_route)
        .or(queue_drag_drop)
        .or(run_route).or(auth_routes).or(session_route).or(with_authorization(Arc::clone(&pool))).with(cors);
    let queue_page = dir("./public/queue.html");
    let main_route = queue_page.or(public_queue_api).or(alias_page).or(routes);

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
        let _ = grant_points_task("216105918", Arc::clone(&pool_clone)).await;
    });

    /*// Discord Bot Task
    tokio::spawn(async {
        loop {
            run_discord_bot().await;
            time::sleep(Duration::from_secs(5)).await;
        }
    });*/

    // Chat Bot Task
    if let Err(e) = run_chat_bot(Arc::clone(&pool), Arc::clone(&bot_state)).await {
        eprintln!("Chat bot error: {}", e);
    }

    Ok(())
}
/*
Giveaway -> Config - start time & keyword // Database - entries
Command start -> while function -> announcements for in how long it ends -> last 5 seconds countdown

*/