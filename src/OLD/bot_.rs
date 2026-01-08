

pub async fn grant_points_task(broadcaster_id: &str, pool: Arc<PgPool>, name: &str, oauth_token: &str, bot_id: &str) -> BotResult<()> {
    let active_viewers = Arc::new(Mutex::new(HashSet::new()));

    loop {
        if is_channel_live(name, &oauth_token, &bot_id).await? {
            let bot_config = BotConfig::load_from_db(&pool).await?;
            let config = bot_config.get_channel_config(name).unwrap();
            info!("{}", name);
            let points = config.points_config.points_per_time;
            let mut viewers = active_viewers.lock().await.clone(); // Get chatters

            // Fetch Lurkers from Twitch API
            let lurkers = fetch_lurkers(broadcaster_id, &oauth_token, &bot_id).await;
            info!("{:?}", lurkers);
            // Combine chatters and lurkers
            viewers.extend(lurkers);

            // Grant points
            for viewer in viewers.iter() {
                sqlx::query!(
                    "INSERT INTO currency (twitch_name, points, channel) VALUES ($1, $2, $3) 
                    ON CONFLICT(twitch_name, channel) DO UPDATE SET points = currency.points + $4",
                    viewer, points, broadcaster_id, points
                ).execute(&*pool).await?;
            }

            active_viewers.lock().await.clear(); // Reset chatters after granting points

            sleep(Duration::from_secs(config.points_config.interval)).await;
        }
        sleep(Duration::from_secs(10)).await;
    }
}
