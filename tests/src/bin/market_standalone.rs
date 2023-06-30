use common::container_runner;
use onomy_test_lib::{
    cosmovisor::{cosmovisor_get_addr, cosmovisor_start, market_standaloned_setup, sh_cosmovisor},
    dockerfiles::onomy_std_cosmos_daemon,
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
        container_runner(&args, &[(
            "market_standaloned",
            &onomy_std_cosmos_daemon(
                "market_standaloned",
                ".onomy_market_standalone",
                "v0.1.0",
                "market_standaloned",
            ),
        )])
        .await
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
        sh_cosmovisor("tx market create-pool", &[
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
