use std::{str::FromStr, sync::Arc};

use once_cell::sync::Lazy;

use crate::{bot::{commands::{CommandGroup, commands::{CommandT, FnCommand}, moderation::connect_channel, CommandRegistration}, db::{ChannelId, config::save_channel_config}, handler::handler::ChatClient, permissions::permissions::PermissionLevel, runtime::channel_lifecycle::reload_channel, state::def::BotError}, cmd};
pub static MODERATION_COMMANDS: Lazy<Arc<CommandGroup>> = Lazy::new(|| {
    Arc::new(CommandGroup {
        name: "moderation".into(),
        commands: vec![
            cmd!(alias_command(), "alias"),
            cmd!(add_package_command(), "add_package"),
            cmd!(connect_command(), "connect"),
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
                    client
                        .send_message(
                            &caller,
                            "Usage: !alias add <alias> <command> | !alias remove <alias>",
                        )
                        .await?;
                    return Ok(());
                }

                let action = args[1].to_lowercase();

                match action.as_str() {
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

                        reload_channel(caller.clone(), state.clone(), &pool).await?;

                        client
                            .send_message(
                                &caller,
                                &format!("Added alias '{}' for command '{}'", alias, command),
                            )
                            .await?;
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

                        reload_channel(caller.clone(), state.clone(), &pool).await?;

                        client.send_message(&caller, &format!("Removed alias '{}'", alias)).await?;
                    }

                    _ => {
                        client
                            .send_message(
                                &caller,
                                "Invalid syntax. Use: !alias add <alias> <command> | !alias remove <alias>",
                            )
                            .await?;
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
                let Some(user) = &event.user else { return Ok(()); };

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
                        "Usage: !connect twitch:channel"
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