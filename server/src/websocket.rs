use hyper::body::Bytes;
use protocol::ResponseBytesBinaryProtocol;
use std::sync::Arc;
use tracing::{event, instrument, Level};

use crate::{error::Error, server::SharedConnections};
use futures::stream::StreamExt;
use hyper_tungstenite::{
    tungstenite::error::ProtocolError, tungstenite::Error::AlreadyClosed,
    tungstenite::Error::ConnectionClosed, tungstenite::Error::Protocol, tungstenite::Message,
    HyperWebsocket,
};
use tokio::sync::Mutex;

use uuid::Uuid;

/// Takes in a websocket connection and sets up contained sender and receiver streams.
/// The client is stored in the servers `sharedConnections` map until disconnected.
#[instrument(name = "websocket_controller", skip(websocket, connections))]
pub async fn websocket_controller(
    websocket: HyperWebsocket,
    connections: SharedConnections,
) -> Result<(), Error> {
    match websocket.await {
        Ok(websocket) => {
            event!(Level::INFO, "Websocket connection established");

            let client_id = Uuid::new_v4();

            let (ws_sender, ws_receiver) = websocket.split();
            let ws_sender = Arc::new(Mutex::new(ws_sender));
            let ws_receiver = Arc::new(Mutex::new(ws_receiver));

            {
                let mut connections = connections.lock().await;
                connections.insert(client_id, (ws_sender.clone(), ws_receiver.clone()));
                event!(
                    Level::INFO,
                    "Websocket connection attached to active connections: {}",
                    client_id
                );
            }

            // let ws_sender_clone = ws_sender.clone();
            // spawn(async move {
            //     sleep(Duration::from_secs(5)).await;
            //     let mut locked_ws_sender = ws_sender_clone.lock().await;
            //     if let Err(e) = locked_ws_sender.send(Message::text("Ping!")).await {
            //         event!(Level::ERROR, "Failed to send 'ping': {}", e);
            //     } else {
            //         event!(Level::DEBUG, "'pinged' client: {}", client_id);
            //     }
            // });

            let ws_receiver_clone = ws_receiver.clone();
            while let Some(ws_message) = ws_receiver_clone.lock().await.next().await {
                match ws_message {
                    Ok(message) => {
                        match message {
                            Message::Text(msg) => println!("Received text message: {msg}"),

                            Message::Binary(msg) => {
                                println!("Received binary message: {msg:02X?}");
                                let response = hyper::Response::<Bytes>::deserialize(&msg)
                                    .await
                                    .expect("Failed to deserialize response");

                                println!("Deserialized");
                                println!("{}", response.status());

                                // TODO: Sent out to the big wide world
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
                connections.remove(&client_id);
                event!(
                    Level::INFO,
                    "Websocket connection removed from active connections: {}",
                    client_id
                )
            }
        }
        Err(err) => {
            event!(Level::ERROR, "Failed to establish websocket connection. Has the upgrade response been sent to the client?: {:?}", err);
        }
    }

    Ok(())
}
