use std::{
    sync::mpsc::{self, Receiver, Sender},
    thread,
};

use api_types::{
    AuthResponse, ClientEvent, CredentialsRequest, ErrorResponse, MessageDto, ServerEvent, UserDto,
};
use futures_util::{SinkExt, StreamExt};
use reqwest::Client;
use tokio::{net::TcpStream, runtime::Builder, sync::mpsc as tokio_mpsc};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{
        Message,
        client::IntoClientRequest,
        http::{HeaderValue, header::AUTHORIZATION},
    },
};

const HTTP_BASE: &str = "http://127.0.0.1:3000";
const WEBSOCKET_URL: &str = "ws://127.0.0.1:3000/ws";

type ClientSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub fn start_network() -> (
    tokio_mpsc::UnboundedSender<NetworkCommand>,
    Receiver<NetworkEvent>,
) {
    let (command_sender, command_receiver) = tokio_mpsc::unbounded_channel();
    let (event_sender, event_receiver) = mpsc::channel();

    let _network_thread = thread::spawn(move || {
        let runtime = Builder::new_current_thread().enable_all().build();
        match runtime {
            Ok(runtime) => runtime.block_on(run_network(command_receiver, event_sender.clone())),
            Err(error) => send_event(
                &event_sender,
                NetworkEvent::Status(format!("Could not start network runtime: {error}")),
            ),
        }
    });

    (command_sender, event_receiver)
}

async fn run_network(
    mut commands: tokio_mpsc::UnboundedReceiver<NetworkCommand>,
    events: Sender<NetworkEvent>,
) {
    let client = Client::new();

    while let Some(command) = commands.recv().await {
        match command {
            NetworkCommand::Authenticate {
                credentials,
                register,
            } => {
                authenticate(&client, credentials, register, &mut commands, &events).await;
            }
            _ => send_event(
                &events,
                NetworkEvent::Status("Log in before using messages.".to_owned()),
            ),
        }
    }
}

async fn authenticate(
    client: &Client,
    credentials: CredentialsRequest,
    register: bool,
    commands: &mut tokio_mpsc::UnboundedReceiver<NetworkCommand>,
    events: &Sender<NetworkEvent>,
) {
    let endpoint = if register {
        "/auth/register"
    } else {
        "/auth/login"
    };
    let response = match client
        .post(format!("{HTTP_BASE}{endpoint}"))
        .json(&credentials)
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            send_event(
                events,
                NetworkEvent::Status(format!("Could not reach the server: {error}")),
            );
            return;
        }
    };
    if !response.status().is_success() {
        send_event(events, NetworkEvent::Status(response_error(response).await));
        return;
    }

    let auth = match response.json::<AuthResponse>().await {
        Ok(auth) => auth,
        Err(error) => {
            send_event(
                events,
                NetworkEvent::Status(format!("Could not read the sign-in response: {error}")),
            );
            return;
        }
    };
    let socket = match connect_authenticated(&auth.token).await {
        Ok(socket) => socket,
        Err(error) => {
            let _ = client
                .post(format!("{HTTP_BASE}/auth/logout"))
                .bearer_auth(&auth.token)
                .send()
                .await;
            send_event(
                events,
                NetworkEvent::Status(format!(
                    "Signed in, but WebSocket connection failed: {error}"
                )),
            );
            return;
        }
    };

    send_event(events, NetworkEvent::Authenticated(auth.user));
    run_authenticated_session(client, auth.token, commands, events, socket).await;
}

async fn connect_authenticated(token: &str) -> Result<ClientSocket, String> {
    let mut request = WEBSOCKET_URL
        .into_client_request()
        .map_err(|error| format!("Could not create WebSocket request: {error}"))?;
    let authorization = HeaderValue::from_str(&format!("Bearer {token}"))
        .map_err(|error| format!("Could not create authorization header: {error}"))?;
    request.headers_mut().insert(AUTHORIZATION, authorization);

    connect_async(request)
        .await
        .map(|(socket, _)| socket)
        .map_err(|error| error.to_string())
}

async fn run_authenticated_session(
    client: &Client,
    token: String,
    commands: &mut tokio_mpsc::UnboundedReceiver<NetworkCommand>,
    events: &Sender<NetworkEvent>,
    socket: ClientSocket,
) {
    let (mut socket_sender, mut socket_receiver) = socket.split();
    let mut logged_out = false;

    loop {
        tokio::select! {
            command = commands.recv() => {
                let Some(command) = command else {
                    break;
                };
                match command {
                    NetworkCommand::Authenticate { .. } => send_event(
                        events,
                        NetworkEvent::Status("You are already signed in.".to_owned()),
                    ),
                    NetworkCommand::LoadConversation { recipient_username } => {
                        load_conversation(client, &token, recipient_username, events).await;
                    }
                    NetworkCommand::SendMessage {
                        recipient_username,
                        content,
                    } => {
                        let request = ClientEvent::SendMessage {
                            recipient_username,
                            content,
                        };
                        match serde_json::to_string(&request) {
                            Ok(text) => {
                                if let Err(error) = socket_sender.send(Message::Text(text.into())).await {
                                    send_event(
                                        events,
                                        NetworkEvent::Disconnected(format!("Could not send message: {error}")),
                                    );
                                    break;
                                }
                            }
                            Err(error) => send_event(
                                events,
                                NetworkEvent::Status(format!("Could not encode message: {error}")),
                            ),
                        }
                    }
                    NetworkCommand::Logout => {
                        match client
                            .post(format!("{HTTP_BASE}/auth/logout"))
                            .bearer_auth(&token)
                            .send()
                            .await
                        {
                            Ok(response) if response.status().is_success() => {
                                logged_out = true;
                                send_event(events, NetworkEvent::LoggedOut);
                                break;
                            }
                            Ok(response) => send_event(
                                events,
                                NetworkEvent::Status(response_error(response).await),
                            ),
                            Err(error) => send_event(
                                events,
                                NetworkEvent::Status(format!("Could not log out: {error}")),
                            ),
                        }
                    }
                }
            }
            frame = socket_receiver.next() => {
                match frame {
                    Some(Ok(Message::Text(payload))) => {
                        match serde_json::from_str::<ServerEvent>(payload.as_str()) {
                            Ok(ServerEvent::MessageCreated { message }) => {
                                send_event(events, NetworkEvent::Message(message));
                            }
                            Ok(ServerEvent::Error { message }) => {
                                send_event(events, NetworkEvent::Status(message));
                            }
                            Err(error) => send_event(
                                events,
                                NetworkEvent::Status(format!("Invalid server message: {error}")),
                            ),
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        send_event(events, NetworkEvent::Status("WebSocket disconnected.".to_owned()));
                        break;
                    }
                    Some(Err(error)) => {
                        send_event(
                            events,
                            NetworkEvent::Status(format!("WebSocket receive error: {error}")),
                        );
                        break;
                    }
                    Some(Ok(_)) => {}
                }
            }
        }
    }

    if !logged_out {
        let _ = client
            .post(format!("{HTTP_BASE}/auth/logout"))
            .bearer_auth(&token)
            .send()
            .await;
        send_event(events, NetworkEvent::LoggedOut);
    }
}

async fn load_conversation(
    client: &Client,
    token: &str,
    recipient_username: String,
    events: &Sender<NetworkEvent>,
) {
    let response = match client
        .get(format!("{HTTP_BASE}/messages"))
        .query(&[("with", recipient_username.as_str())])
        .bearer_auth(token)
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            send_event(
                events,
                NetworkEvent::Status(format!("Could not load conversation: {error}")),
            );
            return;
        }
    };
    if !response.status().is_success() {
        send_event(events, NetworkEvent::Status(response_error(response).await));
        return;
    }

    match response.json::<Vec<MessageDto>>().await {
        Ok(messages) => send_event(
            events,
            NetworkEvent::ConversationLoaded {
                recipient_username,
                messages,
            },
        ),
        Err(error) => send_event(
            events,
            NetworkEvent::Status(format!("Could not read conversation: {error}")),
        ),
    }
}

async fn response_error(response: reqwest::Response) -> String {
    let status = response.status();
    response
        .json::<ErrorResponse>()
        .await
        .map(|error| error.message)
        .unwrap_or_else(|_| format!("Server returned HTTP {status}."))
}

fn send_event(sender: &Sender<NetworkEvent>, event: NetworkEvent) {
    let _ = sender.send(event);
}

pub enum NetworkCommand {
    Authenticate {
        credentials: CredentialsRequest,
        register: bool,
    },
    LoadConversation {
        recipient_username: String,
    },
    SendMessage {
        recipient_username: String,
        content: String,
    },
    Logout,
}

pub enum NetworkEvent {
    Authenticated(UserDto),
    LoggedOut,
    ConversationLoaded {
        recipient_username: String,
        messages: Vec<MessageDto>,
    },
    Message(MessageDto),
    Disconnected(String),
    Status(String),
}
