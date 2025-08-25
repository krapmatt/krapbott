use dotenvy::{dotenv, var};
use sqlx::SqlitePool;
use std::{borrow::BorrowMut, sync::Arc};
use tmi::Client;
use crate::commands::{update_dispatcher_if_needed, words};
use crate::models::{BotResult, Package, SharedQueueGroup};
use crate::queue::is_valid_bungie_name;
use crate::{api::{get_membershipid, get_users_clears, MemberShip}, bot::BotState, database::{load_membership, remove_command, save_command, save_to_user_database}, models::{BotError, CommandAction, TwitchUser}};

impl BotState {
    //Get total clears of raid of a player
    pub async fn total_raid_clears(&self, msg: &tmi::Privmsg<'_>, client: &mut Client, pool: &SqlitePool) -> BotResult<()> {
        let mut membership = MemberShip { id: String::new(), type_m: -1 };
        let words: Vec<&str> = msg.text().split_ascii_whitespace().collect();
        
        let reply = if words.len() > 1 {
            let mut name = words[1..].to_vec().join(" ").to_string();
            
            if let Some(name) = is_valid_bungie_name(&name) {
                match get_membershipid(&name, &self.x_api_key).await {
                    Ok(ship) => membership = ship,
                    Err(err) => client.privmsg(msg.channel(), &format!("Error: {}", err)).send().await?,
                }
            } else {
                if name.starts_with("@") {
                    name.remove(0); 
                }
                if let Some(ship) = load_membership(&pool, name.clone()).await {
                    membership = ship;
                } else {
                    client.privmsg(msg.channel(), "Twitch user isn't registered in the database! Use their Bungie name!").send().await?;
                    return Ok(());
                }
            }
            let clears = get_users_clears(membership.id, membership.type_m, self.x_api_key.clone()).await? as i32;
            format!("{} has total {} raid clears", name, clears)
        } else {
            if let Some(membership) = load_membership(&pool, msg.sender().name().to_string()).await {
                let clears = get_users_clears(membership.id, membership.type_m, self.x_api_key.clone()).await? as i32;
                format!("You have total {} raid clears", clears)
            } else {
                format!("ItsBoshyTime {} is not registered to the database. Use !register <yourbungiename#0000>", msg.sender().name())
            }
        };
        client.privmsg(msg.channel(), &reply).send().await?;
        Ok(())
    }

    pub async fn add_remove_package(&mut self, msg: &tmi::Privmsg<'_>, client: Arc<tokio::sync::Mutex<tmi::Client>>, state: Package, pool: &SqlitePool) -> BotResult<()> {
        let config = self.config.get_channel_config_mut(msg.channel());
        let words: Vec<&str> = words(&msg);

        if words.len() <= 1 {
            send_message(&msg, client.lock().await.borrow_mut(), "❌ No mention of package!").await?;
            return Ok(());
        }

        let package_name = words[1..].join(" ").to_string();
        let reply = match state {
            Package::Add => {
                if config.packages.contains(&package_name) {
                    "You already have this package 📦".to_string()
                } else {
                    config.packages.push(package_name.clone());
                    update_dispatcher_if_needed(msg.channel(), &self.config, pool, Arc::clone(&self.dispatchers)).await?;
                    self.config.save_config();
                    format!("📦 Package {} has been added", package_name)
                }
            },
            Package::Remove => {
                if let Some(index) = config.packages.iter().position(|x| *x.to_lowercase() == package_name.to_lowercase()) {
                    config.packages.remove(index);
                    update_dispatcher_if_needed(msg.channel(), &self.config, pool, Arc::clone(&self.dispatchers)).await?;
                    self.config.save_config();
                    format!("📦 Package {} has been removed", package_name)
                } else {
                    format!("📦 Package {} does not exist or you don't have it activated", package_name)
                }
            }
        };
        
        reply_to_message(&msg, client.lock().await.borrow_mut(), &reply).await?;

        Ok(())
    }

    /// Get the SharedQueueGroup that this channel belongs to (if any)
    pub fn get_group_for_channel(&self, channel: &str) -> Option<&SharedQueueGroup> {
        if let Some(main) = self.channel_to_main.get(channel) {
            self.shared_groups.get(main)
        } else {
            None
        }
    }

    /// Get mutable reference
    pub fn get_group_for_channel_mut(&mut self, channel: &str) -> Option<&mut SharedQueueGroup> {
        if let Some(main) = self.channel_to_main.get(channel) {
            self.shared_groups.get_mut(main)
        } else {
            None
        }
    }
}

pub async fn register_user(pool: &SqlitePool, twitch_name: &str, bungie_name: &str) -> Result<String, BotError> {
    dotenv().ok();
    let x_api_key = var("XAPIKEY").expect("No bungie api key");
    let reply = if let Some(bungie_name) = is_valid_bungie_name(bungie_name) {
        let new_user = TwitchUser {
            twitch_name: twitch_name.to_string(),
            bungie_name: bungie_name.to_string()
        };
        save_to_user_database(&pool, new_user, x_api_key).await?
    } else {
        "❌ Invalid format of Bungie Name, This is correct format! -> bungiename#0000".to_string()
    };
    Ok(reply)
}
//if is/not in database
pub async fn bungiename(msg: &tmi::Privmsg<'_>, client: &mut Client, pool: &SqlitePool, twitch_name: String) -> BotResult<()> {
    let result = sqlx::query_scalar!(
        "SELECT bungie_name FROM user WHERE twitch_name = ?",
        twitch_name
    ).fetch_optional(pool).await?;

    let reply = match result {
        Some(bungie_name) => format!("Twitch Name: {twitch_name} | Bungie Name: {bungie_name}"),
        None => format!("{}, you are not registered ❌", twitch_name),
    };

    send_message(msg, client, &reply).await?;

    Ok(())
}

pub async fn send_message(msg: &tmi::Privmsg<'_>, client: &mut Client, reply: &str) -> BotResult<()> {
    client.privmsg(msg.channel(), &reply).send().await?;
    Ok(())
}
pub async fn reply_to_message(msg: &tmi::Privmsg<'_>, client: &mut Client, reply: &str) -> BotResult<()> {
    client.privmsg(msg.channel(), &reply).reply_to(msg.id()).send().await?;
    Ok(())
}

pub async fn unban_player_from_queue(msg: &tmi::Privmsg<'_>, client:Arc<tokio::sync::Mutex<Client>>, pool: &SqlitePool) -> BotResult<()> {
    let words: Vec<&str> = msg.text().split_ascii_whitespace().collect();
    let reply = if words.len() < 2 {
        "Maybe try to add a Twitch name. Somebody deserves the unban. krapmaStare".to_string()
    } else {
        let twitch_name = words[1].strip_prefix("@").unwrap_or(words[1]).to_string();
        if let Some(membership) = load_membership(pool, twitch_name.clone()).await {
            let affected_rows = sqlx::query!(
                "DELETE FROM banlist WHERE membership_id = ?",
                membership.id
            ).execute(pool).await?.rows_affected();
            if affected_rows > 0 {
                format!("User {} has been unbanned from queue! They are free to enter again. krapmaHeart", twitch_name)
            } else {
                format!("User {} was not found in the banlist.", twitch_name)
            }
        } else {
            "Error".to_string()
        }
    };
    send_message(msg, client.lock().await.borrow_mut(), &reply).await?;
    Ok(())
}

pub enum ModAction {
    Timeout,
    Ban, 
}

pub async fn mod_action_user_from_queue(msg: &tmi::Privmsg<'_>, client: Arc<tokio::sync::Mutex<tmi::Client>>, pool: &SqlitePool, mod_action: ModAction) -> BotResult<()> {
    let words: Vec<&str> = msg.text().split_ascii_whitespace().collect();
    if words.len() < 2 {
        send_message(msg, client.lock().await.borrow_mut(), "Usage: !mod_ban <twitch name> Optional(reason)").await?;
        return Ok(());
    }
    let twitch_name = words[1].strip_prefix("@").unwrap_or(words[1]).to_owned();
    let reply = if let Some(membership) = load_membership(&pool, twitch_name.clone()).await {
        let reply = match mod_action {
            ModAction::Timeout => {
                let reason = if words.len() > 3 { words[3..].join(" ") } else { String::new() };
                let seconds: u32 = words[2].parse::<u32>().unwrap_or(0);
                let result = sqlx::query!(
                    "INSERT INTO banlist (membership_id, banned_until, reason) VALUES (?1, datetime('now', ?2 || ' seconds'), ?3) ON CONFLICT(membership_id) DO UPDATE SET 
                    banned_until = datetime('now', ?2 || ' seconds'), 
                    reason = ?3;",
                    membership.id, seconds, reason
                ).execute(pool).await;
                match result {
                    Ok(_) => {
                        format!("User {} has been timed out from entering queue for {} hours.", twitch_name, seconds/3600)
                    }
                    Err(_) => {
                        "An error occurred while timing out the user.".to_string()
                    }
                } 
            },
            ModAction::Ban => {
                let reason = if words.len() > 2 { words[2..].join(" ") } else { String::new() };
                let result = sqlx::query!(
                    "INSERT INTO banlist (membership_id, banned_until, reason) VALUES (?1, NULL, ?2) ON CONFLICT(membership_id) DO UPDATE SET reason = ?2",
                    membership.id, reason
                ).execute(pool).await;
                match result {
                    Ok(_) => {
                        format!("User {} has been banned from entering queue.", twitch_name)
                    }
                    Err(_) => {
                        "An error occurred while banning the user.".to_string()
                    }
                } 
            }
        };
        reply
    } else {
        "User has never entered queue, !mod_register them! -> !help mod_register".to_string()
    };
    reply_to_message(&msg, client.lock().await.borrow_mut(), &reply).await?;
    Ok(())
}

pub async fn modify_command(msg: &tmi::Privmsg<'_>, client:Arc<tokio::sync::Mutex<Client>>, pool: &SqlitePool, action: CommandAction, channel: Option<String>) -> BotResult<()> {
    let words: Vec<&str> = msg.text().split_whitespace().collect();
    let mut reply;
    if words.len() < 2 {
        reply = "Use: !help (and your desired command)".to_string();
    }
    
    let command = words[1].to_string().to_ascii_lowercase();
    let reply_to_command = words[2..].join(" ").to_string();
    
    match action {
        CommandAction::Add => {
            reply = save(&pool, command, reply_to_command, channel, "Use: !help !addcommand").await?;
        }
        CommandAction::Remove => {
            if remove_command(&pool, &command).await {
                reply = format!("Command !{} removed.", command)
            } else {
                reply = format!("Command !{} doesn't exist.", command)
            }
        }
        CommandAction::AddGlobal => {
            reply = save(&pool, command, reply_to_command, None, "Use: !help !addglobalcommand").await?;
            
        } 
    };
    send_message(msg, client.lock().await.borrow_mut(), &reply).await?;
    Ok(())
}

async fn save(pool: &SqlitePool, command: String, reply: String, channel: Option<String>, error_mess: &str) -> Result<String, BotError> {
    if !reply.is_empty() {
        save_command(&pool, command.clone(), reply, channel).await;
        Ok(format!("Command !{} added.", command))
    } else {
        Ok(error_mess.to_string())
    }
}

