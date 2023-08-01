use common::container_runner;
use onomy_test_lib::{
    cosmovisor::{cosmovisor_get_addr, cosmovisor_start, sh_cosmovisor, sh_cosmovisor_tx},
    dockerfiles::onomy_std_cosmos_daemon,
    onomy_std_init,
    setups::market_standalone_setup,
    super_orchestrator::{
        sh,
        stacked_errors::{Error, Result, StackableErr},
    },
    Args, TIMEOUT,
};

#[tokio::main]
async fn main() -> Result<()> {
    let args = onomy_std_init()?;

    if let Some(ref s) = args.entry_name {
        match s.as_str() {
            "standalone" => standalone_runner(&args).await,
            _ => Err(Error::from(format!("entry_name \"{s}\" is not recognized"))),
        }
    } else {
        sh("make --directory ./../market/ build-standalone", &[]).await?;
        // copy to dockerfile resources (docker cannot use files from outside cwd)
        sh(
            "cp ./../market/market-standaloned \
             ./tests/dockerfiles/dockerfile_resources/market-standaloned",
            &[],
        )
        .await?;
        container_runner(&args, &[(
            "standalone",
            &onomy_std_cosmos_daemon("market", ".onomy_market", "v0.1.0", "market-standaloned"),
        )])
        .await
    }
}

async fn standalone_runner(args: &Args) -> Result<()> {
    let daemon_home = args.daemon_home.as_ref().stack()?;
    market_standalone_setup(daemon_home, "market").await?;
    let mut cosmovisor_runner = cosmovisor_start("standalone_runner.log", None).await?;

    let addr: &String = &cosmovisor_get_addr("validator").await?;
    dbg!(addr);
    println!(
        "cosmovisor run tx bank send {addr} onomy1a69w3hfjqere4crkgyee79x2mxq0w2pfj9tu2m \
         1337afootoken --gas auto --gas-adjustment 1.3 -y -b block"
    );
    println!(
        "cosmovisor run tx bank send {addr} onomy1a69w3hfjqere4crkgyee79x2mxq0w2pfj9tu2m \
         1337afootoken -y -b block --fees 100000afootoken"
    );
    // --gas-prices

    // there are also `show-` versions of these
    sh_cosmovisor("query market list-burnings", &[]).await?;
    sh_cosmovisor("query market list-drop", &[]).await?;
    sh_cosmovisor("query market list-member", &[]).await?;
    sh_cosmovisor("query market list-order", &[]).await?;
    sh_cosmovisor("query market list-pool", &[]).await?;

    sh_cosmovisor("query market params", &[]).await?;

    pub async fn market_create_pool(
        from_key: &str,
        gas_base: &str,
        coin_a: &str,
        coin_b: &str,
    ) -> Result<()> {
        sh_cosmovisor_tx("market create-pool", &[
            coin_a,
            coin_b,
            "-y",
            "-b",
            "block",
            "--gas",
            "auto",
            "--gas-adjustment",
            "1.3",
            "--gas-prices",
            gas_base,
            "--from",
            from_key,
        ])
        .await?;

        Ok(())
    }

    let gas_base = "1anative";
    let coin_a = "5000000afootoken";
    let coin_b = "1000000anative";

    market_create_pool(addr, gas_base, coin_a, coin_b).await?;

    //sh_cosmovisor("query market book [denom-a] [denom-b] [order-type]",
    // &[]).await?;
    //sh_cosmovisor("query market bookends [coin-a] [coin-b] [order-type] [rate]
    // [flags]", &[]).await?;

    //sh_cosmovisor_tx("market create-pool [coin-a] [coin-b]").await?;

    //sh_cosmovisor_tx("market create-drop [pair] [drops]").await?;
    //sh_cosmovisor_tx("market redeem-drop [uid]").await?;

    //sh_cosmovisor_tx("market market-order [denom-ask] [denom-bid] [amount-bid]
    // [quote-ask] [slippage]").await?;

    //sh_cosmovisor_tx("market create-order [denom-ask] [denom-bid] [order-type]
    // [amount] [rate] [prev] [next]").await?; cosmovisor("tx market
    // cancel-order [uid]").await?;

    cosmovisor_runner.terminate(TIMEOUT).await?;
    Ok(())
}
