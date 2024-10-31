mod bot_commands;
mod bot;
mod database;
mod gui;
pub mod models;
pub mod api;
pub mod discord_bot;
pub mod commands;
use std::{sync::Arc, thread::spawn, time::{Duration, SystemTime}};

use bot::run_chat_bot;
use discord_bot::run_discord_bot;
use gui::AppState;
use models::{BotConfig, SharedState};
use tokio::time::sleep;

#[tokio::main]
async fn main() {
    
    spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            loop {
                run_discord_bot().await;
                println!("Restarting Discord Krapbott!");
                sleep(Duration::from_secs(5)).await;
            }
        });
    });
    
    let shared_state = Arc::new(std::sync::Mutex::new(SharedState::new()));
    let shared_state_clone = Arc::clone(&shared_state);
    
    spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            //Loop for if a error accours bot ,,doesnt" panics
            loop {   
                if let Err(e) = run_chat_bot(shared_state_clone.clone()).await {
                    eprintln!("Error running chat bot: {} Time: {:?}", e, SystemTime::now());
                }
                //Pokud error je nevyhnutelný, nezaloopování
                sleep(Duration::from_secs(5)).await;
                println!("Restarting Twitch Krapbott!");
            }
        });
    });
    
    //Run the GUI
    let app_state = AppState::new(shared_state);
    let native_options = eframe::NativeOptions::default();
    eframe::run_native("Twitch Queue Manager", native_options, 
        Box::new(|_cc| Box::new(app_state)));
    
}


