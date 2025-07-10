use std::{borrow::BorrowMut, sync::Arc, time::Duration};

use crate::{bot_commands::send_message, commands::{oldcommands::FnCommand, traits::CommandT, words}, models::{AnnouncementState, PermissionLevel}};
pub fn add_announcement_command() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |msg, client, pool, _bot_state| {
            let fut = async move {
                let channel = msg.channel_id().to_string();
                let msg_vec: Vec<&str> = msg.text().split_ascii_whitespace().collect();

                let reply = if msg_vec.len() >= 4 {
                    let state = msg_vec[1].to_lowercase();
                    let name = msg_vec[2].to_string();
                    let announcement = msg_vec[3..].join(" ");

                    sqlx::query!(
                        "INSERT INTO announcements (name, announcement, channel, state)
                         VALUES (?, ?, ?, ?)
                         ON CONFLICT(name, channel)
                         DO UPDATE SET announcement = excluded.announcement",
                        name, announcement, channel, state
                    ).execute(&pool).await?;

                    format!("✅ Announcement '{}' has been added!", name)
                } else {
                    "❌ Invalid usage // Use: <state: Active/ActivityName> <name> <Message>".to_string()
                };

                send_message(&msg, client.lock().await.borrow_mut(), &reply).await?;
                Ok(())
            };
            Box::pin(fut)
        },
        "Add an announcement",
        "!add_announcement <state> <name> <message>",
        "Add Announcement",
        PermissionLevel::Moderator,
    ))
}

pub fn remove_announcement_command() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |msg, client, pool, _bot_state| {
            let fut = async move {
                let msg_vec: Vec<&str> = msg.text().split_ascii_whitespace().collect();
                let channel_id = msg.channel_id().to_string();

                let reply = if msg_vec.len() <= 1 {
                    "❌ Invalid usage".to_string()
                } else if msg_vec.len() == 2 {
                    let name = msg_vec[1].to_string();

                    let result = sqlx::query!(
                        "DELETE FROM announcements WHERE name = ? AND channel = ?",
                        name,
                        channel_id
                    )
                    .execute(&pool)
                    .await?;

                    if result.rows_affected() > 0 {
                        "✅ Announcement has been removed!".to_string()
                    } else {
                        "⚠️ No announcement found with that name.".to_string()
                    }
                } else {
                    "❌ Invalid usage".to_string()
                };

                send_message(&msg, client.lock().await.borrow_mut(), &reply).await?;
                Ok(())
            };
            Box::pin(fut)
        },
        "Remove an announcement",
        "!remove_announcement <name>",
        "Remove Announcement",
        PermissionLevel::Moderator,
    ))
}

pub fn play_announcement_command() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |msg, _client, pool, bot_state| {
            let fut = async move {
                let msg_vec: Vec<&str> = words(&msg);
                if msg_vec.len() != 2 {
                    return Ok(());
                }

                let channel_id = msg.channel_id().to_string();
                let name = msg_vec[1].to_string();

                let result = sqlx::query!(
                    "SELECT announcement FROM announcements WHERE name = ? AND channel = ?",
                    name,
                    channel_id
                )
                .fetch_optional(&pool)
                .await?;

                if let Some(row) = result {
                    let announ = row.announcement;
                    let bot_state = bot_state.read().await;
                    crate::twitch_api::announcement(msg.channel_id(), "1091219021", &bot_state.oauth_token_bot, bot_state.bot_id.clone(), announ).await?;
                }
                Ok(())
            };
            Box::pin(fut)
        },
        "Play an announcement",
        "!play_announcement <name>",
        "Play Announcement",
        PermissionLevel::Moderator,
    ))
}

pub fn announcement_freq_command() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |msg, client, _pool, bot_state| {
            let fut = async move {
                let msg_vec: Vec<&str> = words(&msg);

                if msg_vec.len() == 2 {
                    let mut bot_state = bot_state.write().await;
                    if let Ok(seconds) = msg_vec[1].parse::<u64>() {
                        bot_state
                            .config
                            .get_channel_config_mut(msg.channel())
                            .announcement_config
                            .interval = Duration::from_secs(seconds);
                        bot_state.config.save_config();

                        send_message(&msg, client.lock().await.borrow_mut(), "✅ Frequency has been updated.").await?;
                    }
                }

                Ok(())
            };
            Box::pin(fut)
        },
        "Change interval of announcement frequency",
        "!announcement_interval <seconds>",
        "Announcement Interval",
        PermissionLevel::Moderator,
    ))
}

pub fn announcement_state_command() -> Arc<dyn CommandT> {
    Arc::new(FnCommand::new(
        |msg, client, _pool, bot_state| {
            let fut = async move {
                let msg_vec: Vec<&str> = msg.text().split_ascii_whitespace().collect();

                if msg_vec.len() == 2 {
                    let mut bot_state = bot_state.write().await;
                    let state_input = msg_vec[1].to_lowercase();

                    let state = match state_input.as_str() {
                        "paused" => AnnouncementState::Paused,
                        "active" => AnnouncementState::Active,
                        custom => AnnouncementState::Custom(custom.to_string()),
                    };

                    bot_state.config.get_channel_config_mut(msg.channel()).announcement_config.state = state.clone();
                    bot_state.config.save_config();

                    send_message(&msg, client.lock().await.borrow_mut(), &format!("✅ Announcement state set to: {:?}", state)).await?;
                }
                Ok(())
            };
            Box::pin(fut)
        },
        "Change announcement state (Paused, Active, or ActivityName)",
        "!announcement_state <state>",
        "Announcement State",
        PermissionLevel::Moderator,
    ))
}