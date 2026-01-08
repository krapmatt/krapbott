pub async fn get_aliases_handler(pool: Arc<PgPool>, cookies: Option<String>) -> Result<impl warp::Reply, warp::Rejection> {
    if let Some(twitch_name) = get_twitch_name_from_cookie(cookies, &*pool).await {
        let channel = format!("{twitch_name}").to_ascii_lowercase();
        let rows = sqlx::query!(
            "SELECT alias, command FROM command_aliases WHERE channel = $1",
            channel
        ).fetch_all(&*pool).await.map_err(|_| warp::reject::not_found())?;

        let aliases: HashMap<String, String> = rows
            .into_iter()
            .map(|row| (row.alias, row.command))
            .collect();

        return Ok(warp::reply::json(&aliases));
    }
    
    Err(warp::reject())
}

#[derive(Deserialize, Debug)]
pub struct AliasUpdate {
    alias: String,
    command: String,
}

pub async fn set_alias_handler(pool: Arc<PgPool>, cookies: Option<String>, bot_state: Arc<RwLock<BotState>>, body: AliasUpdate) -> Result<impl warp::Reply, warp::Rejection> {
    if let Some(twitch_name) = get_twitch_name_from_cookie(cookies, &*pool).await {
        let channel = format!("{twitch_name}").to_ascii_lowercase();
        let alias = body.alias.to_ascii_lowercase();
        let command = body.command.to_ascii_lowercase();

        sqlx::query!(
            "INSERT INTO command_aliases (channel, alias, command) 
             VALUES ($1, $2, $3)
             ON CONFLICT (channel, alias) 
             DO UPDATE SET command = EXCLUDED.command",
            channel, alias, command
        ).execute(&*pool).await.map_err(|_| warp::reject::reject())?;
        let bot_state = bot_state.write().await;
        update_dispatcher_if_needed(&channel, &bot_state.config, &pool, Arc::clone(&bot_state.dispatchers)).await
            .map_err(|_| warp::reject())?;
        return Ok(warp::reply::with_status("Alias updated", warp::http::StatusCode::OK));
    }
    Err(warp::reject())
}

#[derive(Deserialize)]
pub struct AliasDelete {
    alias: String,
}

pub async fn delete_alias_handler(pool: Arc<PgPool>, cookies: Option<String>, bot_state: Arc<RwLock<BotState>>, body: AliasDelete) -> Result<impl warp::Reply, warp::Rejection> {
    if let Some(twitch_name) = get_twitch_name_from_cookie(cookies, &*pool).await {
        let channel = format!("{twitch_name}").to_ascii_lowercase();
        let alias = body.alias.to_ascii_lowercase();
        sqlx::query!(
            "DELETE FROM command_aliases WHERE channel = $1 AND alias = $2",
            channel, alias
        ).execute(&*pool).await.map_err(|_| warp::reject::reject())?;
        let bot_state = bot_state.write().await;
        update_dispatcher_if_needed(&channel, &bot_state.config, &pool, Arc::clone(&bot_state.dispatchers)).await
            .map_err(|_| warp::reject())?;
        return Ok(warp::reply::with_status("Alias removed", warp::http::StatusCode::OK));
    }
    Err(warp::reject())
}

#[derive(Serialize)]
pub struct CommandAliasView {
    pub command: String,
    pub default_aliases: Vec<String>,
    pub removed_default_aliases: Vec<String>,
    pub default_disabled: bool,
    pub custom_aliases: Vec<String>,
}

pub async fn get_all_command_aliases(cookies: Option<String>, pool: Arc<PgPool>) -> Result<impl warp::Reply, warp::Rejection> {
    if let Some(twitch_name) = get_twitch_name_from_cookie(cookies, &*pool).await {
        let channel = format!("{twitch_name}").to_ascii_lowercase();
        // Get default alias removals
        let removed_aliases: HashSet<String> = query!(
            "SELECT alias FROM command_alias_removals WHERE channel = $1",
            channel
        ).fetch_all(&*pool).await.map_err(|_| warp::reject::reject())?.into_iter().map(|row| row.alias.to_lowercase()).collect();

        // Get disabled commands
        let disabled_commands: HashSet<String> = query!(
            "SELECT command FROM command_disabled WHERE channel = $1",
            channel
        ).fetch_all(&*pool).await.map_err(|_| warp::reject::reject())?.into_iter().map(|row| row.command.to_lowercase()).collect();

        // Get custom aliases
        let custom_aliases: Vec<(String, String)> = query!(
            "SELECT alias, command FROM command_aliases WHERE channel = $1",
            channel
        ).fetch_all(&*pool).await.map_err(|_| warp::reject::reject())?.into_iter().map(|row| (row.command.to_lowercase(), row.alias.to_lowercase())).collect();

        let mut commands: HashMap<String, CommandAliasView> = HashMap::new();

        for (_pkg, group) in COMMAND_GROUPS.iter() {
            for reg in &group.commands {
                let cmd = reg.command.name().to_string();
                let mut active = Vec::new();
                let mut removed = Vec::new();

                for alias in &reg.aliases {
                    let lower = alias.to_lowercase();
                    if removed_aliases.contains(&lower) {
                        removed.push(lower);
                    } else {
                        active.push(lower);
                    }
                }

                let custom = custom_aliases.iter()
                    .filter_map(|(c, a)| if c == &cmd.to_lowercase() { Some(a.clone()) } else { None })
                    .collect();

                commands.insert(cmd.clone(), CommandAliasView {
                    command: cmd.clone(),
                    default_aliases: active,
                    removed_default_aliases: removed,
                    default_disabled: disabled_commands.contains(&cmd),
                    custom_aliases: custom,
                });
            }
        }

        let mut result: Vec<_> = commands.into_values().collect();
        result.sort_by(|a, b| a.command.cmp(&b.command));
        return Ok(warp::reply::json(&result));
    }
    Err(warp::reject())
}

#[derive(Deserialize)]
pub struct DisableToggleRequest {
    command: String,
}

pub async fn toggle_default_command_handler(pool: Arc<PgPool>, cookies: Option<String>, bot_state: Arc<RwLock<BotState>>, body: DisableToggleRequest) -> Result<impl warp::Reply, warp::Rejection> {
    if let Some(twitch_name) = get_twitch_name_from_cookie(cookies, &*pool).await {
        let channel = format!("{twitch_name}").to_ascii_lowercase();
        let command = body.command.to_ascii_lowercase();

        let existing = sqlx::query!(
            "SELECT * FROM command_disabled WHERE channel = $1 AND command = $2",
            channel, command
        ).fetch_optional(&*pool).await.map_err(|_| warp::reject())?;

        if existing.is_some() {
            sqlx::query!(
                "DELETE FROM command_disabled WHERE channel = $1 AND command = $2",
                channel, command
            ).execute(&*pool).await.map_err(|_| warp::reject())?;
        } else {
            sqlx::query!(
                "INSERT INTO command_disabled (channel, command) VALUES ($1, $2)",
                channel, command
            ).execute(&*pool).await.map_err(|_| warp::reject())?;
        }
        let bot_state = bot_state.write().await;
        update_dispatcher_if_needed(&channel, &bot_state.config, &pool, Arc::clone(&bot_state.dispatchers)).await
            .map_err(|_| warp::reject())?;

        return Ok(warp::reply::with_status("Toggle success", warp::http::StatusCode::OK));
    }
    Err(warp::reject())
}

#[derive(Deserialize)]
pub struct DefaultAliasRemoval {
    command: String,
    alias: String,
}

pub async fn remove_default_alias_handler(pool: Arc<PgPool>, cookies: Option<String>, bot_state: Arc<RwLock<BotState>>, body: DefaultAliasRemoval) -> Result<impl warp::Reply, warp::Rejection> {
    if let Some(twitch_name) = get_twitch_name_from_cookie(cookies, &*pool).await {
        let channel = format!("{twitch_name}").to_ascii_lowercase();
        let alias = body.alias.to_ascii_lowercase();

        sqlx::query!(
            "INSERT INTO command_alias_removals (channel, alias) 
             VALUES ($1, $2)
             ON CONFLICT (channel, alias) DO NOTHING",
            channel,alias
        ).execute(&*pool).await.map_err(|_| warp::reject())?;
        let bot_state = bot_state.write().await;
        update_dispatcher_if_needed(&channel, &bot_state.config, &pool, Arc::clone(&bot_state.dispatchers))
            .await
            .map_err(|_| warp::reject())?;

        Ok(warp::reply::with_status("Alias removed", warp::http::StatusCode::OK))
    } else {
        Err(warp::reject())
    }
}

pub async fn restore_default_alias_handler(pool: Arc<PgPool>, cookies: Option<String>, bot_state: Arc<RwLock<BotState>>, body: DefaultAliasRemoval,) -> Result<impl warp::Reply, warp::Rejection> {
    if let Some(twitch_name) = get_twitch_name_from_cookie(cookies, &*pool).await {
        let channel = format!("{twitch_name}").to_ascii_lowercase();
        let alias = body.alias.to_ascii_lowercase();

        sqlx::query!(
            "DELETE FROM command_alias_removals WHERE channel = $1 AND alias = $2",
            channel, alias
        ).execute(&*pool).await.map_err(|_| warp::reject())?;
        let bot_state = bot_state.write().await;
        update_dispatcher_if_needed(&channel, &bot_state.config, &pool, bot_state.dispatchers.clone())
            .await
            .map_err(|_| warp::reject())?;

        Ok(warp::reply::with_status("Alias restored", warp::http::StatusCode::OK))
    } else {
        Err(warp::reject())
    }
}