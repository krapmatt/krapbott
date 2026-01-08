use std::sync::Arc;

use futures::future::BoxFuture;
use once_cell::sync::Lazy;
use sqlx::PgPool;

use crate::{api::twitch_api::resolve_twitch_user_id, bot::{chat_event::chat_event::{ChatEvent, Platform}, commands::{CommandGroup, CommandRegistration, commands::{BotResult, CommandT, FnCommand, parse_channel_id}, queue::logic::{QueueEntry, QueueKey, is_valid_bungie_name, next_handler, process_queue_entry, randomize_queue, resolve_queue_owner, toggle_queue}}, db::{ChannelId, UserId, config::save_channel_config}, handler::handler::{ChatClient, UnifiedChatClient}, permissions::permissions::PermissionLevel, state::def::{AppState, BotError}, web::sse::SseEvent}, cmd};

pub static QUEUE_COMMANDS: Lazy<Arc<CommandGroup>> = Lazy::new(|| {
    Arc::new(CommandGroup {
        name: "queue".into(),
        commands: vec![
            cmd!(Arc::new(JoinCommand), "j", "q", "queue"),
            cmd!(Arc::new(NextCommand), "next"),
            cmd!(Arc::new(ForceAddCommand), "add"),
            cmd!(Arc::new(QueueSize), "queue_len", "len"),
            cmd!(Arc::new(QueueLength), "queue_size", "size"),
            cmd!(list(), "list"),
            cmd!(random(), "random"),
            cmd!(toggle_queue_command(true), "open", "open_queue"),
            cmd!(toggle_queue_command(false), "close", "close_queue"),
            cmd!(queue_share(), "queue_share", "share"),
            cmd!(leave_command(), "leave"),
            cmd!(move_command(), "move"),
            cmd!(remove_command(), "remove"),
            cmd!(prio_command(), "prio", "bribe"),
            cmd!(pos(), "pos", "position"),


        ],
    })
});

pub struct JoinCommand;

impl CommandT for JoinCommand {
    fn name(&self) -> &str { "join" }
    fn description(&self) -> &str { "Join the queue" }
    fn usage(&self) -> &str { "!join" }
    fn permission(&self) -> PermissionLevel { PermissionLevel::Everyone }

    fn execute(&self, event: ChatEvent, pool: PgPool, state: Arc<AppState>, client: Arc<UnifiedChatClient>) -> BoxFuture<'static, BotResult<()>> {
        Box::pin(async move {
            let reply = state.handle_join(event.clone(), &pool).await?;
            if let Some(msg) = reply {
                client.send_message(&ChannelId::new(event.platform, &event.channel), &msg).await?;
                &state.sse_bus.send(SseEvent::QueueUpdated { channel: ChannelId::new(event.platform, &event.channel) })?;
            }
            Ok(())
        })
    }
}

pub struct ForceAddCommand;

impl CommandT for ForceAddCommand {
    fn name(&self) -> &str { "add" }
    fn description(&self) -> &str { "Force add a user to the queue" }
    fn usage(&self) -> &str { "!add @twitchname BungieName#1234" }
    fn permission(&self) -> PermissionLevel { PermissionLevel::Moderator }

    fn execute(&self, event: ChatEvent, pool: PgPool, state: Arc<AppState>, client: Arc<UnifiedChatClient>) -> BoxFuture<'static, BotResult<()>> {
        Box::pin(async move {
            let words: Vec<&str> = event.message.split_whitespace().collect();
            let mut reply = if words.len() < 3 {
                "Usage: !add @name BungieName#1234"
            } else {
                "User added to the queue."
            };

            let name = words[1].strip_prefix("@").unwrap_or(words[1]).to_string();
            let bungie_name = words[2..].join(" ");
            let entry = if event.platform == Platform::Twitch {
                let (platform_id, display_name) = resolve_twitch_user_id(&name, &state.secrets).await?;
                let user_id = UserId::new(Platform::Twitch, platform_id);
                QueueEntry {
                    user_id,
                    bungie_name: bungie_name.clone(),
                    display_name: display_name.clone(),
                }
            } else {
                return Err(BotError::Custom("Missing Platform".to_string()))
            };
            reply = if is_valid_bungie_name(&bungie_name).is_none() {
                "Please provide a valid Bungie name (Name#1234)."
            } else {
                "User added to the queue."
            };

            

            let channel_id = ChannelId::new(event.platform, &event.channel);

            let cfg = {
                let s = state.config.read().await;
                match s.get_channel_config(&channel_id) {
                    Some(c) => c.clone(),
                    None => {
                        client.send_message(&channel_id, "Channel configuration not found.").await?;
                        return Ok(());
                    }
                }
            };
            
            let reply = process_queue_entry(&pool, event, cfg.len, entry, &channel_id, crate::bot::commands::queue::logic::Queue::ForceJoin, cfg.random_queue).await?;
            &state.sse_bus.send(SseEvent::QueueUpdated { channel: channel_id.clone() })?;
            client.send_message(&channel_id, &reply).await?;
            Ok(())
        })
    }
}


pub struct NextCommand;

impl CommandT for NextCommand {
    fn name(&self) -> &str { "next" }
    fn description(&self) -> &str { "Get the next user in the queue" }
    fn usage(&self) -> &str { "!next" }
    fn permission(&self) -> PermissionLevel { PermissionLevel::Broadcaster }

    fn execute(&self, event: ChatEvent, pool: PgPool, state: Arc<AppState>, client: Arc<UnifiedChatClient>) -> BoxFuture<'static, BotResult<()>> {
        Box::pin(async move {
            let caller = ChannelId::new(event.platform, &event.channel);
            let owner = resolve_queue_owner(&state, &caller).await?;
            let (teamsize, random_queue) = {
                let cfg = state.config.read().await;
                let c = cfg.get_channel_config(&owner).ok_or(BotError::ConfigMissing(owner.clone()))?;
                (c.teamsize as i64, c.random_queue)
            };

            let result = if random_queue {
                randomize_queue(&owner, &pool, teamsize).await?
            } else {
                next_handler(&owner, &pool, teamsize).await?
            };

            {
                let mut cfg = state.config.write().await;
                cfg.get_channel_config_mut(owner).runs += 1;
            }

            client.send_message(&caller, &result).await?;
            &state.sse_bus.send(SseEvent::QueueUpdated { channel: ChannelId::new(event.platform, &event.channel) })?;
            Ok(())
        })
    }
}

pub struct QueueSize;

impl CommandT for QueueSize {
    fn name(&self) -> &str { "queue_size" }
    fn description(&self) -> &str { "Update size of group" }
    fn usage(&self) -> &str { "!queue_size number" }
    fn permission(&self) -> PermissionLevel { PermissionLevel::Moderator }
    fn execute(&self, event: ChatEvent, pool: PgPool, state: Arc<AppState>, client: Arc<UnifiedChatClient>) -> BoxFuture<'static, BotResult<()>> {
        Box::pin(async move {
            let caller = ChannelId::new(event.platform, &event.channel);
            let owner = resolve_queue_owner(&state, &caller).await?;

            let new_len: usize = event.message.split_whitespace().nth(1).ok_or_else(|| BotError::Custom("Usage: !len <n>".to_string()))?
            .parse().map_err(|_| BotError::Custom("Invalid Number".to_string()))?;
            {
                let mut cfg = state.config.write().await;
                cfg.get_channel_config_mut(owner.clone()).len = new_len;
                save_channel_config(&pool, &owner, &cfg).await?;
            }

            
            client.send_message(&caller, &format!("Queue size updated to {}.", new_len)).await?;
            Ok(())
        })    
    }
}

pub struct QueueLength;

impl CommandT for QueueLength {
    fn name(&self) -> &str { "queue_len" }
    fn usage(&self) -> &str { "!queue_len number" }
    fn description(&self) -> &str { "Change the lenght of queue" }
    fn permission(&self) -> PermissionLevel { PermissionLevel::Moderator }
    fn execute(&self, event: ChatEvent, pool: PgPool, state: Arc<AppState>, client: Arc<UnifiedChatClient>) -> BoxFuture<'static, BotResult<()>> {
        Box::pin(async move {
            let caller = ChannelId::new(event.platform, &event.channel);
            let owner = resolve_queue_owner(&state, &caller).await?;

            let new_size: usize = event.message.split_whitespace().nth(1).ok_or_else(|| BotError::Custom("Usage: !size <n>".to_string()))?
            .parse().map_err(|_| BotError::Custom("Invalid Number".to_string()))?;
            {
                let mut cfg = state.config.write().await;
                cfg.get_channel_config_mut(owner.clone()).teamsize = new_size;
                save_channel_config(&pool, &owner, &cfg).await?;
            }

            
            client.send_message(&caller, &format!("Team size updated to {}.", new_size)).await?;

            Ok(())
        })    
    }
}

pub fn toggle_queue_command(open: bool) -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        move |event, pool, state, client| {
            Box::pin(async move {

                let caller = ChannelId::new(event.platform, &event.channel);
                let owner = resolve_queue_owner(&state, &caller).await?;

                let msg = toggle_queue(&pool, &state, &owner, open).await?;

                client.send_message(&caller, &msg).await?;
                Ok(())
            })
        },
        if open { "Open queue" } else { "Close queue" },
        if open { "!open" } else { "!close" },
        if open { "open" } else { "close" },
        PermissionLevel::Moderator
    ))
}

pub fn list() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |event, pool, state, client| {
            Box::pin(async move {
                let caller = ChannelId::new(event.platform, &event.channel);
                let owner = resolve_queue_owner(&state, &caller).await?;

                let (teamsize, random_queue) = {
                    let cfg = state.config.read().await;
                    let c = cfg.channels
                        .get(&owner)
                        .ok_or(BotError::ConfigMissing(owner.clone()))?;
                    (c.teamsize as usize, c.random_queue)
                };

                let queue_entries = sqlx::query!(
                    r#"
                    SELECT display_name, bungie_name
                    FROM krapbott_v2.queue
                    WHERE channel_id = $1
                    ORDER BY position ASC, locked_first DESC, group_priority ASC
                    "#,
                    owner.as_str()
                )
                .fetch_all(&pool)
                .await?;

                if queue_entries.is_empty() {
                    client.send_message(&caller, "Queue is empty!").await?;
                    return Ok(());
                }

                let queue_msg: Vec<String> = queue_entries
                    .iter()
                    .enumerate()
                    .map(|(i, q)| format!("{}. {} ({})", i + 1, q.display_name, q.bungie_name))
                    .collect();

                let format_group = |group: &[String]| group.join(", ");

                let reply = if random_queue {
                    let chosen = &queue_msg[..queue_msg.len().min(teamsize)];
                    let rest = &queue_msg[chosen.len()..];
                    format!(
                        "Chosen: {} // Entered people: {}",
                        format_group(chosen),
                        format_group(rest)
                    )
                } else if queue_msg.iter().map(|s| s.len()).sum::<usize>() < 400 {
                    let live = &queue_msg[..queue_msg.len().min(teamsize)];
                    let next = &queue_msg
                        [teamsize..queue_msg.len().min(teamsize * 2)];
                    let rest = &queue_msg
                        [queue_msg.len().min(teamsize * 2)..];

                    format!(
                        "LIVE: {} || NEXT: {} || QUEUE: {}",
                        format_group(live),
                        format_group(next),
                        format_group(rest)
                    )
                } else {
                    format!(
                        "You can find queue here: https://krapbott-rajo.shuttle.app/queue.html?streamer={}",
                        owner.channel()
                    )
                };

                client.send_message(&caller, &reply).await?;
                Ok(())
            })
        },
        "Shows the queue list or site",
        "!list, !queue",
        "list",
        PermissionLevel::Everyone
    ))
}

pub fn random() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |event, pool, state, client| {
            Box::pin(async move {
                let caller = ChannelId::new(event.platform, &event.channel);
                let owner = resolve_queue_owner(&state, &caller).await?;

                {
                    let mut cfg = state.config.write().await;
                    let channel_config = cfg.get_channel_config_mut(owner.clone());
                    let random = channel_config.random_queue;
                    channel_config.random_queue = !random;

                    save_channel_config(&pool, &owner, &cfg).await?;

                    let reply = if !random {
                        "Raffle Mode Active"
                    } else {
                        "Queue Mode Active"
                    };
                    client.send_message(&caller, reply).await?;
                }
                Ok(())
            })
        },
        "Raffle Mode",
        "",
        "Random",
        PermissionLevel::Moderator
    ))
}

pub fn pos() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |event, pool, state, client| {
            Box::pin(async move {
                let user = event.user.as_ref().ok_or_else(|| BotError::Custom("No user".to_string()))?;
                let caller = ChannelId::new(event.platform, &event.channel);
                let owner = resolve_queue_owner(&state, &caller).await?;

                let (teamsize, random_queue, open, max_len) = {
                    let cfg = state.config.read().await;
                    let c = cfg.get_channel_config(&owner).ok_or(BotError::ConfigMissing(owner.clone()))?;
                    (
                        c.teamsize as i64,
                        c.random_queue,
                        c.open,
                        c.len as i64
                    )
                };
                let user_id = UserId::new(user.identity.platform, user.identity.platform_user_id.clone());
                let max_count: i64 = sqlx::query_scalar!(
                    r#"SELECT COUNT(*) FROM krapbott_v2.queue WHERE channel_id = $1"#,
                    owner.as_str()
                ).fetch_one(&pool).await?.unwrap_or(0);

                let pos: Option<i64> = sqlx::query_scalar!(
                    r#"
                    WITH RankedQueue AS (
                        SELECT user_id,
                               ROW_NUMBER() OVER (ORDER BY position) AS pos
                        FROM krapbott_v2.queue
                        WHERE channel_id = $1
                    )
                    SELECT pos FROM RankedQueue WHERE user_id = $2"#,
                    owner.as_str(), user_id.as_str()
                ).fetch_optional(&pool).await?.flatten();

                let sender = &user.name.display;

                let reply = if !random_queue {
                    match pos {
                        Some(index) => {
                            let group = (index - 1) / teamsize + 1;
                            if group == 1 {
                                format!("You are at position {}/{} and in LIVE group! DinoDance", index, max_count)
                            } else if group == 2 {
                                format!("You are at position {}/{} and in NEXT group! GoldPLZ", index, max_count)
                            } else {
                                format!("You are at position {}/{} (Group {})!", index, max_count, group)
                            }
                        }
                        None => {
                            if !open {
                                format!("The queue is CLOSED 🚫 and you are not in queue, {}", sender)
                            } else if max_count >= max_len {
                                format!("Queue is FULL and you are not in queue, {}", sender)
                            } else {
                                format!("You are not in queue, {}. There is {} users in queue", sender, max_count)
                            }
                        }
                    }
                } else {
                    match pos {
                        Some(_) => format!("✅ You are entered in the raffle, {}", sender),
                        None => format!("❌ You are not entered in the raffle, {}", sender),
                    }
                };

                client.send_message(&caller, &reply).await?;

                Ok(())
            })
        },
        "Show position in queue",
        "!pos",
        "position",
        PermissionLevel::Everyone
    ))
}

pub fn bungie_name_command() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |event, pool, state, client| {
            let fut = async move {
                /*// If the message is only 11 characters long, assume it's just the command (use self)
                let words = words(&msg);
                let name = if words.len() == 1 {
                    msg.sender.name.clone()
                } else {
                    let (_, twitch_name) = msg
                        .message_text.split_once(' ').expect("How did it panic, what happened? // Always is something here");

                    let mut twitch_name = twitch_name.to_string();
                    if twitch_name.starts_with('@') {
                        twitch_name.remove(0);
                    }

                    twitch_name
                };
                bungiename(msg, client, &pool, name).await?;*/
                Ok(())
            };

            Box::pin(fut)
        },
        "Shows registered Bungie name",
        "!bungiename [@twitchname]",
        "bungiename",
        PermissionLevel::Everyone,
    ))
}

pub fn queue_share() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |event, pool, state, client| {
            Box::pin(async move {
                let caller = ChannelId::new(event.platform, &event.channel);
                let args: Vec<&str> = event.message.split_whitespace().collect();

                // ─────────────────────────────
                // SHOW STATUS
                // ─────────────────────────────
                if args.len() == 1 {
                    let cfg = state.config.read().await;
                    let c = cfg.get_channel_config(&caller).ok_or(BotError::ConfigMissing(caller.clone()))?;

                    let reply = match &c.queue_target {
                        QueueKey::Single(_) => "Queue mode: SINGLE".to_string(),
                        QueueKey::Shared(owner) => {
                            let members: Vec<String> = cfg.channels
                                .iter()
                                .filter(|(_, c)| matches!(&c.queue_target, QueueKey::Shared(o) if o == owner))
                                .map(|(id, _)| id.as_str().to_string())
                                .collect();

                            format!(
                                "Queue mode: SHARED\nOwner: {}\nMembers: {:?}",
                                owner.as_str(),
                                members
                            )
                        }
                    };

                    client.send_message(&caller, &reply).await?;
                    return Ok(());
                }

                // ─────────────────────────────
                // DISABLE SHARED QUEUE
                // ─────────────────────────────
                if args[1].eq_ignore_ascii_case("off") {
                    {
                        let mut cfg = state.config.write().await;
                        let c = cfg
                            .channels
                            .get_mut(&caller)
                            .ok_or(BotError::ConfigMissing(caller.clone()))?;

                        c.queue_target = QueueKey::Single(caller.clone());
                        save_channel_config(&pool, &caller, &cfg).await?;
                    }

                    client.send_message(&caller, "Shared queue DISABLED").await?;
                    return Ok(());
                }

                // ─────────────────────────────
                // ENABLE / DEFINE SHARED QUEUE
                // ─────────────────────────────
                if args.len() < 3 {
                    client
                        .send_message(
                            &caller,
                            "Usage: !queue_share twitch:main kick:other1 ... OR !queue_share off",
                        )
                        .await?;
                    return Ok(());
                }

                let mut channels: Vec<ChannelId> = args[1..]
                    .iter()
                    .map(|a| parse_channel_id(a, event.platform))
                    .collect::<Result<_, _>>()?;

                let owner = channels.remove(0);
                let shared_key = QueueKey::Shared(owner.clone());

                {
                    let mut cfg = state.config.write().await;

                    if let Some(c) = cfg.channels.get_mut(&owner) {
                        c.queue_target = shared_key.clone();
                    }

                    for ch in &channels {
                        if let Some(c) = cfg.channels.get_mut(ch) {
                            c.queue_target = shared_key.clone();
                        }
                    }
                    
                    save_channel_config(&pool, &owner, &cfg).await?;
                    for ch in &channels {
                        save_channel_config(&pool, ch, &cfg).await?;
                    }
                }

                

                client.send_message(&caller,&format!("Shared queue ENABLED\nOwner: {}\nMembers: {:?}", owner.as_str(), channels.iter().map(|c| c.as_str()).collect::<Vec<_>>())).await?;

                Ok(())
            })
        },
        "Manage shared queue (enable / disable / status)",
        "!queue_share",
        "queue_share",
        PermissionLevel::Moderator,
    ))
}

pub fn leave_command() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |event, pool, state, client| {
            Box::pin(async move {
                let user = event.user.as_ref().ok_or_else(|| {
                    BotError::Custom("No user".into())
                })?;

                let caller = ChannelId::new(event.platform, &event.channel);
                let owner = resolve_queue_owner(&state, &caller).await?;

                let (teamsize, random_queue) = {
                    let cfg = state.config.read().await;
                    let c = cfg
                        .channels
                        .get(&owner)
                        .ok_or(BotError::ConfigMissing(owner.clone()))?;
                    (c.teamsize as i64, c.random_queue)
                };

                let user_id = UserId::new(user.identity.platform, user.identity.platform_user_id.clone());
                let name = &user.name.display;

                // fetch position
                let position= sqlx::query_scalar!(
                    r#"
                    SELECT position
                    FROM krapbott_v2.queue
                    WHERE user_id = $1 AND channel_id = $2
                    "#,
                    user_id.as_str(), owner.as_str()
                ).fetch_optional(&pool).await?;

                let reply = if let Some(pos) = position {
                    if pos <= teamsize as i32 {
                        "You cannot leave the LIVE group! Ask the streamer or wait for !next".to_string()
                    } else {
                        let mut tx = pool.begin().await?;

                        // delete user
                        sqlx::query!(
                            r#"
                            DELETE FROM krapbott_v2.queue
                            WHERE user_id = $1 AND channel_id = $2
                            "#,
                            user_id.as_str(), owner.as_str()
                        ).execute(&mut *tx).await?;

                        // re-pack positions
                        sqlx::query!(
                            r#"
                            WITH ranked AS (
                                SELECT user_id,
                                       ROW_NUMBER() OVER (ORDER BY position) AS new_pos
                                FROM krapbott_v2.queue
                                WHERE channel_id = $1
                            )
                            UPDATE krapbott_v2.queue q
                            SET position = r.new_pos
                            FROM ranked r
                            WHERE q.user_id = r.user_id
                              AND q.channel_id = $1
                            "#,
                            owner.as_str()
                        ).execute(&mut *tx).await?;

                        tx.commit().await?;

                        if random_queue {
                            format!("BigSad {} has left the raffle.", name)
                        } else {
                            format!("BigSad {} has left the queue.", name)
                        }
                    }
                } else {
                    format!("You were already free, {}", name)
                };

                client.send_message(&caller, &reply).await?;
                &state.sse_bus.send(SseEvent::QueueUpdated { channel: ChannelId::new(event.platform, &event.channel) });
                Ok(())
            })
        },
        "Leave the queue",
        "!leave",
        "leave",
        PermissionLevel::Everyone,
    ))
}

//prio, remove, move, clear, deprio

pub fn move_command() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |event, pool, state, client| {
            Box::pin(async move {
                let args: Vec<&str> = event.message.split_whitespace().collect();
                if args.len() < 2 {
                    client.send_message(&ChannelId::new(event.platform, &event.channel), "Usage: !move <user>").await?;
                    return Ok(());
                }

                let caller = ChannelId::new(event.platform, &event.channel);
                let owner = resolve_queue_owner(&state, &caller).await?;

                let teamsize = {
                    let cfg = state.config.read().await;
                    cfg.channels
                        .get(&owner)
                        .ok_or(BotError::ConfigMissing(owner.clone()))?
                        .teamsize as i64
                };

                let target = args[1].trim_start_matches('@');

                let mut tx = pool.begin().await?;

                let pos: Option<i32> = sqlx::query_scalar!(
                    r#"
                    SELECT position FROM krapbott_v2.queue
                    WHERE display_name = $1 AND channel_id = $2
                    "#,
                    target,
                    owner.as_str()
                ).fetch_optional(&mut *tx).await?;

                let pos = match pos {
                    Some(p) => p,
                    None => {
                        client.send_message(&caller, &format!("User {} isn’t in the queue!", target)).await?;
                        return Ok(());
                    }
                };

                let max_pos: i32 = sqlx::query_scalar!(
                    r#"SELECT COALESCE(MAX(position), 0) FROM krapbott_v2.queue WHERE channel_id = $1"#,
                    owner.as_str()
                ).fetch_one(&mut *tx).await?.unwrap_or(0);

                let new_pos = pos + (teamsize as i32);
                if new_pos > max_pos { 
                    client.send_message(&caller, &format!("User {} is already in the last group.", target)).await?;
                    return Ok(());
                }

                let temp = max_pos + 1000;

                sqlx::query!(
                    r#"UPDATE krapbott_v2.queue SET position = $1 WHERE display_name = $2 AND channel_id = $3"#,
                    temp, target, owner.as_str()
                ).execute(&mut *tx).await?;

                sqlx::query!(
                    r#"
                    UPDATE krapbott_v2.queue
                    SET position = position - 1
                    WHERE channel_id = $1 AND position BETWEEN $2 AND $3
                    "#,
                    owner.as_str(), pos + 1, new_pos
                ).execute(&mut *tx).await?;

                sqlx::query!(
                    r#"UPDATE krapbott_v2.queue SET position = $1 WHERE display_name = $2 AND channel_id = $3"#,
                    new_pos, target, owner.as_str()
                ).execute(&mut *tx).await?;

                tx.commit().await?;

                client.send_message(&caller, &format!("User {} has been moved to the next group.", target)).await?;
                &state.sse_bus.send(SseEvent::QueueUpdated { channel: ChannelId::new(event.platform, &event.channel) });
                Ok(())
            })
        },
        "Move user to next group",
        "!move <user>",
        "move",
        PermissionLevel::Moderator,
    ))
}

pub fn remove_command() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |event, pool, state, client| {
            Box::pin(async move {
                let args: Vec<&str> = event.message.split_whitespace().collect();
                if args.len() != 2 {
                    return Ok(());
                }

                let target = args[1].trim_start_matches('@');
                let caller = ChannelId::new(event.platform, &event.channel);
                let owner = resolve_queue_owner(&state, &caller).await?;

                let mut tx = pool.begin().await?;

                let pos = sqlx::query_scalar!(
                    r#"SELECT position FROM krapbott_v2.queue WHERE display_name = $1 AND channel_id = $2"#,
                    target, owner.as_str()
                ).fetch_optional(&mut *tx).await?;

                let reply = if pos.is_some() {
                    sqlx::query!(
                        r#"DELETE FROM krapbott_v2.queue WHERE display_name = $1 AND channel_id = $2"#,
                        target, owner.as_str()
                    ).execute(&mut *tx).await?;

                    sqlx::query!(
                        r#"
                        WITH ranked AS (
                            SELECT user_id, ROW_NUMBER() OVER (ORDER BY position) AS p
                            FROM krapbott_v2.queue
                            WHERE channel_id = $1
                        )
                        UPDATE krapbott_v2.queue q
                        SET position = ranked.p
                        FROM ranked
                        WHERE q.user_id = ranked.user_id AND q.channel_id = $1
                        "#,
                        owner.as_str()
                    ).execute(&mut *tx).await?;

                    format!("{} has been removed from the queue.", target)
                } else {
                    format!("User {} not found in the queue. FailFish", target)
                };

                tx.commit().await?;
                client.send_message(&caller, &reply).await?;
                &state.sse_bus.send(SseEvent::QueueUpdated { channel: ChannelId::new(event.platform, &event.channel) });
                Ok(())
            })
        },
        "Remove user from queue",
        "!remove <user>",
        "remove",
        PermissionLevel::Moderator,
    ))
}

pub fn prio_command() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |event, pool, state, client| {
            Box::pin(async move {
                let args: Vec<&str> = event.message.split_whitespace().collect();
                if args.len() < 2 {
                    client.send_message(
                        &ChannelId::new(event.platform, &event.channel),
                        "Usage: !prio <user> [runs]",
                    ).await?;
                    return Ok(());
                }

                let target = args[1].trim_start_matches('@');
                let runs = args.get(2).and_then(|r| r.parse::<i32>().ok());

                let caller = ChannelId::new(event.platform, &event.channel);
                let owner = resolve_queue_owner(&state, &caller).await?;

                let teamsize = {
                    let cfg = state.config.read().await;
                    cfg.channels
                        .get(&owner)
                        .ok_or(BotError::ConfigMissing(owner.clone()))?
                        .teamsize as i32
                };

                let second_group = teamsize + 1;
                let mut tx = pool.begin().await?;

                let exists: Option<i32> = sqlx::query_scalar!(
                    r#"SELECT position FROM krapbott_v2.queue WHERE display_name = $1 AND channel_id = $2"#,
                    target, owner.as_str()
                ).fetch_optional(&mut *tx).await?;

                if exists.is_none() {
                    client.send_message(&caller, &format!("User {} not found in the queue", target)).await?;
                    return Ok(());
                }

                sqlx::query!(
                    r#"UPDATE krapbott_v2.queue SET position = position + 10000 WHERE channel_id = $1 AND position >= $2"#,
                    owner.as_str(), second_group
                ).execute(&mut *tx).await?;

                if let Some(runs) = runs {
                    sqlx::query!(
                        r#"
                        UPDATE krapbott_v2.queue
                        SET position = $1,
                            group_priority = 1,
                            priority_runs_left = $2,
                            locked_first = FALSE
                        WHERE display_name = $3 AND channel_id = $4
                        "#,
                        second_group, runs, target, owner.as_str()
                    ).execute(&mut *tx).await?;
                } else {
                    sqlx::query!(
                        r#"UPDATE krapbott_v2.queue SET position = $1 WHERE display_name = $2 AND channel_id = $3"#,
                        second_group, target, owner.as_str()
                    ).execute(&mut *tx).await?;
                }

                sqlx::query!(
                    r#"
                    WITH ranked AS (
                        SELECT user_id, ROW_NUMBER() OVER (ORDER BY position) AS p
                        FROM krapbott_v2.queue
                        WHERE channel_id = $1
                    )
                    UPDATE krapbott_v2.queue q
                    SET position = ranked.p
                    FROM ranked
                    WHERE q.user_id = ranked.user_id AND q.channel_id = $1
                    "#,
                    owner.as_str()
                ).execute(&mut *tx).await?;

                tx.commit().await?;

                let reply = match runs {
                    Some(r) => format!("{} has been promoted to priority for {} runs", target, r),
                    None => format!("{} has been pushed to the second group", target),
                };
                &state.sse_bus.send(SseEvent::QueueUpdated { channel: ChannelId::new(event.platform, &event.channel) });
                client.send_message(&caller, &reply).await?;
                Ok(())
            })
        },
        "Give priority or move to second group",
        "!prio <user> [runs]",
        "prio",
        PermissionLevel::Moderator,
    ))
}