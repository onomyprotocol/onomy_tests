use std::time::Duration;

use serde_json::{json, Value};
use super_orchestrator::{
    get_separated_val, sh, wait_for_ok, Command, CommandRunner, FileOptions, MapAddError, Result,
    STD_TRIES,
};

use crate::{nom, ONE_SEC};

/// A wrapper around `super_orchestrator::sh` that prefixes "cosmovisor run"
/// onto `cmd_with_args` and removes the first line of output (in order to
/// remove the INF line that always shows with cosmovisor runs)
pub async fn cosmovisor(cmd_with_args: &str, args: &[&str]) -> Result<String> {
    let stdout = sh(&format!("cosmovisor run {cmd_with_args}"), args).await?;
    Ok(stdout
        .split_once('\n')
        .map_add_err(|| "cosmovisor run command did not have expected info line")?
        .1
        .to_owned())
}

/// NOTE: this is stuff you would not want to run in production.
/// NOTE: this is intended to be run inside containers only
pub async fn cosmovisor_setup(daemon_home: &str, gov_period: &str) -> Result<()> {
    let chain_id = "onomy";
    let global_min_self_delegation = "225000000000000000000000";
    cosmovisor("config chain-id", &[chain_id]).await?;
    cosmovisor("config keyring-backend test", &[]).await?;
    cosmovisor("init --overwrite", &[chain_id]).await?;
    let genesis_file_path = format!("{daemon_home}/config/genesis.json");

    let genesis_s = FileOptions::read_to_string(&genesis_file_path).await?;

    // rename all "stake" to "anom"
    let genesis_s = genesis_s.replace("\"stake\"", "\"anom\"");
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
    FileOptions::write_str(&genesis_file_path, &genesis_s).await?;

    cosmovisor("keys add validator", &[]).await?;
    cosmovisor("add-genesis-account validator", &[&nom(2.0e6)]).await?;
    // Even if we don't test the bridge, we need this because SetValsetRequest is
    // called by the gravity module. There are parallel validators for the
    // gravity module, and they need all their own `gravity` variations of `gentx`
    // and `collect-gentxs`
    cosmovisor("keys add orchestrator", &[]).await?;
    let eth_keys = cosmovisor("eth_keys add", &[]).await?;
    let eth_addr = &get_separated_val(&eth_keys, "\n", "address", ":")?;
    let orch_addr = &cosmovisor("keys show orchestrator -a", &[])
        .await?
        .trim()
        .to_owned();
    cosmovisor("add-genesis-account orchestrator", &[&nom(2.0e6)]).await?;

    cosmovisor("gravity gentx validator", &[
        &nom(1.0e6),
        eth_addr,
        orch_addr,
        "--chain-id",
        chain_id,
        "--min-self-delegation",
        global_min_self_delegation,
    ])
    .await?;
    cosmovisor("gravity collect-gentxs", &[]).await?;
    cosmovisor("collect-gentxs", &[]).await?;

    Ok(())
}

/// NOTE: this is stuff you would not want to run in production.
/// NOTE: this is intended to be run inside containers only
///
/// This additionally puts the single validator mnemonic in
/// `/root/.hermes/mnemonic.txt`
pub async fn onomyd_setup(daemon_home: &str, gov_period: &str) -> Result<()> {
    let chain_id = "onomy";
    let global_min_self_delegation = "225000000000000000000000";
    cosmovisor("config chain-id", &[chain_id]).await?;
    cosmovisor("config keyring-backend test", &[]).await?;
    cosmovisor("init --overwrite", &[chain_id]).await?;

    let genesis_file_path = format!("{daemon_home}/config/genesis.json");
    let genesis_s = FileOptions::read_to_string(&genesis_file_path).await?;

    // rename all "stake" to "anom"
    let genesis_s = genesis_s.replace("\"stake\"", "\"anom\"");
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
    FileOptions::write_str(&genesis_file_path, &genesis_s).await?;

    // we need the stderr to get the mnemonic
    let comres = Command::new("cosmovisor run keys add validator", &[])
        .run_to_completion()
        .await?;
    comres.assert_success()?;
    let mnemonic = comres
        .stderr
        .trim()
        .lines()
        .last()
        .map_add_err(|| "no last line")?
        .trim();
    FileOptions::write_str("/root/.hermes/mnemonic.txt", &mnemonic).await?;

    cosmovisor("add-genesis-account validator", &[&nom(2.0e6)]).await?;
    cosmovisor("gentx validator", &[
        &nom(1.0e6),
        "--chain-id",
        chain_id,
        "--min-self-delegation",
        global_min_self_delegation,
    ])
    .await?;
    cosmovisor("collect-gentxs", &[]).await?;

    Ok(())
}

/// Note that this interprets "null" height as 0
pub async fn get_height() -> Result<u64> {
    let block_s = cosmovisor("query block", &[]).await?;
    let block: Value = serde_json::from_str(&block_s)?;
    let height = &block["block"]["header"]["height"].to_string();
    Ok(height
        .to_string()
        .trim_matches('"')
        .parse::<u64>()
        .unwrap_or(0))
}

pub async fn wait_for_height(num_tries: u64, delay: Duration, height: u64) -> Result<()> {
    async fn height_is_ge(height: u64) -> Result<()> {
        if get_height().await? >= height {
            Ok(())
        } else {
            ().map_add_err(|| ())
        }
    }
    wait_for_ok(num_tries, delay, || height_is_ge(height)).await
}

// TODO
// This is just a simple wrapper struct to facilitate checked message passing
//#[derive(Debug, Clone, Decode, Encode)]
//pub struct PersistentPeer {
//    pub lkj: u32
//}

pub async fn get_persistent_peer_info(hostname: &str) -> Result<String> {
    let s = cosmovisor("tendermint show-node-id", &[]).await?;
    let tendermint_id = s.trim();
    Ok(format!("{tendermint_id}@{hostname}:26656"))
}

/// This starts cosmovisor and waits for height 1
///
/// If `listen`, then `--p2p.laddr` is used on the standard"tcp://0.0.0.0:26656"
///
/// `peer` should be the `tendermint_id@host_ip:port` of the peer
pub async fn cosmovisor_start(
    log_file_name: &str,
    listen: bool,
    peer: Option<String>,
) -> Result<CommandRunner> {
    let cosmovisor_log = FileOptions::write2("/logs", log_file_name);

    let mut args = vec![];
    if listen {
        // TODO this is actually the default?
        //args.push("--p2p.laddr");
        //args.push("tcp://0.0.0.0:26656");
        args.push("--rpc.laddr");
        args.push("tcp://0.0.0.0:26657");
    }
    if let Some(ref peer) = peer {
        args.push("--p2p.persistent_peers");
        args.push(peer);
    }

    let cosmovisor_runner = Command::new("cosmovisor run start --inv-check-period  1", &args)
        .stderr_log(&cosmovisor_log)
        .stdout_log(&cosmovisor_log)
        .run()
        .await?;
    // wait for status to be ok and daemon to be running
    println!("waiting for daemon to run");
    wait_for_ok(STD_TRIES, ONE_SEC, || cosmovisor("status", &[])).await?;
    println!("waiting for block height to increase");
    wait_for_height(STD_TRIES, ONE_SEC, 1).await?;

    Ok(cosmovisor_runner)
}

pub async fn get_delegations_to_validator() -> Result<String> {
    let validator_addr = get_separated_val(
        &cosmovisor("keys show validator", &[]).await?,
        "\n",
        "address",
        ":",
    )?;
    let addr_bytes = get_separated_val(
        &cosmovisor("keys parse", &[&validator_addr]).await?,
        "\n",
        "bytes",
        ":",
    )?;
    let valoper_addr = format!(
        "onomyvaloper1{}",
        get_separated_val(
            &cosmovisor("keys parse", &[&addr_bytes]).await?,
            "\n",
            "- onomyvaloper",
            "1"
        )?
    );
    cosmovisor("query staking delegations-to", &[&valoper_addr]).await
}
