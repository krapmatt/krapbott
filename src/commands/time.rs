use std::sync::Arc;
use chrono::Utc;
use chrono_tz::{Tz, CET, US::Pacific};
use futures::future::BoxFuture;

use sqlx::PgPool;
use tokio::sync::RwLock;
use twitch_irc::message::PrivmsgMessage;

use crate::{bot::{BotState, TwitchClient}, commands::traits::CommandT, models::{AliasConfig, BotResult, PermissionLevel}};

struct TimeCommand {
    name: String,
    description: String,
    usage: String,
    permission: PermissionLevel,
    timezone: Tz,
}

impl CommandT for TimeCommand {
    fn name(&self) -> &str { &self.name }
    fn description(&self) -> &str { &self.description }
    fn usage(&self) -> &str { &self.usage }
    fn permission(&self) -> PermissionLevel { self.permission }

    fn execute(&self, msg: PrivmsgMessage, client: TwitchClient, _db: PgPool, _state: Arc<RwLock<BotState>>, _alias_config: Arc<AliasConfig>) -> BoxFuture<'static, BotResult<()>> {
        let timezone = self.timezone;
        let name = self.name.clone();

        Box::pin(async move {
            let now = Utc::now().with_timezone(&timezone);
            let time_str = now.time().format("%-I:%M %p").to_string();

            let reply = format!("{} time: {}", name, time_str);
            client.say(msg.channel_login, reply).await?;
            Ok(())
        })
    }
}

fn create_time_command(name: &str, tz: Tz, description: &str, usage: &str, permission: PermissionLevel) -> impl CommandT {
    TimeCommand {
        name: name.to_string(),
        description: description.to_string(),
        usage: usage.to_string(),
        permission,
        timezone: tz,
    }
}

pub fn matt_time() -> Arc<dyn CommandT> {
    Arc::new(create_time_command(
        "matt_time",
        CET,
        "Shows current time of KrapMatt",
        "!mattbed",
        PermissionLevel::User,
    ))
}

pub fn samosa_time() -> Arc<dyn CommandT> {
    Arc::new(create_time_command(
        "samosa_time",
        Pacific,
        "Shows current time of Samosa Mimosa Leviosa Glosa",
        "!samoanbed",
        PermissionLevel::User,
    ))
}

pub fn cindi_time() -> Arc<dyn CommandT> {
    Arc::new(create_time_command(
        "cindi_time",
        chrono_tz::GMT0,
        "Shows current time of Cindi",
        "!cindibed",
        PermissionLevel::User,
    ))
}