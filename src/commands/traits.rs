use std::{borrow::BorrowMut, future::Future, sync::Arc};

use futures::future::BoxFuture;
use sqlx::SqlitePool;
use tmi::Privmsg;
use tokio::{sync::{Mutex, RwLock}};

use crate::{bot::BotState, models::{BotError, PermissionLevel}};

pub trait CommandT: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn usage(&self) -> &str;
    fn permission(&self) -> PermissionLevel;

    fn execute(
        &self,
        msg: Privmsg<'static>,
        client: Arc<Mutex<tmi::Client>>,
        db: SqlitePool,
        state: Arc<RwLock<BotState>>,
    ) -> BoxFuture<'static, Result<(), BotError>>;
}

pub async fn with_client<F, Fut>(client: Arc<Mutex<tmi::Client>>, f: F) -> Result<(), BotError>
where
    F: FnOnce(&mut tmi::Client) -> Fut,
    Fut: Future<Output = Result<(), BotError>>,
{
    let mut locked = client.lock().await;
    f(&mut locked.borrow_mut()).await
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

    fn execute(&self, msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, pool: SqlitePool, bot_state: Arc<RwLock<BotState>>) -> BoxFuture<'static, Result<(), BotError>> {
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

