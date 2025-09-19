use std::{sync::Arc};
use futures::future::BoxFuture;
use sqlx::PgPool;
use tokio::{sync::RwLock};
use twitch_irc::message::PrivmsgMessage;

use crate::{bot::{BotState, TwitchClient}, commands::{generate_variables, parse_template, traits::CommandT, words}, models::{AliasConfig, BotResult, PermissionLevel, TemplateManager}, twitch_api::{self, get_twitch_user_id}};

pub struct FnCommand<F> {
    func: F,
    desc: String,
    usage: String,
    name: String,
    permission: PermissionLevel,
}

impl<F> FnCommand<F> 
where
    F: Fn(PrivmsgMessage, TwitchClient, PgPool, Arc<RwLock<BotState>>) -> BoxFuture<'static, BotResult<()>> + Send + Sync + 'static,
{
    pub fn new(func: F, desc: impl Into<String>, usage: impl Into<String>, name: impl Into<String>, permission: PermissionLevel) -> Self {
        Self {
            func,
            desc: desc.into(),
            usage: usage.into(),
            name: name.into(),
            permission,
        }
    }
}

impl<F> CommandT for FnCommand<F>
where
    F: Fn(PrivmsgMessage, TwitchClient, PgPool, Arc<RwLock<BotState>>) -> BoxFuture<'static, BotResult<()>>
        + Send + Sync + 'static,
{
    fn execute(&self, msg: PrivmsgMessage, client: TwitchClient, pool: PgPool, bot_state: Arc<RwLock<BotState>>, _alias_config: Arc<AliasConfig>) -> BoxFuture<'static, BotResult<()>> {
        (self.func)(msg, client, pool, bot_state)
    }

    fn description(&self) -> &str {
        &self.desc
    }

    fn usage(&self) -> &str {
        &self.usage
    }

    fn permission(&self) -> PermissionLevel {
        self.permission
    }

    fn name(&self) -> &str {
        &self.name
    }
}


pub fn so() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(|msg, client, pool, bot_state| {
            Box::pin(async move {
                let template_manager = TemplateManager {
                    pool: pool.clone().into(),
                };
                let channel = format!("#{}", &msg.channel_login);
                let words: Vec<&str> = words(&msg);
                let reply = if words.len() == 2 {
                    let template = template_manager.get_template("Shoutout".to_string(), "!so".to_string(), Some(channel.clone())).await.unwrap_or("Let's give a big Shoutout to https://www.twitch.tv/%receiver% ! Make sure to check them out and give them a FOLLOW <3! They are amazing person!".to_string());
                    let mut variables = generate_variables(&msg);
                    let mut twitch_name =
                        words[1].strip_prefix("@").unwrap_or(words[1]).to_string();
                    if twitch_name.to_ascii_lowercase() == "krapmatt" {
                        if let Some(x) = variables.get_mut("receiver") {
                            *x = msg.sender.login.clone();
                        }
                        twitch_name = msg.sender.login.clone();
                        client.say(msg.channel_login.clone(), "Get outskilled :P".to_string()).await?;
                    }
                    let bot_state = bot_state.read().await;
                    if let Ok(twitch_user_id) = get_twitch_user_id(&twitch_name, &bot_state.oauth_token_bot, &bot_state.bot_id).await {
                        twitch_api::shoutout(&bot_state.oauth_token_bot, bot_state.clone().bot_id, &twitch_user_id, &msg.channel_id).await?;
                    }
                    parse_template(&template, &variables)
                } else {
                    "Arent you missing something?".to_string()
                };
                client.say(msg.channel_login, reply).await?;
                Ok(())
            })
        },
        "Shoutout a channel. Has template",
        "!so @name",
        "Shoutout" ,
        PermissionLevel::Vip
    ))
}