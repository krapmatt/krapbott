pub(crate) mod api;
pub(crate) mod bot;
use sqlx::PgPool;
use tracing::info;
use warp::Filter;
use std::{collections::{HashMap, HashSet}, sync::Arc, time::Duration};
use tokio::sync::RwLock;
use include_dir::{include_dir, Dir};

use crate::{api::{kick_oauth::KickAuthManager, twitch_api::create_twitch_app_token}, bot::{chat_event::chat_event::ChatEvent, commands::{CommandRegistry, commands::BotResult}, db::{ChannelId, config::{load_bot_config_from_db, save_channel_config}, initialize_database}, handler::handler::UnifiedChatClient, platforms::{kick::event_loop::run_kick_loop, twitch::{event_loop::run_twitch_loop, twitch::build_twitch_client}}, run_event_loop, state::def::{AliasConfig, AppState, BotRuntime, BotSecrets, ChannelConfig}, web::{auth::{kick_callback, kick_login, twitch_callback, twitch_login}, obs::{obs_alias_add, obs_alias_remove, obs_alias_remove_default, obs_alias_restore, obs_alias_restore_default, obs_alias_toggle_command, obs_aliases, obs_combined_page, obs_queue, obs_queue_events, obs_queue_len, obs_queue_next, obs_queue_remove, obs_queue_reorder, obs_queue_reset, obs_queue_size, obs_queue_toggle, obs_sessions, obs_switch_session}}}};
use kick_rust::KickClient;

#[tokio::main]
async fn main() -> BotResult<()> {
    let database_url = std::env::var("DATABASE_URL")
    .expect("DATABASE_URL not set");

    let pool = loop {
        match PgPool::connect(&database_url).await {
            Ok(pool) => break pool,
            Err(e) => {
                tracing::warn!("DB not ready yet: {}", e);
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    };
    
    
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
    initialize_database(&pool).await.unwrap();
    info!("KrapBott started");
    let config = Arc::new(RwLock::new(load_bot_config_from_db(&pool).await.expect("Missing config table")));
    if config.read().await.channels.is_empty() {
        let channel_id = ChannelId::new(bot::chat_event::chat_event::Platform::Twitch, "krapmatt");
        let chal_config = ChannelConfig::new(channel_id.clone());
        {config.write().await.channels.insert(channel_id.clone(), chal_config);}
        
        let cfg = config.read().await;
        save_channel_config(&pool, &channel_id, &cfg).await?;
    }
    let registry = Arc::new(CommandRegistry::new());

    let runtime = BotRuntime {
        dispatchers: RwLock::new(HashMap::new()),
    };
    
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<ChatEvent>();
    let secrets = Arc::new(BotSecrets::from_env().expect("Missing secrets"));
    let twitch_token = create_twitch_app_token(&secrets).await.expect("Invalid twitch Response");
    let (twitch_rx, twitch_client) = build_twitch_client("Kr4pTr4p".to_string(), secrets.user_access_token.clone());
    let kick_auth = Arc::new(KickAuthManager::from_secrets(&secrets));
    kick_auth.bootstrap().await?;

    let chat_client = Arc::new(UnifiedChatClient {
        twitch: twitch_client,
        kick: KickClient::new(),
        kick_tx: tx.clone(),
        kick_auth,
    });

    let (sse_tx, _) = tokio::sync::broadcast::channel(32);

    let state = Arc::new(AppState {
        secrets: secrets.clone(),
        config,
        runtime: Arc::new(runtime),
        chat_client,
        registry: registry.clone(),
        sse_bus: sse_tx,
        twitch_auth: Arc::new(RwLock::new(twitch_token))
    });

    
    // Twitch input
    tokio::spawn(run_twitch_loop(twitch_rx, tx.clone(), state.clone()));
    tokio::spawn(run_kick_loop(tx.clone(), state.clone()));

    // Core dispatcher
    tokio::spawn(run_event_loop(pool.clone(), state.clone(), rx));

    let pool_filter = warp::any().map({
        let pool = Arc::new(pool.clone());
        move || Arc::clone(&pool)
    });
    
    let state_filter = warp::any().map({
        let state = Arc::clone(&state);
        move || Arc::clone(&state)
    });

    let secrets_filter = {
        let secrets = Arc::clone(&secrets);
        warp::any().map(move || Arc::clone(&secrets))
    };

    let auth_twitch = warp::path!("auth" / "twitch")
    .and(secrets_filter.clone())
    .and_then(twitch_login);

    let auth_callback = warp::path!("auth" / "callback")
        .and(warp::query())
        .and(pool_filter.clone())
        .and(state_filter.clone())
        .and_then(twitch_callback);

    let auth_kick = warp::path!("auth" / "kick")
        .and(state_filter.clone())
        .and_then(kick_login);

    let auth_kick_callback = warp::path!("auth" / "kick" / "callback")
        .and(warp::query())
        .and(pool_filter.clone())
        .and(state_filter.clone())
        .and_then(kick_callback);

    let obs_combined = warp::path!("obs")
        .and(warp::path::end())
        .and(warp::header::optional("cookie"))
        .and(pool_filter.clone())
        .and_then(obs_combined_page);

    let obs_queue = warp::path!("api" / "obs" / "queue")
        .and(warp::path::end())
        .and(warp::get())
        .and(warp::header::optional("cookie"))
        .and(pool_filter.clone())
        .and(state_filter.clone())
        .and_then(obs_queue);

    let obs_next = warp::path!("api" / "obs" / "queue" / "next")
        .and(warp::post())
        .and(warp::header::optional("cookie"))
        .and(pool_filter.clone())
        .and(state_filter.clone())
        .and_then(obs_queue_next);
    let obs_remove = warp::path!("api" / "obs" / "queue" / "remove")
        .and(warp::post())
        .and(warp::header::optional("cookie"))
        .and(warp::body::json())
        .and(pool_filter.clone())
        .and(state_filter.clone())
        .and_then(obs_queue_remove);

    let obs_reorder = warp::path!("api" / "obs" / "queue" / "reorder")
        .and(warp::path::end())
        .and(warp::post())
        .and(warp::header::optional("cookie"))
        .and(warp::body::json())
        .and(pool_filter.clone())
        .and(state_filter.clone())
        .and_then(obs_queue_reorder);
    let obs_toggle = warp::path!("api" / "obs" / "queue" / "toggle")
        .and(warp::path::end())
        .and(warp::post())
        .and(warp::header::optional("cookie"))
        .and(warp::body::json())
        .and(pool_filter.clone())
        .and(state_filter.clone())
        .and_then(obs_queue_toggle);
    let obs_queue_size = warp::path!("api" / "obs" / "queue" / "size")
        .and(warp::path::end())
        .and(warp::post())
        .and(warp::header::optional("cookie"))
        .and(warp::body::json())
        .and(pool_filter.clone())
        .and(state_filter.clone())
        .and_then(obs_queue_size);
    let obs_queue_len = warp::path!("api" / "obs" / "queue" / "length")
        .and(warp::path::end())
        .and(warp::post())
        .and(warp::header::optional("cookie"))
        .and(warp::body::json())
        .and(pool_filter.clone())
        .and(state_filter.clone())
        .and_then(obs_queue_len);
    let obs_sse = warp::path!("api" / "obs" / "queue" / "events")
        .and(warp::path::end())
        .and(warp::get())
        .and(warp::header::optional("cookie"))
        .and(pool_filter.clone())
        .and(state_filter.clone())
        .and_then(obs_queue_events);

    let obs_aliases = warp::path!("api" / "obs" / "aliases")
        .and(warp::path::end())
        .and(warp::get())
        .and(warp::header::optional("cookie"))
        .and(pool_filter.clone())
        .and(state_filter.clone())
        .and_then(obs_aliases);
    let obs_aliases_add = warp::path!("api" / "obs" / "aliases" / "add")
        .and(warp::post())
        .and(warp::header::optional("cookie"))
        .and(warp::body::json())
        .and(pool_filter.clone())
        .and(state_filter.clone())
        .and_then(obs_alias_add);
    let obs_aliases_remove = warp::path!("api" / "obs" / "aliases" / "remove")
        .and(warp::post())
        .and(warp::header::optional("cookie"))
        .and(warp::body::json())
        .and(pool_filter.clone())
        .and(state_filter.clone())
        .and_then(obs_alias_remove);
    
    let obs_aliases_toggle = warp::path!("api" / "obs" / "aliases" / "toggle")
        .and(warp::post())
        .and(warp::header::optional("cookie"))
        .and(warp::body::json())
        .and(pool_filter.clone())
        .and(state_filter.clone())
        .and_then(obs_alias_toggle_command);
    
    let obs_aliases_restore = warp::path!("api" / "obs" / "aliases" / "restore")
        .and(warp::post())
        .and(warp::header::optional("cookie"))
        .and(warp::body::json())
        .and(pool_filter.clone())
        .and(state_filter.clone())
        .and_then(obs_alias_restore);
    let obs_aliases_remove_default = warp::path!("api" / "obs" / "aliases" / "remove-default")
        .and(warp::post())
        .and(warp::header::optional("cookie"))
        .and(warp::body::json())
        .and(pool_filter.clone())
        .and(state_filter.clone())
        .and_then(obs_alias_remove_default);
    let obs_aliases_restore_default = warp::path!("api" / "obs" / "aliases" / "restore-default")
        .and(warp::post())
        .and(warp::header::optional("cookie"))
        .and(warp::body::json())
        .and(pool_filter.clone())
        .and(state_filter.clone())
        .and_then(obs_alias_restore_default);
    let obs_sessions = warp::path!("api" / "obs" / "sessions")
        .and(warp::path::end())
        .and(warp::get())
        .and(warp::header::optional("cookie"))
        .and(pool_filter.clone())
        .and(state_filter.clone())
        .and_then(obs_sessions);
    let obs_switch = warp::path!("api" / "obs" / "sessions" / "switch")
        .and(warp::path::end())
        .and(warp::post())
        .and(warp::header::optional("cookie"))
        .and(warp::body::json())
        .and(pool_filter.clone())
        .and_then(obs_switch_session);
    let obs_queue_reset = warp::path!("api" / "obs" / "queue" / "reset")
        .and(warp::post())
        .and(warp::header::optional("cookie"))
        .and(pool_filter.clone())
        .and(state_filter.clone())
        .and_then(obs_queue_reset);
    let favicon = warp::path("favicon.ico")
        .and(warp::get())
        .map(|| warp::reply::with_status("", warp::http::StatusCode::NO_CONTENT));

    // CORS
    let cors = warp::cors()
        .allow_origin("https://krapbott.up.railway.app")
        .allow_methods(vec!["GET", "POST", "OPTIONS"])
        .allow_headers(vec!["Content-Type", "Cookie"])
        .allow_credentials(true);

    let options = warp::options()
        .map(|| warp::reply());

    let routes = auth_twitch
    .or(auth_callback)
    .or(auth_kick)
    .or(auth_kick_callback)
    .or(obs_combined)
    .or(obs_queue)
    .or(obs_next)
    .or(obs_remove)
    .or(obs_reorder)
    .or(obs_toggle)
    .or(obs_queue_size)
    .or(obs_queue_len)
    .or(obs_queue_reset)
    .or(obs_aliases)
    .or(obs_aliases_add)
    .or(obs_aliases_remove)
    .or(obs_aliases_toggle)
    .or(obs_aliases_restore)
    .or(obs_aliases_remove_default)
    .or(obs_aliases_restore_default)
    .or(obs_sessions)
    .or(obs_switch)
    .or(obs_sse)
    .or(favicon)
    .or(options)
    .with(cors)
    .boxed();

    let port: u16 = std::env::var("PORT")
    .unwrap_or_else(|_| "8080".into())
    .parse()
    .expect("Invalid PORT");

    warp::serve(routes)
        .run(([0, 0, 0, 0], port))
        .await;
    Ok(())
}

/* TODO!
- BoTConfig Database
- ALIASES DONT work
- PRIORITY OF RESPONSES HAVE CLOSED QUEUE FIRST
*/
