use std::time::Duration;

use common::container_runner;
use log::info;
use onomy_test_lib::{
    cosmovisor::{
        cosmovisor_get_addr, cosmovisor_start, get_apr_annual, get_delegations_to,
        get_staking_pool, sh_cosmovisor,
    },
    dockerfiles::onomy_std_cosmos_daemon,
    onomy_std_init, reprefix_bech32,
    setups::gravity_standalone_setup,
    super_orchestrator::{
        sh,
        stacked_errors::{Error, Result, StackableErr},
    },
    Args, TIMEOUT,
};
use tokio::time::sleep;

#[tokio::main]
async fn main() -> Result<()> {
    let args = onomy_std_init()?;

    if let Some(ref s) = args.entry_name {
        match s.as_str() {
            "gravity" => gravity_runner(&args).await,
            _ => Err(Error::from(format!("entry_name \"{s}\" is not recognized"))),
        }
    } else {
        sh("make --directory ./../arc/module clean", &[])
            .await
            .stack()?;
        sh("make --directory ./../arc/module build", &[])
            .await
            .stack()?;
        sh(
            "cp ./../arc/module/build/gravity ./tests/dockerfiles/dockerfile_resources/gravity",
            &[],
        )
        .await
        .stack()?;
        container_runner(&args, &[(
            "gravity",
            &onomy_std_cosmos_daemon("gravity", ".gravity", "v0.1.0", "gravity"),
        )])
        .await
        .stack()
    }
}

async fn gravity_runner(args: &Args) -> Result<()> {
    let daemon_home = args.daemon_home.as_ref().stack()?;
    gravity_standalone_setup(daemon_home).await.stack()?;
    let mut cosmovisor_runner = cosmovisor_start("gravity_runner.log", None).await.stack()?;

    let addr: &String = &cosmovisor_get_addr("validator").await.stack()?;
    let valoper_addr = &reprefix_bech32(addr, "onomyvaloper").stack()?;
    info!("{}", get_apr_annual(valoper_addr).await.stack()?);

    info!("{}", get_delegations_to(valoper_addr).await.stack()?);
    info!("{:?}", get_staking_pool().await.stack()?);

    sleep(Duration::from_secs(3)).await;
    cosmovisor_runner.terminate(TIMEOUT).await.stack()?;
    // test that exporting works
    let _ = sh_cosmovisor("export", &[]).await.stack()?;

    Ok(())
}
