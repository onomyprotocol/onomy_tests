use std::env;

use lazy_static::lazy_static;
use super_orchestrator::{sh, Result};

lazy_static! {
    static ref DAEMON_NAME: String = env::var("DAEMON_NAME").unwrap();
    static ref DAEMON_HOME: String = env::var("DAEMON_HOME").unwrap();
    static ref ONOMY_CURRENT_VERSION: String = env::var("ONOMY_CURRENT_VERSION").unwrap();
    static ref ONOMY_UPGRADE_VERSION: String = env::var("ONOMY_UPGRADE_VERSION").unwrap();
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::new()
        .filter_level(log::LevelFilter::Info)
        .init();

    // NOTE: this is stuff you would not want to run in production

    let chain_id = "onomy";
    sh("cosmovisor", &["run", "config", "chain_id", chain_id]).await?;

    Ok(())
}
