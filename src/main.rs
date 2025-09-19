pub mod api;
mod bot;
mod bot_commands;
pub mod commands;
mod database;
pub mod models;
pub mod obs_dock;
pub mod twitch_api;
pub mod queue;
use bot::{handle_obs_message, run_chat_bot};
use models::BotConfig;
use obs_dock::{
    check_session, get_public_queue, get_queue_handler, get_queue_state_handler, get_run_counter_handler, next_queue_handler, remove_from_queue_handler, toggle_queue_handler, twitch_callback, update_queue_order, with_authorization, AuthCallbackQuery
};
use shuttle_runtime::SecretStore;
use shuttle_warp::{warp::{self, reply::Reply, Filter}};
use tracing::{error, info};
use std::sync::Arc;
use tokio::{sync::{mpsc, RwLock}};
use crate::{bot::BotState, obs_dock::{delete_alias_handler, get_aliases_handler, get_all_command_aliases, remove_default_alias_handler, restore_default_alias_handler, set_alias_handler, toggle_default_command_handler}};
use include_dir::{include_dir, Dir};
static PUBLIC_DIR: Dir = include_dir!("public");

#[shuttle_runtime::main]
async fn main(#[shuttle_shared_db::Postgres] pool: sqlx::PgPool, #[shuttle_runtime::Secrets] secrets: SecretStore) -> shuttle_warp::ShuttleWarp<(impl Reply,)> {
    //let file_appender = rolling::daily("logs", "krapbott.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(std::io::stdout());
    if tracing_subscriber::fmt()
        .with_writer(non_blocking)
        .with_target(false)
        .with_timer(tracing_subscriber::fmt::time::uptime())
        .with_level(true)
        .try_init()
        .is_err()
    {
        // Shuttle already installed a global subscriber, just skip
    }
    info!("KrapBott started");
    
    let pool = Arc::new(pool);
    let bot_state = Arc::new(RwLock::new(BotState::new(&pool, secrets).await));
    /*let newconfig = BotConfig::new();
    newconfig.save_all(&pool).await.unwrap();
    bot_state.write().await.config = newconfig;*/

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

    // static assets

    let queue_page = warp::path("queue_dock.html").map(|| {
        warp::reply::html(
            PUBLIC_DIR
                .get_file("queue_dock.html")
                .unwrap()
                .contents_utf8()
                .unwrap(),
        )
    });

    let public_queue_page = warp::path("queue.html").map(|| {
        warp::reply::html(
            PUBLIC_DIR
                .get_file("queue.html")
                .unwrap()
                .contents_utf8()
                .unwrap(),
        )
    });

    let alias_page = warp::path("alias.html").map(|| {
        warp::reply::html(
            PUBLIC_DIR
                .get_file("alias.html")
                .unwrap()
                .contents_utf8()
                .unwrap(),
        )
    });
    // auth
    let session_route = warp::path!("auth" / "session")
        .and(warp::header::optional("cookie"))
        .and(pool_filter.clone())
        .and_then(check_session);

    let auth_callback = warp::path!("auth" / "callback")
        .and(warp::query::<AuthCallbackQuery>())
        .and(pool_filter.clone())
        .and_then(twitch_callback);

    // queue management
    let get_queue = warp::path!("queue")
        .and(warp::get())
        .and(warp::header::optional("cookie"))
        .and(pool_filter.clone()).and(state_filter.clone())
        .and_then(get_queue_handler);

    let remove_route = warp::path!("queue" / "remove")
        .and(warp::path::end())
        .and(warp::post())
        .and(warp::header::optional("cookie"))
        .and(warp::body::json())
        .and(pool_filter.clone())
        .and_then(remove_from_queue_handler);

    let reorder_queue = warp::path!("queue" / "reorder")
        .and(warp::post())
        .and(warp::header::optional("cookie"))
        .and(warp::body::json())
        .and(pool_filter.clone()).and(state_filter.clone())
        .and_then(update_queue_order);

    let next_route = warp::path!("queue" / "next")
        .and(warp::path::end())
        .and(warp::post())
        .and(warp::header::optional("cookie"))
        .and(tx_filter)
        .and(pool_filter.clone())
        .and_then(next_queue_handler);

    let run_counter = warp::path!("queue" / "run-counter")
        .and(warp::get())
        .and(warp::header::optional("cookie"))
        .and(pool_filter.clone()).and(state_filter.clone())
        .and_then(get_run_counter_handler);

    let toggle_queue = warp::path!("queue" / String / "toggle")
        .and(warp::post())
        .and(warp::header::optional("cookie"))
        .and(pool_filter.clone()).and(state_filter.clone())
        .and_then(toggle_queue_handler);

    let queue_state = warp::path!("queue" / "state")
        .and(warp::get())
        .and(warp::header::optional("cookie"))
        .and(pool_filter.clone()).and(state_filter.clone())
        .and_then(get_queue_state_handler);

    // public queue (no login required)
    let public_queue = warp::path!("public" / "queue" / String)
        .and(warp::get())
        .and(pool_filter.clone()).and(state_filter.clone())
        .and_then(get_public_queue);

    // alias APIs
    let alias_base = warp::path("api")
        .and(warp::path("aliases"))
        .and(pool_filter.clone())
        .and(warp::header::optional("cookie"));

    let alias_get = alias_base.clone()
        .and(warp::get())
        .and_then(get_aliases_handler);

    let alias_post = alias_base.clone()
        .and(warp::post())
        .and(state_filter.clone())
        .and(warp::body::json())
        .and_then(set_alias_handler);

    let alias_delete = alias_base.clone()
        .and(warp::delete())
        .and(state_filter.clone())
        .and(warp::body::json())
        .and_then(delete_alias_handler);

    let alias_all = warp::path!("api" / "aliases")
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

    let alias_remove_default = warp::path!("api" / "aliases" / "remove-default-alias")
        .and(warp::post())
        .and(pool_filter.clone())
        .and(warp::header::optional("cookie"))
        .and(state_filter.clone())
        .and(warp::body::json())
        .and_then(remove_default_alias_handler);

    let alias_restore_default = warp::path!("api" / "aliases" / "restore-default-alias")
        .and(warp::post())
        .and(pool_filter.clone())
        .and(warp::header::optional("cookie"))
        .and(state_filter.clone())
        .and(warp::body::json())
        .and_then(restore_default_alias_handler);

    // CORS
    let cors = warp::cors()
        .allow_any_origin()
        .allow_methods(vec!["GET", "POST", "DELETE"])
        .allow_headers(vec!["Content-Type", "Authorization", "Cookie"])
        .allow_credentials(true);

    // combine all
    let routes = queue_page
        .or(alias_page)
        .or(public_queue_page)
        .or(session_route)
        .or(auth_callback)
        .or(get_queue)
        .or(remove_route)
        .or(reorder_queue)
        .or(next_route)
        .or(run_counter)
        .or(toggle_queue)
        .or(queue_state)
        .or(public_queue)
        .or(alias_get)
        .or(alias_post)
        .or(alias_delete)
        .or(alias_all)
        .or(alias_toggle)
        .or(alias_remove_default)
        .or(alias_restore_default)
        .with(cors)
        .boxed();

    /*tokio::spawn(async move {
        warp::serve(main_route)
            .tls()
            .cert_path("ssl/cert.pem")
            .key_path("ssl/key_pkcs8.pem")
            .run(([0, 0, 0, 0], 443))
            .await;
    });*/
    // OBS Bot Task
    let pool_clone = Arc::clone(&pool);
    let bot_clone = Arc::clone(&bot_state);
    tokio::spawn(async move {
        while let Some((channel_id, command)) = rx.recv().await {
            if let Err(e) = handle_obs_message(channel_id.clone(), command.clone(), Arc::clone(&pool_clone), Arc::clone(&bot_clone)).await {
                error!(%channel_id, ?command, "OBS bot error: {}", e);
            }
        }
    });

    // Chat Bot Task
    let pool_clone = Arc::clone(&pool);
    let bot_clone = Arc::clone(&bot_state);
    tokio::spawn(async move {
        if let Err(e) = run_chat_bot(pool_clone, bot_clone).await {
            eprintln!("Chat bot failed: {}", e);
        }
    });

    Ok(shuttle_warp::WarpService(routes.boxed()))
}
