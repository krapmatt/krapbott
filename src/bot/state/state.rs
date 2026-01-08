use std::collections::{HashMap, HashSet};

use shuttle_runtime::SecretStore;

use crate::bot::{chat_event::chat_event::Platform, commands::queue::logic::QueueKey, db::ChannelId, state::def::{AliasConfig, BotConfig, BotError, BotSecrets, ChannelConfig, Giveaway, PointsConfig}};


impl PointsConfig {
    fn new() -> PointsConfig {
        PointsConfig { name: "Points".to_string(), interval: 600, points_per_time: 10 }
    }
}

impl ChannelConfig {
    pub fn new(channel_id: ChannelId) -> Self {
        ChannelConfig { 
            open: false,
            queue_target: QueueKey::Single(channel_id),
            len: 1, 
            teamsize: 1, 
            packages: vec!["moderation".to_string()], 
            runs: 0,   
            prefix: "!".to_string(), 
            random_queue: false, 
            giveaway: Giveaway { duration: 100, max_tickets: 100, ticket_cost: 100, active: false }, 
            points_config: PointsConfig { name: "Points".to_string(), interval: 10000, points_per_time: 0 },
        }
    }
}

#[derive(Debug)]
pub enum SecretError {
    Missing(&'static str),
}

impl BotSecrets {
    pub fn from_shuttle(store: &SecretStore) -> Result<Self, SecretError> {
        let oauth_token_bot = store
            .get("TWITCH_OAUTH_TOKEN_BOTT")
            .ok_or(SecretError::Missing("TWITCH_OAUTH_TOKEN_BOTT"))?
            .to_string();

        let bot_id = store
            .get("TWITCH_CLIENT_ID")
            .ok_or(SecretError::Missing("TWITCH_CLIENT_ID"))?
            .to_string();

        let x_api_key = store
            .get("XAPIKEY")
            .ok_or(SecretError::Missing("XAPIKEY"))?
            .to_string();
        let client_secret = store.get("CLIENT_SECRET")
            .ok_or(SecretError::Missing("CLIENT_SECRET"))?
            .to_string();

        Ok(Self {
            oauth_token_bot,
            bot_id,
            x_api_key,
            client_secret
        })
    }
}
impl BotConfig {
    pub fn new() -> Self {
        let mut hash = HashMap::new();
        hash.insert(ChannelId::new(Platform::Twitch, "krapmatt".to_string()), ChannelConfig {open: true, len: 1, teamsize: 2, packages: vec!["queue".to_string()], runs: 0, queue_target: QueueKey::Single(ChannelId::new(Platform::Twitch, "krapmatt".to_string())), prefix: "!".to_string(), random_queue: false, giveaway: Giveaway { duration: 1000, max_tickets: 10, ticket_cost: 10, active: false }, points_config: PointsConfig { name: "Dirt".to_string(), interval: 1000, points_per_time: 15 }});
        BotConfig {
            channels: hash,
        }
    }



    /// Load all channel configs from the database
    /*pub async fn load_from_db(pool: &PgPool) -> BotResult<Self> {
        let rows = sqlx::query!("SELECT channel_id, config_json FROM bot_config").fetch_all(pool).await?;

        let mut channels = HashMap::new();
        for row in rows {
            let config: ChannelConfig = serde_json::from_value(row.config_json)
                .map_err(|e| BotError::Custom(format!("Failed to parse config for channel {}: {:?}", row.channel_id, e)))?;
            channels.insert(row.channel_id.clone(), config);
        }

        Ok(BotConfig { channels })
    }

    pub async fn save_channel(&self, pool: &PgPool, channel_id: &ChannelId) -> BotResult<()> {
        if let Some(channel_config) = self.channels.get(channel_id) {
            let config_json = serde_json::to_value(channel_config)
                .map_err(|e| BotError::Custom(format!("Failed to serialize config: {:?}", e)))?;
            sqlx::query!(
                r#"
                INSERT INTO bot_config (channel_id, config_json)
                VALUES ($1, $2)
                ON CONFLICT (channel_id) DO UPDATE
                SET config_json = $2
                "#,
                channel_id, config_json
            ).execute(pool).await?;
        }
        Ok(())
    }

    pub async fn save_all(&self, pool: &PgPool) -> BotResult<()> {
        for (channel_id, channel_config) in &self.channels {
            let config_json = serde_json::to_value(channel_config)?;
            
            sqlx::query!(
                r#"
                INSERT INTO bot_config (channel_id, config_json)
                VALUES ($1, $2)
                ON CONFLICT (channel_id)
                DO UPDATE SET config_json = $2
                "#,
                channel_id,
                config_json
            )
            .execute(pool)
            .await?;
        }
        Ok(())
    } */

    /*pub async fn update_channel<F>(&mut self, pool: &PgPool, channel_id: &str, mutator: F) -> BotResult<()> where F: FnOnce(&mut ChannelConfig) {
        let cfg = self.channels.entry(channel_id.to_string()).or_insert_with(|| ChannelConfig::new(channel_id.to_string()));

        mutator(cfg);
        self.save_channel(pool, channel_id).await?;
        Ok(())
    }*/

    pub fn get_channel_config(&self, channel_id: &ChannelId) -> Option<&ChannelConfig> {
        self.channels.get(channel_id)
    }

    pub fn get_channel_config_mut(&mut self, key: ChannelId) -> &mut ChannelConfig {
        self.channels.entry(key.clone()).or_insert_with(|| ChannelConfig::new(key))
    }

    pub fn is_group_allowed(&self, channel_id: &ChannelId, group_name: &str) -> bool {
        if let Some(channel_config) = self.channels.get(channel_id) {
            channel_config.packages.contains(&group_name.to_string())
        } else {
            false
        }
    }
}


impl Giveaway {
 pub fn new() -> Self {
    Self { duration: 3600, max_tickets: 100, ticket_cost: 15, active: false }
 }
}

impl From<()> for BotError {
    fn from(_: ()) -> Self {
        BotError::Custom("unit error".to_string())
    }
}

impl AliasConfig {
    pub fn get_aliases(&self, name: &str) -> Vec<String> {
        self.aliases.iter().filter_map(|(key, val)| if val == name { Some(key.to_owned()) } else { None }).collect()
    }
    pub fn get_removed_aliases(&self, name: &str) -> bool {
        self.removed_aliases.get(name).is_some()
    }
}



