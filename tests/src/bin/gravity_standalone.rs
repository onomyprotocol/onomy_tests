use std::time::Duration;

use common::container_runner;
use log::info;
use onomy_test_lib::{
    cosmovisor::{
        cosmovisor_get_addr, cosmovisor_gov_file_proposal, cosmovisor_start, sh_cosmovisor,
        wait_for_num_blocks,
    },
    dockerfiles::onomy_std_cosmos_daemon,
    onomy_std_init,
    setups::gravity_standalone_setup,
    super_orchestrator::{
        sh,
        stacked_errors::{ensure_eq, Error, Result, StackableErr},
        stacked_get, FileOptions,
    },
    token18, yaml_str_to_json_value, Args, TIMEOUT,
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
        sh(["make --directory ./../arc/module clean"])
            .await
            .stack()?;
        sh(["make --directory ./../arc/module build"])
            .await
            .stack()?;
        sh(["cp ./../arc/module/build/gravity ./tests/dockerfiles/dockerfile_resources/gravity"])
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
    gravity_standalone_setup(daemon_home, false, "onomy")
        .await
        .stack()?;
    let mut cosmovisor_runner = cosmovisor_start("gravity_runner.log", None).await.stack()?;

    let addr: &String = &cosmovisor_get_addr("validator").await.stack()?;
    info!("{addr}");
    //let valoper_addr = &reprefix_bech32(addr, "onomyvaloper").stack()?;
    //info!("{}", get_apr_annual(valoper_addr).await.stack()?);
    //info!("{}", get_delegations_to(valoper_addr).await.stack()?);
    //info!("{:?}", get_staking_pool().await.stack()?);

    let test_crisis_denom = "anom";
    let test_deposit = token18(2000.0, "anom");
    cosmovisor_gov_file_proposal(
        daemon_home,
        Some("param-change"),
        &format!(
            r#"
    {{
        "title": "Parameter Change",
        "description": "Making a parameter change",
        "changes": [
          {{
            "subspace": "crisis",
            "key": "ConstantFee",
            "value": {{"denom":"{test_crisis_denom}","amount":"1337"}}
          }}
        ],
        "deposit": "{test_deposit}"
    }}
    "#
        ),
        "1anom",
    )
    .await
    .stack()?;
    wait_for_num_blocks(1).await.stack()?;
    // just running this for debug, param querying is weird because it is json
    // inside of yaml, so we will instead test the exported genesis
    sh_cosmovisor("query params subspace crisis ConstantFee", &[])
        .await
        .stack()?;

    //sleep(TIMEOUT).await;
    sleep(Duration::ZERO).await;
    cosmovisor_runner.terminate(TIMEOUT).await.stack()?;
    // test that exporting works
    let exported = sh_cosmovisor("export", &[]).await.stack()?;
    FileOptions::write_str("/logs/gravity_export.json", &exported)
        .await
        .stack()?;
    let exported = yaml_str_to_json_value(&exported)?;
    ensure_eq!(
        stacked_get!(exported["app_state"]["crisis"]["constant_fee"]["denom"]),
        test_crisis_denom
    );
    ensure_eq!(
        stacked_get!(exported["app_state"]["crisis"]["constant_fee"]["amount"]),
        "1337"
    );

    Ok(())
}
