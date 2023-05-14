use std::{env, time::Duration};

use common::{
    cosmovisor::{cosmovisor, cosmovisor_setup, cosmovisor_start},
    nom,
};
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

    // note: there seems to be some race condition where the submit proposal happens
    // too soon even though status is showing as good
    sleep(Duration::from_secs(5)).await;

    let upgrade_height = "10";
    let proposal_id = "1";
    let gas_args = [
        "--gas",
        "auto",
        "--gas-adjustment",
        "1.3",
        "-y",
        "-b",
        "block",
        "--from",
        "validator",
    ]
    .as_slice();

    let upgrade_version = ONOMY_UPGRADE_VERSION.as_str();
    let description = &format!("\"upgrade {upgrade_version}\"");
    cosmovisor(
        "tx gov submit-proposal software-upgrade",
        &[
            [
                upgrade_version,
                "--title",
                description,
                "--description",
                description,
                "--upgrade-height",
                upgrade_height,
            ]
            .as_slice(),
            gas_args,
        ]
        .concat(),
    )
    .await?;
    cosmovisor(
        "tx gov deposit",
        &[[proposal_id, &nom(2000.0)].as_slice(), gas_args].concat(),
    )
    .await?;
    cosmovisor(
        "tx gov vote",
        &[[proposal_id, "yes"].as_slice(), gas_args].concat(),
    )
    .await?;

    sleep(common::TIMEOUT).await;
    cosmovisor_runner.terminate().await?;

    Ok(())
}
