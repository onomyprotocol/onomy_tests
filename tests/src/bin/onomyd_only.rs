use std::time::Duration;

use common::container_runner;
use log::info;
use onomy_test_lib::{
    cosmovisor::{
        cosmovisor_get_addr, cosmovisor_start, get_apr_annual, get_delegations_to,
        get_staking_pool, get_treasury, get_treasury_inflation_annual, onomyd_setup, sh_cosmovisor,
        wait_for_num_blocks,
    },
    onomy_std_init, reprefix_bech32,
    super_orchestrator::{
        sh,
        stacked_errors::{MapAddError, Result},
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
        container_runner(&args, &[("onomyd", "onomyd")]).await
    }
}

async fn onomyd_runner(args: &Args) -> Result<()> {
    let daemon_home = args.daemon_home.as_ref().map_add_err(|| ())?;
    onomyd_setup(daemon_home).await?;
    let mut cosmovisor_runner = cosmovisor_start("onomyd_runner.log", None).await?;

    let addr: &String = &cosmovisor_get_addr("validator").await?;
    let valoper_addr = &reprefix_bech32(addr, "onomyvaloper").unwrap();
    info!("{}", get_apr_annual(valoper_addr).await?);

    info!("{}", get_delegations_to(valoper_addr).await?);
    info!("{:?}", get_staking_pool().await?);
    info!("{}", get_treasury().await?);
    info!("{}", get_treasury_inflation_annual().await?);
    info!("{}", get_apr_annual(valoper_addr).await?);

    wait_for_num_blocks(5).await?;
    info!("{}", get_apr_annual(valoper_addr).await?);

    sh(
        &format!(
            "cosmovisor run tx bank send {addr} onomy1a69w3hfjqere4crkgyee79x2mxq0w2pfj9tu2m \
             1337anom --gas auto --gas-adjustment 1.3 -y -b block"
        ),
        &[],
    )
    .await?;

    sleep(Duration::from_secs(3)).await;
    cosmovisor_runner.terminate(TIMEOUT).await?;
    // test that exporting works
    let _ = sh_cosmovisor("export", &[]).await?;

    Ok(())
}
