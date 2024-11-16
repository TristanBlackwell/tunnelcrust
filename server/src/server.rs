use std::{collections::HashMap, sync::Arc};

use futures::stream::SplitSink;
use futures::stream::SplitStream;
use futures::SinkExt;
use http_body_util::Full;
use hyper::{body::Bytes, upgrade::Upgraded, Request, Response};
use hyper_tungstenite::{is_upgrade_request, tungstenite::Message, upgrade, WebSocketStream};
use hyper_util::rt::TokioIo;
use protocol::RequestIncomingBinaryProtocol;
use tokio::{net::TcpListener, sync::Mutex};
use tracing::{event, instrument, Level};
use uuid::Uuid;

use crate::configuration::Settings;
use crate::error::Error;
use crate::websocket::websocket_controller;

pub type SharedConnections = Arc<
    Mutex<
        HashMap<
            Uuid,
            (
                Arc<Mutex<SplitSink<WebSocketStream<TokioIo<Upgraded>>, Message>>>,
                Arc<Mutex<SplitStream<WebSocketStream<TokioIo<Upgraded>>>>>,
            ),
        >,
    >,
>;

pub struct Server {
    port: u16,
    listener: TcpListener,
    // A HashMap of active websocket connections
    connections: SharedConnections,
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
        })
    }

    /// Start accepting connections on the servers port. This will run until
    /// interrupted.
    pub async fn run(&self) {
        let mut http = hyper::server::conn::http1::Builder::new();
        http.keep_alive(true);

        tracing::info!("Listening on http://0.0.0.0:{}", self.port);

        // Continually accept new incoming connections
        loop {
            match self.listener.accept().await {
                Ok((stream, addr)) => {
                    // TODO: validate the client is authorised to connect.
                    tracing::info!("New incoming client request - {:?}", addr);

                    let connections = self.connections.clone();

                    // our request handler service will upgrade the Websocket connection
                    // (spawning a new thread) or signal that the client should do so.
                    // Either way the responses for this will pass through to the block
                    // below.
                    let connection = http
                        .serve_connection(
                            TokioIo::new(stream),
                            hyper::service::service_fn(move |req| {
                                handle_request(req, connections.clone())
                            }),
                        )
                        .with_upgrades();

                    tokio::spawn(async move {
                        if let Err(err) = connection.await {
                            tracing::error!("Failed to respond to client - {:?}: {:?}", addr, err);
                        } else {
                            tracing::info!("Response sent to client - {:?}", addr)
                        }
                    });
                }
                Err(err) => tracing::error!("Failed to accept client request attempt: {:?}", err),
            }
        }
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Returns the number of actively connected clients.
    pub async fn clients_len(&self) -> usize {
        self.connections.lock().await.len()
    }
}

#[instrument(name = "handle_request", skip(request, connections), fields(client = tracing::field::Empty))]
async fn handle_request(
    mut request: Request<hyper::body::Incoming>,
    connections: SharedConnections,
) -> Result<Response<Full<Bytes>>, Error> {
    tracing::Span::current().record(
        "client",
        tracing::field::display(
            &request
                .headers()
                .get("host")
                .unwrap()
                .to_str()
                .unwrap_or("unknown"),
        ),
    );

    match (request.method(), request.uri().path()) {
        (&hyper::Method::GET, "/health-check") => {
            return Ok(Response::new(Full::from("Alive and kickin!")))
        }
        _ => {
            if is_upgrade_request(&request) {
                event!(Level::INFO, "Client requested an upgrade");

                match upgrade(&mut request, None) {
                    Ok((response, websocket)) => {
                        event!(
                            Level::INFO,
                            "Client request upgraded. Passing to websocket handler to establish connection"
                        );

                        tokio::spawn(async move {
                            if let Err(err) = websocket_controller(websocket, connections).await {
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
            } else {
                event!(
                    Level::INFO,
                    "Incoming HTTP request. Sending back 426 upgrade response"
                );

                let serialized = request
                    .serialize()
                    .await
                    .expect("Failed to serialize request");

                let connections = connections.lock().await;
                if connections.len() > 0 {
                    for (key, value) in connections.iter() {
                        event!(Level::INFO, "Forwarding HTTP request to {}", key);

                        let mut ws_sender = value.0.lock().await;

                        ws_sender
                            .send(Message::Binary(serialized.clone()))
                            .await
                            .unwrap();
                    }
                }

                // Inform the client we're expecting upgrade requests here
                Ok(Response::builder()
                    .status(426)
                    .header("Connection", "upgrade")
                    .header("Upgrade", "websocket")
                    .body(Full::from("Upgrade required"))
                    .unwrap_or(Response::new(Full::from("Unsupported request"))))
            }
        }
    }
}
