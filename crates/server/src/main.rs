use std::{
    collections::HashMap,
    env, io,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use api_types::{
    AuthResponse, ClientEvent, CredentialsRequest, ErrorResponse, MessageDto, ServerEvent, UserDto,
};
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use axum::{
    Json, Router,
    extract::{
        Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode, header::AUTHORIZATION},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, postgres::PgPoolOptions};
use tokio::{
    net::TcpListener,
    sync::{RwLock, mpsc},
};
use uuid::Uuid;

const MAX_MESSAGE_CHARS: usize = 500;
const MAX_HISTORY_MESSAGES: i64 = 100;

#[derive(Clone)]
struct AppState {
    pool: PgPool,
    connections: ConnectionManager,
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, message)
    }

    fn unauthorized() -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "Invalid or expired session.")
    }

    fn internal() -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error.")
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorResponse {
                message: self.message,
            }),
        )
            .into_response()
    }
}

#[derive(Clone, Default)]
struct ConnectionManager {
    inner: Arc<ConnectionManagerInner>,
}

#[derive(Default)]
struct ConnectionManagerInner {
    users: RwLock<HashMap<Uuid, HashMap<u64, mpsc::UnboundedSender<ServerEvent>>>>,
    next_connection_id: AtomicU64,
}

#[derive(Deserialize)]
struct MessageHistoryQuery {
    with: String,
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let database_url = env::var("DATABASE_URL").map_err(|_| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "DATABASE_URL must point to the PostgreSQL database",
        )
    })?;
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&database_url)
        .await?;

    sqlx::migrate!("../../migrations").run(&pool).await?;
    sqlx::query("DELETE FROM sessions WHERE expires_at <= now()")
        .execute(&pool)
        .await?;

    let state = AppState {
        pool,
        connections: ConnectionManager::default(),
    };
    let app = Router::new()
        .route("/", get(root))
        .route("/health", get(health))
        .route("/auth/register", post(register))
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/auth/me", get(me))
        .route("/messages", get(message_history))
        .route("/ws", get(websocket))
        .with_state(state);

    let listener = TcpListener::bind("127.0.0.1:3000").await?;
    println!("API is running at http://127.0.0.1:3000");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn register(
    State(state): State<AppState>,
    Json(credentials): Json<CredentialsRequest>,
) -> Result<(StatusCode, Json<AuthResponse>), ApiError> {
    let username = normalize_username(&credentials.username)?;
    validate_password(&credentials.password)?;
    let password_hash = hash_password(credentials.password).await?;
    let user = UserDto {
        id: Uuid::new_v4(),
        username,
    };

    let result = sqlx::query("INSERT INTO users (id, username, password_hash) VALUES ($1, $2, $3)")
        .bind(user.id)
        .bind(&user.username)
        .bind(password_hash)
        .execute(&state.pool)
        .await;

    match result {
        Ok(_) => {}
        Err(sqlx::Error::Database(error)) if error.code().as_deref() == Some("23505") => {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "That username is already taken.",
            ));
        }
        Err(error) => return Err(log_database_error("register account", error)),
    }

    let token = create_session(&state.pool, user.id).await?;
    Ok((StatusCode::CREATED, Json(AuthResponse { user, token })))
}

async fn login(
    State(state): State<AppState>,
    Json(credentials): Json<CredentialsRequest>,
) -> Result<Json<AuthResponse>, ApiError> {
    let username = normalize_username(&credentials.username)?;
    let account = sqlx::query_as::<_, (Uuid, String, String)>(
        "SELECT id, username, password_hash FROM users WHERE username = $1",
    )
    .bind(username)
    .fetch_optional(&state.pool)
    .await
    .map_err(|error| log_database_error("load account", error))?;

    let Some((id, username, password_hash)) = account else {
        return Err(ApiError::unauthorized());
    };
    let password = credentials.password;
    let stored_hash = password_hash.clone();
    let password_matches = tokio::task::spawn_blocking(move || {
        let Ok(parsed_hash) = PasswordHash::new(&stored_hash) else {
            return false;
        };
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed_hash)
            .is_ok()
    })
    .await
    .map_err(|error| {
        eprintln!("password verification task failed: {error}");
        ApiError::internal()
    })?;

    if !password_matches {
        return Err(ApiError::unauthorized());
    }

    let user = UserDto { id, username };
    let token = create_session(&state.pool, user.id).await?;
    Ok(Json(AuthResponse { user, token }))
}

async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Result<StatusCode, ApiError> {
    authenticate(&headers, &state.pool).await?;
    let token_hash = hash_session_token(bearer_token(&headers)?);
    sqlx::query("DELETE FROM sessions WHERE token_hash = $1")
        .bind(token_hash)
        .execute(&state.pool)
        .await
        .map_err(|error| log_database_error("end session", error))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn me(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<UserDto>, ApiError> {
    Ok(Json(authenticate(&headers, &state.pool).await?))
}

async fn message_history(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<MessageHistoryQuery>,
) -> Result<Json<Vec<MessageDto>>, ApiError> {
    let user = authenticate(&headers, &state.pool).await?;
    let peer_username = normalize_username(&query.with)
        .map_err(|_| ApiError::new(StatusCode::NOT_FOUND, "User not found."))?;
    let peer =
        sqlx::query_as::<_, (Uuid, String)>("SELECT id, username FROM users WHERE username = $1")
            .bind(peer_username)
            .fetch_optional(&state.pool)
            .await
            .map_err(|error| log_database_error("load message recipient", error))?
            .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "User not found."))?;

    if peer.0 == user.id {
        return Err(ApiError::bad_request(
            "You cannot open a conversation with yourself.",
        ));
    }

    let mut rows = sqlx::query_as::<_, (Uuid, Uuid, String, Uuid, String, String)>(
        "SELECT m.id, sender.id, sender.username, recipient.id, recipient.username, m.content
         FROM direct_messages AS m
         JOIN users AS sender ON sender.id = m.sender_id
         JOIN users AS recipient ON recipient.id = m.recipient_id
         WHERE (m.sender_id = $1 AND m.recipient_id = $2)
            OR (m.sender_id = $2 AND m.recipient_id = $1)
         ORDER BY m.created_at DESC
         LIMIT $3",
    )
    .bind(user.id)
    .bind(peer.0)
    .bind(MAX_HISTORY_MESSAGES)
    .fetch_all(&state.pool)
    .await
    .map_err(|error| log_database_error("load conversation", error))?;
    rows.reverse();

    let messages = rows
        .into_iter()
        .map(
            |(id, from_id, from_username, to_id, to_username, content)| MessageDto {
                id,
                from: UserDto {
                    id: from_id,
                    username: from_username,
                },
                to: UserDto {
                    id: to_id,
                    username: to_username,
                },
                content,
            },
        )
        .collect();
    Ok(Json(messages))
}

async fn websocket(
    State(state): State<AppState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    let user = authenticate(&headers, &state.pool).await?;
    Ok(ws.on_upgrade(move |socket| handle_socket(socket, user, state)))
}

async fn create_session(pool: &PgPool, user_id: Uuid) -> Result<String, ApiError> {
    let token = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO sessions (token_hash, user_id, expires_at)
         VALUES ($1, $2, now() + interval '30 days')",
    )
    .bind(hash_session_token(&token))
    .bind(user_id)
    .execute(pool)
    .await
    .map_err(|error| log_database_error("create session", error))?;
    Ok(token)
}

async fn authenticate(headers: &HeaderMap, pool: &PgPool) -> Result<UserDto, ApiError> {
    let token_hash = hash_session_token(bearer_token(headers)?);
    sqlx::query_as::<_, (Uuid, String)>(
        "SELECT users.id, users.username
         FROM sessions
         JOIN users ON users.id = sessions.user_id
         WHERE sessions.token_hash = $1 AND sessions.expires_at > now()",
    )
    .bind(token_hash)
    .fetch_optional(pool)
    .await
    .map_err(|error| log_database_error("authenticate request", error))?
    .map(|(id, username)| UserDto { id, username })
    .ok_or_else(ApiError::unauthorized)
}

fn bearer_token(headers: &HeaderMap) -> Result<&str, ApiError> {
    let value = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(ApiError::unauthorized)?;
    let Some((scheme, token)) = value.split_once(' ') else {
        return Err(ApiError::unauthorized());
    };
    if !scheme.eq_ignore_ascii_case("bearer") || token.is_empty() {
        return Err(ApiError::unauthorized());
    }
    Ok(token)
}

fn hash_session_token(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

async fn hash_password(password: String) -> Result<String, ApiError> {
    tokio::task::spawn_blocking(move || {
        Argon2::default()
            .hash_password(password.as_bytes())
            .map(|hash| hash.to_string())
    })
    .await
    .map_err(|error| {
        eprintln!("password hashing task failed: {error}");
        ApiError::internal()
    })?
    .map_err(|error| {
        eprintln!("password hashing failed: {error}");
        ApiError::internal()
    })
}

fn normalize_username(raw: &str) -> Result<String, ApiError> {
    let username = raw.trim().to_ascii_lowercase();
    let valid_length = (3..=32).contains(&username.len());
    let valid_characters = username
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_');
    if !valid_length || !valid_characters {
        return Err(ApiError::bad_request(
            "Username must be 3-32 characters using letters, numbers, or underscores.",
        ));
    }
    Ok(username)
}

fn validate_password(password: &str) -> Result<(), ApiError> {
    let length = password.chars().count();
    if !(8..=128).contains(&length) {
        return Err(ApiError::bad_request(
            "Password must be between 8 and 128 characters.",
        ));
    }
    Ok(())
}

fn log_database_error(operation: &str, error: sqlx::Error) -> ApiError {
    eprintln!("database error while attempting to {operation}: {error}");
    ApiError::internal()
}

impl ConnectionManager {
    async fn register(&self, user_id: Uuid, sender: mpsc::UnboundedSender<ServerEvent>) -> u64 {
        let connection_id = self
            .inner
            .next_connection_id
            .fetch_add(1, Ordering::Relaxed);
        self.inner
            .users
            .write()
            .await
            .entry(user_id)
            .or_default()
            .insert(connection_id, sender);
        connection_id
    }

    async fn unregister(&self, user_id: Uuid, connection_id: u64) {
        let mut users = self.inner.users.write().await;
        let remove_user = if let Some(connections) = users.get_mut(&user_id) {
            connections.remove(&connection_id);
            connections.is_empty()
        } else {
            false
        };
        if remove_user {
            users.remove(&user_id);
        }
    }

    async fn send_message(&self, message: MessageDto) {
        let event = ServerEvent::MessageCreated {
            message: message.clone(),
        };
        let mut users = self.inner.users.write().await;
        for user_id in [message.from.id, message.to.id] {
            if let Some(connections) = users.get_mut(&user_id) {
                connections.retain(|_, sender| sender.send(event.clone()).is_ok());
            }
        }
    }
}

async fn handle_socket(socket: WebSocket, user: UserDto, state: AppState) {
    let (event_sender, mut event_receiver) = mpsc::unbounded_channel();
    let connection_id = state
        .connections
        .register(user.id, event_sender.clone())
        .await;
    let (mut socket_sender, mut socket_receiver) = socket.split();

    loop {
        tokio::select! {
            incoming = socket_receiver.next() => {
                let Some(Ok(message)) = incoming else {
                    break;
                };
                match message {
                    Message::Text(text) => {
                        let client_event = match serde_json::from_str::<ClientEvent>(text.as_str()) {
                            Ok(event) => event,
                            Err(_) => {
                                send_error(&event_sender, "Invalid message payload.");
                                continue;
                            }
                        };
                        match client_event {
                            ClientEvent::SendMessage { recipient_username, content } => {
                                create_message(&state, &user, recipient_username, content, &event_sender).await;
                            }
                        }
                    }
                    Message::Close(_) => break,
                    _ => {}
                }
            }
            event = event_receiver.recv() => {
                let Some(event) = event else {
                    break;
                };
                let Ok(text) = serde_json::to_string(&event) else {
                    continue;
                };
                if socket_sender.send(Message::Text(text.into())).await.is_err() {
                    break;
                }
            }
        }
    }

    state.connections.unregister(user.id, connection_id).await;
}

async fn create_message(
    state: &AppState,
    sender: &UserDto,
    recipient_username: String,
    content: String,
    errors: &mpsc::UnboundedSender<ServerEvent>,
) {
    let content = content.trim();
    if content.is_empty() {
        send_error(errors, "Message cannot be empty.");
        return;
    }
    if content.chars().count() > MAX_MESSAGE_CHARS {
        send_error(errors, "Message cannot exceed 500 characters.");
        return;
    }

    let recipient_username = match normalize_username(&recipient_username) {
        Ok(username) => username,
        Err(_) => {
            send_error(errors, "Enter a valid recipient username.");
            return;
        }
    };
    let recipient = match sqlx::query_as::<_, (Uuid, String)>(
        "SELECT id, username FROM users WHERE username = $1",
    )
    .bind(recipient_username)
    .fetch_optional(&state.pool)
    .await
    {
        Ok(Some((id, username))) => UserDto { id, username },
        Ok(None) => {
            send_error(errors, "Recipient account was not found.");
            return;
        }
        Err(error) => {
            log_database_error("find message recipient", error);
            send_error(errors, "Could not send message.");
            return;
        }
    };

    if recipient.id == sender.id {
        send_error(errors, "You cannot send a message to yourself.");
        return;
    }

    let message = MessageDto {
        id: Uuid::new_v4(),
        from: sender.clone(),
        to: recipient,
        content: content.to_owned(),
    };
    if let Err(error) = sqlx::query(
        "INSERT INTO direct_messages (id, sender_id, recipient_id, content)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(message.id)
    .bind(message.from.id)
    .bind(message.to.id)
    .bind(&message.content)
    .execute(&state.pool)
    .await
    {
        log_database_error("store direct message", error);
        send_error(errors, "Could not send message.");
        return;
    }

    state.connections.send_message(message).await;
}

fn send_error(sender: &mpsc::UnboundedSender<ServerEvent>, message: &str) {
    let _ = sender.send(ServerEvent::Error {
        message: message.to_owned(),
    });
}

async fn root() -> &'static str {
    "It works!"
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}
