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

pub const COMMAND_TABLE: &str = "CREATE TABLE IF NOT EXISTS commands (
    id INTEGER PRIMARY KEY,
    command TEXT NOT NULL,
    reply TEXT NOT NULL,
    UNIQUE (command)
)";



pub fn initialize_database() -> anyhow::Result<Connection> {
    let conn = Connection::open("commands.db")?;
    conn.execute(USER_TABLE, [])?;
    conn.execute(QUEUE_TABLE, [])?;
    conn.execute(COMMAND_TABLE, [])?;
    Ok(conn)
}

pub async fn save_to_user_database(conn: &Mutex<Connection>, user: &TwitchUser) -> Result<usize, rusqlite::Error> {
    conn.lock().await.execute(
        "INSERT INTO user (twitch_name, bungie_name) VALUES (?1, ?2)
         ON CONFLICT(twitch_name) DO UPDATE SET bungie_name = excluded.bungie_name",
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

pub fn save_command(conn: &Connection, command: &str, reply: &str) -> anyhow::Result<()> {
    let mut command = command.to_string();
    command.insert(0, '!');
    conn.execute("INSERT INTO commands (command, reply) VALUES (?1, ?2)", params![command, reply])?;
    Ok(())
}

pub fn get_command_response(conn: &Connection, command: &str) -> anyhow::Result<Option<String>> {
    
    let mut stmt = conn.prepare("SELECT reply FROM commands WHERE command = ?1")?;
    match stmt.query_row(params![command], |row| row.get::<_, String>(0)) {
        Ok(reply) => {
            println!("Command found: {:?}", reply);
            return Ok(Some(reply))
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            println!("No command found");
            return Ok(None)
        }
        Err(e) => {
            println!("Database error: {:?}", e);
            return Err(e.into())
        }
    }

    
}

pub fn remove_command(conn: &Connection, command: &str) -> bool {
    let mut command = command.to_string();
    command.insert(0, '!');
    if conn.execute("DELETE FROM commands WHERE command = ?1", params![command]).expect("Remove command went wrong") > 1 {
        true
    } else {
        false
    }
   
}