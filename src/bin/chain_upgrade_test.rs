use std::env;

use clap::Parser;
use common::{
    cosmovisor::{
        cosmovisor, cosmovisor_setup, cosmovisor_start, get_delegations_to_validator,
        wait_for_height,
    },
    nom, Args, ONE_SEC, TIMEOUT,
};
use lazy_static::lazy_static;
use super_orchestrator::{
    docker::{Container, ContainerNetwork},
    sh, std_init, MapAddError, Result, STD_TRIES,
};

lazy_static! {
    static ref DAEMON_NAME: String = env::var("DAEMON_NAME").unwrap();
    static ref DAEMON_HOME: String = env::var("DAEMON_HOME").unwrap();
    static ref ONOMY_CURRENT_VERSION: String = env::var("ONOMY_CURRENT_VERSION").unwrap();
    static ref ONOMY_UPGRADE_VERSION: String = env::var("ONOMY_UPGRADE_VERSION").unwrap();
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
    let dockerfile = "./dockerfiles/chain_upgrade_test.dockerfile";
    let container_target = "x86_64-unknown-linux-gnu";
    let logs_dir = "./logs";
    let entrypoint = "chain_upgrade_test";

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
    // as long as none of our operations are delayed longer than a block, this works
    let upgrade_height = "5";
    let proposal_id = "1";
    let gov_period = "4s";

    cosmovisor_setup(DAEMON_HOME.as_str(), gov_period).await?;
    let mut cosmovisor_runner = cosmovisor_start("entrypoint_cosmovisor.log", false, None).await?;

    dbg!(super_orchestrator::DisplayStr(
        &get_delegations_to_validator().await?
    ));

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

    wait_for_height(STD_TRIES, ONE_SEC, 5).await?;
    dbg!(super_orchestrator::DisplayStr(
        &get_delegations_to_validator().await?
    ));
    // TODO check that the upgrade was successful

    cosmovisor_runner.terminate().await?;

    Ok(())
}
