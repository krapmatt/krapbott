use sqlx::PgPool;
use tokio::sync::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

use crate::bot::chat_event::chat_event::ChatEvent;
use crate::bot::commands::CommandMap;
use crate::bot::commands::CommandRegistry;
use crate::bot::commands::commands::BotResult;
use crate::bot::commands::commands::CommandT;
use crate::bot::db::ChannelId;
use crate::bot::db::aliases::fetch_aliases_from_db;
use crate::bot::handler::handler::ChatClient;
use crate::bot::handler::handler::UnifiedChatClient;
use crate::bot::permissions::permissions::has_permission;
use crate::bot::platforms::twitch::twitch::build_twitch_client;
use crate::bot::runtime::channel_runtime::ChannelRuntime;
use crate::bot::state::def::AliasConfig;
use crate::bot::state::def::AppState;
use crate::bot::state::def::BotError;
use crate::bot::state::def::ChannelConfig;




pub type DispatcherCache = HashMap<ChannelId, ChannelRuntime>;


pub async fn dispatch_message(commands: CommandMap, state: Arc<AppState>, event: &mut ChatEvent, pool: PgPool) -> BotResult<()> {
    let channel_id = ChannelId::new(event.platform.clone(), &event.channel);

    let prefix = {
        let cfg = state.config.read().await;
        cfg.channels
            .get(&channel_id)
            .map(|c| c.prefix.clone())
            .unwrap_or("!".into())
    };

    if !event.message.starts_with(&prefix) {
        return Ok(());
    }

    let cmd_name = event.message.trim_start_matches(&prefix).split_whitespace().next().unwrap_or("");
    let client = state.chat_client.clone();
    if let Some(cmd) = commands.get(cmd_name) {
        if has_permission(event, cmd.permission(), &state.secrets).await {
            cmd.execute(event.clone(), pool, state, client).await?;
        } else {
            client.send_message(&channel_id, &format!("You need to be {} to use this command", cmd.permission())).await?;
        }
    }

    Ok(())
}


impl CommandRegistry {
    pub async fn build_for_channel(&self, channel_id: &ChannelId, cfg: &ChannelConfig, alias_cfg: AliasConfig) -> CommandMap {
        let mut map: CommandMap = HashMap::new();

        for package in &cfg.packages {
            if let Some(group) = self.groups.get(&package.to_owned().to_ascii_lowercase()) {
                for reg in &group.commands {
                    let cmd = reg.command.clone();
                    let name = cmd.name().to_string();

                    // Disabled → skip entirely
                    if alias_cfg.disabled_commands.contains(&name) {
                        continue;
                    }

                    // Register default aliases
                    for alias in &reg.aliases {
                        if alias_cfg.removed_aliases.contains(alias) {
                            continue;
                        }

                        map.insert(alias.clone(), cmd.clone());
                    }
                }
            }
        }

        // 2️⃣ Apply custom aliases (override everything)
        for (alias, target) in &alias_cfg.aliases {
            if let Some(cmd) = map.get(target).cloned() {
                map.insert(alias.clone(), cmd);
            }
        }

        map
    }
}

pub async fn build_dispatcher_for_channel(channel_id: &ChannelId, state: Arc<AppState>, registry: &CommandRegistry) -> BotResult<CommandMap> {
    let (alias, config) = {
        let cfg = state.config.read().await;
        let alias = state.runtime.alias_config.read().await;
        (
            alias.clone(),
            cfg.channels.get(channel_id).cloned().ok_or_else(|| BotError::Custom("Config Missing".to_string()))?)
    };

    Ok(registry.build_for_channel(channel_id, &config, alias).await)
}