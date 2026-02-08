use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use dashmap::DashMap;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::bot::{
    commands::commands::BotResult,
    state::def::{BotError, BotSecrets},
};

#[derive(Debug)]
pub struct KickAuthManager {
    state: RwLock<KickAuthState>,
    config: KickOAuthConfig,
    pending: DashMap<String, PkceState>,
    token_store_path: PathBuf,
}

#[derive(Debug, Clone, Default)]
struct KickAuthState {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_at: Option<Instant>,
}

#[derive(Debug, Clone)]
struct KickOAuthConfig {
    client_id: Option<String>,
    client_secret: Option<String>,
}

#[derive(Debug, Clone)]
struct PkceState {
    code_verifier: String,
    redirect_uri: String,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    token_type: Option<String>,
    #[serde(default)]
    scope: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
struct PersistedKickTokens {
    access_token: Option<String>,
    refresh_token: Option<String>,
}

impl KickAuthManager {
    pub fn from_secrets(secrets: &BotSecrets) -> Self {
        let token_store_path = token_store_path_from_env();
        let persisted = load_persisted_tokens(&token_store_path).unwrap_or_default();

        let state = KickAuthState {
            access_token: persisted.access_token.or(secrets.kick_access_token.clone()),
            refresh_token: persisted
                .refresh_token
                .or(secrets.kick_refresh_token.clone()),
            expires_at: None,
        };

        Self {
            state: RwLock::new(state),
            config: KickOAuthConfig {
                client_id: secrets.kick_client_id.clone(),
                client_secret: secrets.kick_client_secret.clone(),
            },
            pending: DashMap::new(),
            token_store_path,
        }
    }

    pub async fn bootstrap(&self) -> BotResult<()> {
        let mut state = self.state.write().await;

        if has_fresh_access_token(&state) {
            return Ok(());
        }

        if let Some(refresh) = state.refresh_token.clone() {
            if let Ok(token) = refresh_kick_token(&refresh, &self.config).await {
                apply_token(&mut state, token);
                persist_tokens_with_warning(&self.token_store_path, &state);
                return Ok(());
            }
        }

        if state.access_token.is_some() {
            // Refresh failed, but we still have a token to try.
            warn!("Kick OAuth refresh failed; using existing access token.");
            return Ok(());
        }

        warn!("Kick OAuth not configured; sending will be disabled.");
        Ok(())
    }

    pub async fn get_access_token(&self) -> BotResult<String> {
        {
            let state = self.state.read().await;
            if has_fresh_access_token(&state) {
                return Ok(state.access_token.clone().unwrap_or_default());
            }
        }

        let mut state = self.state.write().await;
        if has_fresh_access_token(&state) {
            return Ok(state.access_token.clone().unwrap_or_default());
        }

        if let Some(refresh) = state.refresh_token.clone() {
            if let Ok(token) = refresh_kick_token(&refresh, &self.config).await {
                apply_token(&mut state, token);
                persist_tokens_with_warning(&self.token_store_path, &state);
                return Ok(state.access_token.clone().unwrap_or_default());
            }
        }

        if let Some(existing) = state.access_token.clone() {
            warn!("Returning existing Kick access token without known expiry.");
            return Ok(existing);
        }

        Err(BotError::Custom("Kick access token unavailable".to_string()))
    }

    pub fn build_authorize_url(&self, redirect_uri: &str, scope: &str) -> BotResult<String> {
        let client_id = self
            .config
            .client_id
            .clone()
            .ok_or_else(|| BotError::Custom("KICK_CLIENT_ID not set".to_string()))?;

        let state = random_urlsafe(16);
        let code_verifier = random_urlsafe(32);
        let code_challenge = code_challenge_s256(&code_verifier);

        self.pending.insert(
            state.clone(),
            PkceState {
                code_verifier,
                redirect_uri: redirect_uri.to_string(),
            },
        );

        let url = format!(
            "https://id.kick.com/oauth/authorize?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}&code_challenge={}&code_challenge_method=S256",
            urlencoding::encode(&client_id),
            urlencoding::encode(redirect_uri),
            urlencoding::encode(scope),
            urlencoding::encode(&state),
            urlencoding::encode(&code_challenge),
        );

        Ok(url)
    }

    pub async fn exchange_code(&self, code: &str, state: &str) -> BotResult<()> {
        let Some((_, pkce)) = self.pending.remove(state) else {
            return Err(BotError::Custom("Kick OAuth state invalid".to_string()));
        };

        let token = authorization_code_token(code, &pkce, &self.config).await?;
        let mut state_guard = self.state.write().await;
        apply_token(&mut state_guard, token);
        persist_tokens_with_warning(&self.token_store_path, &state_guard);
        Ok(())
    }
}

fn apply_token(state: &mut KickAuthState, token: TokenResponse) {
    let expires_at = token
        .expires_in
        .map(|s| Instant::now() + Duration::from_secs(s.saturating_sub(30)));
    state.access_token = Some(token.access_token);
    state.refresh_token = token.refresh_token.or(state.refresh_token.clone());
    state.expires_at = expires_at;

    info!(
        "Kick OAuth token updated (access_present={}, refresh_present={})",
        state.access_token.is_some(),
        state.refresh_token.is_some()
    );
}

fn has_fresh_access_token(state: &KickAuthState) -> bool {
    match (&state.access_token, state.expires_at) {
        (Some(_), Some(expires_at)) => expires_at > Instant::now(),
        _ => false,
    }
}

fn token_store_path_from_env() -> PathBuf {
    match std::env::var("KICK_TOKEN_STORE_PATH") {
        Ok(path) if !path.trim().is_empty() => PathBuf::from(path),
        _ => PathBuf::from(".secrets/kick_oauth_tokens.json"),
    }
}

fn load_persisted_tokens(path: &Path) -> Option<PersistedKickTokens> {
    let data = match fs::read_to_string(path) {
        Ok(data) => data,
        Err(err) => {
            if err.kind() != std::io::ErrorKind::NotFound {
                warn!("Failed to read Kick token store {}: {}", path.display(), err);
            }
            return None;
        }
    };

    match serde_json::from_str::<PersistedKickTokens>(&data) {
        Ok(tokens) => Some(tokens),
        Err(err) => {
            warn!(
                "Failed to parse Kick token store {}: {}",
                path.display(),
                err
            );
            None
        }
    }
}

fn persist_tokens_with_warning(path: &Path, state: &KickAuthState) {
    let tokens = PersistedKickTokens {
        access_token: state.access_token.clone(),
        refresh_token: state.refresh_token.clone(),
    };

    if tokens.access_token.is_none() && tokens.refresh_token.is_none() {
        return;
    }

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(err) = fs::create_dir_all(parent) {
                warn!(
                    "Failed to create Kick token store directory {}: {}",
                    parent.display(),
                    err
                );
                return;
            }
        }
    }

    match serde_json::to_string_pretty(&tokens) {
        Ok(serialized) => {
            if let Err(err) = fs::write(path, serialized) {
                warn!("Failed to write Kick token store {}: {}", path.display(), err);
            }
        }
        Err(err) => {
            warn!("Failed to serialize Kick tokens: {}", err);
        }
    }
}

async fn refresh_kick_token(refresh_token: &str, config: &KickOAuthConfig) -> BotResult<TokenResponse> {
    let (client_id, client_secret) = get_client_credentials(config)?;

    let body = format!(
        "grant_type=refresh_token&client_id={}&client_secret={}&refresh_token={}",
        urlencoding::encode(&client_id),
        urlencoding::encode(&client_secret),
        urlencoding::encode(refresh_token),
    );

    let client = reqwest::Client::new();
    let response = client
        .post("https://id.kick.com/oauth/token")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(BotError::Custom(format!(
            "Kick token refresh failed ({status}): {text}"
        )));
    }

    Ok(response.json::<TokenResponse>().await?)
}

async fn client_credentials_token(config: &KickOAuthConfig) -> BotResult<TokenResponse> {
    let (client_id, client_secret) = get_client_credentials(config)?;

    let body = format!(
        "grant_type=client_credentials&client_id={}&client_secret={}",
        urlencoding::encode(&client_id),
        urlencoding::encode(&client_secret),
    );

    let client = reqwest::Client::new();
    let response = client
        .post("https://id.kick.com/oauth/token")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(BotError::Custom(format!(
            "Kick client credentials failed ({status}): {text}"
        )));
    }

    Ok(response.json::<TokenResponse>().await?)
}

async fn authorization_code_token(
    code: &str,
    pkce: &PkceState,
    config: &KickOAuthConfig,
) -> BotResult<TokenResponse> {
    let (client_id, client_secret) = get_client_credentials(config)?;

    let body = format!(
        "grant_type=authorization_code&client_id={}&client_secret={}&redirect_uri={}&code_verifier={}&code={}",
        urlencoding::encode(&client_id),
        urlencoding::encode(&client_secret),
        urlencoding::encode(&pkce.redirect_uri),
        urlencoding::encode(&pkce.code_verifier),
        urlencoding::encode(code),
    );

    let client = reqwest::Client::new();
    let response = client
        .post("https://id.kick.com/oauth/token")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(BotError::Custom(format!(
            "Kick authorization code failed ({status}): {text}"
        )));
    }

    Ok(response.json::<TokenResponse>().await?)
}

fn get_client_credentials(config: &KickOAuthConfig) -> BotResult<(String, String)> {
    let client_id = config
        .client_id
        .clone()
        .ok_or_else(|| BotError::Custom("KICK_CLIENT_ID not set".to_string()))?;
    let client_secret = config
        .client_secret
        .clone()
        .ok_or_else(|| BotError::Custom("KICK_CLIENT_SECRET not set".to_string()))?;

    Ok((client_id, client_secret))
}

fn random_urlsafe(len_bytes: usize) -> String {
    let mut bytes = vec![0u8; len_bytes];
    let mut rng = rand::thread_rng();
    rng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn code_challenge_s256(verifier: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let digest = hasher.finalize();
    URL_SAFE_NO_PAD.encode(digest)
}
