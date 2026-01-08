use std::{convert::Infallible, str::FromStr, sync::Arc};

use serde::{Deserialize, Serialize};
use shuttle_warp::warp::{self, filters::sse::Event, reply::{Reply, Response}};
use sqlx::PgPool;
use tokio_tungstenite::tungstenite::http::Uri;
use tracing::info;

use crate::bot::{commands::queue::logic::{remove_from_queue, reorder_queue, resolve_queue_owner, run_next, set_queue_open}, db::{UserId, queue::fetch_queue_for_owner}, handler::handler::ChatClient, state::def::{AppState, ObsQueueEntry}, web::sessions::channel_from_session};

pub async fn obs_page(cookies: Option<String>, pool: Arc<sqlx::PgPool>) -> Result<impl warp::Reply, warp::Rejection> {
    if channel_from_session(cookies, &pool).await.is_err() {
        return Ok(warp::redirect::temporary(Uri::from_static("/auth/twitch")).into_response());
    }

    Ok(warp::reply::html(include_str!("public/queue_dock.html")).into_response())
}

#[derive(Serialize)]
pub struct ObsQueueResponse {
    pub open: bool,
    pub teamsize: usize,
    pub queue: Vec<ObsQueueEntry>,
}

pub async fn obs_queue(cookies: Option<String>, pool: Arc<PgPool>, state: Arc<AppState>) -> Result<Response, warp::Rejection> {
    let channel = match channel_from_session(cookies, &pool).await {
        Ok(c) => c,
        Err(_) => return Err(warp::reject()),
    };

    let owner = resolve_queue_owner(&state, &channel).await.map_err(|_| warp::reject())?;

    let (teamsize, open) = {
        let cfg = state.config.read().await;
        (cfg.get_channel_config(&owner)
            .map(|c| c.teamsize)
            .unwrap_or(1), cfg.get_channel_config(&owner).map(|c| c.open).unwrap_or(false))
    };

    let queue = fetch_queue_for_owner(&pool, &owner, teamsize).await.map_err(|_| warp::reject())?;
    info!("{:?}", queue);
    Ok(warp::reply::json(&ObsQueueResponse {
        open,
        teamsize,
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

    remove_from_queue(&pool, &owner, &user_id)
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
        "🟢 Queue is now OPEN!"
    } else {
        "🔴 Queue is now CLOSED!"
    };

    state.chat_client.send_message(&owner, msg).await.map_err(|_| warp::reject::custom(ObsToggleError))?;

    Ok(warp::reply::json(&serde_json::json!({
        "ok": true,
        "open": body.open
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

