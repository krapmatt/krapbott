use sqlx::PgPool;

use crate::bot::{chat_event::chat_event::{ChatUser, Platform}, commands::{commands::BotResult, queue::logic::QueueUser}, db::{ChannelId, UserId}};

pub const USERS_TABLE: &str = "
    CREATE TABLE IF NOT EXISTS krapbott_v2.streamusers (
        id TEXT PRIMARY KEY,             -- platform:platform_user_id
        platform TEXT NOT NULL,          -- 'Twitch' nebo 'Kick'
        platform_user_id TEXT NOT NULL,  -- ID uživatele na platformě
        login_name TEXT NOT NULL,        -- login username (lowercase pro Twitch)
        display_name TEXT NOT NULL,      -- zobrazované jméno
        bungie_name TEXT NOT NULL,                -- Bungie jméno
        membership_id TEXT NOT NULL,              -- Bungie membership id
        membership_type INTEGER NOT NULL,          -- Bungie membership type
        UNIQUE(platform, platform_user_id)
    );
";

pub const SESSIONS_TABLE: &str = "
    CREATE TABLE IF NOT EXISTS krapbott_v2.sessions (
        session_id TEXT PRIMARY KEY,
        platform TEXT NOT NULL,
        platform_user_id TEXT NOT NULL,
        login TEXT NOT NULL,
        created_at TIMESTAMP DEFAULT now()
    );
";

#[derive(Debug, Clone)]
pub struct User {
    pub id: String,
    pub platform: Platform,
    pub platform_user_id: String,
    pub login_name: String,
    pub display_name: String,
    pub bungie_name: String,
    pub membership_id: String,
    pub membership_type: i32,
}

/// Přidání / aktualizace uživatele
pub async fn upsert_stream_user(pool: &PgPool, user: &User) -> BotResult<()> {
    // Vytvoříme unikátní ID jako platform:platform_user_id
    let user_id = UserId::new(user.platform, user.id.clone());

    sqlx::query(
        r#"
        INSERT INTO krapbott_v2.streamusers (id, platform, platform_user_id, login_name, display_name, bungie_name, membership_id, membership_type)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        ON CONFLICT (id) DO UPDATE
        SET login_name = EXCLUDED.login_name,
            display_name = EXCLUDED.display_name,
            bungie_name = EXCLUDED.bungie_name,
            membership_id = EXCLUDED.membership_id,
            membership_type = EXCLUDED.membership_type
        "#
    ).bind(&user_id.0).bind(&user.platform.as_str()).bind(&user.platform_user_id).bind(&user.login_name).bind(&user.display_name).bind(&user.bungie_name).bind(&user.membership_id).bind(&user.membership_type).execute(pool).await?;

    Ok(())
}

#[derive(sqlx::FromRow)]
struct StreamUserRow {
    platform: Platform,
    platform_user_id: String,
    login_name: String,
    display_name: String,
    bungie_name: String,
    membership_id: String,
    membership_type: i32,
}
//TODO refactor to use UserId type
/// Načte uživatele podle složeného ID
pub async fn get_queue_user_by_id(pool: &PgPool, user_id: &UserId) -> BotResult<Option<QueueUser>> {
    let row = sqlx::query!(
        "SELECT login_name, display_name, bungie_name, membership_id, membership_type FROM krapbott_v2.streamusers WHERE id=$1",
        &user_id.0
    ).fetch_optional(pool).await?;

    Ok(row.map(|r| QueueUser {
        user_id: user_id.clone(),
        login_name: r.login_name,
        display_name: r.display_name,
        bungie_name: r.bungie_name,
        membership_id: r.membership_id,
        membership_type: r.membership_type,
    }))
}

