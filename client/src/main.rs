/*!
This crate is the client CLI for tunnelcrust. It extends helper methods to easily spin up a tunnel from
your local machine.

The simplest way to get started (assuming you are using cargo directly) is to use the `--port` argument.
This will start the client, connect to the server, and direct any traffic to localhost on the port specified:

```bash
cargo run -- --port 3000
```

you can use the help command to view the available options:

```bash
cargo run -- --help
```
*/

use std::env;

use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use http_body_util::Full;
use hyper::body::Bytes;
use hyper::client::conn::http1::SendRequest;
use hyper::Request;
use hyper_util::rt::TokioIo;
use protocol::{RequestBytesBinaryProtocol, ResponseIncomingBinaryProtocol};
use tokio::net::TcpStream;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Error, Message},
};
use uuid::Uuid;

// CLI arguments
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[clap(flatten)]
    location: Location,
}

// Location argument group. One of these is provided for the server to know where
// traffic is being redirected to. Use `--url` to specific a full domain name or
// `--port` to target a specific localhost port.
#[derive(Debug, clap::Args)]
#[group(required = true, multiple = false)]
struct Location {
    #[clap(
        long,
        help = "Full URL in which to redirect requests e.g. http://localhost:3000"
    )]
    url: Option<String>,
    #[clap(
        short,
        long,
        help = "Alias for '--url' with localhost to only specify a port. e.g. '--port 3000' is equal to '--url http://localhost:3000'"
    )]
    port: Option<u16>,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    let address = if let Some(url) = args.location.url {
        url
    } else {
        format!(
            "http://0.0.0.0:{}",
            args.location
                .port
                .expect("Unable to determine port. Please use '--url' argument instead.")
        )
    };

    let mut sender = establish_local_connection(&address).await;

    let server_url = env::var("TUNNELCRUST_SERVER_URL")
        .expect("Unknown server address. Is the `TUNNELCRUST_SERVER_URL` set?");

    println!("Connecting to the remote tunnel");

    let (_, mut stdin_rx) = futures_channel::mpsc::unbounded::<Message>();

    // Establish the WebSocket connection to the proxy server.
    let (ws_stream, _) = connect_async(server_url).await.expect("Failed to connect");

    println!("Tunnel connected");

    let (mut write, mut read) = ws_stream.split();

    loop {
        tokio::select! {
            // Incoming message
            Some(message) = read.next() => {
                match message {
                    Ok(Message::Text(text)) => println!("Received text: {}", text),
                    Ok(Message::Binary(bytes)) => {
                        let request_parts = process_bytes(bytes).await;

                        println!("Request {} received from tunnel. Forwarding to local service...", request_parts.0);

                        let mut res = sender
                            .send_request(request_parts.1)
                            .await
                            .expect("Failed to send request");

                        println!("Response status: {}", res.status());

                        let serialized = res
                            .serialize()
                            .await
                            .expect("Failed to serialize response");

                            let request_id_bytes = request_id.to_bytes_le();
                            if let Err(e) = write.send(Message::Binary([&request_id_bytes, serialized.clone().as_slice()].concat())).await {
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

    println!("Connection closed.");
}

async fn establish_local_connection(address: &str) -> SendRequest<Full<Bytes>> {
    println!("Establishing connection with {}...", address);

    // TODO: Warn on local service not running and implement continuous retry logic
    let stream = TcpStream::connect(&address.replace("http://", ""))
        .await
        .expect("Failed to connect to address");

    // Use an adapter to access something implementing `tokio::io` traits as if they implement
    // `hyper::rt` IO traits.
    let io = TokioIo::new(stream);

    // Create the Hyper client
    let (sender, conn) = hyper::client::conn::http1::handshake(io)
        .await
        .expect("Failed to create the client");

    // Spawn a task to poll the connection, driving the HTTP state
    tokio::task::spawn(async move {
        if let Err(err) = conn.await {
            eprintln!("Connection failed: {:?}", err);
        }
    });

    println!("Connection established");

    sender
}

/// Reconstructs incoming bytes from the tunnel websocket connection
/// as an forwarded request.
async fn process_bytes(bytes: Vec<u8>) -> (Uuid, Request<Full<Bytes>>) {
    let request_id_bytes = &bytes[0..16];
    let array: [u8; 16] = request_id_bytes
        .try_into()
        .expect("Slice has wrong length!");
    let request_id = Uuid::from_bytes_le(array);

    let request = hyper::Request::<Bytes>::deserialize(&bytes)
        .await
        .expect("Failed to deserialize request");

    (request_id, request.clone().map(Full::new))
}
