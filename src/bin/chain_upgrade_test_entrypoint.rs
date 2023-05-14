use std::env;

use common::{
    cosmovisor::{cosmovisor, cosmovisor_setup, cosmovisor_start, get_delegations_to_validator, wait_for_height},
    nom, ONE_SEC,
};
use lazy_static::lazy_static;
use super_orchestrator::{std_init, Result, STD_TRIES};
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

    let upgrade_height = "15";
    let proposal_id = "1";

    cosmovisor_setup(DAEMON_HOME.as_str(), GOV_PERIOD.as_str()).await?;
    let mut cosmovisor_runner = cosmovisor_start().await?;

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

    wait_for_height(STD_TRIES, ONE_SEC, 10).await?;
    dbg!(super_orchestrator::DisplayStr(&get_delegations_to_validator().await?));
    wait_for_height(STD_TRIES, ONE_SEC, 16).await?;
    dbg!(super_orchestrator::DisplayStr(&get_delegations_to_validator().await?));

    sleep(common::TIMEOUT).await;
    cosmovisor_runner.terminate().await?;

    Ok(())
}
