use core::fmt;
use std::{error::Error, str::FromStr};

use serde::{Deserialize, Serialize};
use sqlx::{Decode, Encode, PgPool, Postgres, Type, encode::IsNull, postgres::{PgArgumentBuffer, PgTypeInfo, PgValueRef}};

use crate::bot::{chat_event::chat_event::Platform};

pub mod users;
pub mod queue;
pub mod aliases;
pub mod bungie;
pub mod config;


        

pub async fn initialize_database(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("CREATE SCHEMA IF NOT EXISTS  krapbott_v2;").execute(pool).await?;
    sqlx::query("SET search_path TO krapbott_v2;").execute(pool).await?;

    sqlx::query!(
        r#"
        CREATE INDEX IF NOT EXISTS idx_queue_channel_position
        ON krapbott_v2.queue (channel_id, position);
        "#
    ).execute(pool).await?;

    sqlx::query!(
        r#"
        CREATE INDEX IF NOT EXISTS idx_queue_channel_user
        ON krapbott_v2.queue (channel_id, user_id);
        "#
    ).execute(pool).await?;
    sqlx::query!(
        r#"
        CREATE INDEX IF NOT EXISTS idx_queue_channel
        ON krapbott_v2.queue (channel_id);
        "#
    ).execute(pool).await?;
    sqlx::query!(
        r#"
        CREATE UNIQUE INDEX IF NOT EXISTS sessions_unique_user
        ON krapbott_v2.sessions (platform, platform_user_id);
        "#
    ).execute(pool).await?;
    Ok(())
}
impl Type<Postgres> for Platform {
    fn type_info() -> PgTypeInfo {
        <String as Type<Postgres>>::type_info()
    }
}
impl Platform {
    pub fn as_str(&self) -> &str {
        match self {
            Platform::Twitch => "twitch",
            Platform::Kick => "kick",
            Platform::Obs => "obs",
        }
    }
}
impl<'r> sqlx::Decode<'r, Postgres> for Platform {
    fn decode(
        value: PgValueRef<'r>
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let s = <String as sqlx::Decode<Postgres>>::decode(value)?;
        Ok(Platform::from_str(&s).unwrap_or(Platform::Obs))
    }
}

impl<'q> sqlx::Encode<'q, Postgres> for Platform {
    fn encode_by_ref(
        &self,
        buf: &mut PgArgumentBuffer
    ) -> Result<IsNull, Box<(dyn std::error::Error + std::marker::Send + Sync + 'static)>> {
        <String as sqlx::Encode<Postgres>>::encode(self.to_string(), buf)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct UserId(String);

impl UserId {
    pub fn new(platform: Platform, platform_user_id: impl AsRef<str>) -> Self {
        let id = platform_user_id.as_ref();

        assert!(!id.is_empty(), "platform_user_id must not be empty");
        assert!(!id.contains(':'), "platform_user_id must not contain ':'");

        UserId(format!("{}:{}", platform, id))
    }

    pub fn platform(&self) -> Platform {
        let (p, _) = self.0.split_once(':').expect("invalid UserId format");
        Platform::from_str(p).expect("invalid platform")
    }

    pub fn platform_user_id(&self) -> &str {
        self.0.split_once(':').unwrap().1
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::str::FromStr for UserId {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (platform, id) = s.split_once(':').ok_or("invalid channel id")?;
        let platform = Platform::from_str(platform)?;

        Ok(UserId::new(platform, id))
    }
}

impl fmt::Display for UserId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Type<Postgres> for UserId {
    fn type_info() -> PgTypeInfo {
        <String as Type<Postgres>>::type_info()
    }
}

impl<'r> sqlx::Decode<'r, Postgres> for UserId {
    fn decode(value: PgValueRef<'r>) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let s = <String as sqlx::Decode<Postgres>>::decode(value)?;
        Ok(UserId::from_str(&s)?)
    }
}

impl<'q> sqlx::Encode<'q, Postgres> for UserId {
    fn encode_by_ref(&self, buf: &mut PgArgumentBuffer) -> Result<IsNull, Box<(dyn std::error::Error + std::marker::Send + Sync + 'static)>> {
        <String as sqlx::Encode<Postgres>>::encode(self.0.clone(), buf)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub struct ChannelId(String);

impl ChannelId {
    pub fn new(platform: Platform, channel: impl AsRef<str>) -> Self {
        let channel = channel.as_ref();

        assert!(!channel.is_empty(), "channel must not be empty");
        assert!(!channel.contains(':'), "channel must not contain ':'");

        ChannelId(format!("{}:{}", platform, channel))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn platform(&self) -> Platform {
        let (p, _) = self.0.split_once(':').unwrap();
        Platform::from_str(p).unwrap()
    }

    pub fn channel(&self) -> &str {
        self.0.split_once(':').unwrap().1
    }
}

impl fmt::Display for ChannelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::str::FromStr for ChannelId {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (platform, channel) = s.split_once(':').ok_or("invalid channel id")?;
        let platform = Platform::from_str(platform)?;

        Ok(ChannelId::new(platform, channel))
    }
}

impl Type<Postgres> for ChannelId {
    fn type_info() -> sqlx::postgres::PgTypeInfo {
        <String as Type<Postgres>>::type_info()
    }

    fn compatible(ty: &sqlx::postgres::PgTypeInfo) -> bool {
        <String as Type<Postgres>>::compatible(ty)
    }
}

impl<'q> Encode<'q, Postgres> for ChannelId {
    fn encode_by_ref(&self, buf: &mut PgArgumentBuffer) -> Result<IsNull, Box<(dyn std::error::Error + std::marker::Send + Sync + 'static)>> {
        <String as Encode<Postgres>>::encode_by_ref(&self.0, buf)
    }

    fn size_hint(&self) -> usize {
        self.0.len()
    }
}

impl<'r> Decode<'r, Postgres> for ChannelId {
    fn decode(value: PgValueRef<'r>) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let s = <String as Decode<Postgres>>::decode(value)?;
        Ok(ChannelId(s))
    }
}