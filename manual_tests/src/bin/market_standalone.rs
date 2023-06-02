use std::env;

use clap::Parser;
use lazy_static::lazy_static;
use onomy_test_lib::{
    cosmovisor::{cosmovisor_start, market_standaloned_setup, sh_cosmovisor},
    Args, TIMEOUT,
};
use stacked_errors::{MapAddError, Result};
use super_orchestrator::{
    docker::{Container, ContainerNetwork},
    remove_files_in_dir, sh, std_init,
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
            "market_standaloned" => market_standaloned_runner().await,
            _ => format!("entrypoint \"{s}\" is not recognized").map_add_err(|| ()),
        }
    } else {
        container_runner().await
    }
}

async fn container_runner() -> Result<()> {
    let container_target = "x86_64-unknown-linux-gnu";
    let logs_dir = "./logs";
    let this_bin = "market_standalone";

    // build internal runner with `--release`
    sh("cargo build --release --bin", &[
        this_bin,
        "--target",
        container_target,
    ])
    .await?;

    // build binaries

    sh("make --directory ./../market/ build_standalone", &[]).await?;
    // copy to dockerfile resources (docker cannot use files from outside cwd)
    sh(
        "cp ./../market/market_standaloned ./dockerfiles/dockerfile_resources/market_standaloned",
        &[],
    )
    .await?;

    // prepare volumed resources
    remove_files_in_dir("./resources/keyring-test/", &["address", "info"]).await?;

    let entrypoint = &format!("./target/{container_target}/release/{this_bin}");
    let volumes = vec![("./logs", "/logs")];
    let mut cn = ContainerNetwork::new(
        "test",
        vec![Container::new(
            "market_standaloned",
            Some("./dockerfiles/market_standaloned.dockerfile"),
            None,
            &[],
            &volumes,
            entrypoint,
            &["--entrypoint", "market_standaloned"],
        )],
        true,
        logs_dir,
    )?;
    cn.run_all(true).await?;
    cn.wait_with_timeout_all(true, TIMEOUT).await?;
    Ok(())
}

async fn market_standaloned_runner() -> Result<()> {
    let daemon_home = DAEMON_HOME.as_str();
    market_standaloned_setup(daemon_home).await?;
    let mut cosmovisor_runner =
        cosmovisor_start("market_standaloned_runner.log", true, None).await?;

    // also `show-` versions of all these
    sh_cosmovisor("query market list-asset", &[]).await?;
    sh_cosmovisor("query market list-burnings", &[]).await?;
    sh_cosmovisor("query market list-drop", &[]).await?;
    sh_cosmovisor("query market list-member", &[]).await?;
    sh_cosmovisor("query market list-pool", &[]).await?;

    sh_cosmovisor("query market params", &[]).await?;
    //sh_cosmovisor("query market get-book [denom-a] [denom-b] [order-type]",
    // &[]).await?;

    //sh_cosmovisor("tx market create-pool [coin-a] [coin-b]").await?;

    //sh_cosmovisor("tx market create-drop [pair] [drops]").await?;
    //sh_cosmovisor("tx market redeem-drop [uid]").await?;

    //sh_cosmovisor("tx market market-order [denom-ask] [denom-bid] [amount-bid]
    // [quote-ask] [slippage]").await?;

    //sh_cosmovisor("tx market create-order [denom-ask] [denom-bid] [order-type]
    // [amount] [rate] [prev] [next]").await?; cosmovisor("tx market
    // cancel-order [uid]").await?;

    sleep(TIMEOUT).await;
    cosmovisor_runner.terminate().await?;
    Ok(())
}
