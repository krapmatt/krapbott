use std::sync::Arc;

use async_sqlite::rusqlite::params;
use egui::{ Color32, IconData, Label, PlatformOutput, RichText, Sense, TextEdit};
use image::GenericImageView;


use crate::{commands::COMMAND_GROUPS, database::{initialize_database, load_from_queue}, SharedState};
pub struct AppState {
    shared_state: Arc<std::sync::Mutex<SharedState>>,
    ban_field: String,
    reason_field: String
}

impl AppState {
    pub fn new(shared_state: Arc<std::sync::Mutex<SharedState>>) -> Self {
        AppState { shared_state, ban_field: String::new(), reason_field: String::new() }
    }
}

impl eframe::App for AppState {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let conn = initialize_database();
        let mut queue_vec = load_from_queue(&conn, "#krapmatt");
        queue_vec.sort_by(|(a, _), (b, _)| a.cmp(b));
        ctx.set_visuals(egui::Visuals::dark());
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading(
                RichText::new("⚔️ Queue Management ⚔️")
                    .color(Color32::from_rgb(75, 0, 130))
                    .strong()
                    .italics()
                    .size(24.0),
            );
            // Queue list section with Destiny-themed hover and click effects
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (index, item) in queue_vec {
                    ui.horizontal(|ui| {
                        // Alternate colors for even and odd rows to give it a sci-fi panel effect
                        let bg_color = if index % 2 == 0 { Color32::from_rgb(44, 44, 84) } else { Color32::from_rgb(54, 54, 94) };
                        let text = format!("{}. 🛡️ {} - 🔫 {}", index, item.twitch_name, item.bungie_name);
                        let label = Label::new(
                            RichText::new(text)
                                .background_color(bg_color)
                                .color(Color32::from_rgb(180, 180, 255)),
                        )
                        .sense(Sense::click());
                        
                        let queue_name = ui.add(label);
                        if queue_name.clone().on_hover_text("Left click to copy/Right click to delete").clicked() {
                            let copied_text = item.bungie_name.clone();
                            ui.output_mut(|o| o.copied_text = copied_text);
                        } else if queue_name.secondary_clicked() {
                            // Remove entry and shift positions down
                            if let Ok(pos) = conn.query_row(
                                "SELECT position FROM queue WHERE twitch_name = ?1 AND channel_id = ?2",
                                params![item.twitch_name, "#krapmatt"],
                                |row| row.get::<_, i32>(0),
                            ) {
                                let _ = conn.execute(
                                    "DELETE FROM queue WHERE twitch_name = ?1 AND channel_id = ?2",
                                    params![item.twitch_name, "#krapmatt"],
                                );
                                let _ = conn.execute(
                                    "UPDATE queue SET position = position - 1 WHERE position > ?1 AND channel_id = ?2",
                                    params![pos, "#krapmatt"],
                                );
                            }
                        }
                    });
                }

                });
                
            ui.separator();
            
            ui.label(RichText::new("📊 Statistics").color(Color32::from_rgb(245, 189, 31)).strong());
            
            // Display statistics with a glow effect
            ui.horizontal(|ui| {
                let run_count_stat = format!("Total Runs Completed: {}", self.shared_state.lock().unwrap().run_count);
                ui.label(
                    RichText::new(run_count_stat)
                        .color(Color32::LIGHT_GREEN)
                        
                );
            });
            
            ui.separator();
            ui.label(RichText::new("🚫 Ban from Queue").color(Color32::from_rgb(255, 69, 0)).strong());
            
            // Ban entry with Destiny-themed placeholder and layout adjustments
            ui.horizontal(|ui| {
                ui.add(TextEdit::singleline(&mut self.ban_field).hint_text("Enter Guardian's name"));
                ui.add(TextEdit::singleline(&mut self.reason_field).hint_text("Reason for ban (optional)"));
            });
            
            if ui.button(RichText::new("🛑 Ban").color(Color32::LIGHT_RED)).clicked() {
                conn.execute(
                    "INSERT INTO banlist (twitch_name, reason) VALUES (?1, ?2)",
                    params![self.ban_field, self.reason_field],
                )
                .unwrap();
                self.ban_field.clear();
                self.reason_field.clear();
            }
            
            
            
            ui.heading(RichText::new("📦 Available Command Groups").color(Color32::from_rgb(245, 189, 31)).strong());
            ui.horizontal(|ui| {
                // Iterate over each command group and display its details
                for command_group in COMMAND_GROUPS.iter() {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&command_group.name).color(Color32::LIGHT_BLUE).strong());
                        
                        // List all commands within this command group
                        for (command_name, _) in &command_group.command {
                            ui.label(format!("🔹 {}", command_name));
                        }
                    });
                    ui.separator();
                }
            })
            
            
        });
        
    }
}

pub fn load_icon(path: &str) -> IconData {
    let image = image::open(path).expect("No image");
    let (width, height) = image.dimensions();
    let rgba = image.into_rgba8();
    let pixels = rgba.into_raw();

    IconData {
        rgba: pixels,
        width,
        height,
    }
}