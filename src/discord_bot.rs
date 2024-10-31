use std::{sync::Arc, time::Duration};


use serenity::{all::{ChannelId, Context, CreateEmbed, CreateEmbedFooter, CreateMessage, EditMessage, EventHandler, GatewayIntents, GetMessages, Message, MessageBuilder, Ready}, async_trait, Client};
use tokio::{sync::Mutex, time::sleep};

use crate::{database::{initialize_database, load_from_queue}, models::BotConfig,};

struct Handler {
    queue_truck: Arc<Mutex<Option<Message>>>,
    queue_samoan: Arc<Mutex<Option<Message>>>
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, context: Context, msg: Message) {
        
        
        if msg.content == "!jk" {
            
            let channel = match msg.channel_id.to_channel(&context).await {
                Ok(channel) => channel,
                Err(why) => {
                    println!("Error getting channel: {why:?}");

                    return;
                },
            };
            
            // The message builder allows for creating a message by mentioning users dynamically,
            // pushing "safe" versions of content (such as bolding normalized content), displaying
            // emojis, and more.
            let response = MessageBuilder::new()
                .push("Hello JK")
                .build();

            if let Err(why) = msg.channel_id.say(&context.http, &response).await {
                println!("Error sending message: {why:?}");
            }
        }
    }
    async fn ready(&self, ctx: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);

        let queue_samoan = Arc::clone(&self.queue_samoan);
        let queue_truck = Arc::clone(&self.queue_truck);
        
        let channel_samoan = ChannelId::new(1298990333891182664);
        let channel_truck = ChannelId::new(1291081521935159418);

        for channel_id in vec![channel_samoan, channel_truck] {
            if let Ok(messages) = channel_id.messages(&ctx.http, GetMessages::new()).await {
                for id in messages {
                    channel_id.delete_message(&ctx.http, id).await.unwrap();
                }
            }
        }

        loop {
        
            discord_queue_embed(channel_samoan, &queue_samoan, "samoan_317", &ctx).await;
            discord_queue_embed(channel_truck, &queue_truck, "nyc62truck", &ctx).await;

            sleep(Duration::from_secs(30)).await;
        }
    }
}

pub async fn run_discord_bot() {
    dotenv::dotenv().ok();
    // Configure the client with your Discord bot token in the environment.
    let token = dotenv::var("DISCORD_TOKEN").expect("Expected a token in the environment");
    let intents = GatewayIntents::all();
    let mut client =
        Client::builder(&token, intents).event_handler(Handler {queue_samoan: Arc::new(Mutex::new(None)), queue_truck: Arc::new(Mutex::new(None)),}).await.expect("Err creating client");

    if let Err(why) = client.start().await {
        println!("Client error: {why:?}");
    }
}


async fn discord_queue_embed(channel_id: ChannelId, queue_message: &Arc<Mutex<Option<Message>>>, channel: &str, ctx: &Context) {
    let conn = initialize_database();
    let queue = load_from_queue(&conn, &format!("#{}", channel));
    
    // Create fancy embed message
    let embed_content = CreateEmbed::default()
        .title(format!("📝 Queue: {}", channel))
        .description("Here is the current list of participants:")
        .color(0x00FFFF)
        .footer(CreateEmbedFooter::new("Updates every 30 seconds"))
        .to_owned();

    // Populate the queue details
    let embed = if queue.is_empty() {
        embed_content.description("🚫 No one is currently in the queue.")
    } else {
        let mut embed = embed_content;
        for (i, entry) in queue.iter().enumerate() {
            let config = BotConfig::load_config(channel);
            let mut twitch = String::new();
            let mut bungie = String::new();
            if i < config.teamsize {
                twitch = format!("🎮 {}. ```{}```", i + 1, entry.twitch_name);
                bungie = format!("**Bungie Name**: ```{}```\n", entry.bungie_name);
            } else if i < config.teamsize *2 {
                twitch = format!("⚔️ {}. ```{}```", i + 1, entry.twitch_name);
                bungie = format!("**Bungie Name**: ```{}```\n", entry.bungie_name);
            } else {
                twitch = format!("⏳ {}. ```{}```", i + 1, entry.twitch_name);
                bungie = format!("**Bungie Name**: ```{}```\n", entry.bungie_name);
            }
            
            embed = embed.field(twitch, bungie, false);
        }
        embed
    };

    // Lock and edit the existing queue message or send a new one
    let mut queue_message_lock = queue_message.lock().await;
    if let Some(ref message_id) = *queue_message_lock {
        // Edit the existing embed message
        if let Ok(channel) = channel_id.to_channel(&ctx.http).await {
            if let Some(channel) = channel.guild() {
                let _ = channel
                    .edit_message(&ctx.http, message_id, EditMessage::new().add_embed(embed))
                    .await;
            }
        } else {
            return;
        }
    } else {
        // Send a new embed message and store its message ID
        if let Ok(message) = channel_id.send_message(&ctx.http, CreateMessage::new().add_embed(embed)).await {
            *queue_message_lock = Some(message);
        }
    }
}