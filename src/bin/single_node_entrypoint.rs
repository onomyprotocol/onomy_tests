use std::env;

use common::cosmovisor::cosmovisor_setup;
use lazy_static::lazy_static;
use super_orchestrator::{
    get_separated_val, sh, std_init, wait_for_ok, Command, LogFileOptions, Result, STD_DELAY,
    STD_TRIES,
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

    cosmovisor_setup(DAEMON_HOME.as_str(), GOV_PERIOD.as_str()).await?;

    // done preparing
    let mut cosmovisor = Command::new("cosmovisor run start --inv-check-period  1", &[])
        .stderr_log(&cosmovisor_log)
        .stdout_log(&cosmovisor_log)
        .run()
        .await?;
    wait_for_ok(STD_TRIES, STD_DELAY, || sh("cosmovisor run status", &[])).await?;

    let validator_addr = get_separated_val(
        &sh("cosmovisor run keys show validator", &[]).await?,
        "\n",
        "address",
        ":",
    )?;
    let addr_bytes = get_separated_val(
        &sh("cosmovisor run keys parse", &[&validator_addr]).await?,
        "\n",
        "bytes",
        ":",
    )?;
    let valoper_addr = format!(
        "onomyvaloper1{}",
        get_separated_val(
            &sh("cosmovisor run keys parse", &[&addr_bytes]).await?,
            "\n",
            "- onomyvaloper",
            "1"
        )?
    );
    println!(
        "{}",
        sh("cosmovisor run query staking delegations-to", &[
            &valoper_addr
        ])
        .await?
    );

    //

    sleep(common::TIMEOUT).await;
    cosmovisor.terminate().await?;

    Ok(())
}
