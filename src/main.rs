mod bot_commands;
mod bot;
mod database;
mod gui;
pub mod models;
pub mod api;
use std::{fs::File, io::Write, sync::Arc, thread::spawn, time::Duration};
use bot::run_chat_bot;


use gui::AppState;
use models::{BotConfig, SharedState};
use tokio::time::sleep;


pub fn check_config_file() {
    match File::open("Config.json") {
        Ok(..) => {return ()},
        Err(..) => {
            let mut file = File::create("Config.json").expect("Windows cannot create a file");
            let _ = file.write(serde_json::to_string_pretty(&BotConfig::new()).expect("Json serialization is wrong? Check Creating config function").as_bytes());
        }
    }
}

#[tokio::main]
async fn main() {
    check_config_file();
    
    let shared_state = Arc::new(std::sync::Mutex::new(SharedState::new()));
    let shared_state_clone = Arc::clone(&shared_state);
    
    spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            //Loop for if a error accours bot ,,doesnt" panics
            loop {   
                if let Err(e) = run_chat_bot(shared_state_clone.clone()).await {
                    eprintln!("Error running chat bot: {}", e);
                }
                //Pokud error je nevyhnutelný, nezaloopování
                sleep(Duration::from_secs(5)).await;
            }
        });
    });
    
    //Run the GUI
    let app_state = AppState::new(shared_state);
    let native_options = eframe::NativeOptions::default();
    eframe::run_native("Twitch Queue Manager", native_options, 
        Box::new(|_cc| Box::new(app_state)));
    
}


