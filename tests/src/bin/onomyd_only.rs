use common::container_runner;
use log::warn;
use onomy_test_lib::{
    cosmovisor::{
        self, cosmovisor_start, get_apr_annual, get_staking_pool, get_treasury,
        get_treasury_inflation_annual, onomyd_setup, sh_cosmovisor, wait_for_num_blocks,
    },
    onomy_std_init, Args, TIMEOUT,
};
use stacked_errors::{MapAddError, Result};
use super_orchestrator::{get_separated_val, sh};
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
        container_runner(&args, &[("onomyd", "onomyd")]).await
    }
}

async fn onomyd_runner(args: &Args) -> Result<()> {
    let daemon_home = args.daemon_home.as_ref().map_add_err(|| ())?;
    onomyd_setup(daemon_home, false).await?;
    let mut cosmovisor_runner = cosmovisor_start("onomyd_runner.log", false, None).await?;

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
