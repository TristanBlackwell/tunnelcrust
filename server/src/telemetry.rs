use std::{
    fs::File,
    io::{self, Stdout, Write},
    sync::Mutex,
};

use tracing::{dispatcher::set_global_default, Subscriber};
use tracing_bunyan_formatter::{BunyanFormattingLayer, JsonStorageLayer};
use tracing_log::LogTracer;
use tracing_subscriber::{fmt::MakeWriter, layer::SubscriberExt, EnvFilter, Registry};

/// A writer that sends output to both stdout and a file.
struct MultiWriter {
    stdout: Stdout,
    file: Mutex<File>, // We use a Mutex to allow safe concurrent access
}

impl<'a> MakeWriter<'a> for MultiWriter {
    type Writer = MultiWriterGuard<'a>;

    fn make_writer(&'a self) -> Self::Writer {
        MultiWriterGuard {
            stdout: io::stdout(),
            file: self.file.lock().unwrap(),
        }
    }
}

/// A guard that wraps the actual writers.
struct MultiWriterGuard<'a> {
    stdout: Stdout,
    file: std::sync::MutexGuard<'a, File>, // Guard for safe access to the file
}

impl<'a> Write for MultiWriterGuard<'a> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.stdout.write_all(buf)?;

        self.file.write_all(buf)?;

        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stdout.flush()?;
        self.file.flush()
    }
}

/// Gets a tracing subscriber set at the `RUST_LOG` environment variable (otherwise the value provided)
/// with a JSON storage layer & bunyan formatting.
pub fn get_subscriber(name: String, env_filter: String) -> impl Subscriber + Send + Sync {
    // Will open or create a log file.
    let file = File::create("./logs/logs.log").expect("Failed to create log file");

    // The multi-writer will write logs to std out & to the log file above.
    let multi_writer = MultiWriter {
        stdout: io::stdout(),
        file: Mutex::new(file),
    };

    // Default log level to `RUST_LOG` env var otherwise whatever is provided here.
    let env_filter = EnvFilter::try_from_default_env().unwrap_or(EnvFilter::new(env_filter));
    let formatting_layer = BunyanFormattingLayer::new(name, multi_writer);

    Registry::default()
        .with(env_filter)
        .with(JsonStorageLayer)
        .with(formatting_layer)
}

/// Creates a `log` instance as the global logger with the subscriber as the
/// global default.
pub fn init_subscriber(subscriber: impl Subscriber + Send + Sync) {
    LogTracer::init().expect("Failed to initialise logger");

    set_global_default(subscriber.into()).expect("Failed to set subscriber for logging");
}
