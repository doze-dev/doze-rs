//! The console over the "larder" fixtures, with no broker: what the design boards show, served for a browser.
//!
//! `cargo run -p kafka-console --features examples --example boards [ADDR]` (default `127.0.0.1:19093`). The state of Home
//! is chosen with `/?state=quiet` and `/?state=attention`; a live console decides by its data. Stops on Ctrl-C.

use kafka_console::serve::{Settings, start};
use kafka_console::source::Source;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let addr = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:19093".to_owned());
    let settings = Settings {
        addr: addr
            .parse()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?,
        prefix: "/".to_owned(),
        allow_hosts: Vec::new(),
    };
    let running = start(Source::Fixture, settings).await?;
    println!(
        "boards on http://{}/ (fixtures, no broker); Ctrl-C stops",
        running.local_addr()
    );
    tokio::signal::ctrl_c().await?;
    running.shutdown().await;
    Ok(())
}
