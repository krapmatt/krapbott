use std::collections::{HashMap, HashSet};

use tokio::task::JoinHandle;

use crate::bot::{commands::CommandMap, state::def::AliasConfig};

pub struct ChannelRuntime {
    pub dispatcher: CommandMap,
    pub tasks: Vec<JoinHandle<()>>,
    pub alias_config: AliasConfig,
}

impl ChannelRuntime {
    pub fn new(dispatcher: CommandMap, alias_config: AliasConfig) -> Self {
        Self {
            dispatcher,
            tasks: Vec::new(),
            alias_config: alias_config,
        }
    }

    pub fn add_task(&mut self, task: JoinHandle<()>) {
        self.tasks.push(task);
    }

    pub fn shutdown(self) {
        for task in self.tasks {
            task.abort();
        }
    }
}