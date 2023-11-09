use lazy_static::lazy_static;
use std::env;
use std::fmt::format;
use std::process::exit;
use std::sync::Arc;
use std::time::Duration;

use serenity::async_trait;
use serenity::http::{CacheHttp, Http};
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::model::prelude::*;
use serenity::prelude::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::OnceCell;
use tokio::time::{sleep as asleep, Timeout};

const UPDATE_INTERVAL: Duration = Duration::from_secs(1);

lazy_static! {
    // TCP Stream to strawberry chat server
    static ref STBCHAT_STREAM: Mutex<TcpStream> = {
        smol::block_on(async {
            let stream = TcpStream::connect("127.0.0.1:8080").await.expect("Failed to open stream");
            return Mutex::new(stream)
        })
    };

    // Bot account username and password
    static ref BOT_UNAME: String = env::var("BOT_UNAME").expect("Failed to get BOT_UNAME env variable");
    static ref BOT_PASSWD: String = env::var("BOT_PASSWD").expect("Failed to get BOT_PASSWD env variable");

    // Discord HTTP
    static ref DISCORD_HTTP: OnceCell<Arc<Http>> = OnceCell::new();
}

async fn c2d_thread() {
    let mut buffer = [0u8; 2048];
    let http = DISCORD_HTTP.get();
    loop {
        match tokio::time::timeout(Duration::from_secs(1),STBCHAT_STREAM.lock().await.read(&mut buffer)).await {
            Ok(Ok(n_bytes)) => {
                if n_bytes == 0 {
                    eprintln!("Server closed connection, exiting");
                    exit(1);
                }
                let read = buffer[..n_bytes].to_vec();
                let read_str = String::from_utf8(read).expect("Failed to parse received bytes as UTF-8");
                let read_str = strip_ansi_escapes::strip_str(read_str);
                println!("Received (ansi-stripped): {:?}", read_str);

                if read_str.starts_with("User not found.") {
                    eprint!("Wrong bot username, exiting");
                    exit(1)
                }

                if read_str.starts_with("Wrong username or password.") {
                    eprintln!("Wrong bot password, exiting");
                    exit(1)
                }

                if read_str.starts_with(format!("{}:", BOT_UNAME.clone()).as_str()) {
                    eprintln!("Message is from myself");
                    continue;
                }

                ChannelId(1140708601372086313).say(DISCORD_HTTP.get().unwrap(), read_str).await.expect("Failed to say in channel");
            }
            _ => {}
        }
    }
}

struct Handler;

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, ctx: Context, msg: Message) {
        if msg.channel_id != ChannelId(1140708601372086313) {
            println!("Not queuing discord message - Wrong channel (not strawberry-chat channel)");
            return;
        }

        if msg.author.id == UserId(1099801198103633920) {
            println!("Message is from bot");
            return;
        }

        STBCHAT_STREAM
            .lock()
            .await
            .write_all(format!("{} via Discord: {}", msg.author.name, msg.content).as_bytes())
            .await
            .expect("Failed to send message to stream");
    }

    async fn ready(&self, ctx: Context, _: Ready) {
        ctx.set_activity(Activity::watching("404 Not Found")).await;
        DISCORD_HTTP.set(ctx.http).expect("Failed to set HTTP");
        tokio::task::spawn(async { c2d_thread().await });
        println!("Bot is connected!");
    }
}

#[tokio::main]
async fn main() {
    // Configure the client with your Discord bot token in the environment.
    let token = env::var("DISCORD_TOKEN").expect("Failed to get DISCORD_TOKEN env variable");
    // Set gateway intents, which decides what events the bot will be notified about
    let intents = GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::DIRECT_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT;

    // Create a new instance of the Client, logging in as a bot. This will
    // automatically prepend your bot token with "Bot ", which is a requirement
    // by Discord for bot users.
    let mut client = Client::builder(&token, intents)
        .event_handler(Handler)
        .await
        .expect("Error creating client");

    // Authenticate to stbchat
    STBCHAT_STREAM
        .lock()
        .await
        .write_all(BOT_UNAME.as_bytes())
        .await
        .expect("Failed to write to stream");
    asleep(Duration::from_secs(2)).await;

    STBCHAT_STREAM
        .lock()
        .await
        .write_all(BOT_PASSWD.as_bytes())
        .await
        .expect("Failed to write to stream");

    STBCHAT_STREAM
        .lock()
        .await
        .read(&mut [0u8; 1000])
        .await
        .expect("Failed to read while clearing read buffer");

    // Finally, start a single shard, and start listening to events.
    //
    // Shards will automatically attempt to reconnect, and will perform
    // exponential backoff until it reconnects.
    if let Err(why) = client.start().await {
        println!("Client error: {:?}", why);
    }
}
