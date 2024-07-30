use rusqlite::{params, Connection};
use tokio::sync::Mutex;

use crate::{models::BotError, models::TwitchUser};
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
) ";



pub fn initialize_database() -> Connection {
    let conn = Connection::open("commands.db").unwrap();
    conn.execute(USER_TABLE, []).unwrap();
    conn.execute(QUEUE_TABLE, []).unwrap();
    conn.execute(COMMAND_TABLE, []).unwrap();
    return conn
}

pub async fn save_to_user_database(conn: &Mutex<Connection>, user: &TwitchUser) -> Result<usize, BotError> {
    Ok(conn.lock().await.execute(
        "INSERT INTO user (twitch_name, bungie_name) VALUES (?1, ?2)
         ON CONFLICT(twitch_name) DO UPDATE SET bungie_name = excluded.bungie_name",
        params![user.twitch_name, user.bungie_name],        
    )?)
    
}

pub fn load_from_queue(conn: &Connection) -> Vec<TwitchUser> {
    let mut stmt = conn.prepare("SELECT twitch_name, bungie_name FROM queue").unwrap();
    let queue_iter = stmt.query_map([], |row| {
        Ok(TwitchUser {
            twitch_name: row.get(0)?,
            bungie_name: row.get(1)?,
        })
    }).expect("There should be only valid names");
    
    let mut queue = Vec::new();
    for entry in queue_iter {
        queue.push(entry.expect("How it can be a error"));
    }
    queue
}
//TODO have the commands for multiple channels TOOD!!!!!!!!!
//bungiename doesnt work!!!!!!
pub async fn save_command(conn: &Mutex<Connection>, command: &str, reply: &str, channel: &str) {
    let mut command = command.to_string();
    let conn = conn.lock().await;
    command.insert(0, '!');
    conn.execute("INSERT INTO commands (command, reply, channel) VALUES (?1, ?2, ?3)
        ON CONFLICT(command) DO UPDATE SET reply=excluded.reply, channel=excluded.channel", params![command, reply, channel]).unwrap();
}

pub async fn get_command_response(conn: &Mutex<Connection>, command: &str, channel: &str) -> Result<Option<String>, BotError> {
    let conn = conn.lock().await;
    let mut stmt = conn.prepare("SELECT reply FROM commands WHERE command = ?1 AND channel = ?2")?;
    match stmt.query_row(params![command, channel], |row| row.get::<_, String>(0)) {
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

pub async fn remove_command(conn: &Mutex<Connection>, command: &str) -> bool {
    let mut command = command.to_string();
    let conn = conn.lock().await;
    command.insert(0, '!');
    if conn.execute("DELETE FROM commands WHERE command = ?1", params![command]).expect("Remove command went wrong") > 0 {
        true
    } else {
        false
    }
   
}

pub fn pick_random(conn: &mut Connection, teamsize: usize) -> Result<(), BotError> {
    let tx = conn.transaction().unwrap();
    let mut stmt = tx.prepare("SELECT queue.id FROM queue ORDER BY RANDOM() LIMIT ?1")?;
    let ids: Vec<i64> = stmt.query_map(params![teamsize], |row| row.get(0))?
        .map(|id| id.unwrap())
        .collect();
    if ids.is_empty() {
        println!("No rows selected.");
        return Ok(());
    }

    //Nereálné id vybraným
    for (i, id) in ids.iter().enumerate() {
        tx.execute(
            "UPDATE queue SET id = ?1 WHERE id = ?2",
            params![-(i as i64 + 1), id],
        )?;
    }

    //Posunou existující id o počet aby bylo místo pro náhodně vybrané
    tx.execute(
        "UPDATE queue SET id = id + ?1 WHERE id >= 1",
        params![ids.len() as i64],
    )?;

    //vrátit nazpět správné id
    for (new_id, _) in (1..=ids.len()).enumerate() {
        tx.execute(
            "UPDATE queue SET id = ?1 WHERE id = ?2",
            params![new_id as i64 + 1, -(new_id as i64 + 1)],
        )?;
    }
    drop(stmt);
    tx.commit()?;
    Ok(())
}