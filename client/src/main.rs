/*!
This crate is the client CLI for tunnelcrust. It extends helper methods to easily spin up a tunnel from
your local machine.

```bash
client --url http://localhost:3000
```
*/

use clap::Parser;

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

fn main() {
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

    println!("Setting up a tunnel connection to '{}'...", address);
}
