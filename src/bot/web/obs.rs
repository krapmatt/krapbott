use std::{collections::HashMap, convert::Infallible, str::FromStr, sync::Arc};

use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tracing::info;
use warp::{filters::sse::Event, http::StatusCode, reply::{Reply, Response}};

use crate::bot::{
    chat_event::chat_event::Platform,
    commands::queue::logic::{remove_from_queue, reorder_queue, reset_queue_runs, resolve_queue_owner, run_next, set_queue_len, set_queue_open, set_queue_size},
    db::{UserId, aliases::fetch_aliases_from_db, queue::fetch_queue_for_owner},
    dispatcher::dispatcher::refresh_channel_dispatcher,
    handler::handler::ChatClient,
    replies::Replies,
    state::def::{AppState, ObsQueueEntry},
    web::sessions::{channel_from_session, session_cookie_header, sessions_from_cookies},
};

pub async fn obs_combined_page(cookies: Option<String>, pool: Arc<sqlx::PgPool>) -> Result<impl Reply, warp::Rejection> {
    let _ = cookies;
    let _ = pool;
    Ok(warp::reply::html(include_str!("public/queue_alias_dock.html")).into_response())
}

#[derive(Serialize)]
pub struct ObsSessionView {
    pub platform: String,
    pub login: String,
    pub active: bool,
}

#[derive(Serialize)]
pub struct ObsSessionsResponse {
    pub active_platform: Option<String>,
    pub active_login: Option<String>,
    pub sessions: Vec<ObsSessionView>,
}

pub async fn obs_sessions(cookies: Option<String>, pool: Arc<PgPool>) -> Result<Response, warp::Rejection> {
    let active = channel_from_session(cookies.clone(), &pool).await.ok();
    let sessions = sessions_from_cookies(cookies, &pool).await.unwrap_or_default();

    let rows = sessions
        .iter()
        .map(|s| ObsSessionView {
            platform: s.channel.platform().to_string(),
            login: s.channel.channel().to_string(),
            active: active.as_ref().map(|a| a == &s.channel).unwrap_or(false),
        })
        .collect::<Vec<_>>();

    Ok(warp::reply::json(&ObsSessionsResponse {
        active_platform: active.as_ref().map(|a| a.platform().to_string()),
        active_login: active.as_ref().map(|a| a.channel().to_string()),
        sessions: rows,
    }).into_response())
}

#[derive(Deserialize)]
pub struct ObsSwitchSessionPayload {
    pub platform: String,
}

pub async fn obs_switch_session(
    cookies: Option<String>,
    body: ObsSwitchSessionPayload,
    pool: Arc<PgPool>,
) -> Result<Response, warp::Rejection> {
    let platform = Platform::from_str(&body.platform).map_err(|_| warp::reject())?;
    let sessions = sessions_from_cookies(cookies, &pool).await.map_err(|_| warp::reject())?;

    let Some(found) = sessions.into_iter().find(|s| s.channel.platform() == platform) else {
        let reply = warp::reply::with_status(
            warp::reply::json(&serde_json::json!({ "ok": false, "error": "No linked session for that platform" })),
            StatusCode::BAD_REQUEST,
        );
        return Ok(reply.into_response());
    };

    let reply = warp::reply::with_header(
        warp::reply::json(&serde_json::json!({
            "ok": true,
            "platform": platform.to_string(),
            "login": found.channel.channel()
        })),
        "Set-Cookie",
        session_cookie_header("session_id", &found.session_id),
    );

    Ok(reply.into_response())
}

#[derive(Serialize)]
pub struct ObsQueueResponse {
    pub open: bool,
    pub teamsize: usize,
    pub length: usize,
    pub runs: usize,
    pub queue: Vec<ObsQueueEntry>,
}

pub async fn obs_queue(cookies: Option<String>, pool: Arc<PgPool>, state: Arc<AppState>) -> Result<Response, warp::Rejection> {
    let channel = match channel_from_session(cookies, &pool).await {
        Ok(c) => c,
        Err(_) => return Err(warp::reject()),
    };

    let owner = resolve_queue_owner(&state, &channel).await.map_err(|_| warp::reject())?;

    let (teamsize, open, len, runs) = {
        let cfg = state.config.read().await;
        (
            cfg.get_channel_config(&owner).map(|c| c.teamsize).unwrap_or(1), 
            cfg.get_channel_config(&owner).map(|c| c.open).unwrap_or(false), 
            cfg.get_channel_config(&owner).map(|c| c.size).unwrap_or(1),
            cfg.get_channel_config(&owner).map(|c| c.runs).unwrap_or(0)
        )
    };

    let queue = fetch_queue_for_owner(&pool, &owner, teamsize).await.map_err(|_| warp::reject())?;
    info!("{:?}", queue);
    Ok(warp::reply::json(&ObsQueueResponse {
        open,
        teamsize,
        length: len,
        runs,
        queue,
    }).into_response())
}

pub async fn obs_queue_next(cookies: Option<String>, pool: Arc<PgPool>, state: Arc<AppState>) -> Result<Response, warp::Rejection> {
    let channel = match channel_from_session(cookies, &pool).await {
        Ok(c) => c,
        Err(_) => return Err(warp::reject()),
    };

    let owner = resolve_queue_owner(&state, &channel).await.map_err(|_| warp::reject())?;

    let reply = run_next(&pool, state.clone(), &owner).await.map_err(|_| warp::reject())?;

    // Send chat message
    state.chat_client.send_message(&channel, &reply).await.map_err(|_| warp::reject())?;

    Ok(warp::reply::json(&serde_json::json!({
        "ok": true,
        "message": reply
    })).into_response())
}

#[derive(Deserialize)]
pub struct RemovePayload {
    pub user_id: String,
}

pub async fn obs_queue_remove(cookies: Option<String>, body: RemovePayload, pool: Arc<PgPool>, state: Arc<AppState>) -> Result<impl Reply, warp::Rejection> {
    let channel = match channel_from_session(cookies, &pool).await {
        Ok(c) => c,
        Err(_) => return Err(warp::reject()),
    };
    let owner = resolve_queue_owner(&state, &channel).await.map_err(|_| warp::reject())?;

    let user_id = UserId::from_str(&body.user_id).map_err(|_| warp::reject())?;

    remove_from_queue(&pool, &owner, &user_id, state)
        .await
        .map_err(|_| warp::reject())?;

    Ok(warp::reply::json(&serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct ReorderPayload {
    pub order: Vec<String>,
}

pub async fn obs_queue_reorder(cookies: Option<String>, body: ReorderPayload, pool: Arc<PgPool>, state: Arc<AppState>) -> Result<impl Reply, warp::Rejection> {
    let channel = match channel_from_session(cookies, &pool).await {
        Ok(c) => c,
        Err(_) => return Err(warp::reject()),
    };
    let owner = resolve_queue_owner(&state, &channel).await.map_err(|_| warp::reject())?;

    let users = body
        .order
        .into_iter()
        .map(|s| UserId::from_str(&s))
        .collect::<Result<_, _>>()
        .map_err(|_| warp::reject())?;

    reorder_queue(&pool, &owner, users)
        .await
        .map_err(|_| warp::reject())?;

    Ok(warp::reply::json(&serde_json::json!({ "ok": true })))
}

#[derive(serde::Deserialize)]
pub struct ToggleQueuePayload {
    pub open: bool,
}

#[derive(Debug)]
struct ObsToggleError;
impl warp::reject::Reject for ObsToggleError {}

pub async fn obs_queue_toggle(cookies: Option<String>, body: ToggleQueuePayload, pool: Arc<PgPool>, state: Arc<AppState>) -> Result<impl warp::Reply, warp::Rejection> {
    let channel = match channel_from_session(cookies, &pool).await {
        Ok(c) => c,
        Err(_) => return Err(warp::reject()),
    };

    let owner = resolve_queue_owner(&state, &channel).await.map_err(|_| warp::reject::custom(ObsToggleError))?;

    set_queue_open(&pool, state.clone(), &owner, body.open).await.map_err(|_| warp::reject::custom(ObsToggleError))?;

    let msg = if body.open {
        &Replies::queue_opened()
    } else {
        &Replies::queue_closed()
    };

    state.chat_client.send_message(&owner, msg).await.map_err(|_| warp::reject::custom(ObsToggleError))?;

    Ok(warp::reply::json(&serde_json::json!({
        "ok": true,
        "open": body.open
    })))
}

#[derive(serde::Deserialize)]
pub struct SizeQueuePayload {
    pub teamsize: usize,
}

#[derive(Debug)]
struct ObsQueueSizeError;
impl warp::reject::Reject for ObsQueueSizeError {}

pub async fn obs_queue_size(cookies: Option<String>, body: SizeQueuePayload, pool: Arc<PgPool>, state: Arc<AppState>) -> Result<impl warp::Reply, warp::Rejection> {
    let channel = match channel_from_session(cookies, &pool).await {
        Ok(c) => c,
        Err(_) => return Err(warp::reject()),
    };

    let owner = resolve_queue_owner(&state, &channel).await.map_err(|_| warp::reject::custom(ObsQueueSizeError))?;

    set_queue_size(&pool, state.clone(), &owner, body.teamsize).await.map_err(|_| warp::reject::custom(ObsQueueSizeError))?;

    state.chat_client.send_message(&owner, &Replies::queue_size(&body.teamsize.to_string())).await.map_err(|_| warp::reject::custom(ObsQueueSizeError))?;

    Ok(warp::reply::json(&serde_json::json!({
        "ok": true,
        "teamsize": body.teamsize
    })))
}

#[derive(serde::Deserialize)]
pub struct LenQueuePayload {
    pub length: usize,
}

#[derive(Debug)]
struct ObsQueueLenError;
impl warp::reject::Reject for ObsQueueLenError {}

pub async fn obs_queue_len(cookies: Option<String>, body: LenQueuePayload, pool: Arc<PgPool>, state: Arc<AppState>) -> Result<impl warp::Reply, warp::Rejection> {
    let channel = match channel_from_session(cookies, &pool).await {
        Ok(c) => c,
        Err(_) => return Err(warp::reject()),
    };

    let owner = resolve_queue_owner(&state, &channel).await.map_err(|_| warp::reject::custom(ObsQueueLenError))?;

    set_queue_len(&pool, state.clone(), &owner, body.length).await.map_err(|_| warp::reject::custom(ObsQueueLenError))?;

    state.chat_client.send_message(&owner, &Replies::queue_length(&body.length.to_string())).await.map_err(|_| warp::reject::custom(ObsQueueLenError))?;

    Ok(warp::reply::json(&serde_json::json!({
        "ok": true,
        "length": body.length
    })))
}

pub async fn obs_queue_events(cookies: Option<String>, pool: Arc<PgPool>, state: Arc<AppState>) -> Result<impl warp::Reply, warp::Rejection> {
    let channel = match channel_from_session(cookies, &pool).await {
        Ok(c) => c,
        Err(_) => return Err(warp::reject()),
    };

    let mut rx = state.sse_bus.subscribe();

    let stream = async_stream::stream! {
        loop {
            let _ = rx.recv().await;
            yield Ok::<Event, Infallible>(
                Event::default().data("update")
            );
        }
    };

    Ok(warp::sse::reply(
        warp::sse::keep_alive().stream(stream)
    ))
}

#[derive(Serialize)]
pub struct ObsAliasResponse {
    pub aliases: HashMap<String, String>,  // custom aliases
    pub removed_aliases: Vec<String>,      // removed default aliases
    pub disabled_commands: Vec<String>,    // disabled commands
    pub commands: Vec<ObsCommandInfo>,     // all commands
}

#[derive(Serialize)]
pub struct ObsCommandInfo {
    pub name: String,
    pub description: String,
    pub default_aliases: Vec<String>,
}

pub async fn obs_aliases(cookies: Option<String>, pool: Arc<PgPool>, state: Arc<AppState>) -> Result<impl Reply, warp::Rejection> {
    let channel = channel_from_session(cookies, &pool).await.map_err(|_| warp::reject())?;

    let alias_config = fetch_aliases_from_db(&channel, &pool).await.map_err(|_| warp::reject())?;

    let commands = state.registry
        .groups
        .values()
        .flat_map(|g| &g.commands)
        .map(|reg| ObsCommandInfo {
            name: reg.command.name().to_string(),
            description: reg.command.description().to_string(),
            default_aliases: reg.aliases.clone(),
        })
        .collect::<Vec<_>>();

    Ok(warp::reply::json(&ObsAliasResponse {
        aliases: alias_config.aliases.clone(),
        removed_aliases: alias_config.removed_aliases.iter().cloned().collect(),
        disabled_commands: alias_config.disabled_commands.iter().cloned().collect(),
        commands,
    }))
}

#[derive(Deserialize)]
pub struct AddAliasPayload {
    pub alias: String,
    pub command: String,
}

pub async fn obs_alias_add(cookies: Option<String>, body: AddAliasPayload, pool: Arc<PgPool>, state: Arc<AppState>) -> Result<impl Reply, warp::Rejection> {
    let channel =
        channel_from_session(cookies, &pool).await.map_err(|_| warp::reject())?;

    sqlx::query!(
        r#"
        INSERT INTO krapbott_v2.command_aliases (channel, alias, command)
        VALUES ($1, $2, $3)
        ON CONFLICT (channel, alias)
        DO UPDATE SET command = EXCLUDED.command
        "#,
        channel.as_str(), body.alias.to_lowercase(), body.command.to_lowercase()
    ).execute(&*pool).await.map_err(|_| warp::reject())?;

    refresh_channel_dispatcher(&channel, state, &pool).await.map_err(|_| warp::reject())?;

    Ok(warp::reply::json(&serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct RemoveAliasPayload {
    pub alias: String,
}

pub async fn obs_alias_remove(cookies: Option<String>, body: RemoveAliasPayload, pool: Arc<PgPool>, state: Arc<AppState>) -> Result<impl Reply, warp::Rejection> {
    let channel =
        channel_from_session(cookies, &pool).await.map_err(|_| warp::reject())?;

    sqlx::query!(
        "DELETE FROM krapbott_v2.command_aliases WHERE channel = $1 AND alias = $2",
        channel.as_str(), body.alias.to_lowercase()
    ).execute(&*pool).await.map_err(|_| warp::reject())?;

    refresh_channel_dispatcher(&channel, state, &pool).await.map_err(|_| warp::reject())?;

    Ok(warp::reply::json(&serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct ToggleCommandPayload {
    pub command: String,
    pub disable: bool,
}

pub async fn obs_alias_toggle_command(cookies: Option<String>, body: ToggleCommandPayload, pool: Arc<PgPool>, state: Arc<AppState>) -> Result<impl Reply, warp::Rejection> {
    let channel =
        channel_from_session(cookies, &pool).await.map_err(|_| warp::reject())?;

    if body.disable {
        sqlx::query!(
            "INSERT INTO krapbott_v2.command_disabled (channel, command)
             VALUES ($1, $2) ON CONFLICT DO NOTHING",
            channel.as_str(), body.command
        ).execute(&*pool).await.map_err(|_| warp::reject())?;
    } else {
        sqlx::query!(
            "DELETE FROM krapbott_v2.command_disabled
             WHERE channel = $1 AND command = $2",
            channel.as_str(), body.command
        ).execute(&*pool).await.map_err(|_| warp::reject())?;
    }

    refresh_channel_dispatcher(&channel, state, &pool).await.map_err(|_| warp::reject())?;

    Ok(warp::reply::json(&serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct RestoreAliasPayload {
    pub alias: String,
}

pub async fn obs_alias_restore(cookies: Option<String>, body: RestoreAliasPayload, pool: Arc<PgPool>, state: Arc<AppState>) -> Result<impl Reply, warp::Rejection> {
    let channel = channel_from_session(cookies, &pool).await.map_err(|_| warp::reject())?;

    sqlx::query!(
        "DELETE FROM krapbott_v2.command_alias_removals
         WHERE channel = $1 AND alias = $2",
        channel.as_str(), body.alias.to_lowercase()
    ).execute(&*pool).await.map_err(|_| warp::reject())?;

    refresh_channel_dispatcher(&channel, state, &pool).await.map_err(|_| warp::reject())?;

    Ok(warp::reply::json(&serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct DefaultAliasPayload {
    pub alias: String,
}

pub async fn obs_alias_remove_default(cookies: Option<String>, body: DefaultAliasPayload, pool: Arc<PgPool>, state: Arc<AppState>) -> Result<impl Reply, warp::Rejection> {
    let channel = channel_from_session(cookies, &pool).await.map_err(|_| warp::reject())?;

    sqlx::query!(
        "INSERT INTO krapbott_v2.command_alias_removals (channel, alias)
         VALUES ($1, $2)
         ON CONFLICT DO NOTHING",
        channel.as_str(),
        body.alias.to_lowercase()
    ).execute(&*pool).await.map_err(|_| warp::reject())?;

    refresh_channel_dispatcher(&channel, state, &pool).await.map_err(|_| warp::reject())?;

    Ok(warp::reply::json(&serde_json::json!({ "ok": true })))
}

pub async fn obs_alias_restore_default(cookies: Option<String>, body: DefaultAliasPayload, pool: Arc<PgPool>, state: Arc<AppState>) -> Result<impl Reply, warp::Rejection> {
    let channel = channel_from_session(cookies, &pool).await.map_err(|_| warp::reject())?;

    sqlx::query!(
        "DELETE FROM krapbott_v2.command_alias_removals
         WHERE channel = $1 AND alias = $2",
        channel.as_str(),
        body.alias.to_lowercase()
    ).execute(&*pool).await.map_err(|_| warp::reject())?;

    refresh_channel_dispatcher(&channel, state, &pool).await.map_err(|_| warp::reject())?;

    Ok(warp::reply::json(&serde_json::json!({ "ok": true })))
}

#[derive(Debug)]
struct ObsQueueResetError;
impl warp::reject::Reject for ObsQueueResetError {}

pub async fn obs_queue_reset(
    cookies: Option<String>,
    pool: Arc<PgPool>,
    state: Arc<AppState>,
) -> Result<impl Reply, warp::Rejection> {
    let channel =
        channel_from_session(cookies, &pool).await.map_err(|_| warp::reject())?;

    let owner = resolve_queue_owner(&state, &channel)
        .await
        .map_err(|_| warp::reject::custom(ObsQueueResetError))?;

    reset_queue_runs(&pool, state.clone(), &owner).await.map_err(|_| warp::reject::custom(ObsQueueResetError))?;

    // Chat feedback
    state.chat_client.send_message(&owner, &Replies::queue_runs_reset(&owner)).await.map_err(|_| warp::reject::custom(ObsQueueResetError))?;

    Ok(warp::reply::json(&serde_json::json!({
        "ok": true
    })))
}
