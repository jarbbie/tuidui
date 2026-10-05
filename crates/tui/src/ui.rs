use api_types::UserDto;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::app::{App, AuthFocus, AuthMode, ChatFocus};

pub fn render(frame: &mut Frame, app: &App) {
    if let Some(user) = &app.current_user {
        render_chat(frame, app, user);
    } else {
        render_auth(frame, app);
    }
}

fn render_auth(frame: &mut Frame, app: &App) {
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .margin(2)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(1),
        ])
        .split(frame.area());
    let mode = match app.auth_mode {
        AuthMode::Login => "Login",
        AuthMode::Register => "Register account",
    };

    frame.render_widget(
        Paragraph::new("Tuidui account").block(
            Block::default()
                .title(format!("{mode} (F2 changes mode)"))
                .borders(Borders::ALL),
        ),
        areas[0],
    );
    render_auth_field(
        frame,
        areas[1],
        "Username",
        &app.username,
        app.auth_focus == AuthFocus::Username,
        false,
    );
    render_auth_field(
        frame,
        areas[2],
        "Password",
        &app.password,
        app.auth_focus == AuthFocus::Password,
        true,
    );
    frame.render_widget(
        Paragraph::new("Tab: select field | Enter: continue or submit | Ctrl+C: exit"),
        areas[3],
    );
    frame.render_widget(
        Paragraph::new(app.status.as_str()).style(Style::default().fg(Color::DarkGray)),
        areas[4],
    );
}

fn render_auth_field(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    value: &str,
    active: bool,
    masked: bool,
) {
    let displayed = if masked {
        "•".repeat(value.chars().count())
    } else {
        value.to_owned()
    };
    let border_color = if active { Color::Cyan } else { Color::DarkGray };
    frame.render_widget(
        Paragraph::new(format!("{displayed}_")).block(
            Block::default()
                .title(label)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color)),
        ),
        area,
    );
}

fn render_chat(frame: &mut Frame, app: &App, user: &UserDto) {
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(frame.area());

    let connection_status = if app.connected {
        "WebSocket connected"
    } else {
        "WebSocket disconnected"
    };
    frame.render_widget(
        Paragraph::new(format!(
            "Signed in as @{} | {connection_status}",
            user.username
        ))
        .block(Block::default().borders(Borders::ALL)),
        areas[0],
    );

    let recipient_color = if app.chat_focus == ChatFocus::Recipient {
        Color::Cyan
    } else {
        Color::DarkGray
    };
    frame.render_widget(
        Paragraph::new(format!("@{}_", app.recipient_username)).block(
            Block::default()
                .title("To (Tab switches to message)")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(recipient_color)),
        ),
        areas[1],
    );

    let recipient = app.recipient_username.trim();
    let messages = app
        .messages
        .iter()
        .filter(|message| {
            !recipient.is_empty()
                && ((message.from.id == user.id
                    && message.to.username.eq_ignore_ascii_case(recipient))
                    || (message.to.id == user.id
                        && message.from.username.eq_ignore_ascii_case(recipient)))
        })
        .map(|message| {
            if message.from.id == user.id {
                format!("You: {}", message.content)
            } else {
                format!("@{}: {}", message.from.username, message.content)
            }
        })
        .collect::<Vec<_>>();
    let inner_width = areas[2].width.saturating_sub(2).max(1) as usize;
    let total_rows = messages
        .iter()
        .map(|message| message.chars().count().max(1).div_ceil(inner_width))
        .sum::<usize>();
    let visible_rows = areas[2].height.saturating_sub(2) as usize;
    let scroll = total_rows
        .saturating_sub(visible_rows)
        .min(u16::MAX as usize) as u16;
    frame.render_widget(
        Paragraph::new(messages.join("\n"))
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0))
            .block(
                Block::default()
                    .title(format!(
                        "Conversation with {}",
                        if recipient.is_empty() {
                            "(choose a username)"
                        } else {
                            recipient
                        }
                    ))
                    .borders(Borders::ALL),
            ),
        areas[2],
    );

    let message_color = if app.chat_focus == ChatFocus::Message {
        Color::Cyan
    } else {
        Color::DarkGray
    };
    frame.render_widget(
        Paragraph::new(format!("{}_", app.message_input)).block(
            Block::default()
                .title("Message (Enter sends; Esc signs out)")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(message_color)),
        ),
        areas[3],
    );
    frame.render_widget(
        Paragraph::new(app.status.as_str()).style(Style::default().fg(Color::DarkGray)),
        areas[4],
    );
}
