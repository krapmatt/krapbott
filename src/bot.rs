use crate::{ 
    bot_commands::{announcment, ban_bots, bungiename, is_follower, is_moderator, register_user, send_message, shoutout}, 
    database::{get_command_response, initialize_database_async, remove_command, save_command}, 
    models::{BotConfig, BotError, ChatMessage}, SharedState
};
use async_sqlite::Client as SqliteClient;
use dotenv::dotenv;
use futures::future::BoxFuture;
use rand::Rng;
use regex::Regex;
use tmi::{irc, Client, Privmsg, Tag};

use std::{borrow::BorrowMut, collections::HashMap, env::var, sync::Arc, time::{self, SystemTime}};
use tokio::sync::Mutex;

pub const CHANNELS: &[&str] = &["#krapmatt,#nyc62truck"];

type CommandHandler = Box<dyn Fn(Privmsg<'static>, Arc<Mutex<tmi::Client>>, SqliteClient, Arc<Mutex<BotState>>) -> BoxFuture<'static, Result<(), BotError>> + Send + Sync>;

#[derive(Clone)]
pub struct BotState {
    oauth_token_bot: String,
    pub nickname: String,
    bot_id: String,
    pub x_api_key: String,
    pub first_time_tag: Option<String>,
    pub queue_config: BotConfig
}

impl BotState {
    pub fn new() -> BotState {
        dotenv().ok();
        let oauth_token_bot = var("TWITCH_OAUTH_TOKEN_BOTT").expect("No bot oauth token"); 
        let nickname = var("TWITCH_BOT_NICK").expect("No bot name");   
        let bot_id = var("TWITCH_CLIENT_ID_BOT").expect("msg");
        let x_api_key = var("XAPIKEY").expect("No bungie api key");
        

        BotState { 
            oauth_token_bot: oauth_token_bot,
            nickname: nickname,
            bot_id: bot_id,
            x_api_key: x_api_key,
            first_time_tag: None,
            queue_config: BotConfig::new()
        }
    }

    pub async fn client_builder(&mut self) -> Client {
        let credentials = tmi::Credentials::new(self.nickname.clone(), self.oauth_token_bot.clone());
        let mut client = tmi::Client::builder().credentials(credentials).connect().await.unwrap();
        client.join_all(CHANNELS).await.unwrap();
        client
    }
}


struct Command {
    channels: Vec<String>,
    permission: PermissionLevel,
    handler: CommandHandler
}

#[derive(Clone, Copy)]
enum PermissionLevel {
    User,
    Follower,
    Vip,
    Moderator,
    Broadcaster
}

async fn has_permission(msg: &tmi::Privmsg<'_>, client:Arc<Mutex<Client>>, level: PermissionLevel) -> bool {
    match level {
        PermissionLevel::User => true,
        PermissionLevel::Follower => is_follower(msg, Arc::clone(&client)).await,
        PermissionLevel::Moderator => is_moderator(msg, Arc::clone(&client)).await,
        PermissionLevel::Broadcaster => todo!(),
        PermissionLevel::Vip => todo!(),
    }
}

fn create_command_dispatcher() -> HashMap<String, Command> {
    let mut commands: HashMap<String, Command> = HashMap::new();

    // Command to open the queue
    commands.insert("!open_queue".to_string(), Command {
        channels: vec![],
        permission: PermissionLevel::Moderator,
        handler: Box::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, _: SqliteClient, botstate: Arc<Mutex<BotState>>| {
        let fut = async move {
            let mut bot_state = botstate.lock().await;
            bot_state.queue_config.open = true;
            send_message(&msg, client.lock().await.borrow_mut(), "The queue is now open!").await?;
            bot_state.queue_config.save_config(&msg.channel().replace("#", ""));
            Ok(())
        };
        Box::pin(fut) 
    })});

    // Command to close the queue
    commands.insert("!close_queue".to_string(), Command { 
        channels: vec![],
        permission: PermissionLevel::Moderator, 
        handler: Box::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, _: SqliteClient, bot_state: Arc<Mutex<BotState>>| {
        let fut = async move {
            let mut bot_state = bot_state.lock().await;
            bot_state.queue_config.open = false;
            send_message(&msg, client.lock().await.borrow_mut(), "The queue is now closed!").await?;
            bot_state.queue_config.save_config(&msg.channel().replace("#", ""));
            Ok(())
        };
        Box::pin(fut)
    })});

    // Command to clear the queue
    commands.insert("!clear".to_string(), Command {
        channels: vec![],
        permission: PermissionLevel::Moderator,
        handler: Box::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, conn: SqliteClient, _bot_state: Arc<Mutex<BotState>>| {
        let fut = async move {
            let mut client = client.lock().await; // Lock the client for access
            let channel = msg.channel().replace("#", "");
            conn.conn(move |conn| Ok(conn.execute("DELETE from queue WHERE channel_id = ?", [channel])?)).await?;
            send_message(&msg, client.borrow_mut(), "Queue has been cleared").await?;
            Ok(())
        };
        Box::pin(fut) 
    })});

    // Command to change queue length
    commands.insert("!queue_len".to_string(), Command {
        channels: vec![],
        permission: PermissionLevel::Moderator,
        handler: Box::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, conn: SqliteClient, bot_state: Arc<Mutex<BotState>>| {
        let fut = async move {
            let words: Vec<&str> = msg.text().split_whitespace().collect();
            let reply;
            if words.len() == 2 && is_moderator(&msg, Arc::clone(&client)).await {
                let length = words[1].to_owned();
                let mut bot_state = bot_state.lock().await;
                bot_state.queue_config.len = length.parse().unwrap();
                reply = format!("Queue length has been changed to {}", length);
            } else {
                reply = "Are you sure you had the right command? In case !queue_len <queue length>".to_string();
            }
            client.lock().await.privmsg(msg.channel(), &reply).send().await?;
            Ok(())
        };
        Box::pin(fut)
    })});

    // Command to change fireteam size
    commands.insert("!queue_size".to_string(), Command {
        channels: vec![],
        permission: PermissionLevel::Moderator,
        handler: Box::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, conn: SqliteClient, bot_state: Arc<Mutex<BotState>>| {
        let fut = async move {
            let words: Vec<&str> = msg.text().split_whitespace().collect();
            let reply;
            if words.len() == 2 && is_moderator(&msg, Arc::clone(&client)).await {
                let length = words[1].to_owned();
                let mut bot_state = bot_state.lock().await;
                bot_state.queue_config.teamsize = length.parse().unwrap();
                reply = format!("Queue fireteam size has been changed to {}", length);
            } else {
                reply = "Are you sure you had the right command? In case !queue_size <fireteam size>".to_string();
            }
            client.lock().await.privmsg(msg.channel(), &reply).send().await?;
            Ok(())
        };
        Box::pin(fut)
    })});

    // Command to join the queue
    commands.insert("!join".to_string(), Command {
        channels: vec![],
        permission: PermissionLevel::Follower,
        handler: Box::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, conn: SqliteClient, bot_state: Arc<Mutex<BotState>>| {
        let fut = async move {
            let mut bot_state = bot_state.lock().await;
            bot_state.handle_join(&msg, client, &conn).await?;
            Ok(())
        };
        Box::pin(fut)
    })});

    // Command to go to the next user in the queue
    commands.insert("!next".to_string(), Command {
        channels: vec![],
        permission: PermissionLevel::Moderator,
        handler: Box::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, conn: SqliteClient, bot_state: Arc<Mutex<BotState>>| {
        let fut = async move {
            let mut bot_state = bot_state.lock().await;
            bot_state.handle_next(&msg, client, &conn).await?;
            Ok(())
        };
        Box::pin(fut)
    })});

    // Command to remove a user from the queue
    commands.insert("!remove".to_string(), Command {
        channels: vec![],
        permission: PermissionLevel::Moderator,
        handler: Box::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, conn: SqliteClient, bot_state: Arc<Mutex<BotState>>| {
        let fut = async move {
            let mut bot_state = bot_state.lock().await;
            bot_state.handle_remove(&msg, client, &conn).await?;
            Ok(())
        };
        Box::pin(fut)
    })});

    // Command to get position in the queue
    commands.insert("!pos".to_string(), Command {
        channels: vec![],
        permission: PermissionLevel::User,
        handler: Box::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, conn: SqliteClient, bot_state: Arc<Mutex<BotState>>| {
        let fut = async move {
            let mut bot_state = bot_state.lock().await;
            bot_state.handle_pos(&msg, client, &conn).await?;
            Ok(())
        };
        Box::pin(fut)
    })});

    // Command to leave the queue
    commands.insert("!leave".to_string(), Command {
        channels: vec![],
        permission: PermissionLevel::User,
        handler: Box::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, conn: SqliteClient, bot_state: Arc<Mutex<BotState>>| {
        let fut = async move {
            let mut bot_state = bot_state.lock().await;
            bot_state.handle_leave(&msg, client, &conn).await?;
            Ok(())
        };
        Box::pin(fut)
    })});

    // Command to handle queue listing
    commands.insert("!queue".to_string(), Command {
        channels: vec![],
        permission: PermissionLevel::User,
        handler: Box::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, conn: SqliteClient, bot_state: Arc<Mutex<BotState>>| {
        let fut = async move {
            let mut bot_state = bot_state.lock().await;
            bot_state.handle_queue(&msg, client, &conn).await?;
            Ok(())
        };
        Box::pin(fut)
    })});

    // Command for a random action
    commands.insert("!random".to_string(), Command{
        channels: vec![],
        permission: PermissionLevel::Moderator,
        handler: Box::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, conn: SqliteClient, bot_state: Arc<Mutex<BotState>>| {
        let fut = async move {
            let mut bot_state = bot_state.lock().await;
            bot_state.random(&msg, client, &conn).await?;
            Ok(())
        };
        Box::pin(fut)
    })});

    commands.insert("!connect".to_string(), Command {
        channels: vec![],
        permission: PermissionLevel::Moderator,
        handler: Box::new(|msg, client, _conn, _bot_state| {
        let fut = async move {
            let mut client = client.lock().await;
            if let Some((_, channel)) = msg.text().split_once(" ") {
                client.join(format!("#{}", channel)).await?;
            } else {
                client.privmsg(msg.channel(), "You didn't write the channel to connect to").send().await?;
            }
            Ok(())
        };
        Box::pin(fut)
    })});

    commands.insert("!lurk".to_string(), Command {
        channels: vec!["#krapmatt".to_string()],
        permission: PermissionLevel::User, 
        handler: Box::new(|msg, client, conn, bot_state| {
        let fut = async move {
            send_message(&msg, client.lock().await.borrow_mut(), 
                &format!("Thanks for the krapmaLurk {}! Be sure to leave the tab on low volume, or mute tab, to support stream krapmaHeart", 
                msg.sender().name()
            )).await?;
            Ok(())
        };
        Box::pin(fut)
    })});

    commands.insert("!so".to_string(), Command {
        channels: vec!["#krapmatt".to_string()],
        permission: PermissionLevel::Vip,
        handler: Box::new(|msg, client, _conn, _bot_state| {
        let fut = async move {
            if msg.text().len() > 6 {
                so(&msg, client).await?;
            }
            Ok(())
        };
        Box::pin(fut)
    })});

    commands.insert("!total".to_string(), Command {
        channels: vec![],
        permission: PermissionLevel::User,
        handler: Box::new(|msg, client, conn, bot_state| {
        let fut = async move {
            bot_state.lock().await.total_raid_clears(&msg, client.lock().await.borrow_mut(), &conn).await?;
            Ok(())
        };
        Box::pin(fut)
    })});

    commands.insert("!register".to_string(), Command {
        channels: vec![],
        permission: PermissionLevel::User,
        handler: Box::new(|msg, client, conn, _bot_state| {
        let fut = async move {
            let reply;
                if let Some((_, bungie_name)) = msg.text().split_once(" ") {
                    reply = register_user(&conn, &msg.sender().name(), bungie_name).await?;
                } else {
                    reply = "Invalid command format! Use: !register bungiename#1234".to_string();
                }
                send_message(&msg, client.lock().await.borrow_mut(), &reply).await?;
            Ok(())
        };
        Box::pin(fut)
    })});

    commands.insert("mod_register".to_string(), Command {
        channels: vec![],
        permission: PermissionLevel::Moderator,
        handler: Box::new(|msg, client, conn, _bot_state| {
        let fut = async move {
            let words: Vec<&str> = msg.text().split_whitespace().collect();
            let reply;
            if words.len() >= 3 {
                let mut twitch_name = words[1].to_string();
                let bungie_name = &words[2..].join(" ");
                if twitch_name.starts_with("@") {
                    twitch_name.remove(0);
                }
                reply = register_user(&conn, &twitch_name, bungie_name).await?;
            } else {
                reply = "You are a mod. . . || If you forgot use: !mod_register twitchname bungoname".to_string();
            }
            send_message(&msg, client.lock().await.borrow_mut(), &reply).await?;
            Ok(())
        };
        Box::pin(fut)
    })});

    commands.insert("!bungiename".to_string(), Command {
        channels: vec![],
        permission: PermissionLevel::User,
        handler: Box::new(|msg, client, conn, _bot_state| {
        let fut = async move {
            let mut client = client.lock().await;
            if msg.text().trim_end().len() == 11 {
                bungiename(&msg, &mut client , &conn, msg.sender().name().to_string()).await?;
            } else {
                let (_, twitch_name) = msg.text().split_once(" ").expect("How did it panic, what happened? //Always is something here");
                let mut twitch_name = twitch_name.to_string();
                if twitch_name.starts_with("@") {
                    twitch_name.remove(0);
                }
                bungiename(&msg, &mut client, &conn, twitch_name).await?;
            }
            Ok(())
        };
        Box::pin(fut)
    })});

    commands.insert("!mod_config".to_string(), Command {
        channels: vec![],
        permission: PermissionLevel::Moderator,
        handler: Box::new(|msg, client, _conn, _bot_state| {
        let fut = async move {
            if is_moderator(&msg, Arc::clone(&client)).await {
                let channel_name = msg.channel().replace("#", "");
                let config = BotConfig::load_config(&channel_name);
                let reply = format!("Queue: {} || Length: {} || Fireteam size: {}", config.open, config.len, config.teamsize);
                send_message(&msg, client.lock().await.borrow_mut(), &reply).await?;
            }
            Ok(())
        };
        Box::pin(fut)
    })});

    commands.insert("!addcommand".to_string(), Command {
        channels: vec![],
        permission: PermissionLevel::Moderator,
        handler: Box::new(|msg, client, conn, bot_state| {
        let fut = async move {
            modify_command(&msg, client, conn, CommandAction::Add, Some(msg.channel().to_string())).await?;
            Ok(())
        };
        Box::pin(fut)
    })});

    commands.insert("!removecommand".to_string(), Command {
        channels: vec![],
        permission: PermissionLevel::Moderator,
        handler: Box::new(|msg, client, conn, _bot_state| {
        let fut = async move {
                modify_command(&msg, client, conn, CommandAction::Remove, Some(msg.channel().to_string())).await?;
            Ok(())
        };
        Box::pin(fut)
    })});

    commands.insert("!addglobalcommand".to_string(), Command {
        channels: vec![],
        permission: PermissionLevel::Moderator,
        handler: Box::new(|msg, client, conn, _bot_state| {
        let fut = async move {
            modify_command(&msg, client, conn, CommandAction::AddGlobal, None).await?;
            Ok(())
        };
        Box::pin(fut)
    })});

    commands
}


enum CommandAction {
    Add,
    Remove,
    AddGlobal,
}

async fn modify_command(msg: &tmi::Privmsg<'_>, client:Arc<Mutex<Client>>, conn: SqliteClient, action: CommandAction, channel: Option<String>) -> Result<(), BotError> {
    let words: Vec<&str> = msg.text().split_whitespace().collect();
    let mut client = client.lock().await;
    let mut reply;
    if words.len() < 2 {
        reply = "Usage: !removecommand <command>".to_string();
    }
    
    let command = words[1].to_string().to_ascii_lowercase();
    let reply_to_command = words[2..].join(" ").to_string();
    
    match action {
        CommandAction::Add => {
            reply = save(&conn, command, reply_to_command, channel, "Usage: !addcommand <command> <response>").await?;
        }
        CommandAction::Remove => {
            if remove_command(&conn, &command).await {
                reply = format!("Command !{} removed.", command)
            } else {
                reply = format!("Command !{} doesn't exist.", command)
            }
        }
        CommandAction::AddGlobal => {
            reply = save(&conn, command, reply_to_command, channel, "Usage: !addcommand <command> <response>").await?;
            
        } 
    };
    send_message(msg, &mut client, &reply).await?;
    
    
    
    
    Ok(())
}

async fn save(conn: &SqliteClient, command: String, reply: String, channel: Option<String>, error_mess: &str) -> Result<String, BotError> {
    if !reply.is_empty() {
        save_command(&conn, command.clone(), reply, channel).await;
        Ok(format!("Command !{} added.", command))
    } else {
        Ok(error_mess.to_string())
    }
}

async fn so(msg: &tmi::Privmsg<'_>, client:Arc<Mutex<Client>>) -> Result<(), BotError> {
    if is_moderator(msg, Arc::clone(&client)).await {
        let words:Vec<&str> = msg.text().split_ascii_whitespace().collect();
        let mut twitch_name = words[1].to_string();
        if twitch_name.starts_with("@") {
            twitch_name.remove(0);
        }
        send_message(&msg, client.lock().await.borrow_mut(), &format!("Let's give a big Shoutout to https://www.twitch.tv/{} ! Make sure to check them out and give them a FOLLOW krapmaHeart", twitch_name)).await?;
    }
    Ok(())
}

//Timers/Counters
//Bungie api stuff - evade it
pub async fn run_chat_bot(shared_state: Arc<std::sync::Mutex<SharedState>>) -> Result<(), BotError> {
    let mut bot_state = Mutex::new(BotState::new());

    let mut messeges = 0;
    
    let mut start_time = SystemTime::now();
    let mut client = Arc::new(Mutex::new(bot_state.lock().await.client_builder().await));

    let conn = initialize_database_async().await;
    let command_dispatcher = create_command_dispatcher();
    loop {
        // Borrow the client and call recv() on the inner value
        let irc_msg = client.lock().await.recv().await?;
        let first_time = irc_msg.tag(Tag::FirstMsg).map(|x| x.to_string());
        
        
        match irc_msg.as_ref().as_typed()? {
            tmi::Message::Privmsg(msg) => {
                let mut bot_state = bot_state.lock().await;
                bot_state.first_time_tag = first_time.clone();
                bot_state.queue_config = BotConfig::load_config(&msg.channel().replace("#", ""));
                
                let mut config = bot_state.queue_config.clone();
                config.channel_id = Some(msg.to_owned().channel().to_string());

                let bot_state_arc = Arc::new(Mutex::new(bot_state.to_owned()));
                
                
                
                if is_bannable_link(msg.text()) && first_time == Some("1".to_string()) {
                    ban_bots(&msg, &bot_state.oauth_token_bot, bot_state.bot_id.clone()).await;
                    client.lock().await.privmsg(msg.channel(), "We don't want cheap viewers, only expensive ones <3").send().await?;
                }

                if msg.text().starts_with("!") {
                    let command = msg.text().split_whitespace().next().unwrap_or_default().to_string();
                    if let Some(cmd) = command_dispatcher.get(&command) {
                        if cmd.channels.is_empty() || cmd.channels.contains(&msg.channel().to_string()) {
                            let msg_clone = msg.clone().into_owned(); 
                            let conn_clone = conn.clone(); 
                            let client_arc = Arc::clone(&client);

                            if has_permission(&msg, Arc::clone(&client), cmd.permission).await {
                                // Execute the command handler if permission is granted
                                (cmd.handler)(msg_clone, client_arc, conn_clone, bot_state_arc).await?;
                            }
                        }
                    } else {
                        if let Ok(Some(reply)) = get_command_response(&conn, msg.text().to_string().to_ascii_lowercase(), Some(msg.channel().to_string())).await {
                            send_message(&msg, client.lock().await.borrow_mut(), &reply).await?;
                        }
                    }
                }
                if msg.channel() == "#krapmatt" {
                    messeges += 1;
                }
                config.save_config(&msg.channel().replace("#", ""));
            }
            tmi::Message::Reconnect => {
                let mut client = client.lock().await;
                client.reconnect().await?;
                client.join_all(CHANNELS).await?;
            }
            tmi::Message::Ping(ping) => {
                client.lock().await.pong(&ping).await?;
            }
            _ => {}
        }
        //rendom choose from preset messages
        //Add those messages into a database in future

        if start_time.elapsed().unwrap() > time::Duration::from_secs(800) && messeges >= 10 {
            let bot_state = bot_state.lock().await;
            let mut rand = rand::thread_rng();
            let ch = rand.gen_range(1..4);
            let mut mess = String::new(); 
            
            if ch == 1 {
                mess = "Join my discord krapmaStare : https://discord.gg/jJMwaetjeu".to_string();
            } else if ch == 2 {
                mess = "Don't forget to follow, if you enjoy it here! Also krapmaLurk is greatly appreciated krapmaHeart".to_string();
            } else if ch == 3 {
                mess = "If you need any help with dungeons. Try asking in chat! I just might be able to help you krapmaHeart".to_string();
            } else if ch == 4 {
                mess = "If you need any crota yeets just let me know. If I have a cp i will get it for you. <3".to_string();
            }
            
            announcment("216105918", "1091219021",&bot_state.oauth_token_bot , bot_state.bot_id.to_string(), mess).await?;
            
            messeges = 0;
            start_time = SystemTime::now();
        }
    }
            
} 
        
        
        /*
                let chat_message = ChatMessage {
                    channel: msg.channel().to_string(),
                    user: msg.sender().name().to_string(),
                    text: msg.text().to_string(),
                };
                shared_state.lock().unwrap().add_stats(chat_message, run_count);
                run_count += 1;

            
        }*/

        


lazy_static::lazy_static! {
    static ref CHEAP_VIEWERS_RE: Regex = Regex::new(r"cheap\s*viewers\s*on").unwrap();
    static ref BEST_VIEWERS_RE: Regex = Regex::new(r"best\s*viewers\s*on").unwrap();
    static ref PROMO_RE: Regex = Regex::new(r"hello\s*sorry\s*for\s*bothering\s*you\s*i\s*want\s*to\s*offer\s*promotion\s*of\s*your\s*channel\s*viewers\s*followers\s*views\s*chat\s*bots\s*etc\s*the\s*price\s*is\s*lower\s*than\s*any\s*competitor\s*the\s*quality\s*is\s*guaranteed\s*to\s*be\s*the\s*best\s*flexible\s*and\s*convenient\s*order\s*management\s*panel\s*chat\s*panel\s*everything\s*is\s*in\s*your\s*hands\s*a\s*huge\s*number\s*of\s*custom\s*settings").unwrap();
}

//Find first message
fn is_bannable_link(text: &str) -> bool {
    // Remove non-alphanumeric characters and convert to lowercase
    let cleaned_text: String = text.chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();

    // Check the conditions using precompiled regexes
    (CHEAP_VIEWERS_RE.is_match(&cleaned_text) || BEST_VIEWERS_RE.is_match(&cleaned_text) && text.contains(".")) ||
        PROMO_RE.is_match(&cleaned_text)
}


