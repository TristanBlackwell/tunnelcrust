use std::{collections::HashMap, sync::Arc};

use futures::stream::SplitSink;
use futures::stream::SplitStream;
use http_body_util::Full;
use hyper::{body::Bytes, upgrade::Upgraded, Request, Response};
use hyper_tungstenite::{is_upgrade_request, tungstenite::Message, upgrade, WebSocketStream};
use hyper_util::rt::TokioIo;
use tokio::sync::oneshot;
use tokio::{net::TcpListener, sync::Mutex};
use tracing::{event, instrument, Level};
use uuid::Uuid;

use crate::client::handle_client_request;
use crate::configuration::Settings;
use crate::error::Error;
use crate::websocket::websocket_controller;

pub type WebsocketSender = Arc<Mutex<SplitSink<WebSocketStream<TokioIo<Upgraded>>, Message>>>;
pub type WebsocketReceiver = Arc<Mutex<SplitStream<WebSocketStream<TokioIo<Upgraded>>>>>;
pub type SharedConnections = Arc<Mutex<HashMap<String, (WebsocketSender, WebsocketReceiver)>>>;

pub type PendingRequests = Arc<Mutex<HashMap<Uuid, oneshot::Sender<Response<Full<Bytes>>>>>>;

pub struct Server {
    port: u16,
    listener: TcpListener,
    // A HashMap of active websocket connections
    connections: SharedConnections,
    // A HashMap of requests pending a response from a client
    pending_requests: PendingRequests,
}

impl Server {
    /// Builds the server, obtaining a port ready for listening. NOTE - This does not
    /// start listening for connections. Use the `run()` function to do so.
    pub async fn build(configuration: Settings) -> Result<Self, Error> {
        let listener = TcpListener::bind(format!("0.0.0.0:{}", configuration.port))
            .await
            .unwrap_or_else(|error| {
                panic!("Unable to bind to port {}: {}", configuration.port, error)
            });

        let port = listener
            .local_addr()
            .expect("Unable to determine local address")
            .port();

        Ok(Self {
            port,
            listener,
            connections: Arc::new(Mutex::new(HashMap::new())),
            pending_requests: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Returns the number of actively connected clients.
    pub async fn clients_len(&self) -> usize {
        self.connections.lock().await.len()
    }

    /// Start accepting connections on the servers port. This will run until
    /// an interrupt signal is received.
    pub async fn run_until_stopped(&self) {
        let mut http = hyper::server::conn::http1::Builder::new();
        http.keep_alive(true);

        tracing::info!("Listening on http://0.0.0.0:{}", self.port);

        // Continually listen for new requests
        loop {
            match self.listener.accept().await {
                Ok((stream, addr)) => {
                    tracing::info!("New incoming request - {:?}", addr);

                    let connections = self.connections.clone();
                    let pending_requests = self.pending_requests.clone();

                    // Establish a connection and pass the request to our handler.
                    let connection = http
                        .serve_connection(
                            TokioIo::new(stream),
                            hyper::service::service_fn(move |req| {
                                handle_request(req, connections.clone(), pending_requests.clone())
                            }),
                        )
                        .with_upgrades();

                    tokio::spawn(async move {
                        if let Err(err) = connection.await {
                            tracing::error!("Failed to handle request  - {:?}: {:?}", addr, err);
                        } else {
                            tracing::info!("Request handled - {:?}", addr)
                        }
                    });
                }
                Err(err) => tracing::error!("Failed to establish TCP connection: {:?}", err),
            }
        }
    }
}

/// Handles an incoming API request. This will either match against a known
/// endpoint or it is assumed the request is to be forwarded to a client and
/// will subsequently be forwarded if a match is found based on the requests
/// host header.
#[instrument(name = "handle_request", skip(request, connections, pending_requests), fields(client = tracing::field::Empty))]
async fn handle_request(
    request: Request<hyper::body::Incoming>,
    connections: SharedConnections,
    pending_requests: PendingRequests,
) -> Result<Response<Full<Bytes>>, Error> {
    let host = &request
        .headers()
        .get("host")
        .unwrap()
        .to_str()
        .unwrap_or("unknown");
    tracing::Span::current().record("client", tracing::field::display(&host));

    match (request.method(), request.uri().path()) {
        (&hyper::Method::GET, "/health-check") => {
            return Ok(Response::new(Full::from("Alive and kickin!")))
        }
        _ => {
            if is_upgrade_request(&request) {
                establish_websocket_connection(request, connections, pending_requests).await
            } else {
                handle_client_request(request, connections, pending_requests).await
            }
        }
    }
}

/// Upgrades a HTTP request to a websocket connection. This does **not**
/// check that the request is an upgrade request, which should be done
/// before this is called.
#[instrument(name = "establish_websocket_connection", skip(request, connections, pending_requests), fields(client = tracing::field::Empty))]
async fn establish_websocket_connection(
    mut request: Request<hyper::body::Incoming>,
    connections: SharedConnections,
    pending_requests: PendingRequests,
) -> Result<Response<Full<Bytes>>, Error> {
    event!(Level::INFO, "Client requested an upgrade");

    match upgrade(&mut request, None) {
        Ok((response, websocket)) => {
            event!(
                Level::INFO,
                "Client request upgraded. Passing to websocket handler to establish connection"
            );

            tokio::spawn(async move {
                if let Err(err) =
                    websocket_controller(websocket, connections, pending_requests).await
                {
                    event!(
                        Level::ERROR,
                        "Failure during websocket connection: {:?}",
                        err
                    );
                }
            });

            Ok(response)
        }
        Err(err) => {
            event!(Level::ERROR, "Failed to upgrade client request: {:?}", err);
            event!(Level::ERROR, "Dropping client request");

            return Ok(Response::builder()
                .status(500)
                .body(Full::from("Failed to upgrade request"))
                .unwrap());
        }
    }
}
