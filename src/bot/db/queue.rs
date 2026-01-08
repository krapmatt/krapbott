use chrono::{DateTime, Utc};
use sqlx::{PgPool, types::time::OffsetDateTime};

use crate::bot::{chat_event::chat_event::Platform, commands::{commands::BotResult, queue::logic::{Queue, QueueEntry}}, db::{ChannelId, UserId, bungie::get_membership_id_by_user_id}, state::def::ObsQueueEntry};

pub const QUEUE_TABLE: &str = r#"
    CREATE TABLE IF NOT EXISTS krapbott_v2.queue (
        position INTEGER NOT NULL,
        user_id TEXT NOT NULL REFERENCES krapbott_v2.streamusers(id),
        display_name TEXT NOT NULL,
        bungie_name TEXT NOT NULL,
        channel_id TEXT NOT NULL,
        group_priority INTEGER DEFAULT 2,
        locked_first BOOLEAN DEFAULT FALSE,
        priority_runs_left INTEGER DEFAULT 0,
        PRIMARY KEY(position, channel_id)
    );
"#;

pub const BAN_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS krapbott_v2.banlist (
    membership_id TEXT PRIMARY KEY,
    banned_until TIMESTAMP NULL, -- Null = permanent ban
    reason TEXT
);
"#;

#[derive(Debug, Clone)]
pub enum BanStatus {
    NotBanned,
    Permanent {
        reason: Option<String>,
    },
    Timed {
        reason: Option<String>,
        banned_until: OffsetDateTime,
    },
}

/// Funkce pro kontrolu, zda je uživatel zabanovaný
pub async fn is_banned_from_queue(pool: &PgPool, user_id: &UserId) -> BotResult<BanStatus> {
    let membership_id = get_membership_id_by_user_id(pool, user_id).await?;
    let record = sqlx::query!(
        r#"
        SELECT reason, banned_until
        FROM krapbott_v2.banlist
        WHERE membership_id = $1
          AND (banned_until IS NULL OR banned_until > NOW())
        "#,
        membership_id
    )
    .fetch_optional(pool)
    .await?;

    let Some(record) = record else {
        return Ok(BanStatus::NotBanned);
    };

    match record.banned_until {
        None => Ok(BanStatus::Permanent {
            reason: record.reason,
        }),

        Some(naive) => {
            let utc = naive.assume_utc();

            Ok(BanStatus::Timed {
                reason: record.reason,
                banned_until: utc,
            })
        }
    }
}

//TODO PŘEDĚLAT FUNKCE NA USER_ID PODPORU a CHANNEL_ID Podporu
pub async fn add_to_queue(queue_len: usize, pool: &PgPool, user: &QueueEntry, channel_id: &ChannelId, join_type: Queue, raffle: bool) -> BotResult<String> {
    match join_type {
        Queue::Join => {
            let count: i64 = sqlx::query_scalar!(
                "SELECT COUNT(*) FROM krapbott_v2.queue WHERE channel_id = $1",
                channel_id.as_str()
            ).fetch_one(pool).await?.unwrap_or(0);

            if count >= queue_len as i64 {
                return Ok(if !raffle {
                    format!("❌ {}, you can't enter the queue, it is full", user.display_name)
                } else {
                    format!("❌ {}, you can't enter the raffle, it is full", user.display_name)
                });
            }

            if bungie_name_exists_in_queue(pool, &user.bungie_name, channel_id).await? {
                return Ok(format!("❌ {}, wishes for some jail time ⛓", user.display_name));
            }
        },
        Queue::ForceJoin => {}
    }

    let next_position: i32 = sqlx::query_scalar!(
        "SELECT COALESCE(MAX(position), 0) + 1 FROM krapbott_v2.queue WHERE channel_id = $1",
        channel_id.as_str()
    ).fetch_one(pool).await?.unwrap_or(1);

    let result = sqlx::query(
        "INSERT INTO krapbott_v2.queue (position, user_id, bungie_name, display_name, channel_id) VALUES ($1, $2, $3, $4, $5)",
    ).bind(next_position).bind(user.user_id.clone()).bind(user.bungie_name.clone()).bind(user.display_name.clone()).bind(channel_id.as_str()).execute(pool).await;

    match result {
        Ok(_) => Ok(if !raffle {
            format!("✅ {} entered the queue at position #{next_position}", user.display_name)
        } else {
            format!("✅ {} entered the raffle", user.display_name)
        }),
        Err(sqlx::Error::Database(err)) if err.is_unique_violation() => {
            Ok(format!("❌ Error Occured entering! {}", user.bungie_name))
        }
        Err(e) => Err(e.into())
    }
}

pub async fn user_exists_in_queue(pool: &PgPool, user_id: &UserId, channel_id: &ChannelId) -> BotResult<bool> {
    let exists = sqlx::query_scalar!(
        "SELECT EXISTS(SELECT 1 FROM krapbott_v2.queue WHERE user_id = $1 AND channel_id = $2)",
        user_id.as_str(), channel_id.as_str()
    ).fetch_one(pool).await?.unwrap_or(false);

    Ok(exists)
}

/// Zkontroluje, zda ve frontě existuje uživatel se stejným Bungie jménem
pub async fn bungie_name_exists_in_queue(pool: &PgPool, bungie_name: &str, channel_id: &ChannelId) -> BotResult<bool> {
    let exists: Option<bool> = sqlx::query_scalar!(
        "SELECT EXISTS(SELECT 1 FROM krapbott_v2.queue WHERE bungie_name = $1 AND channel_id = $2)",
        bungie_name, channel_id.as_str()
    ).fetch_one(pool).await?;

    Ok(exists.unwrap_or(false))
}

/// Aktualizuje Bungie jméno uživatele ve frontě pro daný kanál
pub async fn update_queue(pool: &PgPool, user: &QueueEntry, channel_id: &ChannelId) -> BotResult<()> {
    sqlx::query(
        "UPDATE krapbott_v2.queue SET bungie_name = $1, display_name = $2 WHERE user_id = $3 AND channel_id = $4",
    ).bind(user.bungie_name.clone()).bind(user.display_name.clone()).bind(user.user_id.clone()).bind(channel_id.as_str()).execute(pool).await?;
    Ok(())
}

pub async fn fetch_queue_for_owner(pool: &PgPool, owner: &ChannelId, teamsize: usize) -> BotResult<Vec<ObsQueueEntry>> {
    let rows = sqlx::query!(
        r#"
        SELECT position, display_name, bungie_name
        FROM krapbott_v2.queue
        WHERE channel_id = $1
        ORDER BY position ASC
        "#,
        owner.as_str()
    ).fetch_all(pool).await?;

    Ok(rows
        .into_iter()
        .map(|r| ObsQueueEntry {
            position: r.position,
            display_name: r.display_name,
            bungie_name: r.bungie_name,
        }).collect())
}