use anyhow::Context;
use azalea::chat::ChatPacket;
use azalea::{Account, Client, ClientBuilder, Event};
use bevy_ecs::component::StorageType;
use bevy_ecs::prelude::Component;
use dotenv::dotenv;
use sqlx::SqlitePool;
use std::collections::HashSet;
use std::env;
use std::net::ToSocketAddrs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use tokio::sync::OnceCell;
use tokio::task::LocalSet;
use utils::LookAtStuffPlugin;

mod db;
mod utils;
use utils::{Command, DirectMessage, ServerMessage};

static SERVER_HOSTNAME: LazyLock<String> =
    LazyLock::new(|| env::var("SERVER_HOSTNAME").expect("SERVER_HOSTNAME must be set"));

static SERVER_PORT: LazyLock<u16> = LazyLock::new(|| {
    env::var("SERVER_PORT")
        .expect("SERVER_PORT must be set")
        .parse()
        .expect("SERVER_PORT must be a valid port number")
});

static WHITELIST: LazyLock<Vec<String>> = LazyLock::new(|| {
    env::var("WHITELIST")
        .expect("WHITELIST must be set")
        .split(',')
        .map(String::from)
        .collect()
});

static MASTER_USERNAME: LazyLock<String> =
    LazyLock::new(|| env::var("MASTER_USERNAME").expect("MASTER_USERNAME must be set"));

static MASTER_PASSWORD: LazyLock<String> =
    LazyLock::new(|| env::var("MASTER_PASSWORD").expect("MASTER_PASSWORD must be set"));

static PASSWORD_SALT_SECRET: LazyLock<String> =
    LazyLock::new(|| env::var("PASSWORD_SALT_SECRET").expect("PASSWORD_SALT_SECRET must be set"));

static DB_POOL: OnceCell<SqlitePool> = OnceCell::const_new();

static SLAVES: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

#[derive(Clone, Default)]
pub struct State {
    password: Arc<String>,
    has_logged_in: Arc<AtomicBool>,
}

impl State {
    fn for_user(username: &str) -> Self {
        let plain = format!(
            "{}{username}{}",
            *PASSWORD_SALT_SECRET, *PASSWORD_SALT_SECRET
        );
        let mut password = sha256::digest(plain);
        password.truncate(20);
        Self::new(password)
    }

    fn new(password: String) -> Self {
        Self {
            password: Arc::new(password),
            has_logged_in: Arc::new(AtomicBool::new(false)),
        }
    }

    fn register(&self) -> String {
        format!("register {} {0}", self.password)
    }

    fn login(&self) -> String {
        format!("login {}", self.password)
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    // let runtime = utils::runtime()?;
    let db_pool = db::init_db().await?;
    DB_POOL.set(db_pool)?;
    let db_pool = DB_POOL.get().with_context(|| "DB was not initialized!")?;

    let slaves = db::get_slaves(db_pool).await?;
    for slave in slaves {
        spawn_slave_bot(slave)?;
    }

    tokio::spawn(async {
        tokio::signal::ctrl_c().await.unwrap();
        std::process::exit(0);
    });

    let set = LocalSet::new();
    set.spawn_local(
        ClientBuilder::new()
            .set_handler(handle)
            .add_plugins(LookAtStuffPlugin)
            .set_state(State::new(MASTER_PASSWORD.to_string()))
            .start(
                Account::offline(&MASTER_USERNAME),
                format!("{}:{}", *SERVER_HOSTNAME, *SERVER_PORT)
                    .to_socket_addrs()
                    .unwrap()
                    .next()
                    .unwrap(),
            ),
    );

    set.await;
    Ok(())
}

fn spawn_slave_bot(username: String) -> anyhow::Result<()> {
    let mut slaves = SLAVES
        .lock()
        .map_err(|_| anyhow::anyhow!("Error accessing the slaves set"))?;
    if slaves.contains(&username) {
        println!("Slave bot {} already running", username);
        return Ok(());
    }
    slaves.insert(username.clone());
    core::mem::drop(slaves);

    std::thread::spawn(move || -> anyhow::Result<()> {
        utils::runtime()?.block_on(async move {
            loop {
                let result = ClientBuilder::new()
                    .set_handler(handle)
                    .add_plugins(LookAtStuffPlugin)
                    .set_state(State::for_user(&username))
                    .start(
                        Account::offline(&username),
                        format!("{}:{}", *SERVER_HOSTNAME, *SERVER_PORT)
                            .to_socket_addrs()
                            .unwrap()
                            .next()
                            .unwrap(),
                    )
                    .await;

                match result {
                    // Bot exited successfully
                    Ok(_) => break,
                    Err(e) => {
                        eprintln!("Slave bot {} error: {}", username, e);
                        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                    }
                }
            }
        });
        Ok(())
    });
    Ok(())
}

impl Component for State {
    const STORAGE_TYPE: StorageType = StorageType::Table;
}

fn login(bot: &Client, state: &State, message: &ServerMessage) -> anyhow::Result<bool> {
    if state.has_logged_in.load(Ordering::SeqCst) {
        return Ok(true);
    }
    match message {
        ServerMessage::RegisterPrompt => bot.send_command_packet(&state.register()),
        ServerMessage::LoginPrompt => bot.send_command_packet(&state.login()),
        ServerMessage::LoginSuccess => state.has_logged_in.store(true, Ordering::SeqCst),
        _ => {}
    }
    Ok(message == &ServerMessage::LoginSuccess)
}

// Then modify your handle function to use this:
async fn handle(bot: Client, event: Event, state: State) -> anyhow::Result<()>
where
    Client: Send + Sync + 'static,
    Event: Send + Sync + 'static,
    State: Send + Sync + 'static,
{
    #[allow(clippy::single_match)]
    match event {
        // Event::Init => todo!(),
        // Event::Login => todo!(),
        Event::Chat(chat_packet) => handle_chat_event(bot, chat_packet, state).await?,
        // Event::Tick => todo!(),
        // Event::Packet(clientbound_game_packet) => todo!(),
        // Event::AddPlayer(player_info) => todo!(),
        // Event::RemovePlayer(player_info) => todo!(),
        // Event::UpdatePlayer(player_info) => todo!(),
        // Event::Death(clientbound_player_combat_kill) => todo!(),
        // Event::KeepAlive(_) => todo!(),
        // Event::Disconnect(formatted_text) => todo!(),
        _ => (),
    }
    Ok(())
}

// Then modify your handle function to use this:
async fn handle_chat_event(bot: Client, m: ChatPacket, state: State) -> anyhow::Result<()>
where
    Client: Send + Sync + 'static,
{
    let None = m.username() else { return Ok(()) };

    let message = m.message().to_string();
    println!("Received: {}", message);

    let message = ServerMessage::parse(&message);

    if !login(&bot, &state, &message)? {
        return Ok(());
    }

    use ServerMessage::*;
    match message {
        // Gestiti dentro login
        RegisterPrompt | LoginPrompt | LoginSuccess => (),
        TeleportRequest(username) => {
            println!("Received teleport request from: {username}");
            if WHITELIST.contains(&username.to_string()) {
                bot.send_command_packet(&format!("tpaccept {username}"));
                println!("Accepted teleport request from {username}");
            } else {
                bot.send_command_packet(&format!("tpdeny {username}"));
                bot.send_command_packet(&format!("whisper {username} ain't no way lil bro"));
                println!("Denied teleport request from {username}");
            }
        }
        Dm(DirectMessage { from: "me", .. }) => (),
        Dm(DirectMessage { from, .. }) if from == bot.username().as_str() => (),
        Dm(DirectMessage {
            from,
            to: "me",
            command,
        }) => {
            println!("DM from {from}: {command:?}");
            if !WHITELIST.contains(&from.to_string()) {
                bot.send_command_packet(&format!(
                    "whisper {from} You are not allowed to send commands"
                ));
                return Ok(());
            }
            use Command::*;
            match command {
                // Handle spawn command if this is the master bot
                Spawn(slave_name) if bot.username() == MASTER_USERNAME.as_str() => {
                    let db_pool = DB_POOL.get().with_context(|| "DB was not initialized!")?;
                    db::add_slave(db_pool, slave_name).await?;
                    spawn_slave_bot(slave_name.to_string())?;
                    bot.send_command_packet(&format!(
                        "whisper {from} Spawned slave bot: {slave_name}"
                    ));
                    return Ok(());
                }
                EchoGlobal(text) => bot.send_chat_packet(text),
                TeleportAsk(username) => bot.send_command_packet(&format!("tpask {username}")),
                Disconnect => bot.disconnect(),
                Help(cmd) => {
                    let answer = match cmd {
                        Some("spawn") => "Usage: /spawn <username> - Spawn/connect a new slave bot with the given username",
                        Some("echo") => "Usage: /echo <message> - Echo a message to the server in the global chat",
                        Some("tpask") => "Usage: /tpask <username> - The bot will /tpask the player with the given username",
                        Some("disconnect") => "Usage: /disconnect - Disconnect the bot from the server",
                        Some("help") => "Usage: /help [command] - Show help for the given command",
                        Some(_) => "Unrecognized command",
                        None => "Commands: spawn, echo, tpask, disconnect, help",
                    };
                    bot.send_command_packet(&format!("whisper {from} {answer}"));
                }
                Unrecognized(content) => {
                    println!("Unrecognized command received from {from}: {content}");
                    bot.send_command_packet(&format!(
                        "whisper {from} Unrecognized command. See `/whisper bot help` for a list of commands"
                    ));
                }
                _ => (),
            }

            // bot.send_command_packet(&format!("whisper {from} {content}"));
        }
        Dm(_) => println!("DM destined to someone else"),
        Unknown(msg) => {
            println!("Unhandled message: {}", msg)
        }
    }
    Ok(())
}
