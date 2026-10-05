mod api;
mod app;
mod ui;

use std::{io, time::Duration};

use api::{NetworkCommand, start_network};
use api_types::CredentialsRequest;
use app::{App, AuthFocus, AuthMode, ChatFocus};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::DefaultTerminal;

const MAX_USERNAME_CHARS: usize = 32;
const MAX_PASSWORD_CHARS: usize = 128;
const MAX_MESSAGE_CHARS: usize = 500;

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    ratatui::run(app)?;
    Ok(())
}

fn app(terminal: &mut DefaultTerminal) -> io::Result<()> {
    let (commands, network_events) = start_network();
    let mut app = App::default();

    loop {
        while let Ok(event) = network_events.try_recv() {
            app.handle_network_event(event);
        }

        terminal.draw(|frame| ui::render(frame, &app))?;
        if !event::poll(Duration::from_millis(50))? {
            continue;
        }

        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            break Ok(());
        }

        if app.current_user.is_some() {
            handle_chat_key(&mut app, &commands, key.code, key.modifiers);
        } else {
            handle_auth_key(&mut app, &commands, key.code, key.modifiers);
        }
    }
}

fn handle_auth_key(
    app: &mut App,
    commands: &tokio::sync::mpsc::UnboundedSender<NetworkCommand>,
    code: KeyCode,
    modifiers: KeyModifiers,
) {
    match code {
        KeyCode::Esc => {
            app.auth_pending = false;
            app.status = "Press Ctrl+C to exit.".to_owned();
        }
        KeyCode::F(2) if !app.auth_pending => {
            app.auth_mode = match app.auth_mode {
                AuthMode::Login => AuthMode::Register,
                AuthMode::Register => AuthMode::Login,
            };
            app.status = match app.auth_mode {
                AuthMode::Login => "Login mode selected.".to_owned(),
                AuthMode::Register => "Registration mode selected.".to_owned(),
            };
        }
        KeyCode::Tab => {
            app.auth_focus = match app.auth_focus {
                AuthFocus::Username => AuthFocus::Password,
                AuthFocus::Password => AuthFocus::Username,
            };
        }
        KeyCode::Enter if app.auth_focus == AuthFocus::Username => {
            app.auth_focus = AuthFocus::Password;
        }
        KeyCode::Enter if !app.auth_pending => {
            if app.username.trim().is_empty() || app.password.is_empty() {
                app.status = "Enter a username and password.".to_owned();
                return;
            }
            let request = NetworkCommand::Authenticate {
                credentials: CredentialsRequest {
                    username: app.username.trim().to_owned(),
                    password: app.password.clone(),
                },
                register: app.auth_mode == AuthMode::Register,
            };
            match commands.send(request) {
                Ok(()) => {
                    app.auth_pending = true;
                    app.status = match app.auth_mode {
                        AuthMode::Login => "Signing in...".to_owned(),
                        AuthMode::Register => "Creating account...".to_owned(),
                    };
                }
                Err(_) => app.status = "Network task is unavailable.".to_owned(),
            }
        }
        KeyCode::Backspace if !app.auth_pending => match app.auth_focus {
            AuthFocus::Username => {
                app.username.pop();
            }
            AuthFocus::Password => {
                app.password.pop();
            }
        },
        KeyCode::Char(character)
            if !app.auth_pending
                && !modifiers.contains(KeyModifiers::CONTROL)
                && !modifiers.contains(KeyModifiers::ALT) =>
        {
            match app.auth_focus {
                AuthFocus::Username if app.username.chars().count() < MAX_USERNAME_CHARS => {
                    app.username.push(character);
                }
                AuthFocus::Password if app.password.chars().count() < MAX_PASSWORD_CHARS => {
                    app.password.push(character);
                }
                _ => {}
            }
        }
        _ => {}
    }
}

fn handle_chat_key(
    app: &mut App,
    commands: &tokio::sync::mpsc::UnboundedSender<NetworkCommand>,
    code: KeyCode,
    modifiers: KeyModifiers,
) {
    match code {
        KeyCode::Esc if !app.logout_pending => match commands.send(NetworkCommand::Logout) {
            Ok(()) => {
                app.logout_pending = true;
                app.status = "Signing out...".to_owned();
            }
            Err(_) => app.status = "Network task is unavailable.".to_owned(),
        },
        KeyCode::Tab => {
            app.chat_focus = match app.chat_focus {
                ChatFocus::Recipient => ChatFocus::Message,
                ChatFocus::Message => ChatFocus::Recipient,
            };
        }
        KeyCode::Enter if app.chat_focus == ChatFocus::Recipient => {
            let recipient = app.recipient_username.trim().to_ascii_lowercase();
            if recipient.is_empty() {
                app.status = "Enter the username you want to message.".to_owned();
                return;
            }
            app.recipient_username = recipient.clone();
            app.messages.clear();
            match commands.send(NetworkCommand::LoadConversation {
                recipient_username: recipient,
            }) {
                Ok(()) => app.status = "Loading conversation...".to_owned(),
                Err(_) => app.status = "Network task is unavailable.".to_owned(),
            }
        }
        KeyCode::Enter if app.chat_focus == ChatFocus::Message => {
            let recipient = app.recipient_username.trim().to_owned();
            let content = app.message_input.trim().to_owned();
            if recipient.is_empty() {
                app.status = "Choose a recipient first.".to_owned();
                return;
            }
            if content.is_empty() {
                return;
            }
            if !app.connected {
                app.status = "WebSocket is disconnected. Sign in again to reconnect.".to_owned();
                return;
            }
            match commands.send(NetworkCommand::SendMessage {
                recipient_username: recipient,
                content,
            }) {
                Ok(()) => app.message_input.clear(),
                Err(_) => app.status = "Network task is unavailable.".to_owned(),
            }
        }
        KeyCode::Backspace => match app.chat_focus {
            ChatFocus::Recipient => {
                app.recipient_username.pop();
            }
            ChatFocus::Message => {
                app.message_input.pop();
            }
        },
        KeyCode::Char(character)
            if !modifiers.contains(KeyModifiers::CONTROL)
                && !modifiers.contains(KeyModifiers::ALT) =>
        {
            match app.chat_focus {
                ChatFocus::Recipient
                    if app.recipient_username.chars().count() < MAX_USERNAME_CHARS =>
                {
                    app.recipient_username.push(character);
                }
                ChatFocus::Message if app.message_input.chars().count() < MAX_MESSAGE_CHARS => {
                    app.message_input.push(character);
                }
                _ => {}
            }
        }
        _ => {}
    }
}
