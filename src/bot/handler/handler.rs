use std::{sync::Arc};

use sqlx::PgPool;
use tracing::info;

use crate::api::kick_api::send_kick_message;
use crate::bot::{chat_event::chat_event::{ChatEvent, Platform}, commands::{CommandRegistry, commands::BotResult}, db::ChannelId, dispatcher::dispatcher::{dispatch_message}, platforms::{kick::event_loop::spawn_kick_channel, twitch::twitch::TwitchClient}, runtime::channel_lifecycle::start_channels_from_config, state::def::AppState};
use tracing::warn;
use kick_rust::KickClient;
use crate::bot::runtime::channel_lifecycle::start_channel;

pub trait ChatClient: Send + Sync {
    async fn send_message(&self, channel: &ChannelId, message: &str) -> BotResult<()>;
}

pub struct UnifiedChatClient {
    pub twitch: TwitchClient,
    pub kick: KickClient,
    pub kick_tx: tokio::sync::mpsc::UnboundedSender<crate::bot::chat_event::chat_event::ChatEvent>,
    pub kick_access_token: Option<String>,
}

impl ChatClient for UnifiedChatClient {
    async fn send_message(&self, channel: &ChannelId, message: &str) -> BotResult<()> {
        match channel.platform() {
            Platform::Twitch => {
                self.twitch.say(channel.channel().to_owned(), message.to_owned()).await?;
            }

            Platform::Kick => {
                let Some(token) = self.kick_access_token.as_deref() else {
                    return Err(crate::bot::state::def::BotError::Custom(
                        "KICK_ACCESS_TOKEN not set".to_string(),
                    ));
                };
                send_kick_message(channel.channel(), message, token).await?;
            }

            Platform::Obs => todo!(), /*{
                self.obs
                    .broadcast(message)
                    .await?;
            }*/
        }

        Ok(())
    }
}
impl UnifiedChatClient {
    pub async fn join_channel(&self, channel: &ChannelId) -> BotResult<()> {
        match channel.platform() {
            Platform::Twitch => {
                self.twitch.join(channel.channel().to_string())?;
            }
            Platform::Kick => {
                spawn_kick_channel(channel.channel().to_string(), self.kick_tx.clone()).await?;
            }
            Platform::Obs => {}
        }
        Ok(())
    }

    pub async fn leave_channel(&self, channel: &ChannelId) -> BotResult<()> {
        match channel.platform() {
            Platform::Twitch => {
                self.twitch.part(channel.channel().to_string());
            }
            Platform::Kick => {
                warn!("Kick leave not implemented: {}", channel.channel());
            }
            Platform::Obs => {}
        }
        Ok(())
    }
}

pub async fn handle_event(event: &mut ChatEvent, pool: PgPool, state: Arc<AppState>) -> BotResult<()> {
    let channel_id = ChannelId::new(event.platform.clone(), &event.channel);
    info!("im here");
     let dispatcher = {
        let cache = state.runtime.dispatchers.read().await;

        match cache.get(&channel_id) {
            Some(runtime) => runtime.dispatcher.clone(),
            None => {
                drop(cache);
                start_channel(channel_id.clone(), state.clone(), &pool).await?;
                return Ok(());
            }
        }
    };

    dispatch_message(dispatcher, state, event, pool).await
}

pub async fn init_bot_runtime(state: Arc<AppState>, pool: &PgPool) -> BotResult<()> {
    start_channels_from_config(state, pool).await?;
    Ok(())
}
