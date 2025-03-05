use std::{sync::Arc, time::Duration};

use dotenvy::dotenv;
use serde::{Deserialize, Serialize};
use serenity::{
    all::{
        ButtonStyle, ChannelId, Context, CreateActionRow, CreateButton, CreateEmbed,
        CreateEmbedFooter, CreateInteractionResponse, CreateInteractionResponseMessage,
        CreateMessage, EditMessage, EventHandler, GatewayIntents, GetMessages, GuildId,
        Interaction, Message, MessageBuilder, Ready,
    },
    async_trait, Client,
};
use tokio::{sync::Mutex, time::sleep};

use crate::{
    commands::COMMAND_GROUPS,
    database::{initialize_currency_database, load_from_queue},
    models::BotConfig,
};

struct Handler {
    queue_truck: Arc<Mutex<Option<Message>>>,
    info_truck: Arc<Mutex<Option<Message>>>,
    info_samoan: Arc<Mutex<Option<Message>>>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DiscordConfig {
    pub guild_id: GuildId,
    pub queue_channel_id: ChannelId,
    pub info_channel_id: ChannelId,
    pub info_enabled: bool,
    pub queue_name: String,
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, context: Context, msg: Message) {
        if msg.content == "!jk" {
            let response = MessageBuilder::new().push("BLAME JK").build();
            if let Err(why) = msg.channel_id.say(&context.http, &response).await {
                println!("Error sending message: {why:?}");
            }
        }
    }

    async fn ready(&self, ctx: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);

        let channel_ids = vec![
            ChannelId::new(1291081521935159418),
            ChannelId::new(1306951678691643472),
            ChannelId::new(1320511012687843338),
        ];
        for channel_id in &channel_ids {
            if let Ok(messages) = channel_id.messages(&ctx.http, GetMessages::new()).await {
                for id in messages {
                    channel_id.delete_message(&ctx.http, id).await.unwrap();
                }
            }
        }

        display_packages(&ctx, 1306951678691643472).await;
        display_packages(&ctx, 1320511012687843338).await;
        loop {
            discord_queue_embed(
                ChannelId::new(1291081521935159418),
                &self.queue_truck,
                "nyc62truck",
                &ctx,
            )
            .await;
            display_info(
                &ctx,
                1306951678691643472,
                1061466442849075290,
                &self.info_truck,
            )
            .await;
            display_info(
                &ctx,
                1320511012687843338,
                539807398701826058,
                &self.info_samoan,
            )
            .await;

            sleep(Duration::from_secs(15)).await;
        }
    }
    //vzít channel infa ze všech
    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Interaction::Component(component) = interaction {
            let package_name = component
                .data
                .custom_id
                .strip_prefix("add_")
                .or_else(|| component.data.custom_id.strip_prefix("remove_"));

            if let Some(package) = package_name {
                if component.data.custom_id.starts_with("add_") {
                    let mut temp = BotConfig::load_config();

                    temp.get_channel_config_mut(match_ids(component.guild_id.unwrap()))
                        .packages
                        .push(package.to_string());
                    temp.save_config();

                    component
                        .create_response(
                            &ctx.http,
                            CreateInteractionResponse::Message(
                                CreateInteractionResponseMessage::new()
                                    .content(format!("Package {} added!", package)),
                            ),
                        )
                        .await
                        .unwrap();
                } else if component.data.custom_id.starts_with("remove_") {
                    let mut temp = BotConfig::load_config();
                    let config =
                        temp.get_channel_config_mut(match_ids(component.guild_id.unwrap()));

                    if let Some(index) = config
                        .packages
                        .iter()
                        .position(|x| *x == package.to_string())
                    {
                        config.packages.remove(index);
                        temp.save_config();
                    }

                    component
                        .create_response(
                            &ctx.http,
                            CreateInteractionResponse::Message(
                                CreateInteractionResponseMessage::new()
                                    .content(format!("Package {} removed!", package)),
                            ),
                        )
                        .await
                        .unwrap();
                }
                let response_id = component.get_response(&ctx.http).await.unwrap();
                sleep(Duration::from_secs(5)).await;
                let _ = response_id.delete(&ctx.http).await;
            }
        }
    }
}

pub async fn run_discord_bot() {
    dotenv().ok();
    // Configure the client with your Discord bot token in the environment.
    let token = dotenvy::var("DISCORD_TOKEN").expect("Expected a token in the environment");
    let intents = GatewayIntents::all();
    let handler = Handler {
        queue_truck: Arc::new(Mutex::new(None)),
        info_truck: Arc::new(Mutex::new(None)),
        info_samoan: Arc::new(Mutex::new(None)),
    };
    let mut client = Client::builder(&token, intents)
        .event_handler(handler)
        .await
        .expect("Err creating client");

    if let Err(why) = client.start().await {
        println!("Client error: {why:?}");
    }
}

fn match_ids<'a>(channel_id: GuildId) -> &'a str {
    let mappings = vec![
        (GuildId::new(1240716292793565284), "#krapmatt"),
        (GuildId::new(1061466442849075290), "#nyc62truck"),
        (GuildId::new(539807398701826058), "#samoan_317"),
    ];
    mappings
        .into_iter()
        .find(|(id, _)| *id == channel_id)
        .map(|(_, name)| name)
        .unwrap_or("error")
}

async fn display_packages(ctx: &Context, channel_id: u64) {
    for package in &*COMMAND_GROUPS {
        let package = &package.name;
        let embed = CreateEmbed::default()
            .title(format!("Package: {}", package))
            .description("Manage commands in this package")
            .color(0x00FFFF)
            .field("Commands", format!("Commands for {}", package), false)
            .footer(CreateEmbedFooter::new(
                "Use buttons below to add/remove this package",
            ))
            .to_owned();

        let message = CreateMessage::new();
        let buttons = vec![
            CreateButton::new(format!("add_{}", package))
                .label("Add package")
                .style(ButtonStyle::Success),
            CreateButton::new(format!("remove_{}", package))
                .label("Remove package")
                .style(ButtonStyle::Danger),
        ];

        let _ = ChannelId::new(channel_id)
            .send_message(
                ctx,
                message
                    .add_embed(embed)
                    .components(vec![CreateActionRow::Buttons(buttons)]),
            )
            .await;
    }
}

async fn display_info(
    ctx: &Context,
    channel_id: u64,
    guild_id: u64,
    info_message: &Arc<Mutex<Option<Message>>>,
) {
    let mut config = BotConfig::load_config();
    let config = config.get_channel_config_mut(&match_ids(GuildId::new(guild_id)));

    let embed = CreateEmbed::default()
        .title(format!("Info"))
        .description("All the info one needs!")
        .color(0x00FFFF)
        .field("Lenght", format!("{}", config.len), false)
        .field("Fireteam Size", format!("{}", config.teamsize), false)
        .field("Opened", format!("{}", config.open), false)
        .footer(CreateEmbedFooter::new("Info updates every 10 seconds"))
        .to_owned();

    create_or_edit_embed(ChannelId::new(channel_id), ctx, info_message, embed).await;
}

async fn discord_queue_embed(
    channel_id: ChannelId,
    queue_message: &Arc<Mutex<Option<Message>>>,
    channel: &str,
    ctx: &Context,
) {
    let conn = initialize_currency_database().await.unwrap();
    let mut queue = load_from_queue(&conn, &format!("#{}", channel)).await;
    queue.sort_by(|(a, _), (b, _)| a.cmp(b));
    // Create fancy embed message
    let embed_content = CreateEmbed::default()
        .title(format!("📝 Queue: {}", channel))
        .description("Here is the current list of participants:")
        .color(0x00FFFF)
        .footer(CreateEmbedFooter::new("Updates every 10 seconds"))
        .to_owned();

    // Populate the queue details
    let embed = if queue.is_empty() {
        embed_content.description("🚫 No one is currently in the queue.")
    } else {
        let mut embed = embed_content;
        for (i, entry) in queue {
            let mut bot_config = BotConfig::load_config();
            let config = bot_config.get_channel_config_mut(&format!("#{}", channel));

            let twitch;
            let bungie;
            if i < config.teamsize {
                twitch = format!("🎮 {}. ```{}```", i, entry.twitch_name);
                bungie = format!("**Bungie Name**: ```{}```\n", entry.bungie_name);
            } else if i < config.teamsize * 2 {
                twitch = format!("⚔️ {}. ```{}```", i, entry.twitch_name);
                bungie = format!("**Bungie Name**: ```{}```\n", entry.bungie_name);
            } else {
                twitch = format!("⏳ {}. ```{}```", i, entry.twitch_name);
                bungie = format!("**Bungie Name**: ```{}```\n", entry.bungie_name);
            }

            embed = embed.field(twitch, bungie, false);
        }
        embed
    };

    // Lock and edit the existing queue message or send a new one
    create_or_edit_embed(channel_id, ctx, queue_message, embed).await;
}

async fn create_or_edit_embed(
    channel_id: ChannelId,
    ctx: &Context,
    message_lock: &Arc<Mutex<Option<Message>>>,
    embed: CreateEmbed,
) {
    let mut message_guard = message_lock.lock().await;
    if let Some(message) = &*message_guard {
        // Edit existing message
        if let Err(err) = channel_id
            .edit_message(&ctx.http, message, EditMessage::new().add_embed(embed))
            .await
        {
            eprintln!("Failed to edit message: {:?}", err);
        }
    } else {
        // Create new message
        if let Ok(message) = channel_id
            .send_message(&ctx.http, CreateMessage::new().add_embed(embed))
            .await
        {
            *message_guard = Some(message);
        }
    }
}
