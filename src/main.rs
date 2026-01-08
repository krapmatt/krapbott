pub(crate) mod api;
pub(crate) mod bot;


use shuttle_runtime::SecretStore;
use shuttle_warp::{warp::{self, reply::Reply, Filter}};
use sqlx::PgPool;
use tracing::info;
use std::{collections::{HashMap, HashSet}, sync::Arc};
use tokio::sync::RwLock;
use include_dir::{include_dir, Dir};

use crate::bot::{chat_event::chat_event::ChatEvent, commands::{CommandRegistry, commands::BotResult}, db::{ChannelId, config::{load_bot_config_from_db, save_channel_config}, initialize_database}, handler::handler::UnifiedChatClient, platforms::twitch::{event_loop::run_twitch_loop, twitch::build_twitch_client}, run_event_loop, state::def::{AliasConfig, AppState, BotRuntime, BotSecrets, ChannelConfig}, web::{auth::{twitch_callback, twitch_login}, obs::{obs_page, obs_queue, obs_queue_events, obs_queue_next, obs_queue_remove, obs_queue_reorder, obs_queue_toggle}}};
static PUBLIC_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/src/bot/web/public");
pub async fn init_db(pool: &PgPool) -> BotResult<()> {
    sqlx::query!("SET search_path TO krapbott_v2")
        .execute(pool)
        .await?;
    Ok(())
}
#[shuttle_runtime::main]
async fn main(#[shuttle_shared_db::Postgres] pool: sqlx::PgPool, #[shuttle_runtime::Secrets] secrets: SecretStore) -> shuttle_warp::ShuttleWarp<(impl Reply,)> {
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
        save_channel_config(&pool, &channel_id, &cfg).await;
    }
    let registry = Arc::new(CommandRegistry::new());
    info!("{:?}", config);
    let runtime = BotRuntime {
        dispatchers: RwLock::new(HashMap::new()),
        alias_config: RwLock::new(AliasConfig {
            disabled_commands: HashSet::new(),
            removed_aliases: HashSet::new(),
            aliases: HashMap::new(),
        }),
    };
    
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<ChatEvent>();
    let secrets = Arc::new(BotSecrets::from_shuttle(&secrets).expect("Missing secrets"));
    let (twitch_rx, twitch_client) = build_twitch_client("Kr4pTr4p".to_string(), secrets.oauth_token_bot.clone());

    let chat_client = Arc::new(UnifiedChatClient {
        twitch: twitch_client,
    });

    let (sse_tx, _) = tokio::sync::broadcast::channel(32);

    let state = Arc::new(AppState {
        secrets: secrets.clone(),
        config,
        runtime: Arc::new(runtime),
        chat_client,
        registry: registry.clone(),
        sse_bus: sse_tx
    });

    
    // Twitch input
    tokio::spawn(run_twitch_loop(twitch_rx, tx.clone(), state.clone()));

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

    let obs = warp::path!("obs")
        .and(warp::header::optional("cookie"))
        .and(pool_filter.clone())
        .and_then(obs_page);

    let obs_queue = warp::path!("api" / "obs" / "queue")
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
        .and(warp::post())
        .and(warp::header::optional("cookie"))
        .and(warp::body::json())
        .and(pool_filter.clone())
        .and(state_filter.clone())
        .and_then(obs_queue_reorder);
    let obs_toggle = warp::path!("api" / "obs" / "queue" / "toggle")
        .and(warp::post())
        .and(warp::header::optional("cookie"))
        .and(warp::body::json())
        .and(pool_filter.clone())
        .and(state_filter.clone())
        .and_then(obs_queue_toggle);
    let obs_sse = warp::path!("api" / "obs" / "queue" / "events")
        .and(warp::get())
        .and(warp::header::optional("cookie"))
        .and(pool_filter.clone())
        .and(state_filter.clone())
        .and_then(obs_queue_events);


    
    // CORS
    let cors = warp::cors()
        .allow_any_origin()
        .allow_methods(vec!["GET", "POST"])
        .allow_headers(vec!["Content-Type", "Cookie"])
        .allow_credentials(true);


    let routes = auth_twitch
    .or(auth_callback)
    .or(obs)
    .or(obs_queue)
    .or(obs_next)
    .or(obs_remove)
    .or(obs_reorder)
    .or(obs_toggle)
    .or(obs_sse)
    .with(cors)
    .boxed();

    Ok(shuttle_warp::WarpService(routes.boxed()))
}

/* TODO!
- BoTConfig Database
- ALIASES DONT work

*/