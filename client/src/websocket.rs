use crate::configuration::Config;
use futures_util::{SinkExt, StreamExt};
use http_body_util::Full;
use hyper::{body::Bytes, client::conn::http1::SendRequest, Request};
use protocol::{RequestBytesBinaryProtocol, ResponseIncomingBinaryProtocol};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Error, Message},
};
use uuid::Uuid;

/// Runs the application, listening for websocket messages and processing them until an
/// interrupt signal is sent.
pub async fn run_until_stopped(config: &Config, mut sender: SendRequest<Full<Bytes>>) {
    let (_, mut stdin_rx) = futures_channel::mpsc::unbounded::<Message>();

    // Establish the WebSocket connection to the proxy server.
    let (ws_stream, _) = connect_async(&config.server_url)
        .await
        .expect("Failed to connect to the tunnelcrust server.");

    println!("Tunnel connected");

    let (mut write, mut read) = ws_stream.split();

    loop {
        tokio::select! {
            // Incoming message
            Some(message) = read.next() => {
                match message {
                    Ok(Message::Text(text)) => {
                        if text.starts_with("subdomain:") {
                            let subdomain = text.replace("subdomain:", "");
                            let full_domain = format!("http://{}.{}", subdomain, &config.server_url.replace("ws://", ""));
                            println!();
                            println!("Send requests to: {}", &full_domain);
                            println!();
                        } else {
                            println!("Received text message: {}", text);
                        }
                    },
                    Ok(Message::Binary(bytes)) => {
                        let request_parts = process_bytes_as_forwarded_request(bytes).await;

                        println!("Request {} received from tunnel. Forwarding to local service...", request_parts.0);

                        let mut res = sender
                            .send_request(request_parts.1)
                            .await
                            .expect("Failed to send request");

                        println!("Response status: {}", res.status());

                        let serialized_request_id = request_parts.0.to_bytes_le();
                        let serialized_request = res
                            .serialize()
                            .await
                            .expect("Failed to serialize response");

                            let message = [&serialized_request_id, serialized_request.as_slice()].concat();
                            if let Err(e) = write.send(Message::Binary(message)).await {
                                eprintln!("Failed to send message to WebSocket: {}", e);
                                break;
                            }
                    },
                    Ok(Message::Close(frame)) => {
                        println!("Received close: {:#?}", frame);
                        break;  // Exit the loop on close
                    }
                    Err(Error::ConnectionClosed) => {
                        println!("Connection was closed");
                        break;
                    }
                    Err(err) => eprintln!("Error: {}", err),
                    _ => println!("Unhandled message"),
                }
            }
            // Outgoing message
            Some(msg) = stdin_rx.next() => {
                if let Err(e) = write.send(msg).await {
                    eprintln!("Failed to send message to WebSocket: {}", e);
                    break;
                }
            }
            // Handle Ctrl+C interrupt
            _ = tokio::signal::ctrl_c() => {
                println!();
                println!("Closing connection...");

                if let Err(e) = write.send(Message::Close(None)).await {
                    eprintln!("Failed to send close message: {}", e);
                }
                break;  // Exit the loop after sending close message.
            }
        }
    }
}

/// Reconstructs incoming bytes from the tunnel websocket connection
/// as an forwarded hyper request object. This deserializes the bytes into
/// a UUID for request followed by the request itself which is rebuilt from
/// it's parts
async fn process_bytes_as_forwarded_request(bytes: Vec<u8>) -> (Uuid, Request<Full<Bytes>>) {
    let request_id_bytes = &bytes[0..16];
    let array: [u8; 16] = request_id_bytes
        .try_into()
        .expect("Slice has wrong length!");
    let request_id = Uuid::from_bytes_le(array);

    let request = hyper::Request::<Bytes>::deserialize(&bytes[16..])
        .await
        .expect("Failed to deserialize request");

    (request_id, request.clone().map(Full::new))
}
