use common::container_runner;
use onomy_test_lib::{
    cosmovisor::{cosmovisor_get_addr, cosmovisor_start, market_standaloned_setup, sh_cosmovisor},
    onomy_std_init,
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
            "market_standaloned" => market_standaloned_runner(&args).await,
            _ => format!("entry_name \"{s}\" is not recognized").map_add_err(|| ()),
        }
    } else {
        sh("make --directory ./../market/ build_standalone", &[]).await?;
        // copy to dockerfile resources (docker cannot use files from outside cwd)
        sh(
            "cp ./../market/market_standaloned \
             ./tests/dockerfiles/dockerfile_resources/market_standaloned",
            &[],
        )
        .await?;
        container_runner(&args, &[("market_standaloned", "market_standaloned")]).await
    }
}

async fn market_standaloned_runner(args: &Args) -> Result<()> {
    let daemon_home = args.daemon_home.as_ref().map_add_err(|| ())?;
    market_standaloned_setup(daemon_home).await?;
    let mut cosmovisor_runner = cosmovisor_start("market_standaloned_runner.log", None).await?;

    let addr: &String = &cosmovisor_get_addr("validator").await?;
    dbg!(addr);
    println!(
        "cosmovisor run tx bank send {addr} onomy1a69w3hfjqere4crkgyee79x2mxq0w2pfj9tu2m \
         1337afootoken --gas auto --gas-adjustment 1.3 -y -b block --from validator"
    );
    println!(
        "cosmovisor run tx bank send {addr} onomy1a69w3hfjqere4crkgyee79x2mxq0w2pfj9tu2m \
         1337afootoken -y -b block --from validator --fees 100000afootoken"
    );
    // --gas-prices

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
    cosmovisor_runner.terminate(TIMEOUT).await?;
    Ok(())
}
