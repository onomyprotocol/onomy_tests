use std::env;

use common::cosmovisor::cosmovisor_setup;
use lazy_static::lazy_static;
use super_orchestrator::{
    sh, std_init, wait_for_ok, Command, LogFileOptions, Result, STD_DELAY, STD_TRIES,
};
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
    let cosmovisor_log = Some(LogFileOptions::new(
        "/logs",
        "chain_upgrade_test_entrypoint_cosmovisor.log",
        true,
        true,
    ));

    cosmovisor_setup(DAEMON_HOME.as_str(), GOV_PERIOD.as_str()).await?;

    // done preparing
    let mut cosmovisor = Command::new("cosmovisor run start --inv-check-period  1", &[])
        .stderr_log(&cosmovisor_log)
        .stdout_log(&cosmovisor_log)
        .run()
        .await?;
    wait_for_ok(STD_TRIES, STD_DELAY, || sh("cosmovisor run status", &[])).await?;

    sleep(common::TIMEOUT).await;
    cosmovisor.terminate().await?;

    Ok(())
}
