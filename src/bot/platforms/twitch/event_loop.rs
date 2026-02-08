use std::sync::Arc;

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tracing::info;
use twitch_irc::message::ServerMessage;

use crate::bot::{chat_event::chat_event::{ChatEvent, Platform}, commands::commands::BotResult, platforms::twitch::twitch::{build_twitch_client, map_privmsg}, state::def::AppState};



pub async fn run_twitch_loop(mut incoming: UnboundedReceiver<ServerMessage>, tx: UnboundedSender<ChatEvent>, state: Arc<AppState>) -> BotResult<()> {


    // Join all Twitch channels from config
    {
        let config = state.config.read().await;
        for channel_id in config.channels.keys() {
            if channel_id.platform() == Platform::Twitch {
                state.chat_client.twitch.join(channel_id.channel().to_string())?;
                info!("Joined Twitch channel: {}", channel_id.channel());
            }
        }
    }

    while let Some(msg) = incoming.recv().await {
        if let ServerMessage::Privmsg(privmsg) = msg {
            // Ignore forwarded shared-chat messages
            info!("Received Twitch message in channel {}: {}", privmsg.channel_login, privmsg.message_text);
            let room_id = privmsg.source.tags.0.get("room-id");
            let source_room_id = privmsg.source.tags.0.get("source-room-id");
            if let (Some(rid), Some(srid)) = (room_id, source_room_id) {
                if rid != srid {
                    continue;
                }
            }
            let event = map_privmsg(&privmsg);
            let _ = tx.send(event);
            
        }
    }

    Ok(())
}