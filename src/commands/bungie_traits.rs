use std::{borrow::BorrowMut, sync::Arc};

use futures::future::BoxFuture;
use sqlx::SqlitePool;
use tmi::{Client, Privmsg};
use tokio::sync::{Mutex, RwLock};

use crate::{api::{get_master_challenges, get_membershipid}, bot::BotState, bot_commands::reply_to_message, commands::{normalize_twitch_name, traits::CommandT, words}, database::load_membership, models::{AliasConfig, BotError, PermissionLevel}, queue::is_valid_bungie_name};

pub struct TotalCommand;

impl CommandT for TotalCommand {
    fn name(&self) -> &str {
        "Total Raids"
    }
    fn description(&self) -> &str {
        "Shows all the raid clears of bungie name"
    }
    fn usage(&self) -> &str {
        "!total Optional<Bungiename>"
    }
    fn permission(&self) -> PermissionLevel {
        PermissionLevel::User
    }

    fn execute(&self, msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, pool: SqlitePool, bot_state: Arc<RwLock<BotState>>, alias_config: Arc<AliasConfig>) -> BoxFuture<'static, Result<(), BotError>> {
        Box::pin(async move {
            bot_state.read().await.total_raid_clears(&msg, client.lock().await.borrow_mut(), &pool).await?;
            Ok(())
        })
    }
}

pub struct MasterChalCommand;

impl CommandT for MasterChalCommand {
    fn name(&self) -> &str {
        "Master Challenges"
    }
    fn description(&self) -> &str {
        "Get the number of challenges done in a master raid"
    }
    fn usage(&self) -> &str {
        "!cr <activity> <name>"
    }
    fn permission(&self) -> PermissionLevel {
        PermissionLevel::User
    }

    fn execute(&self, msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, pool: SqlitePool, bot_state: Arc<RwLock<BotState>>, alias_config: Arc<AliasConfig>) -> BoxFuture<'static, Result<(), BotError>> {
        Box::pin(async move {
            println!("Here");
            let bot_state = bot_state.read().await;
            let words: Vec<&str> = words(&msg);

            if words.len() <= 1 {
                reply_to_message(&msg, client.lock().await.borrow_mut(), "❌ Invalid usage").await?;
                return Ok(());
            }

            let activity = words[1].to_string();

            let membership = if words.len() == 2 {
                load_membership(&pool, msg.sender().name().to_string()).await
            } else {
                let name = words[2..].join(" ");
                if let Some(bungie_name) = is_valid_bungie_name(&name) {
                    Some(get_membershipid(&bungie_name, &bot_state.x_api_key).await?)
                } else {
                    load_membership(&pool, normalize_twitch_name(&name).to_string()).await
                }
            };

            let membership = match membership {
                Some(m) if m.type_m != -1 => m,
                _ => {
                    reply_to_message(&msg, client.lock().await.borrow_mut(), "Use a correct bungiename!").await?;
                    return Ok(());
                }
            };

            let chall_vec = get_master_challenges(membership.type_m, membership.id, &bot_state.x_api_key, activity).await?;

            reply_to_message(&msg, client.lock().await.borrow_mut(), &chall_vec.join(" || ")).await?;

            Ok(())
        })
    }
}