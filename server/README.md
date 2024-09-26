# Server

This folder holds the Tunnelcrust server. The middleman responsible for receiving requests, forwarding to the client, and sending back responses.

The CLI client can be used to connect to a server instance. This can be found in the [`/client`](../client/) folder.

## Get Started

> [!IMPORTANT]
> Ensure you have met the development prerequisites listed in the main [README](../README.md).

You run this in dev mode 2 ways:

1. `cargo run --bin server` from the repository root.
2. `cd server` followed by `cargo run`.

## Tests

The server has a series of unit tests. You can run these using `cargo test`.

## Logging

The logging implementation uses the [tracing](https://github.com/tokio-rs/tracing) crate and auxillary packages.

Logs follow Bunyan JSON formatting and write to:

1. stdout
2. A `logs.log` file under [`/logs`](./logs/)