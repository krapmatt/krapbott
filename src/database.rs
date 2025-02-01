use std::str::FromStr;

use async_sqlite::{rusqlite::{params, Connection, Error}, Client, ClientBuilder};
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

pub const CURRENCY_TABLE: &str = "CREATE TABLE IF NOT EXISTS currency (
    twitch_name TEXT NOT NULL,
    points INTEGER NOT NULL DEFAULT 0
)";

pub async fn initialize_currency_database() -> Result<SqlitePool, sqlx::Error> {
    let database_url = "sqlite:/D:/program/krapbott/public/currency.db";
    
    // Create the connection pool
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await?;

    // Initialize tables
    let queries = [
        CURRENCY_TABLE
    ];

    for query in queries {
        sqlx::query(query).execute(&pool).await?;
    }

    Ok(pool)
}
pub async fn initialize_database_sqlx() -> Result<SqlitePool, sqlx::Error> {
    let pool = SqlitePool::connect_with(
        SqliteConnectOptions::from_str("sqlite:/D:/program/krapbott/public/commands.db")?
            .create_if_missing(true)
    ).await?;
    Ok(pool)
}
pub fn initialize_database() -> Connection {
    let conn = Connection::open("D:/program/krapbott/public/commands.db").unwrap();
    conn.execute(USER_TABLE, []).unwrap();
    conn.execute(QUEUE_TABLE, []).unwrap();
    conn.execute(COMMAND_TABLE, []).unwrap();
    conn.execute(ANNOUNCEMENT_TABLE, []).unwrap();
    conn.execute(BAN_TABLE, []).unwrap();
    conn.execute(COMMANDS_TEMPLATE, []).unwrap();

    return conn
}

pub async fn initialize_database_async() -> Client {
    let client = ClientBuilder::new()
        .path("/D:/program/krapbott/public/commands.db")
        .journal_mode(async_sqlite::JournalMode::Wal)
        .open()
        .await.unwrap();
    client.conn(|conn| {
        conn.execute(USER_TABLE, []).unwrap();
        conn.execute(QUEUE_TABLE, []).unwrap();
        conn.execute(COMMAND_TABLE, []).unwrap();
        conn.execute(ANNOUNCEMENT_TABLE, []).unwrap();
        conn.execute(BAN_TABLE, []).unwrap();
        conn.execute(COMMANDS_TEMPLATE, []).unwrap();
        //conn.execute(TEST_TABLE, []).unwrap();


        Ok(())
    }).await.expect("Failed to create database");
    client
}

pub async fn is_bungiename(x_api_key: String, bungie_name: String, twitch_name: String, conn: &Client) -> bool {
    if let Ok(user_info) = get_membershipid(bungie_name.clone(), x_api_key).await {
        if user_info.type_m == -1 {
            false
        } else {
            conn.conn(move |conn| Ok(conn.execute(
            "INSERT INTO user (twitch_name, bungie_name, membership_id, membership_type) VALUES (?1, ?2, ?3, ?4)
                ON CONFLICT(twitch_name) DO UPDATE SET bungie_name = excluded.bungie_name",
                params![twitch_name, bungie_name, user_info.id, user_info.type_m],        
            )?)).await;
            true
        }
    } else {
        false
    }
}

pub async fn save_to_user_database(conn: &Client, user: TwitchUser, x_api_key: String) -> Result<String, BotError> {
    if let Ok(user_info) = get_membershipid(user.bungie_name.clone(), x_api_key).await {
        if user_info.type_m == -1 {
            Ok(format!("{} doesn't exist, check if your bungiename is correct", user.bungie_name))
        } else {
            let user_clone = user.clone();    
            conn.conn(move |conn| Ok(conn.execute(
            "INSERT INTO user (twitch_name, bungie_name, membership_id, membership_type) VALUES (?1, ?2, ?3, ?4)
                ON CONFLICT(twitch_name) DO UPDATE SET bungie_name = excluded.bungie_name",
                params![user_clone.twitch_name, user_clone.bungie_name, user_info.id, user_info.type_m],        
            )?)).await?;
            Ok(format!("{} has been registered to database as {}", user.twitch_name, user.bungie_name))
        }
    } else {
        Ok(format!("Problem with API response, restart KrapBott"))
    }

}

pub async fn load_membership(conn: &Client, twitch_name: String) -> Option<MemberShip> {
    let a = conn.conn(move |conn | {
        let mut stmt = conn.prepare("SELECT membership_id, membership_type FROM user WHERE twitch_name = ?1").unwrap();
        match stmt.query_row([&twitch_name], |row| {
            Ok(MemberShip {
                id: row.get(0)?,
                type_m: row.get(1)?,
            })
        }) {
            Ok(membership) => Ok(Some(membership)),
            Err(Error::QueryReturnedNoRows) => Ok(None),
            Err(_) => Ok(None), 
        }
    }).await.unwrap();
    return a;

    
}

pub fn load_from_queue(conn: &Connection, channel: &str) -> Vec<(usize, TwitchUser)> {
    let mut stmt = conn.prepare("SELECT position, twitch_name, bungie_name FROM queue WHERE channel_id = ?1").unwrap();
    let mut queue_vec = vec![];
    let queue_iter = stmt.query_map([channel], |row| {
       Ok((row.get::<_, usize>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
    }).expect("There should be only valid names");
    
    for entry in queue_iter {
        if let Ok(entry) = entry {
            let twitch_user = TwitchUser {
                twitch_name: entry.1,
                bungie_name: entry.2
            };
            queue_vec.push((entry.0, twitch_user));
        }
    }

    queue_vec
}

pub async fn save_command(conn: &Client, mut command: String, reply: String, channel: Option<String>) {
    if !command.starts_with("!") {
        command.insert(0, '!');
    }
    conn.conn(move |conn| {conn.execute(
        "INSERT INTO commands (command, reply, channel) 
        VALUES (?1, ?2, ?3) 
        ON CONFLICT(command)
        DO UPDATE SET reply=excluded.reply", params![command, reply, channel]).unwrap();
        Ok(())
    }).await.unwrap();
    
}

pub async fn get_command_response(conn: &Client, command: String, channel: Option<String>) -> Result<Option<String>, BotError> {
    let command_clone = command.clone();
    if let Some(channel) = channel {
        let result = conn.conn(move |conn| {
            let mut stmt = conn.prepare("SELECT reply FROM commands WHERE command = ?1 AND channel = ?2")?;
            match stmt.query_row(params![command.clone(), &channel], |row| row.get::<_, String>(0)) {
                Ok(reply) => {
                    Ok(Some(reply))
                }
                Err(Error::QueryReturnedNoRows) => {
                    Ok(None)
                }
                Err(e) => {
                    println!("Database error: {:?}", e);
                    Err(e.into())
                }
            }
        }).await?;

        if result.is_some() {
            return Ok(result);
        }
    } else {
        println!("No specific channel provided, checking global command");
    }

    
    let global_command = conn.conn(move |conn| {
            let mut stmt = conn.prepare("SELECT reply FROM commands WHERE command = ?1 AND channel IS NULL")?;
            match stmt.query_row(params![&command_clone], |row| row.get::<_, String>(0)) {
                Ok(reply) => {
                    Ok(Some(reply))
                }
                Err(Error::QueryReturnedNoRows) => {
                    Ok(None)
                }
                Err(e) => {
                    println!("Database error: {:?}", e);
                    Err(e.into())
                }
            }
        }).await?;
    Ok(global_command)
}

pub async fn remove_command(conn: &Client, command: &str) -> bool {
    let mut command = command.to_string();
    command.insert(0, '!');
    if conn.conn(move |conn| { conn.execute("DELETE FROM commands WHERE command = ?1", params![command])}).await.is_ok() {
        true
    } else {
        false
    }
   
}

//pick random wont work anymore
pub async fn pick_random(conn: Client, teamsize: usize) -> Result<Vec<i64>, BotError> {
    Ok(conn.conn_mut( move |conn| {
        
        let mut stmt = conn.prepare("SELECT position FROM queue ORDER BY RANDOM() LIMIT ?1")?;
        let ids: Vec<i64> = stmt.query_map(params![teamsize], |row| row.get(0))?
            .map(|id| id.unwrap()).collect();
        
        Ok(ids)
    }).await?)
}


pub async fn user_exists_in_database(conn: &Client, twitch_name: String) -> Option<String> {
    if let Ok(a) = conn.conn(move |conn| {
        conn.query_row("SELECT bungie_name FROM user WHERE twitch_name = ?1", params![twitch_name], |row| {
            Ok(row.get::<_, String>(0)?)
        })
    }).await {
        Some(a)
    } else {
        None
    }
}