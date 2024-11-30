use futures::stream::StreamExt;
use futures::SinkExt;
use http_body_util::Full;
use hyper::upgrade::Upgraded;
use hyper::{body::Bytes, Request, Response};
use hyper_tungstenite::tungstenite::Message;
use hyper_tungstenite::WebSocketStream;
use hyper_util::rt::TokioIo;
use protocol::RequestIncomingBinaryProtocol;
use std::sync::Arc;
use tokio::sync::oneshot;
use tokio::{spawn, sync::Mutex};
use tracing::{event, instrument, Level};
use uuid::Uuid;

use crate::domain::generate_random_subdomain;
use crate::server::WebsocketReceiver;
use crate::{
    error::Error,
    server::{PendingRequests, SharedConnections, WebsocketSender},
};

/// Takes a newly established websocket connection and prepares the client
/// for interactions by storing the websocket stream and assigning a unique
/// subdomain.
#[instrument(name = "prepare_client_connection", skip(websocket, connections))]
pub async fn prepare_client_connection(
    websocket: WebSocketStream<TokioIo<Upgraded>>,
    connections: &SharedConnections,
) -> (String, WebsocketSender, WebsocketReceiver) {
    // TODO: ensure client subdomains don't conflict
    let client_subdomain = generate_random_subdomain();

    let (ws_sender, ws_receiver) = websocket.split();
    let ws_sender = Arc::new(Mutex::new(ws_sender));
    let ws_receiver = Arc::new(Mutex::new(ws_receiver));

    {
        let mut connections = connections.lock().await;
        connections.insert(
            client_subdomain.clone(),
            (ws_sender.clone(), ws_receiver.clone()),
        );
        event!(
            Level::INFO,
            "Websocket connection attached to active connections: {}",
            &client_subdomain
        );
    }

    let ws_sender_clone = ws_sender.clone();
    let client_subdomain_copy = client_subdomain.clone();
    spawn(async move {
        let mut locked_ws_sender = ws_sender_clone.lock().await;
        if let Err(e) = locked_ws_sender
            .send(Message::text(format!(
                "subdomain:{}",
                client_subdomain_copy
            )))
            .await
        {
            event!(Level::ERROR, "Failed to send client's subdomain: {}", e);
        } else {
            event!(
                Level::DEBUG,
                "client subdomain has been sent: {}",
                client_subdomain_copy
            );
        }
    });

    (client_subdomain, ws_sender, ws_receiver)
}

/// Given an incoming request, finds the client this targets
/// and forwards the request on.
#[instrument(
    name = "handle_client_request",
    skip(request, connections, pending_requests)
)]
pub async fn handle_client_request(
    request: Request<hyper::body::Incoming>,
    connections: SharedConnections,
    pending_requests: PendingRequests,
) -> Result<hyper::Response<http_body_util::Full<Bytes>>, Error> {
    let host = match request.headers().get("host") {
        Some(value) => value.to_str().unwrap_or("unknown"),
        None => "unknown",
    }
    .to_string();

    let subdomain = host.split('.').next().unwrap();
    event!(Level::INFO, "Received request for subdomain: {}", subdomain);

    let connections = connections.lock().await;

    let client = connections.get(subdomain);

    match client {
        Some(client_details) => {
            forward_request_to_client(request, pending_requests, &client_details.0).await
        }
        None => {
            event!(
                Level::WARN,
                "Unable to match subdomain {} against known clients",
                subdomain
            );
            Ok(Response::builder()
                .status(500)
                .body(Full::from("Unknown client"))
                .unwrap())
        }
    }
}

/// Handles the actual forwarding of the request. Serializes the request
/// parts and sends it over the websocket connection. A oneshot channel is
/// created to receive the response from the client or until the configured
/// timeout is reached and the request is aborted.
#[instrument(
    name = "forward_request_to_client",
    skip(request, pending_requests, ws_sender)
)]
async fn forward_request_to_client(
    mut request: Request<hyper::body::Incoming>,
    pending_requests: PendingRequests,
    ws_sender: &WebsocketSender,
) -> Result<hyper::Response<http_body_util::Full<hyper::body::Bytes>>, Error> {
    let request_id = Uuid::new_v4();
    let serialized_request_id = request_id.to_bytes_le();
    let serialized_request = request
        .serialize()
        .await
        .expect("Failed to serialize request");

    // Create a oneshot for this request and store it's transmitter. The websocket
    // listener should pick up the response from a client and pass back the response.
    let (tx, rx) = oneshot::channel();
    {
        let mut pending_requests = pending_requests.lock().await;
        pending_requests.insert(request_id, tx);
    }

    event!(Level::INFO, "Forwarding HTTP...");

    let mut ws_sender = ws_sender.lock().await;

    let message = [
        &serialized_request_id,
        serialized_request.clone().as_slice(),
    ]
    .concat();
    ws_sender.send(Message::Binary(message)).await.unwrap();

    // Awaiting the response from the client.
    match tokio::time::timeout(std::time::Duration::from_secs(10), rx).await {
        Ok(Ok(response)) => {
            // Forward the response back to the original HTTP requestor
            Ok(Response::new(response.into_body()))
        }
        Ok(Err(_)) => {
            event!(
                Level::ERROR,
                "Client disconnected while waiting for response"
            );
            {
                let mut pending_requests = pending_requests.lock().await;
                pending_requests.remove(&request_id);
            }
            Ok(Response::builder()
                .status(500)
                .body(Full::from("Internal server error"))
                .unwrap())
        }
        Err(_) => {
            event!(Level::ERROR, "Timed out waiting for client response");
            {
                let mut pending_requests = pending_requests.lock().await;
                pending_requests.remove(&request_id);
            }
            Ok(Response::builder()
                .status(504)
                .body(Full::from("Gateway timeout"))
                .unwrap())
        }
    }
}
