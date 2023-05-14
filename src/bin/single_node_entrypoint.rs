use std::env;

use common::cosmovisor::{cosmovisor_setup, cosmovisor_start, get_delegations_to_validator};
use lazy_static::lazy_static;
use super_orchestrator::{std_init, Result};
use tokio::time::sleep;

lazy_static! {
    static ref DAEMON_NAME: String = env::var("DAEMON_NAME").unwrap();
    static ref DAEMON_HOME: String = env::var("DAEMON_HOME").unwrap();
    static ref ONOMY_CURRENT_VERSION: String = env::var("ONOMY_CURRENT_VERSION").unwrap();
    static ref ONOMY_UPGRADE_VERSION: String = env::var("ONOMY_UPGRADE_VERSION").unwrap();
    static ref GOV_PERIOD: String = env::var("GOV_PERIOD").unwrap();
}

#[tokio::main]
async fn main() -> Result<()> {
    std_init()?;
    cosmovisor_setup(DAEMON_HOME.as_str(), GOV_PERIOD.as_str()).await?;
    let mut cosmovisor_runner = cosmovisor_start().await?;

    dbg!(get_delegations_to_validator().await?);

    sleep(common::TIMEOUT).await;
    cosmovisor_runner.terminate().await?;

    Ok(())
}
