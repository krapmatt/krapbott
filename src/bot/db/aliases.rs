use sqlx::PgPool;

use crate::bot::{commands::commands::BotResult, db::ChannelId, state::def::AliasConfig};

pub const COMMAND_ALIASES: &str = "
    CREATE TABLE IF NOT EXISTS krapbott_v2.command_aliases (
        channel TEXT NOT NULL,
        alias TEXT NOT NULL,
        command TEXT NOT NULL,
        PRIMARY KEY (channel, alias)
    );
";

pub const COMMAND_DISABLED: &str = "
    CREATE TABLE IF NOT EXISTS krapbott_v2.command_disabled (
        channel TEXT NOT NULL,
        command TEXT NOT NULL,
        PRIMARY KEY (channel, command)
    );
";

pub const COMMAND_ALIASES_REMOVALS: &str = "
    CREATE TABLE IF NOT EXISTS krapbott_v2.command_alias_removals (
        channel TEXT NOT NULL,
        alias TEXT NOT NULL,
        PRIMARY KEY (channel, alias)
    ); 
";


pub async fn fetch_aliases_from_db(channel: &ChannelId, pool: &PgPool) -> BotResult<AliasConfig> {
    let alias_rows = sqlx::query!(
        "SELECT alias, command FROM krapbott_v2.command_aliases WHERE channel = $1",
        channel.as_str()
    ).fetch_all(pool).await?;

    let disabled = sqlx::query!(
        "SELECT command FROM krapbott_v2.command_disabled WHERE channel = $1",
        channel.as_str()
    ).fetch_all(pool).await?;

    let removed = sqlx::query!(
        "SELECT alias FROM krapbott_v2.command_alias_removals WHERE channel = $1",
        channel.as_str()
    ).fetch_all(pool).await?;

    Ok(AliasConfig {
        aliases: alias_rows
            .into_iter()
            .map(|r| (r.alias.to_lowercase(), r.command.to_lowercase()))
            .collect(),
        disabled_commands: disabled.into_iter().map(|r| r.command.to_lowercase()).collect(),
        removed_aliases: removed.into_iter().map(|r| r.alias.to_lowercase()).collect(),
    })
}