use serde_json::{json, Value};
use super_orchestrator::{
    acquire_file_path, close_file, get_separated_val, sh, wait_for_ok, Command, CommandRunner,
    LogFileOptions, MapAddError, Result, STD_DELAY, STD_TRIES,
};
use tokio::{
    fs::OpenOptions,
    io::{AsyncReadExt, AsyncWriteExt},
};

use crate::nom;

/// NOTE: this is stuff you would not want to run in production.
/// NOTE: this is intended to be run inside containers only
pub async fn cosmovisor_setup(daemon_home: &str, gov_period: &str) -> Result<()> {
    let chain_id = "onomy";
    let global_min_self_delegation = "225000000000000000000000";
    sh("cosmovisor run config chain-id", &[chain_id]).await?;
    sh("cosmovisor run config keyring-backend test", &[]).await?;
    sh("cosmovisor run init --overwrite", &[chain_id]).await?;

    let genesis_file_path =
        acquire_file_path(&format!("{}/config/genesis.json", daemon_home)).await?;
    let mut genesis_file = OpenOptions::new()
        .read(true)
        .open(&genesis_file_path)
        .await?;
    let mut genesis_s = String::new();
    genesis_file.read_to_string(&mut genesis_s).await?;
    // when we write back, we ill just reopen as truncated, `set_len` has too many
    // problems
    close_file(genesis_file).await?;

    // rename all "stake" to "anom"
    let genesis_s = genesis_s.replace("\"stake\"", "\"anom\"");
    println!("\n\n{genesis_s}\n\n");
    let mut genesis: Value = serde_json::from_str(&genesis_s)?;

    // put in the test `footoken` and the staking `anom`
    let denom_metadata = json!(
        [{"name": "Foo Token", "symbol": "FOO", "base": "footoken", "display": "mfootoken",
        "description": "A non-staking test token", "denom_units": [{"denom": "footoken",
        "exponent": 0}, {"denom": "mfootoken", "exponent": 6}]},
        {"name": "NOM", "symbol": "NOM", "base": "anom", "display": "nom","description":
        "Nom token", "denom_units": [{"denom": "anom", "exponent": 0}, {"denom": "nom",
        "exponent": 18}]}]
    );
    genesis["app_state"]["bank"]["denom_metadata"] = denom_metadata;

    // decrease the governing period for fast tests
    let gov_period: Value = gov_period.into();
    genesis["app_state"]["gov"]["voting_params"]["voting_period"] = gov_period.clone();
    genesis["app_state"]["gov"]["deposit_params"]["max_deposit_period"] = gov_period;

    // init DAO balance
    let treasury_balance = json!([{"denom": "anom", "amount": "100000000000000000000000000"}]);
    genesis["app_state"]["dao"]["treasury_balance"] = treasury_balance;

    // disable community_tax
    genesis["app_state"]["distribution"]["params"]["community_tax"] = json!("0");

    // min_global_self_delegation
    genesis["app_state"]["staking"]["params"]["min_global_self_delegation"] =
        global_min_self_delegation.into();

    // write back genesis, just reopen
    let genesis_s = serde_json::to_string(&genesis)?;
    let mut genesis_file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&genesis_file_path)
        .await?;
    genesis_file.write_all(genesis_s.as_bytes()).await?;
    close_file(genesis_file).await?;

    sh("cosmovisor run keys add validator", &[]).await?;
    sh("cosmovisor run add-genesis-account validator", &[&nom(
        2.0e6,
    )])
    .await?;
    // Even if we don't test the bridge, we need this because SetValsetRequest is
    // called by the gravity module. There are parallel validators for the
    // gravity module, and they need all their own `gravity` variations of `gentx`
    // and `collect-gentxs`
    sh("cosmovisor run keys add orchestrator", &[]).await?;
    let eth_keys = Command::new("cosmovisor run eth_keys add", &[])
        .run_to_completion()
        .await?
        .stdout;
    let eth_addr = &get_separated_val(&eth_keys, "\n", "address", ":")?;
    // skip the first "INF" line
    let orch_addr = &Command::new("cosmovisor run keys show orchestrator -a", &[])
        .run_to_completion()
        .await?
        .stdout
        .lines()
        .nth(1)
        .map_add_err(|| ())?
        .to_string();
    sh("cosmovisor run add-genesis-account orchestrator", &[&nom(
        2.0e6,
    )])
    .await?;

    sh("cosmovisor run gravity gentx validator", &[
        &nom(1.0e6),
        eth_addr,
        orch_addr,
        "--chain-id",
        chain_id,
        "--min-self-delegation",
        global_min_self_delegation,
    ])
    .await?;
    sh("cosmovisor run gravity collect-gentxs", &[]).await?;
    sh("cosmovisor run collect-gentxs", &[]).await?;

    Ok(())
}

/// This starts cosmovisor (with the logs going to
/// "/logs/entrypoint_cosmovisor.log") and waits for height 1
pub async fn cosmovisor_start() -> Result<CommandRunner> {
    let cosmovisor_log = Some(LogFileOptions::new(
        "/logs",
        "entrypoint_cosmovisor.log",
        true,
        true,
    ));

    let mut cosmovisor = Command::new("cosmovisor run start --inv-check-period  1", &[])
        .stderr_log(&cosmovisor_log)
        .stdout_log(&cosmovisor_log)
        .run()
        .await?;
    // wait for status to be ok and daemon to be running
    println!("waiting for daemon to run");
    wait_for_ok(STD_TRIES, STD_DELAY, || sh("cosmovisor run status", &[])).await?;
    println!("waiting for block height to increase");
    /*wait_for_ok(STD_TRIES, STD_DELAY, || {
        //sh("cosmovisor ")
        Ok(())
    }).await?;*/

    Ok(cosmovisor)
}
