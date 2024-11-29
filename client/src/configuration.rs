use std::env;

use clap::{command, Parser};

/// Configuration for the application
pub struct Config {
    /// Address of the local service which requests
    /// are forwarded to.
    pub address: String,
    pub server_url: String,
}

// CLI arguments
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[clap(flatten)]
    pub location: Location,
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

/// Parses arguments passed in from CLI commands and extracts variables from
/// the environment. This will panic on missing, expected arguments with a clear error message.
pub fn prepare_configuration() -> Config {
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

    let server_url = env::var("TUNNELCRUST_SERVER_URL")
        .expect("Unknown server address. Is the `TUNNELCRUST_SERVER_URL` set?");

    Config {
        address,
        server_url,
    }
}
