use std::sync::Arc;

use futures::future::BoxFuture;
use sqlx::PgPool;
use tokio::{sync::RwLock};
use twitch_irc::message::PrivmsgMessage;

use crate::{bot::{BotState, TwitchClient}, models::{AliasConfig, BotResult, PermissionLevel}};

pub trait CommandT: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn usage(&self) -> &str;
    fn permission(&self) -> PermissionLevel;

    fn execute(&self, msg: PrivmsgMessage, client: TwitchClient, pool: PgPool, bot_state: Arc<RwLock<BotState>>, alias: Arc<AliasConfig>) -> BoxFuture<'static, BotResult<()>>;
}

impl dyn CommandT {
    pub fn usage_with_aliases(&self, alias_config: &AliasConfig) -> String {
        let mut aliases = alias_config.get_aliases(self.name());
        
        // Always include the primary command name if it’s not removed
        if !alias_config.get_removed_aliases(self.name()) {
            aliases.push(self.name().to_string());
        }

        // Remove duplicates + disabled commands
        aliases.retain(|alias| !alias_config.disabled_commands.contains(alias));

        if aliases.is_empty() {
            return format!("Usage: {}", self.usage());
        }

        // Join all aliases
        let alias_list = aliases
            .iter()
            .map(|a| format!("!{}", a))
            .collect::<Vec<_>>()
            .join(" / ");

        format!("Usage: {} → {}", alias_list, self.usage())
    }
}

/*pub struct RateLimiter {
    // Could be per-channel, global, or per-user
    pub tokens: Arc<Mutex<VecDeque<Instant>>>,
    pub max_messages: usize,
    pub window: Duration,
}

pub struct RateLimitedCommand {
    inner: Arc<dyn Command>,
    limiter: Arc<RateLimiter>,
}

impl Command for RateLimitedCommand {
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn description(&self) -> &str {
        self.inner.description()
    }
    fn usage(&self) -> &str {
        self.inner.usage()
    }
    fn permission(&self) -> PermissionLevel {
        self.inner.permission()
    }

    fn execute(&self, msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, pool: PgPool, bot_state: Arc<RwLock<BotState>>) -> BoxFuture<'static, BotResult<()>> {
        let limiter = self.limiter.clone();
        let inner = self.inner.clone();
        Box::pin(async move {
            if limiter.allow().await {
                inner.execute(msg, client, pool, bot_state).await
            } else {
                // Optional: send a "you're being rate-limited" message
                Ok(())
            }
        })
    }
}*/

