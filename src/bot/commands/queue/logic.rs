use std::sync::Arc;
use regex::Regex;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tracing::info;
use crate::bot::db::config::save_channel_config;
use crate::bot::db::queue::BanStatus;
use crate::bot::db::queue::add_to_queue;
use crate::bot::db::queue::is_banned_from_queue;
use crate::bot::db::queue::user_exists_in_queue;
use crate::bot::db::queue::update_queue;
use crate::bot::replies::Replies;
use crate::bot::state::def::BotError;
use crate::bot::web::sse::SseEvent;
use crate::bot::{chat_event::chat_event::ChatEvent, commands::commands::BotResult, db::{ChannelId, UserId, bungie::is_bungiename, users::get_queue_user_by_id}, state::def::AppState};

lazy_static::lazy_static!{
    static ref BUNGIE_REGEX: Regex = Regex::new(r"^(?P<name>.+)#(?P<digits>\d{4})").unwrap();
}

pub enum Queue {
    Join,
    ForceJoin,
}


#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum QueueKey {
    Single(ChannelId),
    Shared(ChannelId),
}

impl QueueKey {
    pub fn owner_channel(&self) -> &ChannelId {
        match self {
            QueueKey::Single(channel_id) => channel_id,
            QueueKey::Shared(shared) => shared,
        }
    }
}

#[derive(Clone, Debug)]
pub struct QueueUser {
    pub user_id: UserId,          // platform:platform_user_id
    pub login_name: String,
    pub display_name: String,
    pub bungie_name: String,
    pub membership_id: String,
    pub membership_type: i32,
}

pub async fn resolve_queue_owner(state: &AppState, caller: &ChannelId) -> BotResult<ChannelId> {
    let cfg = state.config.read().await;

    let channel_cfg = cfg
        .channels
        .get(caller)
        .ok_or(BotError::ConfigMissing(caller.clone()))?;

    Ok(channel_cfg.queue_target.owner_channel().clone())
}

impl AppState {
    pub async fn handle_join(&self, event: ChatEvent, pool: &PgPool) -> BotResult<Option<String>> {
        info!("Handling !join command from user");
        // Kontrola, zda máme user
        let user = match &event.user {
            Some(u) => u,
            None => return Ok(None),
        };

        if let Some(following) =  event.follower {
            if !following {
                return Ok(Some("You are not following the channel 😭".to_string()))
            }
        }

        let channel_id = ChannelId::new(event.platform, &event.channel);

        let (queue_owner, open, queue_len, random_queue) = {
            let state = self.config.read().await;
            let cfg = match state.get_channel_config(&channel_id) {
                Some(cfg) => cfg,
                None => return Ok(None),
            };

            let owner = cfg.queue_target.owner_channel().clone();
            let cfg = match state.get_channel_config(&owner) {
                Some(cfg) => cfg,
                None => return Ok(None),
            };
            (
                owner,
                cfg.open,
                cfg.size,
                cfg.random_queue
            )
        };  
        
        if !open {
            return Ok(Some(Replies::join_closed(&user.name.display)));
        }

        let user_id = UserId::new(user.identity.platform, user.identity.platform_user_id.clone());
        let stored_user = get_queue_user_by_id(&pool, &user_id).await?;
        
        let provided_name = event
            .message
            .split_once(' ')
            .map(|(_, rest)| rest.trim())
            .filter(|s| !s.is_empty())
            .map(String::from);

        let bungie_name = match (provided_name, stored_user) {
            (Some(provided), Some(stored)) => {
                if stored.bungie_name == provided {
                    Some(stored.bungie_name)
                } else if is_valid_bungie_name(&provided).is_some()
                    && is_bungiename(pool, &user, &provided, &self.secrets.x_api_key).await {
                    Some(provided)
                } else {
                    Some(stored.bungie_name)
                }
            }

            (Some(provided), None) => {
                if is_valid_bungie_name(&provided).is_some() && is_bungiename(pool, &user, &provided, &self.secrets.x_api_key).await {
                    Some(provided)
                } else {
                    None
                }
            }

            (None, Some(stored)) => Some(stored.bungie_name),
            (None, None) => None,
        };

        let bungie_name = match bungie_name {
            Some(name) => name,
            None => {
                return Ok(Some(Replies::join_invalid_bungie(&user.name.display)));
            }
        };

        match is_banned_from_queue(pool, &user_id).await? {
            BanStatus::NotBanned => {}
            BanStatus::Permanent { reason } => {
                return Ok(Some(Replies::join_banned(&user.name.display, reason.as_deref())));
            }
            BanStatus::Timed { reason, .. } => {
                return Ok(Some(Replies::join_timed_out(&user.name.display)));
            }
        }

        let entry = QueueEntry {
            user_id,
            bungie_name: bungie_name.clone(),
            display_name: user.name.display.clone(),
        };

        let reply = process_queue_entry(pool, queue_len, entry, &queue_owner, Queue::Join,random_queue).await?;

        Ok(Some(reply))
    }
}
#[derive(Debug)]
pub struct QueueEntry {
    pub user_id: UserId,
    pub bungie_name: String,
    pub display_name: String,
}
pub async fn process_queue_entry(pool: &PgPool, queue_len: usize, user: QueueEntry, channel_id: &ChannelId, queue_join: Queue, raffle: bool) -> BotResult<String> {
    let reply = if user_exists_in_queue(&pool, &user.user_id, channel_id).await? {
        update_queue(&pool, &user, channel_id).await?;
        format!("✅ {} has updated their bungie name to {}", user.display_name, user.bungie_name)
    } else {
        add_to_queue(queue_len, &pool, &user, channel_id, queue_join, raffle).await?
    };
    Ok(reply)
}

pub fn is_valid_bungie_name(name: &str) -> Option<String> {
    BUNGIE_REGEX.captures(name).map(|caps| format!("{}#{}", &caps["name"].trim(), &caps["digits"]))
}

pub async fn randomize_queue(channel: &ChannelId, pool: &PgPool, teamsize: i64) -> Result<String, BotError> {
    let mut tx = pool.begin().await?;
    let entries = sqlx::query!(
        "SELECT user_id FROM krapbott_v2.queue WHERE channel_id = $1 ORDER BY position LIMIT $2",
        channel.as_str(), teamsize
    ).fetch_all(&mut *tx).await?;
    for entry in entries {
        sqlx::query!(
            "DELETE FROM krapbott_v2.queue WHERE channel_id = $1 AND user_id = $2",
            channel.as_str(), entry.user_id
        ).fetch_all(&mut *tx).await?;
    }
    sqlx::query!(
        "UPDATE krapbott_v2.queue
         SET position = position + 10000
         WHERE channel_id = $1;",
        channel.as_str()
    ).execute(&mut *tx).await?;

    // Step 2: Randomly assign new sequential positions
    sqlx::query!(
        "WITH shuffled AS (
            SELECT position, user_id, bungie_name, 
                   ROW_NUMBER() OVER () AS new_position
            FROM (SELECT * FROM krapbott_v2.queue WHERE channel_id = $1 ORDER BY RANDOM())
        )
        UPDATE krapbott_v2.queue
        SET position = (SELECT new_position FROM shuffled WHERE shuffled.position = krapbott_v2.queue.position)
        WHERE channel_id = $1;",
        channel.as_str()
    ).execute(&mut *tx).await?;

    let next_group = sqlx::query!(
        "SELECT display_name, bungie_name FROM krapbott_v2.queue WHERE channel_id = $1 ORDER BY position ASC LIMIT $2",
        channel.as_str(), teamsize
    ).fetch_all(&mut *tx).await?;
    

    tx.commit().await?;
    let selected_team = next_group.iter().map(|q| format!("{}", q.display_name)).collect::<Vec<String>>().join(", ");
        // Announce the random selection
    Ok(Replies::raffle_won(&selected_team))
}

pub async fn next_handler(channel: &ChannelId, pool: &PgPool, teamsize: i64) -> BotResult<String> {
    let mut tx = pool.begin().await?;

    // Step 1: Fetch current group
    let queue_entries = sqlx::query!(
        "SELECT user_id, bungie_name, priority_runs_left, locked_first 
         FROM krapbott_v2.queue 
         WHERE channel_id = $1 
         ORDER BY position ASC 
         LIMIT $2",
        channel.as_str(), teamsize
    ).fetch_all(&mut *tx).await?;

    for entry in &queue_entries {
        match (entry.locked_first.unwrap_or(false), entry.priority_runs_left.unwrap_or(0)) {
            (true, runs_left) if runs_left > 0 => {
                let new_count = runs_left - 1;
                if new_count == 0 {
                    // Priority user done
                    sqlx::query!(
                        "DELETE FROM krapbott_v2.queue WHERE user_id = $1 AND channel_id = $2",
                        entry.user_id, channel.as_str()
                    ).execute(&mut *tx).await?;
                } else {
                    // Decrement runs
                    sqlx::query!(
                        "UPDATE krapbott_v2.queue SET priority_runs_left = $1 WHERE user_id = $2 AND channel_id = $3",
                        new_count, entry.user_id, channel.as_str()
                    ).execute(&mut *tx).await?;
                }
            }
            _ => {
                // Not a prio user — remove immediately
                sqlx::query!(
                    "DELETE FROM krapbott_v2.queue WHERE user_id = $1 AND channel_id = $2",
                    entry.user_id, channel.as_str()
                ).execute(&mut *tx).await?;
            }
        }
    }

    // Step 3: Lock next group of priority users (if any)
    sqlx::query!(
        "UPDATE krapbott_v2.queue SET locked_first = TRUE
         WHERE channel_id = $1 AND group_priority = 1 AND locked_first = FALSE 
         AND user_id IN (
             SELECT user_id FROM krapbott_v2.queue 
             WHERE channel_id = $1 
             ORDER BY position ASC 
             LIMIT $2
         )",
        channel.as_str(), teamsize
    ).execute(&mut *tx).await?;

    // Step 4: Get the new top of the queue for response
    let remaining_queue = sqlx::query!(
        "SELECT display_name, bungie_name FROM krapbott_v2.queue 
         WHERE channel_id = $1 
         ORDER BY position ASC 
         LIMIT $2",
        channel.as_str(), teamsize
    ).fetch_all(&mut *tx).await?;

    let result: Vec<_> = remaining_queue
        .into_iter()
        .map(|row| format!("@{} ({})", row.display_name, row.bungie_name))
        .collect();

    // Step 5: Recalculate positions
    let rows = sqlx::query!(
        "SELECT position, user_id FROM krapbott_v2.queue WHERE channel_id = $1 ORDER BY position ASC",
        channel.as_str()
    ).fetch_all(&mut *tx).await?;

    for (index, row) in rows.iter().enumerate() {
        let new_pos = index as i32 + 1;
    sqlx::query!(
            "UPDATE krapbott_v2.queue SET position = $1 WHERE channel_id = $2 AND user_id = $3",
            new_pos, channel.as_str(), row.user_id
        ).execute(&mut *tx).await?;
    }

    tx.commit().await?;

    Ok(if result.is_empty() {
        Replies::queue_empty(channel.channel())
    } else {
        Replies::next_group(&result.join(", "))
    })
}

pub async fn toggle_queue(pool: &PgPool, state: &AppState, caller: &ChannelId, open: bool) -> BotResult<String> {
    let owner = resolve_queue_owner(state, caller).await?;

    {
        let mut cfg = state.config.write().await;
        let owner_cfg = cfg.channels.get_mut(&owner).ok_or(BotError::ConfigMissing(owner.clone()))?;

        owner_cfg.open = open;
        save_channel_config(pool, &owner, &cfg).await?;
    }

    Ok(if open {
        Replies::queue_opened()
    } else {
        Replies::queue_closed()
    })
}

pub async fn run_next(
    pool: &PgPool,
    state: Arc<AppState>,
    owner: &ChannelId,
) -> BotResult<String> {
    let (teamsize, random_queue) = {
        let cfg = state.config.read().await;
        let c = cfg
            .get_channel_config(owner)
            .ok_or(BotError::ConfigMissing(owner.clone()))?;
        (c.teamsize as i64, c.random_queue)
    };

    let result = if random_queue {
        randomize_queue(owner, pool, teamsize).await?
    } else {
        next_handler(owner, pool, teamsize).await?
    };

    {
        let mut cfg = state.config.write().await;
        let c = cfg.get_channel_config_mut(owner.to_owned());
        c.runs += 1;
        save_channel_config(pool, owner, &cfg).await?;
    }

    
    &state.sse_bus.send(SseEvent::QueueUpdated { channel: owner.clone()});
    Ok(result)
}

pub async fn remove_from_queue(pool: &PgPool, owner: &ChannelId, user_id: &UserId, state: Arc<AppState>) -> BotResult<()> {
    let position = sqlx::query_scalar!(
        r#"
        SELECT position
        FROM krapbott_v2.queue
        WHERE user_id = $1 AND channel_id = $2
        "#,
        user_id.as_str(),
        owner.as_str()
    )
    .fetch_optional(pool)
    .await?;

    let Some(position) = position else {
        return Ok(()); // user not in queue
    };

    // Delete user
    let res = sqlx::query!(
        r#"
        DELETE FROM krapbott_v2.queue
        WHERE user_id = $1 AND channel_id = $2
        "#,
        user_id.as_str(),
        owner.as_str()
    )
    .execute(pool)
    .await?;

    tracing::info!("Deleted rows: {}", res.rows_affected());

    // Shift positions
    sqlx::query!(
        r#"
        UPDATE krapbott_v2.queue
        SET position = position - 1
        WHERE channel_id = $1 AND position > $2
        "#,
        owner.as_str(),
        position
    )
    .execute(pool)
    .await?;

    // Notify OBS
    let _ = &state.sse_bus.send(SseEvent::QueueUpdated { channel: owner.clone() });
    Ok(())
}

pub async fn reorder_queue(pool: &PgPool, owner: &ChannelId, ordered_users: Vec<UserId>) -> BotResult<()> {
    let mut tx = pool.begin().await?;

    for (idx, user_id) in ordered_users.iter().enumerate() {
        let pos = (idx + 1) as i32;

        sqlx::query!(
            r#"
            UPDATE krapbott_v2.queue
            SET position = $1
            WHERE user_id = $2 AND channel_id = $3
            "#,
            pos, user_id.as_str(), owner.as_str()
        ).execute(&mut *tx).await?;
    }

    tx.commit().await?;
    Ok(())
}

pub async fn set_queue_open(pool: &PgPool, state: Arc<AppState>, owner: &ChannelId, open: bool) -> BotResult<()> {
    {
        let mut cfg = state.config.write().await;

        let channel_cfg = cfg
            .channels
            .get_mut(owner)
            .ok_or(BotError::ConfigMissing(owner.clone()))?;

        channel_cfg.open = open;
        save_channel_config(pool, owner, &cfg).await?;

    }

    Ok(())
}

pub async fn set_queue_len(pool: &PgPool, state: Arc<AppState>, owner: &ChannelId, len: usize) -> BotResult<()> {
    {
        let mut cfg = state.config.write().await;

        let channel_cfg = cfg
            .channels
            .get_mut(owner)
            .ok_or(BotError::ConfigMissing(owner.clone()))?;

        channel_cfg.size = len;
        save_channel_config(pool, owner, &cfg).await?;

    }

    Ok(())
}

pub async fn set_queue_size(pool: &PgPool, state: Arc<AppState>, owner: &ChannelId, size: usize) -> BotResult<()> {
    {
        let mut cfg = state.config.write().await;

        let channel_cfg = cfg
            .channels
            .get_mut(owner)
            .ok_or(BotError::ConfigMissing(owner.clone()))?;

        channel_cfg.teamsize = size;
        save_channel_config(pool, owner, &cfg).await?;

    }

    Ok(())
}

pub async fn reset_queue_runs(pool: &PgPool, state: Arc<AppState>, owner: &ChannelId) -> BotResult<()> {
    {
        let mut cfg = state.config.write().await;
        if let Some(c) = cfg.channels.get_mut(owner) {
            c.runs = 0;
        }
        save_channel_config(pool, owner, &cfg).await?;
    }

    // Notify SSE listeners
    let _ = state.sse_bus.send(SseEvent::QueueUpdated { channel: owner.to_owned() });

    Ok(())
}