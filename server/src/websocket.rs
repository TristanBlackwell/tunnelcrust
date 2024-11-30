use http_body_util::Full;
use hyper::{body::Bytes, Response};
use protocol::ResponseBytesBinaryProtocol;
use tracing::{event, instrument, Level};

use crate::{
    client::prepare_client_connection,
    error::Error,
    server::{PendingRequests, SharedConnections},
};
use futures::stream::StreamExt;
use hyper_tungstenite::{
    tungstenite::error::ProtocolError, tungstenite::Error::AlreadyClosed,
    tungstenite::Error::ConnectionClosed, tungstenite::Error::Protocol, tungstenite::Message,
    HyperWebsocket,
};

use uuid::Uuid;

/// Prepares a websocket connection as a connected client and processes
/// any incoming messages
#[instrument(
    name = "websocket_controller",
    skip(websocket, connections, pending_requests)
)]
pub async fn websocket_controller(
    websocket: HyperWebsocket,
    connections: SharedConnections,
    pending_requests: PendingRequests,
) -> Result<(), Error> {
    match websocket.await {
        Ok(websocket) => {
            event!(Level::INFO, "Websocket connection established");

            let (client_subdomain, _, ws_receiver) =
                prepare_client_connection(websocket, &connections).await;

            let ws_receiver_clone = ws_receiver.clone();
            while let Some(ws_message) = ws_receiver_clone.lock().await.next().await {
                match ws_message {
                    Ok(message) => {
                        match message {
                            Message::Text(msg) => println!("Received text message: {msg}"),

                            Message::Binary(bytes) => {
                                let request_parts = process_bytes(bytes).await;

                                println!(
                                    "Request {} received from client. Handling response...",
                                    request_parts.0
                                );

                                if let Some(sender) =
                                    pending_requests.lock().await.remove(&request_parts.0)
                                {
                                    if sender.send(request_parts.1).is_err() {
                                        event!(
                                            Level::ERROR,
                                            "Failed to send response to request handler"
                                        );
                                    }
                                } else {
                                    event!(
                                        Level::ERROR,
                                        "No matching request for ID: {}. Dropping request.",
                                        request_parts.0
                                    );
                                }
                            }
                            Message::Ping(msg) => {
                                // No need to send a reply: tungstenite takes care of this for you.
                                println!("Received ping message: {msg:02X?}");
                            }
                            Message::Pong(msg) => {
                                println!("Received pong message: {msg:02X?}");
                            }
                            Message::Close(msg) => {
                                // No need to send a reply: tungstenite takes care of this for you.
                                if let Some(msg) = &msg {
                                    println!(
                                        "Received close message with code {} and message: {}",
                                        msg.code, msg.reason
                                    );
                                } else {
                                    println!("Received close message");
                                }
                            }
                            Message::Frame(_msg) => {
                                unreachable!();
                            }
                        }
                    }
                    Err(err) => match err {
                        ConnectionClosed => {
                            event!(Level::INFO, "WebSocket close handshake complete");
                        }
                        AlreadyClosed => {
                            event!(Level::WARN, "Websocket connection was already closed")
                        }
                        Protocol(protocol_err) => match protocol_err {
                            ProtocolError::ResetWithoutClosingHandshake => {
                                event!(Level::WARN, "Websocket closed without completing handshake")
                            }
                            _ => event!(Level::ERROR, "Websocket protocol error"),
                        },
                        err => event!(Level::ERROR, "Websocket error during connection: {}", err),
                    },
                }
            }

            {
                let mut connections = connections.lock().await;
                connections.remove(&client_subdomain);
                event!(
                    Level::INFO,
                    "Websocket connection removed from active connections: {}",
                    &client_subdomain
                )
            }
        }
        Err(err) => {
            event!(Level::ERROR, "Failed to establish websocket connection. Has the upgrade response been sent to the client?: {:?}", err);
        }
    }

    Ok(())
}

/// Reconstructs incoming bytes from the websocket connection
/// as an response to a forwarded request.
async fn process_bytes(bytes: Vec<u8>) -> (Uuid, Response<Full<Bytes>>) {
    let request_id_bytes = &bytes[0..16];
    let array: [u8; 16] = request_id_bytes
        .try_into()
        .expect("Slice has wrong length!");
    let request_id = Uuid::from_bytes_le(array);

    let response = hyper::Response::<Bytes>::deserialize(&bytes[16..])
        .await
        .expect("Failed to deserialize response");

    (request_id, response.clone().map(Full::new))
}
