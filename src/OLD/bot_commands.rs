
impl BotState {
    //Get total clears of raid of a player
    pub async fn total_raid_clears(&self, msg: &PrivmsgMessage, client: &TwitchClient, pool: &PgPool) -> BotResult<()> {
        let mut membership = MemberShip { id: String::new(), type_m: -1 };
        let words: Vec<&str> = msg.message_text.split_ascii_whitespace().collect();
        
        let reply = if words.len() > 1 {
            let mut name = words[1..].to_vec().join(" ").to_string();
            
            if let Some(name) = is_valid_bungie_name(&name) {
                match get_membershipid(&name, &self.x_api_key).await {
                    Ok(ship) => membership = ship,
                    Err(err) => client.say(msg.channel_login.clone(), format!("Error: {}", err)).await?,
                }
            } else {
                if name.starts_with("@") {
                    name.remove(0); 
                }
                if let Some(ship) = load_membership(&pool, name.clone()).await {
                    membership = ship;
                } else {
                    client.say(msg.channel_login.clone(), "Twitch user isn't registered in the database! Use their Bungie name!".to_string()).await?;
                    return Ok(());
                }
            }
            let clears = get_users_clears(membership.id, membership.type_m, self.x_api_key.clone()).await? as i32;
            format!("{} has total {} raid clears", name, clears)
        } else {
            if let Some(membership) = load_membership(&pool, msg.sender.name.clone()).await {
                let clears = get_users_clears(membership.id, membership.type_m, self.x_api_key.clone()).await? as i32;
                format!("You have total {} raid clears", clears)
            } else {
                format!("ItsBoshyTime {} is not registered to the database. Use !register <yourbungiename#0000>", &msg.sender.name)
            }
        };
        client.say(msg.channel_login.clone(), reply).await?;
        Ok(())
    }

    

    

pub async fn register_user(pool: &PgPool, twitch_name: &str, bungie_name: &str, bot_state: Arc<RwLock<BotState>>) -> Result<String, BotError> {
    let x_api_key = &bot_state.read().await.x_api_key;

    let reply = if let Some(bungie_name) = is_valid_bungie_name(bungie_name) {
        let new_user = TwitchUser {
            twitch_name: twitch_name.to_string(),
            bungie_name: bungie_name.to_string()
        };
        save_to_user_database(&pool, new_user, x_api_key.to_string()).await?
    } else {
        "❌ Invalid format of Bungie Name, This is correct format! -> bungiename#0000".to_string()
    };
    Ok(reply)
}
//if is/not in database
pub async fn bungiename(msg: PrivmsgMessage, client: TwitchClient, pool: &PgPool, twitch_name: String) -> BotResult<()> {
    let result = sqlx::query_scalar!(
        r#"SELECT bungie_name FROM twitchuser WHERE twitch_name = $1"#,
        twitch_name
    ).fetch_optional(pool).await?.ok_or(BotError::SqlxError(sqlx::Error::WorkerCrashed))?;

    let reply = match result {
        Some(bungie_name) => format!("Twitch Name: {twitch_name} | Bungie Name: {bungie_name}"),
        None => format!("{}, you are not registered ❌", twitch_name),
    };

    client.say(msg.channel_login, reply).await?;

    Ok(())
}

pub async fn unban_player_from_queue(msg: PrivmsgMessage, client: TwitchClient ,pool: &PgPool) -> BotResult<()> {
    let words: Vec<&str> = words(&msg);
    let reply = if words.len() < 2 {
        "Maybe try to add a Twitch name. Somebody deserves the unban. krapmaStare".to_string()
    } else {
        let twitch_name = words[1].strip_prefix("@").unwrap_or(words[1]).to_string();
        if let Some(membership) = load_membership(pool, twitch_name.clone()).await {
            let affected_rows = sqlx::query!(
                "DELETE FROM banlist WHERE membership_id = $1",
                membership.id
            ).execute(pool).await?.rows_affected();
            if affected_rows > 0 {
                format!("User {} has been unbanned from queue! They are free to enter again. krapmaHeart", twitch_name)
            } else {
                format!("User {} was not found in the banlist.", twitch_name)
            }
        } else {
            "Error".to_string()
        }
    };
    client.say(msg.channel_login, reply).await?;
    Ok(())
}

pub enum ModAction {
    Timeout,
    Ban, 
}

pub async fn mod_action_user_from_queue(msg: PrivmsgMessage, client: TwitchClient, pool: &PgPool, mod_action: ModAction) -> BotResult<()> {
    let words: Vec<&str> = words(&msg);
    if words.len() < 2 {
        client.say(msg.channel_login, "Usage: !mod_ban <twitch name> Optional(reason)".to_string()).await?;
        return Ok(());
    }
    let twitch_name = words[1].strip_prefix("@").unwrap_or(words[1]).to_owned();
    let reply = if let Some(membership) = load_membership(&pool, twitch_name.clone()).await {
        let reply = match mod_action {
            ModAction::Timeout => {
                let reason = if words.len() > 3 { words[3..].join(" ") } else { String::new() };
                let seconds: u32 = words[2].parse::<u32>().unwrap_or(0);
                let result = sqlx::query!(
                    "INSERT INTO banlist (membership_id, banned_until, reason)
                        VALUES ($1, NOW() + ($2 || ' seconds')::interval, $3)
                        ON CONFLICT (membership_id) DO UPDATE 
                        SET banned_until = NOW() + ($2 || ' seconds')::interval, reason = $3",
                    membership.id, seconds.to_string(), reason
                ).execute(pool).await;
                match result {
                    Ok(_) => {
                        format!("User {} has been timed out from entering queue for {} hours.", twitch_name, seconds/3600)
                    }
                    Err(_) => {
                        "An error occurred while timing out the user.".to_string()
                    }
                } 
            },
            ModAction::Ban => {
                let reason = if words.len() > 2 { words[2..].join(" ") } else { String::new() };
                let result = sqlx::query!(
                    "INSERT INTO banlist (membership_id, banned_until, reason) VALUES ($1, NULL, $2) ON CONFLICT(membership_id) DO UPDATE SET reason = $2",
                    membership.id, reason
                ).execute(pool).await;
                match result {
                    Ok(_) => {
                        format!("User {} has been banned from entering queue.", twitch_name)
                    }
                    Err(_) => {
                        "An error occurred while banning the user.".to_string()
                    }
                } 
            }
        };
        reply
    } else {
        "User has never entered queue, !mod_register them! -> !help mod_register".to_string()
    };
    client.say_in_reply_to(&msg, reply).await?;
    Ok(())
}

pub async fn add_remove_package(bot_state: Arc<RwLock<BotState>>, msg: PrivmsgMessage, client: TwitchClient, state: Package, pool: &PgPool) -> BotResult<()> {
    let words: Vec<&str> = words(&msg);

    if words.len() <= 1 {
        client.say(msg.channel_login.clone(), "❌ No mention of package!".to_string()).await?;
        return Ok(());
    }

    let package_name = words[1..].join(" ").trim().to_string();
    let channel_login = msg.channel_login.clone();

    let (new_packages, reply) = {
        let state_read = bot_state.read().await;
        let cfg = state_read
            .config
            .get_channel_config(&channel_login)
            .expect("channel config exists");
        let current_packages = cfg.packages.clone();

        match state {
            Package::Add => {
                if current_packages.contains(&package_name) {
                    (current_packages, "You already have this package 📦".to_string())
                } else {
                    let mut updated = current_packages;
                    updated.push(package_name.clone());
                    (updated, format!("📦 Package {} has been added", package_name))
                }
            }
            Package::Remove => {
                if current_packages
                    .iter()
                    .any(|p| p.eq_ignore_ascii_case(&package_name))
                {
                    let mut updated = current_packages;
                    updated.retain(|p| !p.eq_ignore_ascii_case(&package_name));
                    (updated, format!("📦 Package {} has been removed", package_name))
                } else {
                    (
                        current_packages,
                        format!(
                            "📦 Package {} does not exist or you don't have it activated",
                            package_name
                        ),
                    )
                }
            }
        }
    };

    {
        let mut state = bot_state.write().await;
        state.config.update_channel(pool, &channel_login, |cfg| {
            cfg.packages = new_packages.clone();
        }).await?;

        // Update dispatcher after config change
        let cfg_snapshot = state.config.clone();
        let dispatchers = Arc::clone(&state.dispatchers);

        drop(state); // release lock before await
        update_dispatcher_if_needed(&channel_login, &cfg_snapshot, pool, dispatchers).await?;
    }

    // Step 5: reply to user
    client.say_in_reply_to(&msg, reply).await?;

    Ok(())
}