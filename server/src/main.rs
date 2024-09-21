/*!
This crate is the server application for tunnelcrust. A server is an implementation of a _proxy_
in which clients (see client package) will connect to for the tunneling to take place.

The server is responsible for maintaining an active connection to a client, receiving incoming
requests, and forwarding these to the connected client to produce a response.
*/

use std::convert::Infallible;

use http_body_util::Full;
use hyper::{body::Bytes, server::conn::http1, service::service_fn, Request, Response};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    let listener = TcpListener::bind("0.0.0.0:0")
        .await
        .expect("Unable to bind to a random port");
    let port = listener
        .local_addr()
        .expect("Unable to determine local address")
        .port();

    println!("Listening on http://0.0.0.0:{}", port);

    // Continually accept new incoming connections
    loop {
        match listener.accept().await {
            Ok((stream, addr)) => {
                // TODO: validate the client is authorised to connect.
                println!("New client connection: {:?}", addr);

                let io = TokioIo::new(stream);

                // Spawn a task to handle this connection. This allows multiple connections
                // to be served concurrently.
                tokio::task::spawn(async move {
                    if let Err(err) = http1::Builder::new()
                        .serve_connection(io, service_fn(hello))
                        .await
                    {
                        eprintln!("Failed serving client connection: {:?}", err)
                    }
                });
            }
            Err(err) => eprintln!("Failed during client connection: {:?}", err),
        }
    }
}

async fn hello(_: Request<hyper::body::Incoming>) -> Result<Response<Full<Bytes>>, Infallible> {
    Ok(Response::new(Full::new(Bytes::from("Hello, World!"))))
}
