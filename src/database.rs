use std::{str::FromStr, sync::Arc};
use sqlx::{sqlite::{SqliteConnectOptions, SqlitePoolOptions}, Pool, Sqlite, SqlitePool};
use crate::{api::{get_membershipid, MemberShip}, models::{BotError, TwitchUser}};
pub const QUEUE_TABLE: &str = "CREATE TABLE IF NOT EXISTS queue (
    position INTEGER NOT NULL,
    twitch_name TEXT NOT NULL,
    bungie_name TEXT NOT NULL,
    channel_id VARCHAR,
    group_priority INTEGER DEFAULT 2,
    locked_first BOOLEAN DEFAULT FALSE,
    priority_runs_left INTEGER DEFAULT 0,
    PRIMARY KEY(position, channel_id)
)";

pub const COMMANDS_TEMPLATE: &str = "CREATE TABLE IF NOT EXISTS commands_template (
    id INTEGER,
    package TEXT NOT NULL,
    command TEXT NOT NULL,
    template TEXT,
    channel_id VARCHAR,
    UNIQUE (channel_id, command)
)";

pub const TEST_TABLE: &str = "UPDATE announcements SET state = 'active' WHERE state = 'Active'";

pub const USER_TABLE: &str = "CREATE TABLE IF NOT EXISTS user (
    id INTEGER PRIMARY KEY,
    twitch_name TEXT NOT NULL,
    bungie_name TEXT NOT NULL,
    membership_id TEXT,
    membership_type INTEGER,
    UNIQUE (twitch_name)
)";

pub const COMMAND_TABLE: &str = "CREATE TABLE IF NOT EXISTS commands (
    id INTEGER PRIMARY KEY,
    command TEXT NOT NULL,
    reply TEXT NOT NULL,
    channel TEXT,
    UNIQUE(command, channel)
) ";

pub const ANNOUNCEMENT_TABLE: &str = "CREATE TABLE IF NOT EXISTS announcements (
    name TEXT NOT NULL,
    announcement TEXT NOT NULL,
    channel TEXT,
    state TEXT,
    UNIQUE(name, channel)
)";

pub const BAN_TABLE: &str = "CREATE TABLE IF NOT EXISTS banlist (
    id INTEGER PRIMARY KEY,
    twitch_name TEXT NOT NULL,
    reason TEXT
)";

pub const CURRENCY_TABLE: &str = "
    CREATE TABLE IF NOT EXISTS currency (
        twitch_name TEXT NOT NULL,
        channel TEXT NOT NULL,
        points INTEGER NOT NULL DEFAULT 0,
        PRIMARY KEY (twitch_name, channel)
    );
";

pub async fn initialize_currency_database() -> Result<Arc<SqlitePool>, sqlx::Error> {
    let database_url = "sqlite:///D:/program/krapbott/public/commands.db";
    
    // Create the connection pool
    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .connect(database_url)
        .await?;

    // Initialize tables
    let queries = [
        CURRENCY_TABLE
    ];

    for query in queries {
        sqlx::query(query).execute(&pool).await?;
    }

    Ok(Arc::new(pool))
}

pub async fn is_bungiename(x_api_key: String, bungie_name: &str, twitch_name: &str, pool: &SqlitePool) -> bool {
    if let Ok(user_info) = get_membershipid(bungie_name, x_api_key).await {
        if user_info.type_m == -1 {
            return false;
        } else {
            let result = sqlx::query!(
                "INSERT INTO user (twitch_name, bungie_name, membership_id, membership_type) 
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(twitch_name) DO UPDATE SET bungie_name = excluded.bungie_name",
                 twitch_name, bungie_name, user_info.id, user_info.type_m
            ).execute(pool).await;
            match result {
                Ok(_) => return true,
                Err(_) => return false,
            }
        }
    } else {
        return false;
    }
}

pub async fn save_to_user_database(pool: &SqlitePool, user: TwitchUser, x_api_key: String) -> Result<String, BotError> {
    match get_membershipid(&user.bungie_name, x_api_key).await {
        Ok(user_info) if user_info.type_m == -1 => {
            Ok(format!("{} doesn't exist, check if your Bungie name is correct", user.bungie_name))
        }
        Ok(user_info) => {
            sqlx::query!(
                "INSERT INTO user (twitch_name, bungie_name, membership_id, membership_type)
                VALUES (?, ?, ?, ?)
                ON CONFLICT(twitch_name) 
                DO UPDATE SET bungie_name = excluded.bungie_name",
                user.twitch_name, user.bungie_name, user_info.id, user_info.type_m
            ).execute(pool).await?;
            Ok(format!("{} has been registered to the database as {}", user.twitch_name, user.bungie_name))
        }
        Err(_) => Ok("Problem with API response, restart KrapBott".to_string()),
    }

}

pub async fn load_membership(pool: &SqlitePool, twitch_name: String) -> Option<MemberShip> {
    match sqlx::query!(
        "SELECT membership_id, membership_type FROM user WHERE twitch_name = ?",
        twitch_name
    ).fetch_optional(pool).await {
        Ok(Some(row)) => Some(MemberShip {
            id: row.membership_id?,
            type_m: row.membership_type?.try_into().unwrap(),
        }),
        _ => None,
    }
}

pub async fn load_from_queue(pool: &SqlitePool, channel: &str) -> Vec<(usize, TwitchUser)> {
    let rows = sqlx::query!(
        "SELECT position, twitch_name, bungie_name FROM queue WHERE channel_id = ?",
        channel
    ).fetch_all(pool).await.unwrap_or_else(|_| vec![]); 
    rows.into_iter()
        .map(|row| {
            let twitch_user = TwitchUser {
                twitch_name: row.twitch_name,
                bungie_name: row.bungie_name,
            };
            (row.position as usize, twitch_user)
        }).collect()
}

pub async fn save_command(pool: &SqlitePool, mut command: String, reply: String, channel: Option<String>) {
    if !command.starts_with('!') {
        command.insert(0, '!');
    }

    let _ = sqlx::query!(
        "INSERT INTO commands (command, reply, channel) 
        VALUES (?, ?, ?) 
        ON CONFLICT(command) DO UPDATE SET reply = excluded.reply",
        command, reply, channel
    ).execute(pool).await;
}

pub async fn get_command_response(pool: &SqlitePool, command: String, channel: Option<String>) -> Result<Option<String>, BotError> {
    if let Some(channel) = channel {
        if let Ok(Some(row)) = sqlx::query!(
            "SELECT reply FROM commands WHERE command = ? AND channel = ?",
            command, channel
        ).fetch_optional(pool).await {
            return Ok(Some(row.reply));
        }
    }
    // Fallback: Check global command (where channel IS NULL)
    let global_result = sqlx::query!(
        "SELECT reply FROM commands WHERE command = ? AND channel IS NULL",
        command
    ).fetch_optional(pool).await?;

    Ok(global_result.map(|row| row.reply))
}

pub async fn remove_command(pool: &SqlitePool, command: &str) -> bool {
    let mut command = command.to_string();
    if !command.starts_with('!') {
        command.insert(0, '!');
    }
    sqlx::query!("DELETE FROM commands WHERE command = ?", command).execute(pool).await.is_ok() // Returns `true` if successful, `false` otherwise
}

pub async fn user_exists_in_database(pool: &SqlitePool, twitch_name: String) -> Option<String> {
    sqlx::query!(
        "SELECT bungie_name FROM user WHERE twitch_name = ?",
        twitch_name
    ).fetch_optional(pool).await.ok().flatten().map(|row| row.bungie_name) // Extract `bungie_name` if found
}