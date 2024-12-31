use azalea::{prelude::*, Account, Client, ClientBuilder, Event, Event as AzaleaEvent};
use bevy_ecs::component::StorageType;
use bevy_ecs::prelude::Component;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use sqlx::SqlitePool;

mod utils;
mod db;
use utils::ServerMessage;

const WHITELIST: [&str; 3] = ["DavideGamer38", "Its_Koala", "Gr3el_"];
const MASTER_USERNAME: &str = "bot";
const MASTER_PASSWORD: &str = "iamabot";
const PASSWORD_SALT_SECRET: &str = "JIUADSIDJSAJDSAJ";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db_pool = db::init_db().await?;
    let db_pool = Arc::new(db_pool);

    // Start master bot
    let master_account = Account::offline(MASTER_USERNAME);
    let state = State {
        password: Arc::new(Mutex::new(String::new())),
        has_logged_in: Arc::new(AtomicBool::new(false)), 
        db_pool: Arc::clone(&db_pool),
    };

    // Start saved slave bots
    let slaves = db::get_slaves(&db_pool).await?;
    for slave in slaves {
        spawn_slave_bot(slave, Arc::clone(&db_pool));
    }

    // Start master bot in the background
    let handler = move |client: Client, event: Event, state: State| -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> {
        Box::pin(handle(client, event, state))
    };

    tokio::spawn(async move {
        ClientBuilder::new()
            .set_handler(handler)
            .set_state(state)
            .start(master_account, "mc.brailor.me")
            .await
            .map_err(|e| anyhow::anyhow!("Failed to start bot: {}", e))
    });

    // Keep the main task running
    tokio::signal::ctrl_c().await?;
    Ok(())
}

fn spawn_slave_bot(username: String, db_pool: Arc<SqlitePool>) {
    let handler = move |client: Client, event: Event, state: State| -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> {
        Box::pin(handle(client, event, state))
    };

    tokio::spawn(async move {
        let account = Account::offline(&username);
        let state = State {
            password: Arc::new(Mutex::new(String::new())),
            has_logged_in: Arc::new(AtomicBool::new(false)), 
            db_pool,
        };

        loop {
            let result = ClientBuilder::new()
                .set_handler(handler)
                .set_state(state.clone())
                .start(account.clone(), "mc.brailor.me")
                .await;

            match result {
                Ok(_) => break, // Bot exited successfully
                Err(e) => {
                    eprintln!("Slave bot {} error: {}", username, e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                }
            }
        }
    });
}

#[derive(Clone)]
pub struct State {
    password: Arc<Mutex<String>>,
    has_logged_in: Arc<AtomicBool>,
    db_pool: Arc<SqlitePool>,
}

impl Component for State {
    const STORAGE_TYPE: StorageType = StorageType::Table;
}

impl Default for State {
    fn default() -> Self {
        panic!("State must be initialized with a database connection");
    }
}

impl State {
    fn password(&self) -> anyhow::Result<MutexGuard<'_, String>> {
        self.password
            .lock()
            .map_err(|_| anyhow::anyhow!("Lock failed!"))
    }
}

fn generate_password(username: &str) -> String {
    sha256::digest(format!(
        "{PASSWORD_SALT_SECRET}{username}{PASSWORD_SALT_SECRET}"
    ))
}

async fn login(bot: &Client, event: &AzaleaEvent, state: &State) -> anyhow::Result<bool> {
    if state.has_logged_in.load(Ordering::SeqCst) {
        return Ok(true);
    }
    match event {
        Event::Chat(m) if m.username().is_none() => {
            let message = m.message().to_string();
            println!("Received: {}", message);

            if message.contains("/register <password>") {
                let password = match bot.username().as_str() {
                    MASTER_USERNAME => MASTER_PASSWORD.to_string(),
                    username => generate_password(username),
                };
                *state.password()? = password;
                bot.send_command_packet(&format!("register {}", state.password()?));
                println!("Sent register command!");
            }

            if message.contains("/login <password>") {
                bot.send_command_packet(&format!("login {}", MASTER_PASSWORD));
                println!("Sent login command!");
            }

            if message.contains("Successful login!") {
                state.has_logged_in.store(true, Ordering::SeqCst);
                return Ok(true);
            }
        }
        _ => {}
    }
    Ok(false)
}

// Then modify your handle function to use this:
async fn handle(bot: Client, event: AzaleaEvent, state: State) -> anyhow::Result<()> {
    if !login(&bot, &event, &state).await? {
        return Ok(());
    }

    match event {
        Event::Chat(m) => {
            let message = m.message().to_string();
            println!("Received: {}", message);

            match utils::parse_server_message(&message) {
                ServerMessage::LoginPrompt(prompt_type) => {
                    let password = if bot.username() == MASTER_USERNAME {
                        MASTER_PASSWORD.to_string()
                    } else {
                        generate_password(&bot.username())
                    };
                    bot.send_command_packet(&format!("{} {}", prompt_type, password));
                    println!("Sent {} command!", prompt_type);
                }
                ServerMessage::LoginSuccess => {
                    println!("Successfully logged in!");
                }
                ServerMessage::TeleportRequest(username) => {
                    println!("Received teleport request from: {}", username);
                    if WHITELIST.contains(&username.as_str()) {
                        bot.send_command_packet(&format!("tpaccept {}", username));
                        println!("Accepted teleport request from {}", username);
                    } else {
                        bot.send_command_packet(&format!("tpdeny {}", username));
                        bot.send_command_packet(&format!("whisper {} ain't no way bro", username));
                        println!("Denied teleport request from {}", username);
                    }
                }
                ServerMessage::DirectMessage { from, to, content } => {
                    if [bot.username().as_str(), "me"].contains(&from.as_str()) {
                        // Ignore messages from myself
                        return Ok(());
                    }
                    println!("DM from {} to {}: {}", from, to, content);

                    // Handle spawn command if this is the master bot
                    if bot.username() == MASTER_USERNAME && content.starts_with("spawn ") {
                        if let Some(slave_name) = content.strip_prefix("spawn ") {
                            db::add_slave(&state.db_pool, slave_name).await?;
                            spawn_slave_bot(slave_name.to_string(), Arc::clone(&state.db_pool));
                            bot.send_command_packet(&format!("whisper {from} Spawned slave bot: {slave_name}"));
                            return Ok(());
                        }
                    }

                    bot.send_command_packet(&format!("whisper {from} {content}"));
                }
                ServerMessage::Unknown(msg) => {
                    println!("Unhandled message: {}", msg);
                }
            }
        }
        Event::Disconnect(Some(reason)) => {
            println!("Disconnected: {}", reason);
        }
        _ => {}
    }

    Ok(())
}
