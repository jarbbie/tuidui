use api_types::{MessageDto, UserDto};

use crate::api::NetworkEvent;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AuthMode {
    Login,
    Register,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AuthFocus {
    Username,
    Password,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ChatFocus {
    Recipient,
    Message,
}

pub struct App {
    pub auth_mode: AuthMode,
    pub auth_focus: AuthFocus,
    pub username: String,
    pub password: String,
    pub auth_pending: bool,
    pub current_user: Option<UserDto>,
    pub recipient_username: String,
    pub chat_focus: ChatFocus,
    pub message_input: String,
    pub messages: Vec<MessageDto>,
    pub status: String,
    pub connected: bool,
    pub logout_pending: bool,
}

impl Default for App {
    fn default() -> Self {
        Self {
            auth_mode: AuthMode::Login,
            auth_focus: AuthFocus::Username,
            username: String::new(),
            password: String::new(),
            auth_pending: false,
            current_user: None,
            recipient_username: String::new(),
            chat_focus: ChatFocus::Recipient,
            message_input: String::new(),
            messages: Vec::new(),
            status: "F2 switches between login and registration.".to_owned(),
            connected: false,
            logout_pending: false,
        }
    }
}

impl App {
    pub fn handle_network_event(&mut self, event: NetworkEvent) {
        match event {
            NetworkEvent::Authenticated(user) => {
                self.current_user = Some(user);
                self.password.clear();
                self.auth_pending = false;
                self.connected = true;
                self.status = "Signed in. Enter a username to open a conversation.".to_owned();
            }
            NetworkEvent::LoggedOut => {
                self.current_user = None;
                self.connected = false;
                self.logout_pending = false;
                self.auth_pending = false;
                self.recipient_username.clear();
                self.message_input.clear();
                self.messages.clear();
                self.status = "Signed out.".to_owned();
            }
            NetworkEvent::ConversationLoaded {
                recipient_username,
                messages,
            } => {
                if self
                    .recipient_username
                    .trim()
                    .eq_ignore_ascii_case(&recipient_username)
                {
                    self.messages = messages;
                    self.status = format!("Conversation with @{recipient_username} loaded.");
                }
            }
            NetworkEvent::Message(message) => {
                if let Some(user) = &self.current_user {
                    let recipient = self.recipient_username.trim();
                    let is_open_conversation = !recipient.is_empty()
                        && ((message.from.id == user.id
                            && message.to.username.eq_ignore_ascii_case(recipient))
                            || (message.to.id == user.id
                                && message.from.username.eq_ignore_ascii_case(recipient)));
                    if is_open_conversation
                        && !self
                            .messages
                            .iter()
                            .any(|existing| existing.id == message.id)
                    {
                        self.status = format!("New message from @{}.", message.from.username);
                        self.messages.push(message);
                    }
                }
            }
            NetworkEvent::Disconnected(message) => {
                self.connected = false;
                self.status = message;
            }
            NetworkEvent::Status(status) => {
                if self.auth_pending {
                    self.auth_pending = false;
                }
                if self.logout_pending {
                    self.logout_pending = false;
                }
                self.status = status;
            }
        }
    }
}
