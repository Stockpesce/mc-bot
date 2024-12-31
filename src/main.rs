use azalea::prelude::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

mod utils;
use utils::ServerMessage;

const WHITELIST: [&str; 3] = ["DavideGamer38", "Its_Koala", "Gr3el_"];
const MASTER_USERNAME: &str = "bot";
const MASTER_PASSWORD: &str = "iamabot";
const PASSWORD_SALT_SECRET: &str = "JIUADSIDJSAJDSAJ";

#[tokio::main]
async fn main() {
    let account = Account::offline(MASTER_USERNAME);

    ClientBuilder::new()
        .set_handler(handle)
        .start(account, "mc.brailor.me")
        .await
        .unwrap();
}

#[derive(Default, Clone, Component)]
pub struct State {
    password: Arc<Mutex<String>>,
    has_logged_in: Arc<AtomicBool>,
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

async fn login(bot: &Client, event: &Event, state: &State) -> anyhow::Result<bool> {
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
async fn handle(bot: Client, event: Event, state: State) -> anyhow::Result<()> {
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
