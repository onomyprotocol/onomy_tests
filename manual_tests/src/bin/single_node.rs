use std::env;

use clap::Parser;
use lazy_static::lazy_static;
use log::warn;
use onomy_test_lib::{
    cosmovisor::{
        self, cosmovisor_start, get_apr_annual, get_staking_pool, get_treasury,
        get_treasury_inflation_annual, onomyd_setup, sh_cosmovisor, wait_for_num_blocks,
    },
    Args, TIMEOUT,
};
use stacked_errors::{MapAddError, Result};
use super_orchestrator::{
    docker::{Container, ContainerNetwork},
    get_separated_val, sh, std_init,
};
use tokio::time::sleep;

lazy_static! {
    static ref DAEMON_NAME: String = env::var("DAEMON_NAME").unwrap();
    static ref DAEMON_HOME: String = env::var("DAEMON_HOME").unwrap();
}

#[tokio::main]
async fn main() -> Result<()> {
    std_init()?;
    let args = Args::parse();

    if let Some(ref s) = args.entrypoint {
        match s.as_str() {
            "onomyd" => onomyd_runner().await,
            _ => format!("entrypoint \"{s}\" is not recognized").map_add_err(|| ()),
        }
    } else {
        container_runner().await
    }
}

async fn container_runner() -> Result<()> {
    let dockerfile = "./dockerfiles/single_node.dockerfile";
    let container_target = "x86_64-unknown-linux-gnu";
    let logs_dir = "./logs";
    let entrypoint = "single_node";

    /*sh("make --directory ./../onomy/ build", &[]).await?;
    // copy to dockerfile resources (docker cannot use files from outside cwd)
    sh(
        "cp ./../onomy/onomyd ./dockerfiles/dockerfile_resources/onomyd",
        &[],
    )
    .await?;*/

    // build internal runner
    sh("cargo build --release --bin", &[
        entrypoint,
        "--target",
        container_target,
    ])
    .await?;

    let mut cn = ContainerNetwork::new(
        "test",
        vec![Container::new(
            "onomyd",
            Some(dockerfile),
            None,
            &[],
            &[("./logs", "/logs")],
            &format!("./target/{container_target}/release/{entrypoint}"),
            &["--entrypoint", "onomyd"],
        )],
        false,
        logs_dir,
    )?;
    cn.run_all(true).await?;
    cn.wait_with_timeout_all(true, TIMEOUT).await.unwrap();
    Ok(())
}

async fn onomyd_runner() -> Result<()> {
    onomyd_setup(DAEMON_HOME.as_str()).await?;
    let mut cosmovisor_runner = cosmovisor_start("onomyd_runner.log", false, None).await?;

    // the chain is functional and has done its first block, but the rewards don't
    // start until the second block
    wait_for_num_blocks(1).await?;

    warn!("{}", get_apr_annual().await?);

    dbg!(cosmovisor::get_delegations_to_validator().await?);

    dbg!(get_staking_pool().await?);
    dbg!(get_treasury().await?);
    dbg!(get_treasury_inflation_annual().await?);
    dbg!(get_apr_annual().await?);

    wait_for_num_blocks(5).await?;
    warn!("{}", get_apr_annual().await?);

    let validator_addr = get_separated_val(
        &sh_cosmovisor("keys show validator", &[]).await?,
        "\n",
        "address",
        ":",
    )?;
    sh(
        &format!(
            "cosmovisor run tx bank send {validator_addr} \
             onomy1a5vn0tgp5tvqmsyrfaq03nkyh2vh5x58ltsvfs 1337anom --gas auto --gas-adjustment \
             1.3 -y -b block --from validator"
        ),
        &[],
    )
    .await?;

    sleep(TIMEOUT).await;
    cosmovisor_runner.terminate().await?;

    Ok(())
}
