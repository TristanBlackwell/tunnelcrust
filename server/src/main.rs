/*!
This crate is the server application for tunnelcrust. A server is an implementation of a _proxy_
in which clients (see client package) will connect to for the tunneling to take place.

The server is responsible for maintaining an active connection to a client, receiving incoming
requests, and forwarding these to the connected client to produce a response.
*/

use futures::sink::SinkExt;
use futures::stream::StreamExt;
use http_body_util::Full;
use hyper::{body::Bytes, Request, Response};
use hyper_tungstenite::{is_upgrade_request, tungstenite::Message, upgrade, HyperWebsocket};
use hyper_util::rt::TokioIo;
use server::telemetry::{get_subscriber, init_subscriber};
use tokio::net::TcpListener;
use tracing::{event, instrument, Level};

#[tokio::main]
async fn main() {
    // Setup and initialise the logger
    let subscriber = get_subscriber(String::from("tunnelcrust-server"), String::from("info"));
    init_subscriber(subscriber);

    let listener = TcpListener::bind("0.0.0.0:0")
        .await
        .expect("Unable to bind to a random port");
    let port = listener
        .local_addr()
        .expect("Unable to determine local address")
        .port();

    tracing::info!("Listening on http://0.0.0.0:{}", port);

    let mut http = hyper::server::conn::http1::Builder::new();
    http.keep_alive(true);

    // Continually accept new incoming connections
    loop {
        match listener.accept().await {
            Ok((stream, addr)) => {
                // TODO: validate the client is authorised to connect.
                tracing::info!("New incoming client request - {:?}", addr);

                // our request handler service will upgrade the Websocket connection
                // (spawning a new thread) or signal that the client should do so.
                // Either way the responses for this will pass through to the block
                // below.
                let connection = http
                    .serve_connection(
                        TokioIo::new(stream),
                        hyper::service::service_fn(handle_request),
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

#[instrument(name = "handle_request", skip(request), fields(client = tracing::field::Empty))]
async fn handle_request(
    mut request: Request<hyper::body::Incoming>,
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

    if is_upgrade_request(&request) {
        event!(Level::INFO, "Client requested an upgrade");

        match upgrade(&mut request, None) {
            Ok((response, websocket)) => {
                event!(
                    Level::INFO,
                    "Client request upgraded. Passing to websocket handler to establish connection"
                );

                tokio::spawn(async move {
                    if let Err(err) = handle_websocket(websocket).await {
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

type Error = Box<dyn std::error::Error + Send + Sync + 'static>;

#[instrument(name = "handle_websocket", skip(websocket))]
async fn handle_websocket(websocket: HyperWebsocket) -> Result<(), Error> {
    match websocket.await {
        Ok(mut websocket) => {
            event!(
                Level::INFO,
                "Websocket connection established. Awaiting messages from the client"
            );
            while let Some(message) = websocket.next().await {
                match message? {
                    Message::Text(msg) => {
                        println!("Received text message: {msg}");
                        websocket
                            .send(Message::text("Thank you, come again."))
                            .await?;
                    }
                    Message::Binary(msg) => {
                        println!("Received binary message: {msg:02X?}");
                        websocket
                            .send(Message::binary(b"Thank you, come again.".to_vec()))
                            .await?;
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
        }
        Err(err) => {
            event!(Level::ERROR, "Failed to establish websocket connection. Has the upgrade response been sent to the client?: {:?}", err);
        }
    }

    Ok(())
}
