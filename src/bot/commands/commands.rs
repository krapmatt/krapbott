use std::{collections::HashMap, str::FromStr, sync::Arc};
use futures::future::BoxFuture;
use once_cell::sync::Lazy;
use sqlx::PgPool;

use crate::bot::{chat_event::chat_event::{ChatEvent, Platform}, commands::{CommandGroup, CommandRegistration, CommandRegistry, moderation::commands::MODERATION_COMMANDS, queue::commands::QUEUE_COMMANDS}, db::ChannelId, handler::handler::UnifiedChatClient, permissions::permissions::PermissionLevel, state::def::{AppState, BotError}};

//pub type CommandHandler = Arc<dyn Fn(PrivmsgMessage, Arc<Mutex<TwitchClient>>, PgPool, Arc<AppState>) -> BoxFuture<'static, BotResult<()>> + Send + Sync>;

pub type BotResult<T> = Result<T, BotError>;

pub static COMMAND_GROUPS: Lazy<HashMap<&'static str, Arc<CommandGroup>>> = Lazy::new(|| {
    let mut map = HashMap::new();
    map.insert("queue", QUEUE_COMMANDS.clone());
    //map.insert("points", &*POINTS_COMMANDS);
    map.insert("moderation", MODERATION_COMMANDS.clone());
    //map.insert("bungie", &*BUNGIE_COMMANDS);
    map
});

pub trait CommandT: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn usage(&self) -> &str;
    fn permission(&self) -> PermissionLevel;

    fn execute(&self, event: ChatEvent, pool: PgPool, state: Arc<AppState>, client: Arc<UnifiedChatClient>) -> BoxFuture<'static, BotResult<()>>;
}

pub struct FnCommand<F> {func: F, desc: String, usage: String, name: String, permission: PermissionLevel} impl<F> FnCommand<F>
    where
        F: Fn(ChatEvent, PgPool, Arc<AppState>, Arc<UnifiedChatClient>) -> BoxFuture<'static, BotResult<()>> + Send + Sync + 'static {
    pub fn new(func: F, desc: impl Into<String>, usage: impl Into<String>, name: impl Into<String>, permission: PermissionLevel,) -> Self {
        Self {
            func,
            desc: desc.into(),
            usage: usage.into(),
            name: name.into(),
            permission,
        }
    }
}

impl<F> CommandT for FnCommand<F> where
    F: Fn(ChatEvent, PgPool, Arc<AppState>, Arc<UnifiedChatClient>) -> BoxFuture<'static, BotResult<()>> + Send + Sync + 'static {
        fn execute(&self, event: ChatEvent, pool: PgPool, state: Arc<AppState>, client: Arc<UnifiedChatClient>) -> BoxFuture<'static, BotResult<()>> {
            (self.func)(event, pool, state, client)
        }

        fn name(&self) -> &str { &self.name }
        fn description(&self) -> &str { &self.desc }
        fn usage(&self) -> &str { &self.usage }
        fn permission(&self) -> PermissionLevel { self.permission }
}

impl CommandRegistry {
    pub fn new() -> Self {
        let mut groups = HashMap::new();

        for (name, group) in COMMAND_GROUPS.iter() {
            groups.insert((*name).to_string(), Arc::clone(group));
        }

        Self { groups }
    }
}

#[macro_export]
macro_rules! cmd {
    ($command:expr, $($alias:expr),+ $(,)?) => {
        CommandRegistration {
            aliases: vec![$($alias.to_string()),+],
            command: $command,
        }
    };
}

pub fn parse_channel_id(input: &str, default_platform: Platform) -> Result<ChannelId, BotError> {
    let input = input.trim_start_matches('@');

    if let Some((platform, channel)) = input.split_once(':') {
        let platform = Platform::from_str(platform)
            .map_err(|_| BotError::Custom("Invalid platform".into()))?;
        Ok(ChannelId::new(platform, channel.to_lowercase()))
    } else {
        // no platform prefix → assume current platform
        Ok(ChannelId::new(default_platform, input.to_lowercase()))
    }
}