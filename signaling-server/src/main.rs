use std::{collections::HashMap, env, sync::Arc};

use axum::{
    Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, mpsc};

type Rooms = Arc<Mutex<HashMap<String, Room>>>;
type Outgoing = mpsc::UnboundedSender<Message>;

#[derive(Default)]
struct Room {
    peers: Vec<Outgoing>,
    offer: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct SignalMessage {
    #[serde(rename = "type")]
    kind: String,
    session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    sdp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    candidate: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sdp_mid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sdp_m_line_index: Option<u16>,
}

#[tokio::main]
async fn main() {
    let address = env::var("SIGNALING_ADDR").unwrap_or_else(|_| "0.0.0.0:9001".to_string());
    let rooms: Rooms = Arc::new(Mutex::new(HashMap::new()));
    let app = Router::new()
        .route("/", get(websocket_handler))
        .with_state(rooms);

    let listener = tokio::net::TcpListener::bind(&address)
        .await
        .unwrap_or_else(|error| panic!("Could not bind signaling server to {address}: {error}"));
    println!("WebRTC signaling server listening on ws://{address}");

    if let Err(error) = axum::serve(listener, app).await {
        eprintln!("Signaling server stopped: {error}");
    }
}

async fn websocket_handler(ws: WebSocketUpgrade, State(rooms): State<Rooms>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, rooms))
}

async fn handle_socket(socket: WebSocket, rooms: Rooms) {
    let (mut socket_sender, mut socket_receiver) = socket.split();
    let (outgoing, mut outgoing_receiver) = mpsc::unbounded_channel::<Message>();

    let send_task = tokio::spawn(async move {
        while let Some(message) = outgoing_receiver.recv().await {
            if socket_sender.send(message).await.is_err() {
                break;
            }
        }
    });
    let mut joined_rooms = Vec::<String>::new();

    while let Some(Ok(frame)) = socket_receiver.next().await {
        let Message::Text(text) = frame else {
            if matches!(frame, Message::Close(_)) {
                break;
            }
            continue;
        };
        let message: SignalMessage = match serde_json::from_str(text.as_str()) {
            Ok(message) => message,
            Err(error) => {
                send_error(&outgoing, "Invalid signaling message", None);
                eprintln!("Rejected malformed signaling message: {error}");
                continue;
            }
        };

        if message.session_id.trim().is_empty() {
            send_error(
                &outgoing,
                "session_id must not be empty",
                Some(&message.session_id),
            );
            continue;
        }

        match message.kind.as_str() {
            "create" => {
                let mut rooms = rooms.lock().await;
                if rooms.contains_key(&message.session_id) {
                    send_error(
                        &outgoing,
                        "Session already exists",
                        Some(&message.session_id),
                    );
                    continue;
                }
                rooms.insert(
                    message.session_id.clone(),
                    Room {
                        peers: vec![outgoing.clone()],
                        offer: None,
                    },
                );
                joined_rooms.push(message.session_id);
            }
            "join" => {
                let mut rooms = rooms.lock().await;
                if let Some(room) = rooms.get_mut(&message.session_id) {
                    if !joined_rooms.contains(&message.session_id) {
                        room.peers.push(outgoing.clone());
                        joined_rooms.push(message.session_id.clone());
                    }
                    if let Some(sdp) = room.offer.as_ref() {
                        let offer = SignalMessage {
                            kind: "offer".into(),
                            session_id: message.session_id.clone(),
                            sdp: Some(sdp.clone()),
                            candidate: None,
                            sdp_mid: None,
                            sdp_m_line_index: None,
                        };
                        send_message(&outgoing, &offer);
                    }
                } else {
                    send_error(&outgoing, "Session not found", Some(&message.session_id));
                }
            }
            "offer" | "answer" | "ice-candidate" => {
                let mut rooms = rooms.lock().await;
                if let Some(room) = rooms.get_mut(&message.session_id) {
                    if !room.peers.iter().any(|peer| peer.same_channel(&outgoing)) {
                        send_error(
                            &outgoing,
                            "Join this session before sending signaling data",
                            Some(&message.session_id),
                        );
                        continue;
                    }
                    if message.kind == "offer" {
                        room.offer = message.sdp.clone();
                    }
                    room.peers.retain(|peer| {
                        if peer.same_channel(&outgoing) {
                            true
                        } else {
                            peer.send(Message::Text(
                                serde_json::to_string(&message).unwrap_or_default().into(),
                            ))
                            .is_ok()
                        }
                    });
                } else {
                    send_error(&outgoing, "Session not found", Some(&message.session_id));
                }
            }
            _ => send_error(
                &outgoing,
                "Unsupported signaling message type",
                Some(&message.session_id),
            ),
        }
    }

    {
        let mut rooms = rooms.lock().await;
        for session_id in joined_rooms {
            let remove_room = if let Some(room) = rooms.get_mut(&session_id) {
                room.peers.retain(|peer| !peer.same_channel(&outgoing));
                room.peers.is_empty()
            } else {
                false
            };
            if remove_room {
                rooms.remove(&session_id);
            }
        }
    }
    drop(outgoing);
    send_task.abort();
}

fn send_message(sender: &Outgoing, message: &SignalMessage) {
    if let Ok(payload) = serde_json::to_string(message) {
        let _ = sender.send(Message::Text(payload.into()));
    }
}

fn send_error(sender: &Outgoing, reason: &str, session_id: Option<&str>) {
    send_message(
        sender,
        &SignalMessage {
            kind: "error".into(),
            session_id: session_id.unwrap_or_default().to_string(),
            sdp: Some(reason.to_string()),
            candidate: None,
            sdp_mid: None,
            sdp_m_line_index: None,
        },
    );
}
