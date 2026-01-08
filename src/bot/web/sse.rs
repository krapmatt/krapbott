use serde::Serialize;
use tokio::sync::broadcast;

use crate::bot::db::ChannelId;

pub type SseBus = broadcast::Sender<SseEvent>;

#[derive(Clone, Debug, Serialize)]
pub enum SseEvent {
    QueueUpdated {
        channel: ChannelId,
    },
    QueueStateChanged {
        channel: ChannelId,
        open: bool,
    },
}
