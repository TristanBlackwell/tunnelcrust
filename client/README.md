# Client

This folder holds the Tunnelcrust CLI client, used by a local machine to proxy requests via the server.

The server can be found in the [`/server`](../server/) folder.

## Get Started

> [!IMPORTANT]
> - Ensure you have met the development prerequisites listed in the main [README](../README.md).
> - You need an instance of the [`/server`](../server/) running (locally or remote) for the client to be functional.


### Server URL

A client expects a `TUNNELCRUST_SERVER_URL` environment variable containing the Websocket URL of the server in which it will connect to. If you are running this locally that may look something like: `ws://localhost:8000`.

You can declare this inline withe commands e.g. `TUNNELCRUST_SERVER_URL=ws://localhost:8000 cargo run ...` or set the environment variable permanently e.g. `export TUNNELCRUST_SERVER_URL=ws://localhost:8000`. The below commands exclude this for brevity so be sure that this is satisfied.

### Commands

You run this in dev mode 2 ways:

1. `cargo run --bin client <COMMANDS>` from the repository root.
2. `cd client` followed by `cargo run <COMMANDS>`.

The client expects one of two commands to set up a end-to-end connection. Those are both used to describe where traffic should be forwarded to but follow different methods:

**URL**

You can use the `--url` command to supply the full URL where inbound traffic is routed. For example if you wanted to route traffic to `localhost:3000`. The command would look like:

```bash
cargo run --bin client -- --url http://localhost:3000
```

**Port**

The `--port` command can be used as a shorthand for `--url`. This will assume localhost as the target but allow you to omit the localhost part of the address. For example, the equivalent command to above using `--port` would look like:

```bash
cargo run --bin client -- --port 3000
```

> [!TIP]
> - Use `--help` or `-h` to see all available commands and descriptions.
> - When running via cargo, ensure you include the standalone `--`. This forces arguments after this to be forwarded to the CLI rather than the cargo command itself.