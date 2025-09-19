use std::sync::Arc;

use futures::future::BoxFuture;
use sqlx::PgPool;
use tokio::sync::RwLock;
use twitch_irc::message::PrivmsgMessage;

use crate::{api::{get_master_challenges, get_membershipid}, bot::{BotState, TwitchClient}, commands::{normalize_twitch_name, traits::CommandT, words}, database::load_membership, models::{AliasConfig, BotResult, PermissionLevel}, queue::is_valid_bungie_name};

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

    fn execute(&self, msg: PrivmsgMessage, client: TwitchClient, pool: PgPool, bot_state: Arc<RwLock<BotState>>, _alias_config: Arc<AliasConfig>) -> BoxFuture<'static, BotResult<()>> {
        Box::pin(async move {
            bot_state.read().await.total_raid_clears(&msg, &client, &pool).await?;
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

    fn execute(&self, msg: PrivmsgMessage, client: TwitchClient, pool: PgPool, bot_state: Arc<RwLock<BotState>>, _alias_config: Arc<AliasConfig>) -> BoxFuture<'static, BotResult<()>> {
        Box::pin(async move {
            let bot_state = bot_state.read().await;
            let words: Vec<&str> = words(&msg);

            if words.len() <= 1 {
                client.say_in_reply_to(&msg, "❌ Invalid usage".to_string()).await?;
                return Ok(());
            }

            let activity = words[1].to_string();

            let membership = if words.len() == 2 {
                load_membership(&pool, msg.sender.name.clone()).await
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
                    client.say_in_reply_to(&msg, "Use a correct bungiename!".to_string()).await?;
                    return Ok(());
                }
            };

            let chall_vec = get_master_challenges(membership.type_m, membership.id, &bot_state.x_api_key, activity).await?;

            client.say_in_reply_to(&msg, chall_vec.join(" || ")).await?;

            Ok(())
        })
    }
}