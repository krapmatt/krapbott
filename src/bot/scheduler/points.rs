async pub fn start_points_task(channel_id: ChannelId, state: Arc<AppState>, pool: Arc<PgPool>) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            if let Err(e) = tick_points(&channel_id, &state, &pool).await {
                tracing::error!(
                    "Points task error for {}: {:?}",
                    channel_id.as_str(),
                    e
                );
            }

            // fallback sleep to avoid tight loop
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    })
}

async fn tick_points(
    channel_id: &ChannelId,
    state: &AppState,
    pool: &PgPool,
) -> BotResult<()> {
    // 1️⃣ Load channel config
    let cfg = {
        let cfg = state.config.read().await;
        cfg.get_channel_config(channel_id)
            .cloned()
            .ok_or_else(|| BotError::Custom("Missing channel config".into()))?
    };

    // 2️⃣ Check live status (platform-specific)
    let is_live = match channel_id.platform() {
        Platform::Twitch => {
            is_twitch_channel_live(
                channel_id.channel(),
                &state.secrets.oauth_token_bot,
                &state.secrets.bot_id,
            )
            .await?
        }
        _ => false, // future platforms
    };

    if !is_live {
        return Ok(());
    }

    // 3️⃣ Fetch active viewers
    let viewers = fetch_active_viewers(channel_id, state).await?;

    if viewers.is_empty() {
        return Ok(());
    }

    // 4️⃣ Grant points
    grant_points(
        pool,
        channel_id,
        &viewers,
        cfg.points_config.points_per_time,
    )
    .await?;

    // 5️⃣ Sleep for interval
    tokio::time::sleep(Duration::from_secs(cfg.points_config.interval)).await;

    Ok(())
}

async fn fetch_active_viewers(
    channel_id: &ChannelId,
    state: &AppState,
) -> BotResult<HashSet<String>> {
    match channel_id.platform() {
        Platform::Twitch => {
            fetch_twitch_viewers(
                channel_id.channel(),
                &state.secrets.oauth_token_bot,
                &state.secrets.bot_id,
            )
            .await
        }
        _ => Ok(HashSet::new()),
    }
}

async fn fetch_twitch_viewers(
    channel_login: &str,
    oauth_token: &str,
    client_id: &str,
) -> BotResult<HashSet<String>> {
    let mut viewers = HashSet::new();

    let chatters = fetch_chatters(channel_login).await?;
    let lurkers = fetch_lurkers(channel_login, oauth_token, client_id).await?;

    viewers.extend(chatters);
    viewers.extend(lurkers);

    Ok(viewers)
}

async fn grant_points(
    pool: &PgPool,
    channel_id: &ChannelId,
    viewers: &HashSet<String>,
    amount: i32,
) -> BotResult<()> {
    for viewer in viewers {
        sqlx::query!(
            r#"
            INSERT INTO krapbott_v2.currency (user_name, channel_id, points)
            VALUES ($1, $2, $3)
            ON CONFLICT (user_name, channel_id)
            DO UPDATE SET points = currency.points + $3
            "#,
            viewer,
            channel_id.as_str(),
            amount
        )
        .execute(pool)
        .await?;
    }

    Ok(())
}