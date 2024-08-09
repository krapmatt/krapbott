mod bot_commands;
mod bot;
mod database;
mod gui;
pub mod models;
pub mod api;
use std::{sync::Arc, thread::spawn};
use api::{get_character_ids, get_membershipid, get_users_clears};
use bot::{run_chat_bot, BotState};

use database::initialize_database;

use gui::AppState;
use models::SharedState;
use tokio::sync::Mutex;
#[tokio::main]
async fn main() {
    let membership = get_membershipid("KrapMatt#1497".to_string()).await.unwrap();
    
    println!("{:?}", get_users_clears(membership.id, membership.type_m).await);

    let bot_state = Arc::new(Mutex::new(BotState::new()));
    let shared_state = Arc::new(std::sync::Mutex::new(SharedState::new()));
    let shared_state_clone = Arc::clone(&shared_state);
    

    spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            if let Err(e) = run_chat_bot(bot_state, shared_state_clone).await {
                eprintln!("Error running chat bot: {}", e);
            }
        });
    });
    
    //Run the GUI
    let app_state = AppState::new(shared_state);
    let native_options = eframe::NativeOptions::default();
    eframe::run_native("Twitch Queue Manager", native_options, 
        Box::new(|_cc| Box::new(app_state)));
}


