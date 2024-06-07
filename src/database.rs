use rusqlite::{params, Connection};
use tokio::sync::Mutex;

use crate::TwitchUser;
pub const QUEUE_TABLE: &str = "CREATE TABLE IF NOT EXISTS queue (
    id INTEGER PRIMARY KEY,
    twitch_name TEXT NOT NULL,
    bungie_name TEXT NOT NULL
)";

pub const USER_TABLE: &str = "CREATE TABLE IF NOT EXISTS user (
    id INTEGER PRIMARY KEY,
    twitch_name TEXT NOT NULL,
    bungie_name TEXT NOT NULL,
    UNIQUE (twitch_name)
)";

pub fn initialize_database(db_name: &str, sql_query: &str) -> anyhow::Result<Connection> {
    let conn = Connection::open(db_name)?;
    conn.execute(sql_query, [])?;
    Ok(conn)
}

pub async fn save_to_user_database(conn: &Mutex<Connection>, user: &TwitchUser) -> Result<usize, rusqlite::Error> {
    conn.lock().await.execute(
        "INSERT INTO user (twitch_name, bungie_name) VALUES (?1, ?2)",
        params![user.twitch_name, user.bungie_name],
    )
    
}

pub fn load_from_queue(conn: &Connection) -> anyhow::Result<Vec<TwitchUser>> {
    let mut stmt = conn.prepare("SELECT twitch_name, bungie_name FROM queue")?;
    let queue_iter = stmt.query_map([], |row| {
        Ok(TwitchUser {
            twitch_name: row.get(0)?,
            bungie_name: row.get(1)?,
        })
    })?;
    
    let mut queue = Vec::new();
    for entry in queue_iter {
        queue.push(entry?);
    }
    Ok(queue)
}