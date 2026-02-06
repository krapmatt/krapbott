use std::{collections::HashMap, sync::Arc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;

use sqlx::{query, types::time::{self, PrimitiveDateTime}, PgPool};
use uuid::Uuid;
use tokio::sync::{mpsc::UnboundedSender, RwLock};
use tokio_tungstenite::{connect_async, tungstenite::{Message, http::Uri}};
use warp::reply::Reply;
use crate::{bot::{self, chat_event::chat_event::Platform, db::ChannelId, state::def::AppState}};

use crate::bot::state::def::BotSecrets;

pub async fn twitch_login(secrets: Arc<BotSecrets>) -> Result<impl warp::Reply, warp::Rejection> {
    let client_id = &secrets.bot_id;
    let redirect_uri = "https://krapbott.up.railway.app/auth/callback";

    let url = format!(
        "https://id.twitch.tv/oauth2/authorize\
        ?client_id={}&redirect_uri={}\
        &response_type=code&scope=user:read:email",
        client_id,
        urlencoding::encode(redirect_uri)
    );
    let uri: Uri = url.parse().unwrap();
    Ok(warp::redirect::temporary(uri))
}

#[derive(Deserialize)]
pub struct TwitchTokenResponse {
    access_token: String,
}

#[derive(Deserialize)]
pub struct TwitchUser {
    id: String,
    login: String,
}

#[derive(Deserialize)]
pub struct TwitchUsers {
    data: Vec<TwitchUser>,
}

#[derive(Serialize)]
struct TokenRequest<'a> {
    client_id: &'a str,
    client_secret: &'a str,
    code: &'a str,
    grant_type: &'a str,
    redirect_uri: &'a str,
}

pub async fn twitch_callback(query: HashMap<String, String>, pool: Arc<sqlx::PgPool>, state: Arc<AppState>) -> Result<impl warp::Reply, warp::Rejection> {
    let code = query.get("code").ok_or(warp::reject())?;
    let token_request = TokenRequest {
        client_id: &state.secrets.bot_id,
        client_secret: &state.secrets.client_secret,
        code: code,
        grant_type: "authorization_code",
        redirect_uri: "https://krapbott.up.railway.app/auth/callback",
    };
    let client = reqwest::Client::new();
    // Exchange code → token
    let token = client
        .post("https://id.twitch.tv/oauth2/token")
        .json(&token_request)
        .send()
        .await
        .map_err(|_| warp::reject())?
        .json::<TwitchTokenResponse>()
        .await
        .map_err(|_| warp::reject())?;

    // Fetch user
    let user = reqwest::Client::new()
        .get("https://api.twitch.tv/helix/users")
        .header("Client-Id", &state.secrets.bot_id)
        .bearer_auth(&token.access_token)
        .send()
        .await
        .map_err(|_| warp::reject())?
        .json::<TwitchUsers>()
        .await
        .map_err(|_| warp::reject())?
        .data
        .into_iter()
        .next()
        .ok_or(warp::reject())?;

    
    let channel_id = ChannelId::new(Platform::Twitch, &user.login);
    let allowed = {
        let cfg = state.config.read().await;
        cfg.channels.contains_key(&channel_id)
    };

    if !allowed {
        let reply = warp::reply::html("❌ This channel is not authorized.");

        return Ok(warp::reply::with_header(
            reply,
            "Content-Type",
            "text/html; charset=utf-8",
        ).into_response());
    }
    // Create session
    let session_id = uuid::Uuid::new_v4().to_string();
    sqlx::query!(
        r#"
        INSERT INTO krapbott_v2.sessions
        (session_id, platform, platform_user_id, login)
        VALUES ($1, 'twitch', $2, $3)
        ON CONFLICT (platform, platform_user_id)
        DO UPDATE SET
            session_id = EXCLUDED.session_id,
            created_at = NOW()
        "#,
        session_id, user.id, user.login
    ).execute(&*pool).await.map_err(|_| warp::reject())?;

    let reply = warp::redirect::temporary(Uri::from_static("/obs"));

    Ok(warp::reply::with_header(
        reply,
        "Set-Cookie",
        format!(
            "session_id={}; Path=/; HttpOnly; SameSite=None; Secure",
            session_id
        ),
    ).into_response())
}

pub async fn kick_login(state: Arc<AppState>) -> Result<impl warp::Reply, warp::Rejection> {
    let redirect_uri = state
        .secrets
        .kick_redirect_uri
        .as_deref()
        .ok_or_else(warp::reject)?;

    let scope = "chat:write";
    let url = state
        .chat_client
        .kick_auth
        .build_authorize_url(redirect_uri, scope)
        .map_err(|_| warp::reject())?;

    let uri: Uri = url.parse().map_err(|_| warp::reject())?;
    Ok(warp::redirect::temporary(uri))
}

pub async fn kick_callback(query: HashMap<String, String>, state: Arc<AppState>) -> Result<impl warp::Reply, warp::Rejection> {
    if let Some(error) = query.get("error") {
        let desc = query.get("error_description").cloned().unwrap_or_default();
        let reply = warp::reply::html(format!("Kick auth failed: {} {}", error, desc));
        return Ok(warp::reply::with_header(reply, "Content-Type", "text/html; charset=utf-8"));
    }

    let code = query.get("code").ok_or(warp::reject())?;
    let state_param = query.get("state").ok_or(warp::reject())?;

    state
        .chat_client
        .kick_auth
        .exchange_code(code, state_param)
        .await
        .map_err(|_| warp::reject())?;

    let reply = warp::reply::html("Kick authorized. You can close this window.".to_string());
    Ok(warp::reply::with_header(
        reply,
        "Content-Type",
        "text/html; charset=utf-8",
    ))
}
