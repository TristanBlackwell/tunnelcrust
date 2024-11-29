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

use client::{
    configuration::prepare_configuration, local::establish_local_connection,
    websocket::run_until_stopped,
};

#[tokio::main]
async fn main() {
    let config = prepare_configuration();

    let sender = establish_local_connection(&config.address).await;

    println!("Connecting to the remote tunnel");

    run_until_stopped(&config, sender).await;

    println!("Connection closed.");
}
