use axum::Router;
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Path, State, WebSocketUpgrade};
use axum::response::{Html, IntoResponse};
use axum::routing::{get, get_service};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, broadcast, mpsc};
use tokio::time::interval;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use uuid::Uuid;

#[derive(Clone)]
struct AppState {
    conns: Arc<Mutex<HashMap<String, broadcast::Sender<Message>>>>,
    transfers: Arc<Mutex<HashMap<String, TransferInfo>>>,
}

#[derive(Clone, Serialize, Deserialize)]
struct TransferInfo {
    file_name: String,
    file_size: usize,
    sender_id: String,
}

#[tokio::main]
async fn main() {
    let state = AppState {
        conns: Arc::new(Mutex::new(HashMap::new())),
        transfers: Arc::new(Mutex::new(HashMap::new())),
    };
    //serve html frontend along with backend
    let static_files = get_service(ServeDir::new("static"));
    let app = Router::new()
        .route("/", get(index_handler))
        .route("/upload", get(upload_handler))
        .route("/receive/:transfer_id", get(receive_handler))
        .route("/ws", get(websocket_handler_without_id))
        .route("/ws/:transfer_id", get(websocket_handler_with_id))
        .nest_service("/static", static_files)
        .layer(TraceLayer::new_for_http())
        .with_state(state);
    
    // In your main function, adjust the port binding:
    let port = std::env::var("PORT").unwrap_or_else(|_| "8000".to_string());
    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    println!("Server listening on {}", addr);
    
    axum::serve(listener, app).await.unwrap();
}

async fn index_handler() -> impl IntoResponse {
    Html(include_str!("../templates/index.html"))
}

async fn upload_handler() -> impl IntoResponse {
    Html(include_str!("../templates/upload.html"))
}

async fn receive_handler(
    Path(transfer_id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let transfers = state.transfers.lock().await;

    if let Some(transfer_info) = transfers.get(&transfer_id) {
        let template = include_str!("../templates/receive.html")
            .replace("{{TRANSFER_ID}}", &transfer_id)
            .replace("{{FILE_NAME}}", &transfer_info.file_name)
            .replace("{{FILE_SIZE}}", &format!("{}", transfer_info.file_size));

        Html(template)
    } else {
        Html(include_str!("../templates/not_found.html").to_string())
    }
}

// WebSocket handler without transfer ID
async fn websocket_handler_without_id(
    ws: WebSocketUpgrade,
    State(app_state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, app_state, None))
}

// WebSocket handler with transfer ID
async fn websocket_handler_with_id(
    ws: WebSocketUpgrade,
    Path(transfer_id): Path<String>,
    State(app_state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, app_state, Some(transfer_id)))
}

async fn handle_socket(socket: WebSocket, app_state: AppState, transfer_id: Option<String>) {
    let conn_id = Uuid::new_v4().to_string();
    println!("New connection: {}", conn_id);

    let (tx, mut rx) = broadcast::channel(100);
    {
        let mut connections = app_state.conns.lock().await;
        connections.insert(conn_id.clone(), tx.clone());
    }

    let (mut sender, mut receiver) = socket.split();
    let (message_tx, mut message_rx) = mpsc::channel::<Message>(100);

    // Handle sending messages
    let sender_task = tokio::spawn(async move {
        while let Some(message) = message_rx.recv().await {
            if sender.send(message).await.is_err() {
                break;
            }
        }
    });

    // testing ping-pong
    let ping_tx = message_tx.clone();
    let ping_task = tokio::spawn(async move {
        let mut interval = interval(Duration::from_secs(30));
        loop {
            interval.tick().await;
            if ping_tx.send(Message::Ping(vec![])).await.is_err() {
                break;
            }
        }
    });

    // Forward broadcast messages to this connection
    let forward_tx = message_tx.clone();
    let forward_task = tokio::spawn(async move {
        while let Ok(message) = rx.recv().await {
            if forward_tx.send(message).await.is_err() {
                break;
            }
        }
    });

    // Handle incoming messages
    let receive_task = tokio::spawn({
        let state = app_state.clone();
        let tx = tx.clone();
        let conn_id = conn_id.clone();
        let message_tx = message_tx.clone();
        let mut target_map: HashMap<String, String> = HashMap::new();

        // If this is a receiver connecting to a specific transfer
        if let Some(transfer_id) = transfer_id {
            let transfers = state.transfers.lock().await;
            if let Some(transfer_info) = transfers.get(&transfer_id) {
                // Connect to the sender
                target_map.insert(conn_id.clone(), transfer_info.sender_id.clone());

                // Let the sender know someone connected to receive the file
                if let Some(sender_tx) = state.conns.lock().await.get(&transfer_info.sender_id) {
                    let connect_msg = json!({
                        "type": "receiver_connected",
                        "transfer_id": transfer_id,
                        "receiver_id": conn_id
                    })
                    .to_string();
                    let _ = sender_tx.send(Message::Text(connect_msg));
                }

                let ready_msg = json!({
                    "type": "transfer_ready",
                    "file_name": transfer_info.file_name,
                    "file_size": transfer_info.file_size,
                })
                .to_string();
                let _ = message_tx.send(Message::Text(ready_msg)).await;
            }
        }

        async move {
            while let Some(Ok(message)) = receiver.next().await {
                match message {
                    Message::Text(text) => {
                        if let Ok(data) = serde_json::from_str::<Value>(&text) {
                            if data["type"] == "register" {
                                if let Some(id) = data["connectionId"].as_str() {
                                    state.conns.lock().await.insert(id.to_string(), tx.clone());
                                }
                                continue;
                            }

                            if data["type"] == "init_transfer" {
                                if let (Some(file_name), Some(file_size)) =
                                    (data["file_name"].as_str(), data["file_size"].as_u64())
                                {
                                    let transfer_id = Uuid::new_v4().to_string();
                                    let transfer_info = TransferInfo {
                                        file_name: file_name.to_string(),
                                        file_size: file_size as usize,
                                        sender_id: conn_id.clone(),
                                    };
                                    state
                                        .transfers
                                        .lock()
                                        .await
                                        .insert(transfer_id.clone(), transfer_info);

                                    // Send the transfer ID back to the sender
                                    let response = json!({
                                        "type": "transfer_created",
                                        "transfer_id": transfer_id,
                                        "share_url": format!("/receive/{}", transfer_id)
                                    })
                                    .to_string();
                                    let _ = tx.send(Message::Text(response));
                                }
                                continue;
                            }

                            // Handle direct messaging with target_id
                            if let Some(target_id) = data["target_id"].as_str() {
                                target_map.insert(conn_id.clone(), target_id.to_string());
                                if let Some(target_tx) = state.conns.lock().await.get(target_id) {
                                    let _ = target_tx.send(Message::Text(text));
                                }
                            }
                        }
                    }
                    Message::Binary(file) => {
                        if let Some(target_id) = target_map.get(&conn_id) {
                            if let Some(target_tx) = state.conns.lock().await.get(target_id) {
                                let _ = target_tx.send(Message::Binary(file));
                            }
                        }
                    }
                    Message::Close(_) => {
                        break;
                    }
                    _ => {
                        continue;
                    }
                }
            }
        }
    });

    tokio::select! {
        _ = sender_task => {},
        _ = ping_task => {},
        _ = forward_task => {},
        _ = receive_task => {},
    }

    app_state.conns.lock().await.remove(&conn_id);
    println!("Connection closed: {}", conn_id);
}
