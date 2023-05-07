use std::time::Duration;

use tokio::time::sleep;

#[tokio::main]
async fn main() {
    for arg in std::env::args() {
        println!("{arg}");
    }
    sleep(Duration::from_secs(30)).await;
}
