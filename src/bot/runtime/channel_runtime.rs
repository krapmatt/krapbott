use tokio::task::JoinHandle;

use crate::bot::commands::CommandMap;


pub struct ChannelRuntime {
    pub dispatcher: CommandMap,
    pub tasks: Vec<JoinHandle<()>>,
}

impl ChannelRuntime {
    pub fn new(dispatcher: CommandMap) -> Self {
        Self {
            dispatcher,
            tasks: Vec::new(),
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