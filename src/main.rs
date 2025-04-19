use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use futures::{SinkExt, StreamExt};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, broadcast, mpsc};
use tokio::time::interval;
use uuid::Uuid;

#[derive(Clone)]
struct AppState {
    conns: Arc<Mutex<HashMap<String, broadcast::Sender<Message>>>>,
}

#[tokio::main]
async fn main() {
    let state = AppState{
        conns : Arc::new(Mutex::new(HashMap::new()))
    };
    
    let app = Router::new()
        .route("/ws",get(websocket_handler) )
        .with_state(state);
    
    let listener  = tokio::net::TcpListener::bind("0.0.0.0:8000").await.unwrap();
    println!("Server listening on localhost:8000");
    axum::serve(listener, app).await.unwrap();
    
}

async fn websocket_handler(
    ws : WebSocketUpgrade,
    State(app_state) : State<AppState>
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, app_state))
}

async fn handle_socket(socket: WebSocket, app_state: AppState) {
    let conn_id = Uuid::new_v4().to_string();
    let conn_id_clone = conn_id.clone();
    println!("new connection : {} ", conn_id.clone());
    let (tx, mut rx) = broadcast::channel(100);
    {
        let mut connections = app_state.conns.lock().await;
        connections.insert(conn_id.clone(), tx.clone());
    }
    let (mut sender, mut receiver) = socket.split();
    let (message_tx, mut message_rx) = mpsc::channel::<Message>(100);

    let sender_task = tokio::spawn(async move {
        while let Some(message) = message_rx.recv().await {
            // break if error in sending message
            if sender.send(message).await.is_err() {
                break;
            }
        }
    });
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

    let foreward_tx = message_tx.clone();
    let foreward_task = tokio::spawn(async move {
        while let Ok(message) = rx.recv().await {
            if foreward_tx.send(message).await.is_err() {
                break;
            }
        }
    });

    let receive_task = tokio::spawn({
        let state = app_state.clone();
        let tx = tx.clone();
        let mut target_map: HashMap<String, String> = HashMap::new();
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
            _ = foreward_task => {},
            _ = receive_task => {},
    }
    app_state.conns.lock().await.remove(&conn_id_clone);
    println!("connection closed {} ", conn_id_clone);
}
