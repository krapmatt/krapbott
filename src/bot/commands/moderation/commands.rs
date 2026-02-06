use std::{str::FromStr, sync::Arc};

use once_cell::sync::Lazy;

use crate::{bot::{commands::{CommandGroup, CommandRegistration, commands::{CommandT, FnCommand}, moderation::connect_channel, queue::logic::QueueKey}, db::{ChannelId, config::save_channel_config}, dispatcher::dispatcher::refresh_channel_dispatcher, handler::handler::ChatClient, permissions::permissions::PermissionLevel, runtime::channel_lifecycle::reload_channel, state::def::BotError}, cmd};
pub static MODERATION_COMMANDS: Lazy<Arc<CommandGroup>> = Lazy::new(|| {
    Arc::new(CommandGroup {
        name: "moderation".into(),
        commands: vec![
            cmd!(alias_command(), "alias"),
            cmd!(add_package_command(), "add_package"),
            cmd!(connect_command(), "connect"),
            cmd!(config_command(), "config", "mod_config")
        ]
    })
});

pub fn alias_command() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |event, pool, state, client| {
            Box::pin(async move {
                let caller = ChannelId::new(event.platform, &event.channel);
                let args: Vec<&str> = event.message.split_whitespace().collect();
                if args.len() < 3 {
                    return Err(BotError::Chat("Usage: !alias add <alias> <command> | !alias remove <alias>".to_string()));
                }

                match args[1].to_lowercase().as_str() {
                    // ─────────────────────────────
                    // ADD ALIAS
                    // ─────────────────────────────
                    "add" if args.len() == 4 => {
                        let alias = args[2].to_lowercase();
                        let command = args[3].to_lowercase();

                        sqlx::query!(
                            r#"
                            INSERT INTO krapbott_v2.command_aliases (channel, alias, command)
                            VALUES ($1, $2, $3)
                            ON CONFLICT (channel, alias)
                            DO UPDATE SET command = EXCLUDED.command
                            "#,
                            caller.as_str(), alias, command
                        ).execute(&pool).await?;

                        refresh_channel_dispatcher(&caller, state.clone(), &pool).await?;

                        client.send_message(&caller, &format!("✅ Alias '{}' → '{}' added", alias, command)).await?;
                    }

                    // ─────────────────────────────
                    // REMOVE ALIAS
                    // ─────────────────────────────
                    "remove" if args.len() == 3 => {
                        let alias = args[2].to_lowercase();

                        sqlx::query!(
                            r#"
                            DELETE FROM krapbott_v2.command_aliases
                            WHERE channel = $1 AND alias = $2
                            "#,
                            caller.as_str(), alias
                        ).execute(&pool).await?;

                        refresh_channel_dispatcher(&caller, state.clone(), &pool).await?;

                        client.send_message(&caller, &format!("Removed alias '{}'", alias)).await?;
                    }
                    // ───────── REMOVE DEFAULT ALIAS ─────────
                    "remove-default" if args.len() == 3 => {
                        let alias = args[2].to_lowercase();

                        sqlx::query!(
                            r#"
                            INSERT INTO krapbott_v2.command_alias_removals (channel, alias)
                            VALUES ($1, $2)
                            ON CONFLICT DO NOTHING
                            "#,
                            caller.as_str(), alias
                        ).execute(&pool).await?;

                        refresh_channel_dispatcher(&caller, state.clone(), &pool).await?;

                        client.send_message(&caller, &format!("🚫 Default alias '{}' disabled", alias)).await?;
                    }

                    // ───────── RESTORE DEFAULT ALIAS ─────────
                    "restore-default" if args.len() == 3 => {
                        let alias = args[2].to_lowercase();

                        sqlx::query!(
                            r#"
                            DELETE FROM krapbott_v2.command_alias_removals
                            WHERE channel = $1 AND alias = $2
                            "#,
                            caller.as_str(), alias
                        ).execute(&pool).await?;

                        refresh_channel_dispatcher(&caller, state.clone(), &pool).await?;

                        client.send_message(&caller, &format!("♻️ Default alias '{}' restored", alias)).await?;
                    }

                    // ───────── DISABLE COMMAND ─────────
                    "disable" if args.len() == 3 => {
                        let command = args[2].to_lowercase();

                        sqlx::query!(
                            r#"
                            INSERT INTO krapbott_v2.command_disabled (channel, command)
                            VALUES ($1, $2)
                            ON CONFLICT DO NOTHING
                            "#,
                            caller.as_str(), command
                        ).execute(&pool).await?;

                        refresh_channel_dispatcher(&caller, state.clone(), &pool).await?;

                        client.send_message(&caller, &format!("🔕 Command '{}' disabled", command)).await?;
                    }

                    // ───────── ENABLE COMMAND ─────────
                    "enable" if args.len() == 3 => {
                        let command = args[2].to_lowercase();

                        sqlx::query!(
                            r#"
                            DELETE FROM krapbott_v2.command_disabled
                            WHERE channel = $1 AND command = $2
                            "#,
                            caller.as_str(), command
                        ).execute(&pool).await?;

                        refresh_channel_dispatcher(&caller, state.clone(), &pool).await?;

                        client.send_message(&caller, &format!("🔔 Command '{}' enabled", command)).await?;
                    }

                    _ => {
                        return Err(BotError::Chat("Invalid syntax. Use: !alias add <alias> <command> | !alias remove <alias>".to_string()));
                    }
                }

                Ok(())
            })
        },
        "Add or remove command aliases",
        "!alias add <alias> <command> | !alias remove <alias>",
        "alias",
        PermissionLevel::Moderator,
    ))
}

pub fn add_package_command() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |event, pool, state, client| {
            Box::pin(async move {
                let Some(_user) = &event.user else { return Ok(()); };

                let parts: Vec<&str> = event.message.split_whitespace().collect();
                if parts.len() != 2 {
                    client.send_message(
                        &ChannelId::new(event.platform, &event.channel),
                        "Usage: !add_package <package>",
                    ).await?;
                    return Ok(());
                }

                let package = parts[1].to_lowercase();
                let channel_id = ChannelId::new(event.platform, &event.channel);

                

                // 2Update in-memory config
                {
                    let mut cfg = state.config.write().await;
                    let channel_cfg = cfg.channels
                        .get_mut(&channel_id)
                        .ok_or_else(|| BotError::ConfigMissing(channel_id.clone()))?;

                    if !channel_cfg.packages.contains(&package) {
                        channel_cfg.packages.push(package.clone());
                    }
                    save_channel_config(&pool, &channel_id, &cfg.clone()).await?;
                }

                

                // Reload dispatcher
                reload_channel(channel_id.clone(), state.clone(), &pool).await?;

                client.send_message(&channel_id, &format!("✅ Package `{}` enabled.", package)).await?;

                Ok(())
            })
        },
        "Enable a command package",
        "!add_package <package>",
        "add_package",
        PermissionLevel::Moderator,
    ))
}

pub fn connect_command() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |event, pool, state, client| {
            Box::pin(async move {
                let parts: Vec<&str> = event.message.split_whitespace().collect();
                if parts.len() != 2 {
                    client.send_message(
                        &ChannelId::new(event.platform, &event.channel),
                        "Usage: !connect twitch:channel | !connect kick:channel"
                    ).await?;
                    return Ok(());
                }

                let channel_id = ChannelId::from_str(parts[1])
                    .map_err(|_| BotError::Custom("Invalid channel id".into()))?;


                connect_channel(channel_id.clone(), state.clone(), &pool).await?;

                client.send_message(
                    &ChannelId::new(event.platform, &event.channel),
                    &format!("Connected to {}", channel_id.as_str())
                ).await?;

                Ok(())
            })
        },
        "Connect bot to another channel",
        "!connect <platform:channel>",
        "connect",
        PermissionLevel::Broadcaster,
    ))
}

pub fn config_command() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |event, _pool, state, client| {
            Box::pin(async move {
                let caller = ChannelId::new(event.platform, &event.channel);

                let cfg = {
                    let cfg = state.config.read().await;
                    cfg.get_channel_config(&caller)
                        .ok_or(BotError::ConfigMissing(caller.clone()))?
                        .clone()
                };

                let mut lines = Vec::new();

                lines.push(format!(
                    "📋 Channel config for {}",
                    caller.as_str()
                ));

                lines.push(format!(
                    "Queue: {}",
                    if cfg.open { "OPEN ✅" } else { "CLOSED 🚫" }
                ));

                lines.push(format!(
                    "Mode: {}",
                    if cfg.random_queue { "RAFFLE 🎲" } else { "QUEUE 📥" }
                ));

                lines.push(format!("Team size: {}", cfg.teamsize));
                lines.push(format!("Max queue size: {}", cfg.size));
                lines.push(format!("Prefix: {}", cfg.prefix));
                lines.push(format!("Runs today: {}", cfg.runs));

                match &cfg.queue_target {
                    QueueKey::Single(_) => {
                        lines.push("Shared queue: OFF".into());
                    }
                    QueueKey::Shared(owner) => {
                        lines.push("Shared queue: ON".into());
                        lines.push(format!("Owner: {}", owner.as_str()));
                    }
                }

                let reply = lines.join(" | ");

                client.send_message(&caller, &reply).await?;

                Ok(())
            })
        },
        "Show channel configuration",
        "!config",
        "config",
        PermissionLevel::Moderator,
    ))
}
