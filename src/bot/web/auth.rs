use std::{collections::HashMap, sync::Arc};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_tungstenite::tungstenite::http::Uri;
use tracing::error;
use warp::{http::{header::SET_COOKIE, HeaderValue, StatusCode}, reply::Reply};

use crate::bot::{
    chat_event::chat_event::Platform,
    db::ChannelId,
    state::def::{AppState, BotSecrets},
    web::sessions::{platform_session_cookie, session_cookie_header},
};

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

    let mut response = reply.into_response();
    let platform_cookie = session_cookie_header(platform_session_cookie(Platform::Twitch), &session_id);
    let active_cookie = session_cookie_header("session_id", &session_id);
    response.headers_mut().append(
        SET_COOKIE,
        HeaderValue::from_str(&platform_cookie).map_err(|_| warp::reject())?,
    );
    response.headers_mut().append(
        SET_COOKIE,
        HeaderValue::from_str(&active_cookie).map_err(|_| warp::reject())?,
    );
    Ok(response)
}

pub async fn kick_login(state: Arc<AppState>) -> Result<impl warp::Reply, warp::Rejection> {
    let redirect_uri = match state.secrets.kick_redirect_uri.as_deref() {
        Some(uri) => uri,
        None => {
            let reply = warp::reply::with_status(
                warp::reply::html("Kick OAuth misconfigured: KICK_REDIRECT_URI missing".to_string()),
                StatusCode::INTERNAL_SERVER_ERROR,
            );
            return Ok(reply.into_response());
        }
    };

    let scope = "chat:write user:read";
    let url = match state
        .chat_client
        .kick_auth
        .build_authorize_url(redirect_uri, scope)
    {
        Ok(url) => url,
        Err(err) => {
            error!("Kick OAuth build_authorize_url failed: {}", err);
            let reply = warp::reply::with_status(
                warp::reply::html(format!("Kick OAuth setup failed: {err}")),
                StatusCode::INTERNAL_SERVER_ERROR,
            );
            return Ok(reply.into_response());
        }
    };

    let uri: Uri = match url.parse() {
        Ok(uri) => uri,
        Err(err) => {
            error!("Kick OAuth authorize URL parse failed: {}", err);
            let reply = warp::reply::with_status(
                warp::reply::html("Kick OAuth setup failed: invalid authorize URL".to_string()),
                StatusCode::INTERNAL_SERVER_ERROR,
            );
            return Ok(reply.into_response());
        }
    };

    Ok(warp::redirect::temporary(uri).into_response())
}

fn parse_kick_identity(value: &Value) -> Option<(String, String)> {
    let user = if let Some(obj) = value.as_object() {
        if obj.contains_key("data") {
            value
                .get("data")
                .and_then(|d| d.as_array())
                .and_then(|arr| arr.first())
                .unwrap_or(value)
        } else {
            value
        }
    } else {
        value
    };

    let id = user
        .get("id")
        .or_else(|| user.get("user_id"))
        .and_then(|v| v.as_u64().map(|n| n.to_string()).or_else(|| v.as_str().map(|s| s.to_string())))?;

    let login = user
        .get("username")
        .or_else(|| user.get("name"))
        .or_else(|| user.get("slug"))
        .or_else(|| user.get("channel_slug"))
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_ascii_lowercase())?;

    if login.is_empty() {
        return None;
    }
    Some((id, login))
}

pub async fn kick_callback(query: HashMap<String, String>, pool: Arc<sqlx::PgPool>, state: Arc<AppState>) -> Result<impl warp::Reply, warp::Rejection> {
    if let Some(error) = query.get("error") {
        let desc = query.get("error_description").cloned().unwrap_or_default();
        let reply = warp::reply::with_status(
            warp::reply::html(format!("Kick auth failed: {} {}", error, desc)),
            StatusCode::BAD_REQUEST,
        );
        return Ok(reply.into_response());
    }

    let code = match query.get("code") {
        Some(code) => code,
        None => {
            let reply = warp::reply::with_status(
                warp::reply::html("Kick callback missing query parameter: code".to_string()),
                StatusCode::BAD_REQUEST,
            );
            return Ok(reply.into_response());
        }
    };

    let state_param = match query.get("state") {
        Some(state_param) => state_param,
        None => {
            let reply = warp::reply::with_status(
                warp::reply::html("Kick callback missing query parameter: state".to_string()),
                StatusCode::BAD_REQUEST,
            );
            return Ok(reply.into_response());
        }
    };

    if let Err(err) = state
        .chat_client
        .kick_auth
        .exchange_code(code, state_param)
        .await
    {
        error!("Kick OAuth exchange_code failed: {}", err);
        let reply = warp::reply::with_status(
            warp::reply::html(format!("Kick auth failed during token exchange: {err}")),
            StatusCode::INTERNAL_SERVER_ERROR,
        );
        return Ok(reply.into_response());
    }

    let access_token = state
        .chat_client
        .kick_auth
        .get_access_token()
        .await
        .map_err(|_| warp::reject())?;

    let profile = reqwest::Client::new()
        .get("https://api.kick.com/public/v1/users")
        .bearer_auth(&access_token)
        .send()
        .await
        .map_err(|_| warp::reject())?;

    if !profile.status().is_success() {
        let status = profile.status();
        let body = profile.text().await.unwrap_or_default();
        let reply = warp::reply::with_status(
            warp::reply::html(format!("Kick auth failed while fetching user profile ({status}): {body}")),
            StatusCode::BAD_GATEWAY,
        );
        return Ok(reply.into_response());
    }

    let profile_json: Value = profile.json().await.map_err(|_| warp::reject())?;
    let Some((kick_user_id, kick_login)) = parse_kick_identity(&profile_json) else {
        let reply = warp::reply::with_status(
            warp::reply::html("Kick auth failed: could not parse Kick user profile.".to_string()),
            StatusCode::BAD_GATEWAY,
        );
        return Ok(reply.into_response());
    };

    let allowed = {
        let cfg = state.config.read().await;
        cfg.channels
            .keys()
            .any(|ch| ch.platform() == Platform::Kick && ch.channel().eq_ignore_ascii_case(&kick_login))
    };

    if !allowed {
        let reply = warp::reply::with_status(
            warp::reply::html(format!("Kick channel '{kick_login}' is not authorized for this bot.")),
            StatusCode::FORBIDDEN,
        );
        return Ok(reply.into_response());
    }

    let session_id = uuid::Uuid::new_v4().to_string();
    sqlx::query!(
        r#"
        INSERT INTO krapbott_v2.sessions
        (session_id, platform, platform_user_id, login)
        VALUES ($1, 'kick', $2, $3)
        ON CONFLICT (platform, platform_user_id)
        DO UPDATE SET
            session_id = EXCLUDED.session_id,
            created_at = NOW()
        "#,
        session_id,
        kick_user_id,
        kick_login
    )
    .execute(&*pool)
    .await
    .map_err(|_| warp::reject())?;

    let reply = warp::redirect::temporary(Uri::from_static("/obs"));
    let mut response = reply.into_response();
    let platform_cookie = session_cookie_header(platform_session_cookie(Platform::Kick), &session_id);
    let active_cookie = session_cookie_header("session_id", &session_id);
    response.headers_mut().append(
        SET_COOKIE,
        HeaderValue::from_str(&platform_cookie).map_err(|_| warp::reject())?,
    );
    response.headers_mut().append(
        SET_COOKIE,
        HeaderValue::from_str(&active_cookie).map_err(|_| warp::reject())?,
    );
    Ok(response)
}
