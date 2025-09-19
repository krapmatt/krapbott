use sqlx::{postgres::PgConnectOptions, PgPool};

use crate::{
    api::{get_membershipid, MemberShip}, models::{AliasConfig, BotError, TwitchUser}
};
use std::{sync::Arc};
pub const QUEUE_TABLE: &str = "
    CREATE TABLE IF NOT EXISTS queue (
        position INTEGER NOT NULL,
        twitch_name TEXT NOT NULL,
        bungie_name TEXT NOT NULL,
        channel_id TEXT,
        group_priority INTEGER DEFAULT 2,
        locked_first BOOLEAN DEFAULT FALSE,
        priority_runs_left INTEGER DEFAULT 0,
        PRIMARY KEY(position, channel_id)
    );
";

pub const COMMANDS_TEMPLATE: &str = "
CREATE TABLE IF NOT EXISTS commands_template (
    id SERIAL,
    package TEXT NOT NULL,
    command TEXT NOT NULL,
    template TEXT,
    channel_id TEXT,
    UNIQUE (channel_id, command)
);
";

pub const TEST_TABLE: &str = "DROP TABLE IF EXISTS giveaway;";

pub const USER_TABLE: &str = "
CREATE TABLE IF NOT EXISTS twitchuser (
    id SERIAL PRIMARY KEY,
    twitch_name TEXT NOT NULL,
    bungie_name TEXT NOT NULL,
    membership_id TEXT,
    membership_type INTEGER,
    UNIQUE (twitch_name)
);
";

pub const USERS_TABLE: &str = "
CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY,       -- Unique user ID (Twitch ID)
    twitch_name TEXT UNIQUE,   -- Twitch username
    access_token TEXT,         -- OAuth access token
    refresh_token TEXT,        -- OAuth refresh token
    expires_at TIMESTAMP,
    profile_pp TEXT 
);
";

pub const COMMAND_TABLE: &str = "
CREATE TABLE IF NOT EXISTS commands (
    id SERIAL PRIMARY KEY,
    command TEXT NOT NULL,
    reply TEXT NOT NULL,
    channel TEXT
);
";

pub const ANNOUNCEMENT_TABLE: &str = "
CREATE TABLE IF NOT EXISTS announcements (
    name TEXT NOT NULL,
    announcement TEXT NOT NULL,
    channel TEXT,
    state TEXT,
    UNIQUE(name, channel)
);
";

pub const BAN_TABLE: &str = "
CREATE TABLE IF NOT EXISTS banlist (
    membership_id TEXT PRIMARY KEY,
    banned_until TEXT, -- Null = permanent ban
    reason TEXT
);
";

pub const CURRENCY_TABLE: &str = "
CREATE TABLE IF NOT EXISTS currency (
    twitch_name TEXT NOT NULL,
    channel TEXT NOT NULL,
    points INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (twitch_name, channel)
);
";

pub const GIVEAWAY_TABLE: &str = "
CREATE TABLE IF NOT EXISTS giveaway (
    id SERIAL PRIMARY KEY,
    channel_id TEXT NOT NULL,
    twitch_name TEXT NOT NULL,
    tickets INTEGER NOT NULL,
    UNIQUE(twitch_name, channel_id)
);
";

pub const SESSIONS_TABLE: &str = "
CREATE TABLE IF NOT EXISTS sessions (
    session_token TEXT PRIMARY KEY,
    twitch_id TEXT NOT NULL,
    expires_at BIGINT NOT NULL,
    FOREIGN KEY (twitch_id) REFERENCES users(id)
);
";

pub const COMMAND_ALIASES: &str = "
CREATE TABLE IF NOT EXISTS command_aliases (
    channel TEXT NOT NULL,
    alias TEXT NOT NULL,
    command TEXT NOT NULL,
    PRIMARY KEY (channel, alias)
);
";

pub const COMMAND_DISABLED: &str = "
CREATE TABLE IF NOT EXISTS command_disabled (
    channel TEXT NOT NULL,
    command TEXT NOT NULL,
    PRIMARY KEY (channel, command)
);
";

pub const COMMAND_ALIASES_REMOVALS: &str = "
CREATE TABLE IF NOT EXISTS command_alias_removals (
    channel TEXT NOT NULL,
    alias TEXT NOT NULL,
    PRIMARY KEY (channel, alias)
); 
";


/*pub async fn initialize_database() -> Result<Arc<PgPool>, sqlx::Error> {
    let conn = PgConnection::connect("postgres://localhost/mydb").await?;

    // Create the connection pool
    let pool = PgConnectOptions::new().port(5432).database("krapbott").password("postgres").application_name("krapbott-db").username("postgres");

    // Initialize tables
    let queries = [
        COMMAND_ALIASES, COMMAND_ALIASES_REMOVALS, COMMANDS_TEMPLATE, COMMAND_DISABLED, QUEUE_TABLE, SESSIONS_TABLE, BAN_TABLE, USER_TABLE, USERS_TABLE, COMMAND_TABLE, CURRENCY_TABLE, GIVEAWAY_TABLE, ANNOUNCEMENT_TABLE
    ];

    for query in queries {
        sqlx::query(query).execute(&pool).await?;
    }

    Ok(Arc::new(pool))
}*/

pub async fn is_bungiename(x_api_key: String, bungie_name: &str, twitch_name: &str, pool: &PgPool) -> bool {
    if let Ok(user_info) = get_membershipid(bungie_name, &x_api_key).await {
        if user_info.type_m == -1 {
            return false;
        } else {
            let result = sqlx::query(
            r#"
            INSERT INTO twitchuser (twitch_name, bungie_name, membership_id, membership_type)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (twitch_name)
            DO UPDATE SET 
                bungie_name = EXCLUDED.bungie_name,
                membership_id = EXCLUDED.membership_id,
                membership_type = EXCLUDED.membership_type
            "#
            ).bind(twitch_name).bind(bungie_name).bind(user_info.id.to_string()).bind(user_info.type_m.to_string()).execute(pool).await;
            match result {
                Ok(_) => return true,
                Err(_) => return false,
            }
        }
    } else {
        return false;
    }
}

pub async fn save_to_user_database(pool: &PgPool, user: TwitchUser, x_api_key: String) -> Result<String, BotError> {
    match get_membershipid(&user.bungie_name, &x_api_key).await {
        Ok(user_info) if user_info.type_m == -1 => Ok(format!(
            "{} doesn't exist, check if your Bungie name is correct",
            user.bungie_name
        )),
        Ok(user_info) => {
            sqlx::query(
                "INSERT INTO twitchuser (twitch_name, bungie_name, membership_id, membership_type)
                VALUES ($1, $2, $3, $4)
                ON CONFLICT(twitch_name) 
                DO UPDATE SET 
                    bungie_name = excluded.bungie_name,
                    membership_id = excluded.membership_id,
                    membership_type = excluded.membership_type"
            ).bind(user.twitch_name.clone()).bind(user.bungie_name.clone()).bind(user_info.id).bind(user_info.type_m)
            .execute(pool)
            .await?;
            Ok(format!(
                "{} has been registered to the database as {}",
                user.twitch_name, user.bungie_name
            ))
        }
        Err(_) => Ok("Problem with API response, restart KrapBott".to_string()),
    }
}

pub async fn load_membership(pool: &PgPool, twitch_name: String) -> Option<MemberShip> {
    match sqlx::query!(
        r#"SELECT membership_id, membership_type FROM twitchuser WHERE twitch_name = $1"#,
        twitch_name
    ).fetch_optional(pool)
    .await
    {
        Ok(Some(row)) => Some(MemberShip {
            id: row.membership_id?,
            type_m: row.membership_type?.parse::<i32>().unwrap(),
        }),
        _ => None,
    }
}

pub async fn load_from_queue(pool: &PgPool, channel: &str) -> Vec<(usize, TwitchUser)> {
    let rows = sqlx::query!(
        "SELECT position, twitch_name, bungie_name FROM queue WHERE channel_id = $1",
        channel
    )
    .fetch_all(pool)
    .await
    .unwrap_or_else(|_| vec![]);
    rows.into_iter()
        .map(|row| {
            let twitch_user = TwitchUser {
                twitch_name: row.twitch_name,
                bungie_name: row.bungie_name,
            };
            (row.position as usize, twitch_user)
        })
        .collect()
}

pub async fn save_command(
    pool: &PgPool,
    mut command: String,
    reply: String,
    channel: Option<String>,
) {
    if !command.starts_with('!') {
        command.insert(0, '!');
    }

    let _ = sqlx::query!(
        "INSERT INTO commands (command, reply, channel) 
        VALUES ($1, $2, $3) 
        ON CONFLICT(command) DO UPDATE SET reply = excluded.reply",
        command,
        reply,
        channel
    )
    .execute(pool)
    .await;
}

pub async fn get_command_response(pool: &PgPool, command: String, channel: Option<String>) -> Result<Option<String>, BotError> {
    if let Some(channel) = channel {
        if let Ok(Some(row)) = sqlx::query!(
            "SELECT reply FROM commands WHERE command = $1 AND channel = $2",
            command, channel
        ).fetch_optional(pool).await
        {
            return Ok(Some(row.reply));
        }
    }
    // Fallback: Check global command (where channel IS NULL)
    let global_result = sqlx::query!(
        "SELECT reply FROM commands WHERE command = $1 AND channel IS NULL",
        command
    ).fetch_optional(pool).await?;

    Ok(global_result.map(|row| row.reply))
}

pub async fn remove_command(pool: &PgPool, command: &str) -> bool {
    let mut command = command.to_string();
    if !command.starts_with('!') {
        command.insert(0, '!');
    }
    sqlx::query!("DELETE FROM commands WHERE command = $1", command)
        .execute(pool)
        .await
        .is_ok() // Returns `true` if successful, `false` otherwise
}

pub async fn user_exists_in_database(pool: &PgPool, twitch_name: String) -> Option<String> {
    sqlx::query!(
        r#"SELECT bungie_name FROM twitchuser WHERE twitch_name = $1"#,
        twitch_name
    ).fetch_optional(pool).await.ok().flatten().map(|row| row.bungie_name)?
}


pub async fn fetch_aliases_from_db(channel: &str, pool: &PgPool) -> Result<AliasConfig, BotError> {
    let alias_rows = sqlx::query!(
        "SELECT alias, command FROM command_aliases WHERE channel = $1",
        channel
    ).fetch_all(pool).await?;

    let disabled = sqlx::query!(
        "SELECT command FROM command_disabled WHERE channel = $1",
        channel
    ).fetch_all(pool).await?;

    let removed = sqlx::query!(
        "SELECT alias FROM command_alias_removals WHERE channel = $1",
        channel
    ).fetch_all(pool).await?;

    Ok(AliasConfig {
        aliases: alias_rows
            .into_iter()
            .map(|r| (r.alias.to_lowercase(), r.command.to_lowercase()))
            .collect(),
        disabled_commands: disabled.into_iter().map(|r| r.command.to_lowercase()).collect(),
        removed_aliases: removed.into_iter().map(|r| r.alias.to_lowercase()).collect(),
    })
}