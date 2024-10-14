/*!
This crate is the server application for tunnelcrust. A server is an implementation of a _proxy_
in which clients (see client package) will connect to for the tunneling to take place.

The server is responsible for maintaining an active connection to a client, receiving incoming
requests, and forwarding these to the connected client to produce a response.

```bash
cargo run
```
*/

use std::error::Error;

use server::{
    configuration::get_configuration,
    startup::Server,
    telemetry::{get_subscriber, init_subscriber},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    // Setup and initialise the logger
    let subscriber = get_subscriber(String::from("tunnelcrust-server"), String::from("info"));
    init_subscriber(subscriber);

    // Prepare application configuration
    let configuration = get_configuration().expect("Failed to prepare configuration. Have you populated the '/configuration/settings.yaml' file?");

    let server = Server::build(configuration).await?;

    server.run().await;

    Ok(())
}
