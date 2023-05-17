use std::env;

use clap::Parser;
use common::{
    cosmovisor::{cosmovisor_setup, cosmovisor_start, get_delegations_to_validator},
    TIMEOUT,
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
    static ref ONOMY_CURRENT_VERSION: String = env::var("ONOMY_CURRENT_VERSION").unwrap();
}

/// Runs ics_basic
#[derive(Parser, Debug)]
#[command(about)]
struct Args {
    /// If left `None`, the container runner program runs, otherwise this
    /// specifies the entrypoint to run
    #[arg(short, long)]
    entrypoint: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    std_init()?;
    let args = Args::parse();

    if let Some(ref s) = args.entrypoint {
        match s.as_str() {
            "onomyd" => onomyd().await,
            "marketd" => marketd().await,
            _ => format!("entrypoint \"{s}\" is not recognized").map_add_err(|| ()),
        }
    } else {
        container_runner().await
    }
}

async fn container_runner() -> Result<()> {
    let container_target = "x86_64-unknown-linux-gnu";
    let logs_dir = "./logs";
    let this_bin = "ics_basic";

    // build internal runner with `--release`
    sh("cargo build --release --bin", &[
        this_bin,
        "--target",
        container_target,
    ])
    .await?;

    // build binaries
    sh("make --directory ./../onomy_workspace0/onomy/ build", &[]).await?;
    sh("make --directory ./../market/ build", &[]).await?;
    // copy to dockerfile resources (docker cannot use files from outside cwd)
    sh(
        "cp ./../onomy_workspace0/onomy/onomyd ./dockerfiles/dockerfile_resources/onomyd",
        &[],
    )
    .await?;
    sh(
        "cp ./../market/marketd ./dockerfiles/dockerfile_resources/marketd",
        &[],
    )
    .await?;

    let entrypoint = &format!("./target/{container_target}/release/{this_bin}");
    let mut cn = ContainerNetwork::new(
        "test",
        vec![
            Container::new(
                "onomyd",
                Some("./dockerfiles/onomyd.dockerfile"),
                "onomyd_build",
                &[],
                &[("./logs", "/logs")],
                entrypoint,
                &["--entrypoint", "onomyd"],
            ),
            Container::new(
                "marketd",
                Some("./dockerfiles/marketd.dockerfile"),
                "marketd_build",
                &[],
                &[("./logs", "/logs")],
                entrypoint,
                &["--entrypoint", "marketd"],
            ),
        ],
        false,
        logs_dir,
    );
    cn.run(true).await?;

    let ids = cn.get_ids();
    cn.wait_with_timeout(ids, TIMEOUT).await.unwrap();
    Ok(())
}

async fn onomyd() -> Result<()> {
    let gov_period = "20s";
    cosmovisor_setup(DAEMON_HOME.as_str(), gov_period).await?;
    let mut cosmovisor_runner = cosmovisor_start().await?;

    dbg!(get_delegations_to_validator().await?);

    sleep(common::TIMEOUT).await;
    cosmovisor_runner.terminate().await?;
    Ok(())
}

async fn marketd() -> Result<()> {
    let gov_period = "20s";
    cosmovisor_setup(DAEMON_HOME.as_str(), gov_period).await?;
    let mut cosmovisor_runner = cosmovisor_start().await?;

    dbg!(get_delegations_to_validator().await?);

    sleep(common::TIMEOUT).await;
    cosmovisor_runner.terminate().await?;
    Ok(())
}
