pub fn register_command() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |msg, client, pool, bot_state| {
            let fut = async move {
                let reply = if let Some((_, bungie_name)) = msg.message_text.split_once(' ') {
                    register_user(&pool, &msg.sender.name, bungie_name, bot_state).await?
                } else {
                    "Invalid command format! Use: !register bungiename#1234".to_string()
                };

                client.say(msg.channel_login, reply).await?;
                Ok(())
            };
            Box::pin(fut)
        },
        "Register your Bungie name with the bot",
        "!register bungiename#1234",
        "register",
        PermissionLevel::User,
    ))
}

pub fn mod_register_command() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |msg, client, pool, bot_state| {
            let fut = async move {
                let words: Vec<&str> = words(&msg);
                let reply = if words.len() >= 3 {
                    let mut twitch_name = words[1].to_string();
                    let bungie_name = &words[2..].join(" ");

                    if twitch_name.starts_with('@') {
                        twitch_name.remove(0);
                    }

                    register_user(&pool, &twitch_name, bungie_name, bot_state).await?
                } else {
                    "You are a mod... || Use: !mod_register twitchname bungiename".to_string()
                };

                client.say(msg.channel_login, reply).await?;
                Ok(())
            };
            Box::pin(fut)
        },
        "Manually register a user as a mod",
        "!mod_register twitchname bungiename",
        "mod_register",
        PermissionLevel::Moderator,
    ))
}


