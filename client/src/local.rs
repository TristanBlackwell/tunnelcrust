use http_body_util::Full;
use hyper::{body::Bytes, client::conn::http1::SendRequest};
use hyper_util::rt::TokioIo;
use tokio::net::TcpStream;

/// Connects to the local service passed as an argument to the CLI via `port` or `url`.
/// If this cannot establish a connection then the application panics.
pub async fn establish_local_connection(address: &str) -> SendRequest<Full<Bytes>> {
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
