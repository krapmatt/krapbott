use crate::bot::db::ChannelId;

pub struct Replies;

impl Replies {
    pub fn join_closed(user: &str) -> String {
        format!("❌ {} the queue is closed 😭 please try again later 💜", user)
    }

    pub fn join_invalid_bungie(user: &str) -> String {
        format!("❌ {} that was an invalid bungie name 😭please try again! Correct format would be: johnbungie#1234 💜", user)
    }

    pub fn join_banned(user: &str, reason: Option<&str>) -> String {
        match reason {
            Some(r) => format!("❌ {} you are banned from queue for {} 😔", user, r),
            None => format!("❌ {} you are banned from queue, beg for forgiveness 💀", user),
        }
    }

    pub fn join_timed_out(user: &str) -> String {
        format!("❌ {} you are timed out 😔 please try again later 💜", user)
    }

    pub fn add_to_queue(user: &str) -> String {
        format!("✅{} has been added to the queue! 🫡", user)
    }

    pub fn join_added(user: &str, next_position: &str) -> String {
        format!("✅{user} has joined the queue at position {next_position}! 🥳")
    }

    pub fn raffle_won(user: &str) -> String {
        format!("🎯{} have won the next run! 🥳 Please be ready for an invite! 💜", user)
    }

    pub fn queue_empty(broadcaster: &str) -> String {
        format!("💀 {} the queue is empty..? 👁👄👁", broadcaster)
    }

    pub fn next_group(group: &str) -> String {
        format!("🎯The next group - {} !! 🥳Please be ready for an invite! 💜", group)
    }

    pub fn queue_opened() -> String {
        "🔓The queue is open!🔓".to_string()
    }

    pub fn queue_closed() -> String {
        "🔐The queue is closed🔐".to_string()
    }

    pub fn queue_removed(user: &str) -> String {
        format!("💀 {} has been removed from the queue 😥", user)
    }

    pub fn queue_size(size: &str) -> String {
        format!("✅ Group size is now {size} people per run! 🥳")
    }

    pub fn queue_length(length: &str) -> String {
        format!("✅ Queue length is now {length}!! 🤩")
    }

    pub fn prio_queue(user: &str) -> String {
        format!("⭐💎{user} has been given a priority run! 💎⭐")
    }

    pub fn priod_for__queue(user: &str, number: &str) -> String {
        format!("⭐💎{user} has been given {number} priority runs!  💎⭐")
    }

    pub fn pos_reply(group: i64, index: &str, max_count: &str, user: &str) -> String {
        if group == 1 {
            format!("📋 {user} you are at position {}/{} and in LIVE group! DinoDance", index, max_count)
        } else if group == 2 {
            format!("📋 {user} you are at position {}/{} and in NEXT group! GoldPLZ", index, max_count)
        } else {
            format!("📋 {user} you are at position {}/{} (Group {}) 💜", index, max_count, group)
        }
    }

    pub fn config_header(channel: &ChannelId) -> String {
        format!("📋 Channel config for {}", channel.as_str())
    }

    pub fn queue_runs_reset(channel: &ChannelId) -> String {
        format!("📋 Runs reset for {}", channel.as_str())
    }
}