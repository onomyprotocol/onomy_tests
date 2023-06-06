use common::container_runner;
use log::warn;
use onomy_test_lib::{
    cosmovisor::{
        cosmovisor_start, get_apr_annual, get_block_height, get_staking_pool, get_treasury,
        get_treasury_inflation_annual, onomyd_setup, sh_cosmovisor, wait_for_height,
        wait_for_num_blocks,
    },
    nom, onomy_std_init,
    super_orchestrator::{
        stacked_errors::{MapAddError, Result},
        STD_DELAY, STD_TRIES,
    },
    Args, TIMEOUT,
};
use tokio::time::sleep;

#[tokio::main]
async fn main() -> Result<()> {
    let args = onomy_std_init()?;

    if let Some(ref s) = args.entry_name {
        match s.as_str() {
            "onomyd" => onomyd_runner(&args).await,
            _ => format!("entry_name \"{s}\" is not recognized").map_add_err(|| ()),
        }
    } else {
        /*sh("make --directory ./../onomy/ build", &[]).await?;
        // copy to dockerfile resources (docker cannot use files from outside cwd)
        sh(
            "cp ./../onomy/onomyd ./tests/dockerfiles/dockerfile_resources/onomyd",
            &[],
        )
        .await?;*/
        container_runner(&args, &[("chain_upgrade", "onomyd")]).await
    }
}

async fn onomyd_runner(args: &Args) -> Result<()> {
    let onomy_current_version = args.onomy_current_version.as_ref().map_add_err(|| ())?;
    let onomy_upgrade_version = args.onomy_upgrade_version.as_ref().map_add_err(|| ())?;
    let daemon_home = args.daemon_home.as_ref().map_add_err(|| ())?;
    assert_ne!(onomy_current_version, onomy_upgrade_version);
    onomyd_setup(daemon_home, true).await?;
    let mut cosmovisor_runner = cosmovisor_start("onomyd_runner.log", false, None).await?;

    assert_eq!(
        sh_cosmovisor("version", &[]).await?.trim(),
        onomy_current_version
    );

    wait_for_num_blocks(1).await?;

    warn!("{}", get_apr_annual().await?);

    wait_for_num_blocks(1).await?;

    let upgrade_prepare_start = get_block_height().await?;
    let upgrade_height = &format!("{}", upgrade_prepare_start + 4);
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

    let description = &format!("\"upgrade {onomy_upgrade_version}\"");
    sh_cosmovisor(
        "tx gov submit-proposal software-upgrade",
        &[
            [
                onomy_upgrade_version,
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
    sh_cosmovisor(
        "tx gov deposit",
        &[[proposal_id, &nom(2000.0)].as_slice(), gas_args].concat(),
    )
    .await?;
    sh_cosmovisor(
        "tx gov vote",
        &[[proposal_id, "yes"].as_slice(), gas_args].concat(),
    )
    .await?;

    wait_for_height(STD_TRIES, STD_DELAY, upgrade_prepare_start + 5).await?;

    assert_eq!(
        sh_cosmovisor("version", &[]).await?.trim(),
        onomy_upgrade_version
    );

    dbg!(get_staking_pool().await?);
    dbg!(get_treasury().await?);
    dbg!(get_treasury_inflation_annual().await?);
    dbg!(get_apr_annual().await?);

    warn!("{}", get_apr_annual().await?);
    wait_for_num_blocks(20).await?;
    warn!("{}", get_apr_annual().await?);

    sleep(TIMEOUT).await;
    cosmovisor_runner.terminate().await?;

    Ok(())
}
