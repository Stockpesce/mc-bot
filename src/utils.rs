#[derive(Debug, PartialEq)]
pub struct Message {
    pub from: String,
    pub to: String,
    pub content: String,
}

pub fn parse_dm_message(input: &str) -> Option<Message> {
    // Check if the input starts with '[' and contains ']'
    let (s, e) = input
        .find('[')
        .and_then(|start| input.find(']').map(|end| (start, end)))?;

    // Extract the usernames part and the message content
    let users = &input[s + 1..e];
    let content = input[e + 1..].trim().to_string();

    // Split usernames by "->"
    let users: Vec<&str> = users.split("->").map(|s| s.trim()).collect();

    // Ensure we have exactly two usernames
    if users.len() != 2 {
        return None;
    }

    Some(Message {
        from: users[0].to_string(),
        to: users[1].to_string(),
        content,
    })
}

#[derive(Debug)]
pub enum ServerMessage {
    LoginPrompt(String), // Contains the prompt message
    LoginSuccess,
    TeleportRequest(String), // Contains the username
    DirectMessage {
        from: String,
        to: String,
        content: String,
    },
    Unknown(String), // For unhandled messages
}

pub fn parse_server_message(message: &str) -> ServerMessage {
    // Login related messages
    if message.contains("/register <password>") {
        return ServerMessage::LoginPrompt("register".to_string());
    }
    if message.contains("/login <password>") {
        return ServerMessage::LoginPrompt("login".to_string());
    }
    if message.contains("Successful login!") {
        return ServerMessage::LoginSuccess;
    }

    // Teleport requests
    if message.contains("has requested to teleport to you") {
        if let Some(username) = message.split(" has requested").next() {
            return ServerMessage::TeleportRequest(username.to_string());
        }
    }

    // Direct messages - using your existing parse_message function
    if let Some(dm) = parse_dm_message(message) {
        return ServerMessage::DirectMessage {
            from: dm.from,
            to: dm.to,
            content: dm.content,
        };
    }

    // Default case for unhandled messages
    ServerMessage::Unknown(message.to_string())
}

// Example usage:
fn main() {
    let input = "[alice -> bob] Hello, how are you?";
    match parse_dm_message(input) {
        Some(msg) => println!("{:?}", msg),
        None => println!("Invalid message format"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_message() {
        let input = "[alice -> bob] Hello, how are you?";
        let expected = Message {
            from: "alice".to_string(),
            to: "bob".to_string(),
            content: "Hello, how are you?".to_string(),
        };
        assert_eq!(parse_dm_message(input), Some(expected));
    }

    #[test]
    fn test_invalid_format() {
        let input = "alice -> bob] Hello";
        assert_eq!(parse_dm_message(input), None);
    }

    #[test]
    fn test_missing_message() {
        let input = "[alice -> bob]";
        let expected = Message {
            from: "alice".to_string(),
            to: "bob".to_string(),
            content: "".to_string(),
        };
        assert_eq!(parse_dm_message(input), Some(expected));
    }
}
