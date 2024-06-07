mod bot_commands;
mod bot;
use std::{env, fs::File, io::{self, BufRead, BufReader, Write}, sync::Arc, thread::spawn};
use bot::{run_chat_bot, BotState};

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use tokio::{sync::Mutex, task};
#[derive(Debug, Deserialize, Serialize, Clone)]
struct Queue {
    twitch_name: String,
    bungie_name: String,
}

impl Default for Queue {
    fn default() -> Self {
        Queue { twitch_name: "Empty".to_string(), bungie_name: "Empty".to_string() }
    }
}
impl PartialEq for Queue {
    fn eq(&self, other: &Self) -> bool {
        if self.twitch_name == other.twitch_name {
            return true
        } else {
            return false
        }
    }
}

fn save_to_file(data: &Vec<Queue>, filename: &str) -> io::Result<()> {
    let mut file = File::create(filename)?;
    for entry in data {
        let json = serde_json::to_string(entry)?;
        writeln!(file, "{}", json)?;
    }
    Ok(())
}

fn load_from_file(filename: &str) -> io::Result<Vec<Queue>> {
    let file = File::open(filename)?;
    let reader = BufReader::new(file);
    let mut data = Vec::new();
    for line in reader.lines() {
        let line = line?;
        let entry: Queue = serde_json::from_str(&line)?;
        data.push(entry);
    }
    Ok(data)
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
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Queue Management");

            egui::ScrollArea::vertical().show(ui, |ui| {
                /*let queue = load_from_file(FILENAME).unwrap();
                for (index, item) in queue.iter().enumerate() {
                    ui.horizontal(|ui| {
                        let text = format!("{}. {} {}", index + 1, item.twitch_name, item.bungie_name);
                        let queue_name = ui.add(Label::new(text).sense(Sense::click()));
                        if queue_name.clone().on_hover_text("Click to copy").clicked() {
                            let copied_text = item.bungie_name.clone();
                            ui.output().copied_text = String::from(copied_text);
                        }
                        
                    });
                };*/
            });
        });
    }
}


fn initialize_database() -> anyhow::Result<Connection> {
    let conn = Connection::open("queue.db")?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS queue (
            id INTEGER PRIMARY KEY,
            twitch_name TEXT NOT NULL,
            bungie_name TEXT NOT NULL
        )",
        [],
    )?;
    Ok(conn)
}

fn save_to_database(conn: &Connection, queue: &Vec<Queue>) -> anyhow::Result<()> {
    conn.execute("DELETE FROM queue", params![])?; // Clear existing data
    for entry in queue {
        conn.execute(
            "INSERT INTO queue (twitch_name, bungie_name) VALUES (?1, ?2)",
            params![entry.twitch_name, entry.bungie_name],
        )?;
    }
    Ok(())
}

fn load_from_database(conn: &Connection) -> anyhow::Result<Vec<Queue>> {
    let mut stmt = conn.prepare("SELECT twitch_name, bungie_name FROM queue")?;
    let queue_iter = stmt.query_map([], |row| {
        Ok(Queue {
            twitch_name: row.get(0)?,
            bungie_name: row.get(1)?,
        })
    })?;
    
    let mut queue = Vec::new();
    for entry in queue_iter {
        queue.push(entry?);
    }
    Ok(queue)
}


#[tokio::main]
async fn main() -> anyhow::Result<()> {
    
    //TODO queue v databazi??????
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
    
    
        
           
    Ok(())
    
    
    

}


