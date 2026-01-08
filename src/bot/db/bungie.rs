use sqlx::PgPool;

use crate::{api::bungie::{MemberShip, get_membershipid}, bot::{chat_event::chat_event::{ChatUser, Platform}, commands::commands::BotResult, db::{UserId, users::{User, upsert_stream_user}}, state::def::BotError}};

pub async fn register_bungie_name(
    pool: &PgPool,
    platform: Platform,             // "Twitch" nebo "Kick"
    platform_user_id: &str,     // ID uživatele na platformě
    login_name: &str,           // login name (např. twitch login)
    display_name: &str,         // zobrazované jméno
    bungie_name: &str, x_api_key: &str) -> BotResult<String> {
    // Zavoláme Bungie API pro získání membership info
    let membership_info = get_membershipid(bungie_name, x_api_key).await
        .map_err(|_| BotError::Custom("Problem with Bungie API".to_string()))?;

    if membership_info.type_m == -1 {
        return Ok(format!(
            "{} doesn't exist, check if your Bungie name is correct",
            bungie_name
        ));
    }

    let user = User {
        id: format!("{}:{}", platform, platform_user_id),
        platform: platform,
        platform_user_id: platform_user_id.to_string(),
        login_name: login_name.to_string(),
        display_name: display_name.to_string(),
        bungie_name: bungie_name.to_string(),
        membership_id: membership_info.id.to_string(),
        membership_type: membership_info.type_m,
    };

    upsert_stream_user(pool, &user).await?;

    Ok(format!(
        "{} has been registered to the database as {}",
        display_name, bungie_name
    ))
}

// Funkce pro načtení Bungie info podle platformy + ID uživatele
pub async fn load_membership(
    pool: &PgPool,
    platform: Platform,
    platform_user_id: &str,
) -> Option<MemberShip> {
    let user_id = UserId::new(platform, platform_user_id);

    let result = sqlx::query!(
        r#"
        SELECT membership_id, membership_type
        FROM krapbott_v2.streamusers
        WHERE id = $1
        "#,
        user_id.as_str()
    ).fetch_optional(pool).await.ok()?;

    match result {
        Some(row) => {
            Some(MemberShip {
                id: row.membership_id,
                type_m: row.membership_type,
            })
        }
        None => None,
    }
}

/// Kontrola, zda existuje Bungie jméno pro daného uživatele
/// Vrací true pokud existuje a uloží informace do DB
pub async fn is_bungiename(pool: &PgPool, user: &ChatUser, bungie_name: &str, x_api_key: &str) -> bool {
    match get_membershipid(bungie_name, x_api_key).await {
        Ok(info) if info.type_m != -1 => {

            let id = UserId::new(user.identity.platform, user.identity.platform_user_id.clone());
            let _ = sqlx::query(
                r#"
                INSERT INTO krapbott_v2.streamusers (id, platform, platform_user_id, login_name, display_name, bungie_name, membership_id, membership_type)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                ON CONFLICT (id) DO UPDATE SET
                    bungie_name = EXCLUDED.bungie_name,
                    membership_id = EXCLUDED.membership_id,
                    membership_type = EXCLUDED.membership_type
                "#,
            ).bind(id).bind(user.identity.platform.as_str()).bind(user.identity.platform_user_id.clone()).bind(user.name.login.clone()).bind(user.name.display.clone()).bind(bungie_name).bind(info.id.to_string()).bind(info.type_m).execute(pool).await;

            true
        }
        _ => false,
    }
}

pub async fn get_membership_id_by_user_id(pool: &PgPool, user_id: &UserId) -> Result<String, BotError> {
    let record = sqlx::query_scalar!(
        r#"
        SELECT membership_id
        FROM krapbott_v2.streamusers
        WHERE id = $1
        "#,
        user_id.as_str()
    )
    .fetch_one(pool)
    .await?;

    Ok(record)
}