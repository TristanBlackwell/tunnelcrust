use std::{collections::HashMap, sync::Arc, time::Duration};

use futures::stream::SplitSink;
use futures::stream::SplitStream;
use futures::stream::StreamExt;
use futures::SinkExt;
use http_body_util::Full;
use hyper::{body::Bytes, upgrade::Upgraded, Request, Response};
use hyper_tungstenite::{
    is_upgrade_request, tungstenite::error::ProtocolError, tungstenite::Error::AlreadyClosed,
    tungstenite::Error::ConnectionClosed, tungstenite::Error::Protocol, tungstenite::Message,
    upgrade, HyperWebsocket, WebSocketStream,
};
use hyper_util::rt::TokioIo;
use tokio::{net::TcpListener, spawn, sync::Mutex, time::sleep};
use tracing::{event, instrument, Level};
use uuid::Uuid;

type SharedConnections = Arc<
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
    pub async fn build() -> Result<Self, Error> {
        let listener = TcpListener::bind("0.0.0.0:0")
            .await
            .expect("Unable to bind to a random port");
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
}

#[instrument(name = "handle_request", skip(request, connections), fields(client = tracing::field::Empty))]
async fn handle_request(
    mut request: Request<hyper::body::Incoming>,
    connections: SharedConnections,
) -> Result<Response<Full<Bytes>>, Error> {
    tracing::Span::current().record(
        "client",
        &tracing::field::display(
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
                            if let Err(err) = handle_websocket(websocket, connections).await {
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
                    "Client sent a standard HTTP request. Sending back 426 upgrade response"
                );
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

type Error = Box<dyn std::error::Error + Send + Sync + 'static>;

#[instrument(name = "handle_websocket", skip(websocket, connections))]
async fn handle_websocket(
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

            let ws_sender_clone = ws_sender.clone();
            spawn(async move {
                sleep(Duration::from_secs(5)).await;
                let mut locked_ws_sender = ws_sender_clone.lock().await;
                if let Err(e) = locked_ws_sender.send(Message::text("Ping!")).await {
                    event!(Level::ERROR, "Failed to send 'ping': {}", e);
                } else {
                    event!(Level::DEBUG, "'pinged' client: {}", client_id);
                }
            });

            let ws_receiver_clone = ws_receiver.clone();
            while let Some(ws_message) = ws_receiver_clone.lock().await.next().await {
                match ws_message {
                    Ok(message) => {
                        match message {
                            Message::Text(msg) => println!("Received text message: {msg}"),

                            Message::Binary(msg) => println!("Received binary message: {msg:02X?}"),

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

#[tokio::test]
async fn health_check_works() {
    let server = Server::build().await.expect("Failed to build server");
    let port = server.port;

    // Spawn the server on a background task
    tokio::spawn(async move {
        server.run().await;
    });

    let client = reqwest::Client::new();

    let response = client
        .get(&format!("http://127.0.0.1:{}/health-check", port))
        .send()
        .await
        .expect("Failed to execute request");

    assert!(response.status().is_success());

    let text = response
        .text()
        .await
        .expect("Failed to read text from response");

    assert_eq!(text, "Alive and kickin!");
}

#[tokio::test]
async fn requests_upgrade() {
    let server = Server::build().await.expect("Failed to build server");
    let port = server.port;

    // Spawn the server on a background task
    tokio::spawn(async move {
        server.run().await;
    });

    let client = reqwest::Client::new();

    let response = client
        .get(&format!("http://127.0.0.1:{}", port))
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(response.status().as_u16(), 426);

    let headers = response.headers();

    assert_eq!(headers.get("Connection").unwrap(), "upgrade");
    assert_eq!(headers.get("Upgrade").unwrap(), "websocket");
}
