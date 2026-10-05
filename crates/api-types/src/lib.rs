use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UserDto {
    pub id: Uuid,
    pub username: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CredentialsRequest {
    pub username: String,
    pub password: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AuthResponse {
    pub user: UserDto,
    pub token: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ErrorResponse {
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MessageDto {
    pub id: Uuid,
    pub from: UserDto,
    pub to: UserDto,
    pub content: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientEvent {
    SendMessage {
        recipient_username: String,
        content: String,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerEvent {
    MessageCreated { message: MessageDto },
    Error { message: String },
}
