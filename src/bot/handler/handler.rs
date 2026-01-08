use std::{sync::Arc};

use sqlx::PgPool;
use tracing::info;

use crate::bot::{chat_event::chat_event::{ChatEvent, Platform}, commands::{CommandRegistry, commands::BotResult}, db::ChannelId, dispatcher::dispatcher::{dispatch_message}, platforms::twitch::twitch::TwitchClient, runtime::channel_lifecycle::start_channels_from_config, state::def::AppState};
use crate::bot::runtime::channel_lifecycle::start_channel;

pub trait ChatClient: Send + Sync {
    async fn send_message(&self, channel: &ChannelId, message: &str) -> BotResult<()>;
}

pub struct UnifiedChatClient {
    pub twitch: TwitchClient,
}

impl ChatClient for UnifiedChatClient {
    async fn send_message(&self, channel: &ChannelId, message: &str) -> BotResult<()> {
        match channel.platform() {
            Platform::Twitch => {
                self.twitch.say(channel.channel().to_owned(), message.to_owned()).await?;
            }

            Platform::Kick => todo!(), /*{
                self.kick
                    .send(channel.channel(), message)
                    .await?;
            }*/

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
                // future
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
            Platform::Kick => {}
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