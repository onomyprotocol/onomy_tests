use std::env;

use clap::Parser;
use common::{
    cosmovisor::{cosmovisor_setup, cosmovisor_start},
    Args, TIMEOUT,
};
use lazy_static::lazy_static;
use super_orchestrator::{
    docker::{Container, ContainerNetwork},
    sh, std_init, MapAddError, Result,
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
    let gov_period = "20s";
    cosmovisor_setup(DAEMON_HOME.as_str(), gov_period).await?;
    let mut cosmovisor_runner = cosmovisor_start("onomyd_runner.log", false, None).await?;

    dbg!(common::cosmovisor::get_delegations_to_validator().await?);

    sleep(common::TIMEOUT).await;
    cosmovisor_runner.terminate().await?;

    Ok(())
}
