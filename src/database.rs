use async_sqlite::{rusqlite::{params, Connection, Error}, Client, ClientBuilder};

use tokio::sync::Mutex;

use crate::{api::{get_membershipid, MemberShip}, models::{BotError, TwitchUser}};
pub const QUEUE_TABLE: &str = "CREATE TABLE IF NOT EXISTS queue (
    id INTEGER PRIMARY KEY,
    twitch_name TEXT NOT NULL,
    bungie_name TEXT NOT NULL
)";

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

pub const ANNOUNCMENT_TABLE: &str = "CREATE TABLE IF NOT EXISTS announcments (
    id INTEGER PRIMARY KEY,
    announcment TEXT NOT NULL,
    channel TEXT
)";

pub fn initialize_database() -> Connection {
    let conn = Connection::open("D:/program/krapbott/commands.db").unwrap();
    conn.execute(USER_TABLE, []).unwrap();
    conn.execute(QUEUE_TABLE, []).unwrap();
    conn.execute(COMMAND_TABLE, []).unwrap();
    conn.execute(ANNOUNCMENT_TABLE, []).unwrap();
    return conn
}

pub async fn initialize_database_async() -> Client {
    let client = ClientBuilder::new()
                .path("/D:/program/krapbott/commands.db")
                .journal_mode(async_sqlite::JournalMode::Wal)
                .open()
                .await.unwrap();
    client.conn(|conn| {
        conn.execute(USER_TABLE, []).unwrap();
        conn.execute(QUEUE_TABLE, []).unwrap();
        conn.execute(COMMAND_TABLE, []).unwrap();
        conn.execute(ANNOUNCMENT_TABLE, []).unwrap();
        Ok(())
    }).await;
    client
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
            )?)).await;
            Ok(format!("{} has been registered to database as {}", user.twitch_name, user.bungie_name))
        }
    } else {
        Ok(format!("Problem with API response, restart KrapBott"))
    }

}
//  Queue is open use !join <bungiename#0000> >> DO NOT KILL ANYTHING EXCEPT WIZARD. Do not pull to orbit, always change characters!
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

pub async fn save_command(conn: &Client, command: String, reply: String, channel: Option<String>) {
    let mut command = command.to_string();
    command.insert(0, '!');
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
                    println!("Command found for channel {}: {:?}", channel, reply);
                    Ok(Some(reply))
                }
                Err(Error::QueryReturnedNoRows) => {
                    println!("No command found for channel {}, checking global command", channel);
                    Ok(None) // No channel-specific command found, proceed to check global
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
                    println!("Global command found: {:?}", reply);
                    Ok(Some(reply))
                }
                Err(Error::QueryReturnedNoRows) => {
                    println!("No global command found");
                    Ok(None)
                }
                Err(e) => {
                    println!("Database error: {:?}", e);
                    Err(e.into())
                }
            }
        })
        .await?;

    // Return the result, whether it's found or None
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
//Need this to async!!! TODO
pub async fn pick_random(conn: Client, teamsize: usize) -> Result<(), BotError> {
    conn.conn_mut( move |conn| {
        let tx = conn.transaction().unwrap();
        let mut stmt = tx.prepare("SELECT queue.id FROM queue ORDER BY RANDOM() LIMIT ?1")?;
        let ids: Vec<i64> = stmt.query_map(params![teamsize], |row| row.get(0))?
            .map(|id| id.unwrap()).collect();
        if ids.is_empty() {
            println!("No rows selected.");
            return Ok(());
        }

        //Nereálné id vybraným
        for (i, id) in ids.iter().enumerate() {
            tx.execute("UPDATE queue SET id = ?1 WHERE id = ?2", params![-(i as i64 + 1), id])?;
        }

        //Posunou existující id o počet aby bylo místo pro náhodně vybrané
        tx.execute("UPDATE queue SET id = id + ?1 WHERE id >= 1", params![ids.len() as i64])?;

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
    }).await;
    
    Ok(())
}