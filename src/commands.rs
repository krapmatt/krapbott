use crate::bot_commands::ban_player_from_queue;
use crate::bot_commands::modify_command;
use crate::bot_commands::promote_to_priority;
use crate::bot_commands::so;
use crate::bot_commands::unban_player_from_queue;
use crate::models::CommandAction;
use crate::BotConfig;
use crate::bot_commands::register_user;
use std::{borrow::BorrowMut, collections::HashMap, sync::Arc};

use crate::{bot::BotState, bot_commands::{bungiename, is_moderator, send_message}, models::{BotError, PermissionLevel}};
use async_sqlite::Client as SqliteClient;
use futures::future::BoxFuture;
use tmi::Privmsg;
use tokio::sync::Mutex;


type CommandHandler = Box<dyn Fn(Privmsg<'static>, Arc<Mutex<tmi::Client>>, SqliteClient, Arc<Mutex<BotState>>) -> BoxFuture<'static, Result<(), BotError>> + Send + Sync>;

impl Clone for CommandHandler {
    fn clone(&self) -> Self {
        self        
    }
}

pub struct Command {
    pub channels: Vec<String>,
    pub permission: PermissionLevel,
    pub handler: CommandHandler
}

fn add_commands_with_alias(commands: &mut HashMap<String, Command>, aliases: &[&str], command: Command) {
    for alias in aliases {
        commands.insert(alias.to_string(), command);
    }
}

pub fn create_command_dispatcher() -> HashMap<String, Command> {
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
            let mut client = client.lock().await;
            let channel = msg.channel().to_owned();
            conn.conn(move |conn| Ok(conn.execute("DELETE FROM queue WHERE channel_id = ?", [channel])?)).await?;
            send_message(&msg, client.borrow_mut(), "Queue has been cleared").await?;
            Ok(())
        };
        Box::pin(fut) 
    })});

    // Command to change queue length
    commands.insert("!queue_len".to_string(), Command {
        channels: vec![],
        permission: PermissionLevel::Moderator,
        handler: Box::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, _conn: SqliteClient, bot_state: Arc<Mutex<BotState>>| {
        let fut = async move {
            let words: Vec<&str> = msg.text().split_whitespace().collect();
            let reply;
            if words.len() == 2 && is_moderator(&msg, Arc::clone(&client)).await {
                let length = words[1].to_owned();
                let mut bot_state = bot_state.lock().await;
                bot_state.queue_config.len = length.parse().unwrap();
                bot_state.queue_config.save_config(&msg.channel().replace("#", ""));
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
        handler: Box::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, _conn: SqliteClient, bot_state: Arc<Mutex<BotState>>| {
        let fut = async move {
            let words: Vec<&str> = msg.text().split_whitespace().collect();
            let reply;
            if words.len() == 2 && is_moderator(&msg, Arc::clone(&client)).await {
                let length = words[1].to_owned();
                let mut bot_state = bot_state.lock().await;
                bot_state.queue_config.teamsize = length.parse().unwrap();
                bot_state.queue_config.save_config(&msg.channel().replace("#", ""));
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
   // !join 󠀀
    commands.insert("!bribe".to_string(), Command {
        channels: vec![],
        permission: PermissionLevel::Moderator,
        handler: Box::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, conn: SqliteClient, _bot_state: Arc<Mutex<BotState>>| {
        let fut = async move {
            promote_to_priority(&msg, client, &conn).await?;
            Ok(())
        };
        Box::pin(fut)
    })});

    commands.insert("!move".to_string(), Command {
        channels: vec![],
        permission: PermissionLevel::Moderator,
        handler: Box::new(|msg: Privmsg<'static>, client: Arc<Mutex<tmi::Client>>, conn: SqliteClient, bot_state: Arc<Mutex<BotState>>| {
        let fut = async move {
            bot_state.lock().await.move_groups(&msg, client, &conn).await?;
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
                BotState::new().queue_config.save_config(channel);
                client.join(format!("#{}", channel)).await?;
            } else {
                client.privmsg(msg.channel(), "You didn't write the channel to connect to").send().await?;
            }
            Ok(())
        };
        Box::pin(fut)
    })});

    commands.insert("!lurk".to_string(), Command {
        channels: vec!["#krapmatt".to_string(), "#therayii".to_string()],
        permission: PermissionLevel::User, 
        handler: Box::new(|msg, client, _conn, _bot_state| {
        let fut = async move {
            let iter: Vec<&tmi::Badge<'_>> = msg.badges().collect();
            println!("{:?}", iter);
            send_message(&msg, client.lock().await.borrow_mut(), 
                &format!("Thanks for the krapmaLurk {}! Be sure to leave the tab on low volume, or mute tab, to support stream krapmaHeart", 
                msg.sender().name()
            )).await?;
            Ok(())
        };
        Box::pin(fut)
    })});

    commands.insert("!so".to_string(), Command {
        channels: vec!["#krapmatt".to_string(), "#therayii".to_string()],
        permission: PermissionLevel::Vip,
        handler: Box::new(|msg, client, _conn, bot_state| {
        let fut = async move {
            if msg.text().len() > 6 {
                so(&msg, client, bot_state).await?;
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
        handler: Box::new(|msg, client, conn, _bot_state| {
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

    commands.insert("!mod_ban".to_string(), Command {
        channels: vec![],
        permission: PermissionLevel::Moderator,
        handler: Box::new(|msg, client, conn, _bot_state| {
        let fut = async move {
            ban_player_from_queue(&msg, client, &conn).await?;
            Ok(())
        };
        Box::pin(fut)
    })});

    commands.insert("!mod_unban".to_string(), Command {
        channels: vec![],
        permission: PermissionLevel::Moderator,
        handler: Box::new(|msg, client, conn, _bot_state| {
        let fut = async move {
            unban_player_from_queue(&msg, client, &conn).await?;
            Ok(())
        };
        Box::pin(fut)
    })});

    commands
}