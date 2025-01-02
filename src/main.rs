mod bot_commands;
mod bot;
mod database;
pub mod models;
pub mod api;
pub mod discord_bot;
pub mod commands;
pub mod obs_dock;
use std::{thread::spawn, time::{Duration, SystemTime}};
use bot::run_chat_bot;
use discord_bot::run_discord_bot;
use models::BotConfig;
use obs_dock::{connect_to_obs, get_queue_handler, remove_from_queue_handler};
use tokio::time::sleep;
use warp::{filters::fs::dir, Filter};




#[tokio::main]
async fn main() {
    spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            if let Err(err) = connect_to_obs().await {
                eprintln!("OBS WebSocket Error: {}", err);
            }
        })
    });
    
    spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let static_files = dir("./public"); // Assuming queue_dock.html is in the ./static folder
        let get_queue = warp::path("queue").and(warp::get()).and(warp::path::param::<String>()).and_then(get_queue_handler);
        let remove_route = warp::path("remove").and(warp::post()).and(warp::body::json()).and_then(remove_from_queue_handler);
        let cors = warp::cors()
        .allow_any_origin()
        .allow_methods(vec!["GET", "POST"])
        .allow_headers(vec!["Content-Type"]);
        let routes = static_files.or(get_queue).or(remove_route).with(cors);
        rt.block_on(async {
            warp::serve(routes).run(([0, 0, 0, 0], 8080)).await;
        })
    });
    
    




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
    
    
    //Loop for if a error accours bot ,,doesnt" panics
    loop {   
        if let Err(e) = run_chat_bot().await {
            eprintln!("Error running chat bot: {} Time: {:?}", e, SystemTime::now());
        }
        //Pokud error je nevyhnutelný, nezaloopování
        sleep(Duration::from_secs(5)).await;
        println!("Restarting Twitch Krapbott!");
    }
    
    
    /*//Run the GUI
    let app_state = AppState::new(shared_state);
    let native_options = eframe::NativeOptions {
        viewport: ViewportBuilder::default().with_icon(load_icon("pictures/pp.webp")).with_title("Kr4pTr4p").with_inner_size([480.0, 560.0]),
        ..Default::default()
    };
    let _ = eframe::run_native("Twitch Queue Manager", native_options, Box::new(|_cc| Ok(Box::new(app_state))));
    */
}
