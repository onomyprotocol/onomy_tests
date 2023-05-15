use std::env;

use common::cosmovisor::{cosmovisor_start, get_delegations_to_validator, provider_setup};
use lazy_static::lazy_static;
use super_orchestrator::{std_init, Result};
use tokio::time::sleep;

lazy_static! {
    static ref DAEMON_NAME: String = env::var("DAEMON_NAME").unwrap();
    static ref DAEMON_HOME: String = env::var("DAEMON_HOME").unwrap();
    static ref ONOMY_CURRENT_VERSION: String = env::var("ONOMY_CURRENT_VERSION").unwrap();
}

#[tokio::main]
async fn main() -> Result<()> {
    std_init()?;
    let gov_period = "30s";
    provider_setup(DAEMON_HOME.as_str(), gov_period).await?;
    let mut cosmovisor_runner = cosmovisor_start().await?;

    dbg!(get_delegations_to_validator().await?);

    sleep(common::TIMEOUT).await;
    cosmovisor_runner.terminate().await?;

    Ok(())
}
