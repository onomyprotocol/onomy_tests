use std::time::Duration;

use common::container_runner;
use log::info;
use onomy_test_lib::{
    cosmovisor::{
        cosmovisor_get_addr, cosmovisor_get_balances, cosmovisor_start, sh_cosmovisor_no_dbg,
    },
    dockerfiles::onomy_std_cosmos_daemon,
    market::{CoinPair, Market},
    onomy_std_init,
    setups::market_standalone_setup,
    super_orchestrator::{
        sh,
        stacked_errors::{Error, Result, StackableErr},
        Command, FileOptions,
    },
    Args, TIMEOUT,
};
use tokio::time::sleep;

const CHAIN_ID: &str = "market";

#[tokio::main]
async fn main() -> Result<()> {
    let args = onomy_std_init()?;

    if let Some(ref s) = args.entry_name {
        match s.as_str() {
            "standalone" => standalone_runner(&args).await,
            _ => Err(Error::from(format!("entry_name \"{s}\" is not recognized"))),
        }
    } else {
        let mut cmd = Command::new(&format!("go build ./cmd/{CHAIN_ID}d"), &[]).ci_mode(true);
        cmd.cwd = Some("./../market/".to_owned());
        let comres = cmd.run_to_completion().await.stack()?;
        comres.assert_success()?;
        // copy to dockerfile resources (docker cannot use files from outside cwd)
        sh(
            &format!(
                "cp ./../market/{CHAIN_ID}d ./tests/dockerfiles/dockerfile_resources/{CHAIN_ID}d"
            ),
            &[],
        )
        .await
        .stack()?;
        container_runner(&args, &[(
            "standalone",
            &onomy_std_cosmos_daemon(
                &format!("{CHAIN_ID}d"),
                &format!(".{CHAIN_ID}"),
                "v0.1.0",
                &format!("{CHAIN_ID}d"),
            ),
        )])
        .await
        .stack()
    }
}

async fn standalone_runner(args: &Args) -> Result<()> {
    let daemon_home = args.daemon_home.as_ref().stack()?;
    market_standalone_setup(daemon_home, CHAIN_ID)
        .await
        .stack()?;
    let mut cosmovisor_runner = cosmovisor_start(&format!("{CHAIN_ID}d_runner.log"), None).await?;

    let market = Market::new("validator", "1000000anative");

    let addr = &cosmovisor_get_addr("validator").await.stack()?;
    info!("{:?}", cosmovisor_get_balances(addr).await.stack()?);
    let coin_pair = CoinPair::new("afootoken", "anative").stack()?;

    // test numerical limits
    market
        .create_pool(&coin_pair, Market::MAX_COIN, Market::MAX_COIN)
        .await
        .stack()?;
    market
        .create_drop(&coin_pair, Market::MAX_COIN_SQUARED)
        .await
        .stack()?;
    market.show_pool(&coin_pair).await.stack()?;
    market.show_members(&coin_pair).await.stack()?;
    market
        .market_order(
            coin_pair.coin_a(),
            coin_pair.coin_b(),
            Market::MAX_COIN,
            5000,
        )
        .await
        .stack()?;
    market.redeem_drop(1).await.stack()?;
    market
        .create_order(
            coin_pair.coin_a(),
            coin_pair.coin_b(),
            "stop",
            Market::MAX_COIN,
            (1100, 900),
            (0, 0),
        )
        .await
        .stack()?;
    market
        .create_order(
            coin_pair.coin_a(),
            coin_pair.coin_b(),
            "limit",
            Market::MAX_COIN,
            (1100, 900),
            (0, 0),
        )
        .await
        .stack()?;
    market.cancel_order(5).await.stack()?;

    sleep(Duration::ZERO).await;
    cosmovisor_runner.terminate(TIMEOUT).await.stack()?;
    // test that exporting works
    let exported = sh_cosmovisor_no_dbg("export", &[]).await.stack()?;
    FileOptions::write_str(&format!("/logs/{CHAIN_ID}d_export.json"), &exported)
        .await
        .stack()?;

    Ok(())
}
