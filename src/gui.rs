use std::sync::Arc;

use egui::{Label, Sense};
use rusqlite::params;

use crate::{database::{initialize_database, load_from_queue}, SharedState};
pub struct AppState {
    shared_state: Arc<std::sync::Mutex<SharedState>>
}

impl AppState {
    pub fn new(shared_state: Arc<std::sync::Mutex<SharedState>>) -> Self {
        AppState { shared_state }
    }
}

impl eframe::App for AppState {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let conn = initialize_database();
        let mut queue = load_from_queue(&conn);
        let messages = self.shared_state.lock().unwrap().messages.clone();
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Queue Management");
            
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (index, item) in queue.iter_mut().enumerate() {
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
                }
            });
            ui.separator();
            ui.heading("Statistics");
           
            ui.horizontal(|ui| {
                let run_count_stat = format!("Run Count: {}", self.shared_state.lock().unwrap().run_count);
                ui.label(run_count_stat);
                
            });
            ui.separator();
            ui.heading("Chat Messages");
            ui.push_id("2", |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for msg in messages.iter().rev() {
                        ui.horizontal(|ui| {
                            ui.label(format!("Channel:{} // {}: {}", msg.channel, msg.user, msg.text));
                        });
                    }
                });
            });
            
        });
    }
}