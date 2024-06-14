mod bot_commands;
mod bot;
mod database;
use std::{sync::Arc, thread::spawn};
use bot::{run_chat_bot, BotState};

use database::{initialize_database, load_from_queue, QUEUE_TABLE};
use egui::{CentralPanel, Label, Sense};
use egui_dnd::dnd;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
#[derive(Debug, Deserialize, Serialize, Clone)]
struct TwitchUser {
    twitch_name: String,
    bungie_name: String,
}

impl Default for TwitchUser {
    fn default() -> Self {
        TwitchUser { twitch_name: String::new(), bungie_name: String::new() }
    }
}
impl PartialEq for TwitchUser {
    fn eq(&self, other: &Self) -> bool {
        if self.twitch_name == other.twitch_name {
            return true
        } else {
            return false
        }
    }
}

struct AppState {
}

impl AppState {
    fn new() -> Self {
        AppState {}
        
    }
}

impl eframe::App for AppState {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let conn = initialize_database("queue.db", QUEUE_TABLE).unwrap();
        let queue = load_from_queue(&conn).unwrap();
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Queue Management");
            
            egui::ScrollArea::vertical().show(ui, |ui| {
                
                for (index, item) in queue.iter().enumerate() {
                    ui.horizontal(|ui| {
                        let text = format!("{}. {} {}", index + 1, item.twitch_name, item.bungie_name);
                        let queue_name = ui.add(Label::new(text).sense(Sense::click()));
                        if queue_name.clone().on_hover_text("Left click to copy/Right click to delete").clicked() {
                            let copied_text = item.bungie_name.clone();
                            ui.output().copied_text = String::from(copied_text);
                        } else if queue_name.clone().secondary_clicked() {
                            let _ = conn.execute("DELETE FROM queue WHERE twitch_name = ?1", params![item.twitch_name]);
                            
                        }
                        
                    });
                };
            });
        });
    }
}


#[tokio::main]
async fn main() -> anyhow::Result<()> {
    
    let bot_state = Arc::new(Mutex::new(BotState::new()));
    // Start the chat bot in a separate task
    let bot_state_clone = Arc::clone(&bot_state);
    spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            if let Err(e) = run_chat_bot(bot_state_clone).await {
                eprintln!("Error running chat bot: {}", e);
            }
        });
    });
    
    //Run the GUI
    let app_state = AppState::new();
    let native_options = eframe::NativeOptions::default();
    eframe::run_native("Twitch Queue Manager", native_options, Box::new(|_cc| Box::new(app_state)));
    

}


