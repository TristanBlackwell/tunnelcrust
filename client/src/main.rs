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
use http_body_util::BodyExt;
use http_body_util::Empty;
use hyper::body::Bytes;
use hyper::Request;
use hyper_util::rt::TokioIo;
use protocol::RequestBytesBinaryProtocol;
use tokio::io::{self, AsyncWriteExt as _};
use tokio::net::TcpStream;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Error, Message},
};

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
            "http://localhost:{}",
            args.location
                .port
                .expect("Unable to determine port. Please use '--url' argument instead.")
        )
    };

    let server_url = env::var("TUNNELCRUST_SERVER_URL")
        .expect("Unknown server address. Is the `TUNNELCRUST_SERVER_URL` set?");

    println!("Setting up a tunnel connection to '{}'...", address);

    let (_, mut stdin_rx) = futures_channel::mpsc::unbounded::<Message>();

    // Establish the WebSocket connection to the proxy server.
    let (ws_stream, _) = connect_async(server_url).await.expect("Failed to connect");
    println!("WebSocket handshake has been successfully completed");

    let (mut write, mut read) = ws_stream.split();

    loop {
        tokio::select! {
                    // Incoming message
                    Some(message) = read.next() => {
                        match message {
                            Ok(Message::Text(text)) => println!("Received text: {}", text),
                            Ok(Message::Binary(bytes)) => {
                                println!("Received bytes");

                                let request = hyper::Request::<Bytes>::deserialize(&bytes).await.expect("Failed to deserialize request");

                                println!("Method: {:?}, path: {:?}, headers_len: {:?}, body: {:?}", request.method(), request.uri().path(), request.headers(), request.body());

                                // Create our URI. This is the local address we're targetting combined with the path from the request that
                                // has been forward.
                                let uri = format!("{}{}", address, request.uri().path()).parse::<hyper::Uri>().expect("Failed to parse URI");

                                let host = uri.host().expect("uri has no host");
                                let port = uri.port_u16().unwrap_or(80);
                                let address = format!("{}:{}", host, port);
        dbg!(&address);
                                // Open a TCP connection to the remote host
                                let stream = TcpStream::connect(address).await.expect("Failed to connect to address");

                                // Use an adapter to access something implementing `tokio::io` traits as if they implement
                                // `hyper::rt` IO traits.
                                let io = TokioIo::new(stream);

                                // Create the Hyper client
                                let (mut sender, conn) = hyper::client::conn::http1::handshake(io).await.expect("Failed to create the client");

                                // Spawn a task to poll the connection, driving the HTTP state
                                tokio::task::spawn(async move {
                                    if let Err(err) = conn.await {
                                        println!("Connection failed: {:?}", err);
                                    }
                                });

                                // The authority of our URL will be the hostname of the httpbin remote
                                let authority = uri.authority().unwrap().clone();

                                // Create an HTTP request with an empty body and a HOST header
                                let req = Request::builder()
                                    .uri(uri)
                                    .header(hyper::header::HOST, authority.as_str())
                                    .body(Empty::<Bytes>::new()).expect("Failed to build request");

                                // Await the response...
                                let mut res = sender.send_request(req).await.expect("Failed tos send request");

                                println!("Response status: {}", res.status());

                                // Stream the body, writing each frame to stdout as it arrives
                                while let Some(next) = res.frame().await {
                                    let frame = next.expect("Failed to get next frame");
                                    if let Some(chunk) = frame.data_ref() {
                                        io::stdout().write_all(chunk).await.expect("Failed to write response to output");
                                    }
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
