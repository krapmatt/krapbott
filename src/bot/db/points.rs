use sqlx::PgPool;

use crate::bot::commands::commands::BotResult;

pub const CURRENCY_TABLE: &str = "
CREATE TABLE IF NOT EXISTS krapbott_v2.currency (
    user_id TEXT NOT NULL REFERENCES users(id),
    channel TEXT NOT NULL,
    points INTEGER DEFAULT 0,
    PRIMARY KEY (user_id, channel)
);
";

pub async fn migrate_old_currency(pool: &PgPool) -> BotResult<()> {
    // načíst starou tabulku currency
    // pro každého twitch_name -> získat user_id z nové users tabulky
    // vložit body do nové tabulky currency
    Ok(())
}