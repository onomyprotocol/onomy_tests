use std::time::Duration;

use log::warn;
use tokio::time::sleep;

#[tokio::main]
async fn main() {
    env_logger::Builder::new()
        .filter_level(log::LevelFilter::Info)
        .init();

    for arg in std::env::args() {
        println!("{arg}");
    }
    warn!("hello");
    sleep(Duration::from_secs(0)).await;
}
