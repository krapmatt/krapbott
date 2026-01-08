use std::{collections::HashMap, sync::Arc};

use crate::bot::commands::commands::CommandT;

pub mod commands;
pub mod queue;
pub mod moderation;

#[derive(Clone)]
pub struct CommandRegistration {
    pub aliases: Vec<String>,
    pub command: Arc<dyn CommandT>
}

pub struct CommandGroup {
    pub name: String,
    pub commands: Vec<CommandRegistration>,
}

pub struct CommandRegistry {
    pub groups: HashMap<String, Arc<CommandGroup>>,
}

pub type CommandMap = HashMap<String, Arc<dyn CommandT + Send + Sync>>;