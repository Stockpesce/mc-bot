use azalea::entity::metadata::Player;
use azalea::entity::{EyeHeight, LocalEntity, Position};
use azalea::nearest_entity::EntityFinder;
use azalea::prelude::GameTick;
use azalea::LookAtEvent;
use bevy_app::Plugin;
use bevy_ecs::{
    prelude::{Entity, EventWriter},
    query::With,
    system::Query,
};
use std::ops::Not;

use anyhow::Context;
use lazy_regex::regex;

pub fn runtime() -> anyhow::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .with_context(|| "Failed to build the tokio runtime")
}

#[derive(Debug, PartialEq)]
pub enum ServerMessage<'a> {
    RegisterPrompt,
    LoginPrompt,
    LoginSuccess,
    TeleportRequest(&'a str),
    Dm(DirectMessage<'a>),
    Unknown(&'a str),
}

impl ServerMessage<'_> {
    pub fn parse(message: &str) -> ServerMessage<'_> {
        use ServerMessage::*;

        if message.contains("/register <password>") {
            return RegisterPrompt;
        }
        if message.contains("/login <password>") {
            return LoginPrompt;
        }
        if message.contains("Successful login!") {
            return LoginSuccess;
        }
        if let Some((username, _)) = message.split_once(" has requested to teleport to you") {
            return TeleportRequest(username);
        }
        if let Some(dm) = DirectMessage::parse(message) {
            return Dm(dm);
        }
        Unknown(message)
    }
}

#[derive(Debug, PartialEq)]
pub struct DirectMessage<'a> {
    pub from: &'a str,
    pub to: &'a str,
    pub command: Command<'a>,
}

impl DirectMessage<'_> {
    /*pub fn parse(input: &str) -> Option<DirectMessage<'_>> {
        let s = input.find('[')?;
        let e = input[s..].find(']')?;

        let users = &input[s + 1..e];
        let content = input[e + 1..].trim();

        let (from, to) = users.split_once(" -> ")?;
        Some(DirectMessage {
            from,
            to,
            command: content,
        })
    }*/

    pub fn parse(input: &str) -> Option<DirectMessage<'_>> {
        let captures = regex!(
            r"^\[(?<from>[a-zA-Z0-9_]{2,16}) -> (?<to>[a-zA-Z0-9_]{2,16})\] (?<content>.+)$"
        )
        .captures(input)?;

        let from = captures.name("from")?.as_str();
        let to = captures.name("to")?.as_str();
        let content = captures.name("content")?.as_str();

        Some(DirectMessage {
            from,
            to,
            command: Command::parse(content),
        })
    }
}

#[derive(Debug, PartialEq)]
pub enum Command<'a> {
    /// `spawn <name>` crea un nuovo bot con il nome indicato
    Spawn(&'a str),
    /// `tpask <username>` manda una richiesta di teleport al giocatore che ha mandato il messaggio
    TeleportAsk(&'a str),
    /// `disconnect / kill` logoutdel bot
    Disconnect,
    /// `echo <message>` Il bot scrive in chat il messaggio indicato
    EchoGlobal(&'a str),
    /// Mostra l'help
    Help(Option<&'a str>),
    Unrecognized(&'a str),
}

fn parse_command_parts(input: &str) -> Option<(&str, &str)> {
    let captures = regex!(r"^(?<command>\S+)(?: (?<params>.+))?$").captures(input)?;

    let command = captures.name("command")?.as_str();
    // optional params
    let params = captures.name("params").map(|m| m.as_str()).unwrap_or("");

    Some((command, params))
}

// a che serve here?
// 1. sono a casa mia e devo andare in miniera
// 2. scrivo /whisper bot here casa
// 3. il bot mi chiede il /tpask e lo accetto
// 4. vado in miniera
// 5 faccio /tpask casa e il bot mi teletrasporta
//
// :CCC

fn parse_spawn_command_params(params: &str) -> Option<Command<'_>> {
    let captures = regex!(r"^(?<name>[a-zA-Z0-9_]{2,16})$").captures(params)?;
    Some(Command::Spawn(captures.name("name")?.as_str()))
}

impl Command<'_> {
    pub fn parse(input: &str) -> Command<'_> {
        let Some((command, params)) = parse_command_parts(input) else {
            return Command::Unrecognized(input);
        };
        match command {
            "spawn" => parse_spawn_command_params(params).unwrap_or(Command::Unrecognized(input)),
            "echo" => Command::EchoGlobal(params),
            "help" => Command::Help(params.is_empty().not().then_some(params)),
            "tpask" => Command::TeleportAsk(params),
            "disconnect" => Command::Disconnect,
            _ => Command::Unrecognized(input),
        }
    }
}

// https://github.com/azalea-rs/azalea/blob/615d8f9d2ac56b3244d328587243301da253eafd/azalea/examples/nearest_entity.rs

pub struct LookAtStuffPlugin;
impl Plugin for LookAtStuffPlugin {
    fn build(&self, app: &mut bevy_app::App) {
        app.add_systems(GameTick, (look_at_everything,));
    }
}

fn look_at_everything(
    bots: Query<Entity, (With<LocalEntity>, With<Player>)>,
    entities: EntityFinder,
    entity_positions: Query<(&Position, Option<&EyeHeight>)>,
    mut look_at_event: EventWriter<LookAtEvent>,
) {
    for bot_id in bots.iter() {
        let Some(entity) = entities.nearest_to_entity(bot_id, 16.0) else {
            continue;
        };

        let (position, eye_height) = entity_positions.get(entity).unwrap();

        let mut look_target = **position;
        if let Some(eye_height) = eye_height {
            look_target.y += **eye_height as f64;
        }

        look_at_event.send(LookAtEvent {
            entity: bot_id,
            position: look_target,
        });
    }
}
