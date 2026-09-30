use axum::{
    Router,
    Json,
    routing::{get, any},
    extract::ws::{WebSocketUpgrade, WebSocket},
    response::{IntoResponse, Response},
};
use serde::Serialize;
use tokio::net::TcpListener;

#[derive(Serialize)]
struct HealthResponse {
    status: String,
}

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", get(root))
        .route("/health", get(health))
        .route("/ws", any(handler))
    ;
    let listener = TcpListener::bind("127.0.0.1:3000").await.unwrap();

    println!("API is running at http://127.0.0.1:3000");

    axum::serve(listener, app).await.unwrap();
}

async fn handler(ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(handle_socket)
}

async fn handle_socket(mut socket: WebSocket) {
    while let Some(msg) = socket.recv().await {
        let msg = if let Ok(msg) = msg {
            msg
        } else {
            return;
        };

        if socket.send(msg).await.is_err() {
            return;
        }
    }
}

async fn root() -> &'static str {
    "It works!"
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
    })
}

