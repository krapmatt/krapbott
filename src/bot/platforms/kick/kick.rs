use crate::bot::commands::commands::BotResult;


pub async fn run_kick_adapter() -> BotResult<()> {
    // TODO: připoj Kick websocket a převod zpráv
    // Například:
    // let kick_msg = KickMessage { user: "...", text: "..." };
    // let event = ChatEvent {
    //     platform: Platform::Kick,
    //     channel: channel.clone(),
    //     user: Some(ChatUser{...}),
    //     message: kick_msg.text.clone(),
    //     is_first: true,
    //     raw: Some(Box::new(kick_msg)),
    // };
    // handle_chat_event(event, Arc::clone(&ctx)).await?;
    Ok(())
}