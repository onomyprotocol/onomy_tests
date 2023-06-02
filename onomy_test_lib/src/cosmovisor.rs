use std::time::Duration;

use log::info;
use serde_json::{json, Value};
use stacked_errors::{MapAddError, Result};
use super_orchestrator::{
    get_separated_val, sh, sh_no_dbg, wait_for_ok, Command, CommandRunner, FileOptions, STD_DELAY,
    STD_TRIES,
};

use crate::{anom_to_nom, json_inner, nom, token18, yaml_str_to_json_value};

/// A wrapper around `super_orchestrator::sh` that prefixes "cosmovisor run"
/// onto `cmd_with_args` and removes the first line of output (in order to
/// remove the INF line that always shows with cosmovisor runs)
pub async fn sh_cosmovisor(cmd_with_args: &str, args: &[&str]) -> Result<String> {
    let stdout = sh(&format!("cosmovisor run {cmd_with_args}"), args).await?;
    Ok(stdout
        .split_once('\n')
        .map_add_err(|| "cosmovisor run command did not have expected info line")?
        .1
        .to_owned())
}

pub async fn sh_cosmovisor_no_dbg(cmd_with_args: &str, args: &[&str]) -> Result<String> {
    let stdout = sh_no_dbg(&format!("cosmovisor run {cmd_with_args}"), args).await?;
    Ok(stdout
        .split_once('\n')
        .map_add_err(|| "cosmovisor run command did not have expected info line")?
        .1
        .to_owned())
}

/// NOTE: this is stuff you would not want to run in production.
/// NOTE: this is intended to be run inside containers only
///
/// This additionally returns the single validator mnemonic
pub async fn onomyd_setup(daemon_home: &str, arc_module: bool) -> Result<String> {
    let chain_id = "onomy";
    let global_min_self_delegation = &token18(225.0e3, "");
    sh_cosmovisor("config chain-id", &[chain_id]).await?;
    sh_cosmovisor("config keyring-backend test", &[]).await?;
    sh_cosmovisor("init --overwrite", &[chain_id]).await?;

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

    // init DAO balance
    let amount = token18(100.0e6, "");
    let treasury_balance = json!([{"denom": "anom", "amount": amount}]);
    genesis["app_state"]["dao"]["treasury_balance"] = treasury_balance;

    // disable community_tax
    genesis["app_state"]["distribution"]["params"]["community_tax"] = json!("0");

    // min_global_self_delegation
    genesis["app_state"]["staking"]["params"]["min_global_self_delegation"] =
        global_min_self_delegation.to_owned().into();

    // speed up block speed to be one second. NOTE: keep the inflation calculations
    // to expect 5s block times, and just assume 5 second block time because the
    // staking calculations will also assume `app_state.mint.params.blocks_per_year`
    // that we keep constant
    //
    //genesis["app_state"]["mint"]["params"]["blocks_per_year"] =
    // "31536000000".into();
    //
    //genesis["app_state"]["gravity"]["params"]["average_block_time"] =
    // "1000".into();
    let config_file_path = format!("{daemon_home}/config/config.toml");
    let config_s = FileOptions::read_to_string(&config_file_path).await?;
    let mut config: toml::Value = toml::from_str(&config_s).map_add_err(|| ())?;
    // reduce all of these by a factor of 5
    /*
    timeout_propose = "3s"
    timeout_propose_delta = "500ms"
    timeout_prevote = "1s"
    timeout_prevote_delta = "500ms"
    timeout_precommit = "1s"
    timeout_precommit_delta = "500ms"
    timeout_commit = "5s"
     */
    config["consensus"]["timeout_propose"] = "600ms".into();
    config["consensus"]["timeout_propose_delta"] = "100ms".into();
    config["consensus"]["timeout_prevote"] = "200ms".into();
    config["consensus"]["timeout_prevote_delta"] = "100ms".into();
    config["consensus"]["timeout_precommit"] = "200ms".into();
    config["consensus"]["timeout_precommit_delta"] = "100ms".into();
    config["consensus"]["timeout_commit"] = "1000ms".into();
    let config_s = toml::to_string_pretty(&config)?;
    FileOptions::write_str(&config_file_path, &config_s).await?;

    // decrease the governing period for fast tests
    let gov_period = "800ms";
    let gov_period: Value = gov_period.into();
    genesis["app_state"]["gov"]["voting_params"]["voting_period"] = gov_period.clone();
    genesis["app_state"]["gov"]["deposit_params"]["max_deposit_period"] = gov_period;

    // write back genesis
    let genesis_s = serde_json::to_string(&genesis)?;
    FileOptions::write_str(&genesis_file_path, &genesis_s).await?;
    FileOptions::write_str("/logs/genesis.json", &genesis_s).await?;

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
        .trim()
        .to_owned();
    sh_cosmovisor("add-genesis-account validator", &[&nom(2.0e6)]).await?;

    if arc_module {
        // Even if we don't test the bridge, we need this because SetValsetRequest is
        // called by the gravity module. There are parallel validators for the
        // gravity module, and they need all their own `gravity` variations of `gentx`
        // and `collect-gentxs`
        sh_cosmovisor("keys add orchestrator", &[]).await?;
        let eth_keys = sh_cosmovisor("eth_keys add", &[]).await?;
        let eth_addr = &get_separated_val(&eth_keys, "\n", "address", ":")?;
        let orch_addr = &sh_cosmovisor("keys show orchestrator -a", &[])
            .await?
            .trim()
            .to_owned();
        sh_cosmovisor("add-genesis-account orchestrator", &[&nom(1.0e6)]).await?;
        sh_cosmovisor("gravity gentx validator", &[
            &nom(1.0e6),
            eth_addr,
            orch_addr,
            "--chain-id",
            chain_id,
            "--min-self-delegation",
            global_min_self_delegation,
        ])
        .await?;
        sh_cosmovisor("gravity collect-gentxs", &[]).await?;
    } else {
        sh_cosmovisor("gentx validator", &[
            &nom(1.0e6),
            "--chain-id",
            chain_id,
            "--min-self-delegation",
            global_min_self_delegation,
        ])
        .await?;
    }

    sh_cosmovisor("collect-gentxs", &[]).await?;

    Ok(mnemonic)
}

pub async fn market_standaloned_setup(daemon_home: &str) -> Result<String> {
    let chain_id = "market_standalone";
    let global_min_self_delegation = "225000000000000000000000";
    sh_cosmovisor("config chain-id", &[chain_id]).await?;
    sh_cosmovisor("config keyring-backend test", &[]).await?;
    sh_cosmovisor("init --overwrite", &[chain_id]).await?;

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

    // min_global_self_delegation
    genesis["app_state"]["staking"]["params"]["min_global_self_delegation"] =
        global_min_self_delegation.into();

    // speed up block speed to be one second.
    let config_file_path = format!("{daemon_home}/config/config.toml");
    let config_s = FileOptions::read_to_string(&config_file_path).await?;
    let mut config: toml::Value = toml::from_str(&config_s).map_add_err(|| ())?;
    config["consensus"]["timeout_propose"] = "600ms".into();
    config["consensus"]["timeout_propose_delta"] = "100ms".into();
    config["consensus"]["timeout_prevote"] = "200ms".into();
    config["consensus"]["timeout_prevote_delta"] = "100ms".into();
    config["consensus"]["timeout_precommit"] = "200ms".into();
    config["consensus"]["timeout_precommit_delta"] = "100ms".into();
    config["consensus"]["timeout_commit"] = "1000ms".into();
    let config_s = toml::to_string_pretty(&config)?;
    FileOptions::write_str(&config_file_path, &config_s).await?;

    // decrease the governing period for fast tests
    let gov_period = "800ms";
    let gov_period: Value = gov_period.into();
    genesis["app_state"]["gov"]["voting_params"]["voting_period"] = gov_period.clone();
    genesis["app_state"]["gov"]["deposit_params"]["max_deposit_period"] = gov_period;

    // write back genesis
    let genesis_s = serde_json::to_string(&genesis)?;
    FileOptions::write_str(&genesis_file_path, &genesis_s).await?;
    FileOptions::write_str("/logs/market_genesis.json", &genesis_s).await?;

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
        .trim()
        .to_owned();

    sh_cosmovisor("add-genesis-account validator", &[&nom(2.0e6)]).await?;
    sh_cosmovisor("gentx validator", &[
        &nom(1.0e6),
        "--chain-id",
        chain_id,
        "--min-self-delegation",
        global_min_self_delegation,
    ])
    .await?;
    sh_cosmovisor("collect-gentxs", &[]).await?;

    Ok(mnemonic)
}

// TODO when reintroducing the bridge we need
/*
    sh_cosmovisor("keys add orchestrator", &[]).await?;
    let eth_keys = sh_cosmovisor("eth_keys add", &[]).await?;
    let eth_addr = &get_separated_val(&eth_keys, "\n", "address", ":")?;
    let orch_addr = &sh_cosmovisor("keys show orchestrator -a", &[])
        .await?
        .trim()
        .to_owned();
    sh_cosmovisor("add-genesis-account orchestrator", &[&nom(1.0e6)]).await?;

    // note the special gravity variation
    sh_cosmovisor("gravity gentx validator", &[
        &nom(95.0e6),
        eth_addr,
        orch_addr,
        "--chain-id",
        chain_id,
        "--min-self-delegation",
        global_min_self_delegation,
    ])
    .await?;
    sh_cosmovisor("gravity collect-gentxs", &[]).await?;
    sh_cosmovisor("collect-gentxs", &[]).await?;
*/

/// Note that this interprets "null" height as 0
pub async fn get_block_height() -> Result<u64> {
    let block_s = sh_cosmovisor_no_dbg("query block", &[]).await?;
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
        if get_block_height().await? >= height {
            Ok(())
        } else {
            ().map_add_err(|| ())
        }
    }
    info!("waiting for height {height}");
    wait_for_ok(num_tries, delay, || height_is_ge(height)).await
}

/// Waits for `num_blocks`. Note: if you are calling this in some timed sequence
/// that may be started at any time, you should call `wait_for_num_blocks(1)` in
/// the very beginning to make sure execution starts towards the beginning of a
/// new block.
pub async fn wait_for_num_blocks(num_blocks: u64) -> Result<()> {
    let height = get_block_height().await?;
    wait_for_height(STD_TRIES, STD_DELAY, height + num_blocks).await
}

pub async fn get_persistent_peer_info(hostname: &str) -> Result<String> {
    let s = sh_cosmovisor("tendermint show-node-id", &[]).await?;
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
    info!("waiting for daemon to run");
    wait_for_ok(STD_TRIES, STD_DELAY, || sh_cosmovisor("status", &[])).await?;
    wait_for_height(25, Duration::from_millis(300), 1)
        .await
        .map_add_err(|| {
            "daemon could not reach height 1, probably a genesis issue, check runner logs"
        })?;
    info!("daemon has reached height 1");
    // we also wait for height 2, because there are consensus failures and reward
    // propogations that only start on height 2
    wait_for_height(25, Duration::from_millis(300), 2)
        .await
        .map_add_err(|| {
            "daemon could not reach height 2, probably a consensus failure, check runner logs"
        })?;
    info!("daemon has reached height 2");
    Ok(cosmovisor_runner)
}

pub async fn get_valoper_addr() -> Result<String> {
    let validator_addr = get_separated_val(
        &sh_cosmovisor("keys show validator", &[]).await?,
        "\n",
        "address",
        ":",
    )?;
    let addr_bytes = get_separated_val(
        &sh_cosmovisor("keys parse", &[&validator_addr]).await?,
        "\n",
        "bytes",
        ":",
    )?;
    let valoper_addr = format!(
        "onomyvaloper1{}",
        get_separated_val(
            &sh_cosmovisor("keys parse", &[&addr_bytes]).await?,
            "\n",
            "- onomyvaloper",
            "1"
        )?
    );
    Ok(valoper_addr)
}

// TODO some of these become flaky if more than one addresse and delegator gets
// involved

pub async fn get_delegations_to_validator() -> Result<String> {
    let valoper_addr = get_valoper_addr().await?;
    sh_cosmovisor("query staking delegations-to", &[&valoper_addr]).await
}

pub async fn get_treasury() -> Result<f64> {
    let inner = json_inner(
        &yaml_str_to_json_value(&sh_cosmovisor("query dao show-treasury", &[]).await?)?
            ["treasury_balance"][0]["amount"],
    );
    anom_to_nom(&inner).map_add_err(|| format!("inner was: {inner}"))
}

pub async fn get_treasury_inflation_annual() -> Result<f64> {
    wait_for_num_blocks(1).await?;
    let start = get_treasury().await?;
    wait_for_num_blocks(1).await?;
    let end = get_treasury().await?;
    // we assume 5 second blocks
    Ok(((end - start) / (start * 5.0)) * (86400.0 * 365.0))
}

#[derive(Debug)]
pub struct DbgStakingPool {
    pub bonded_tokens: f64,
    pub unbonded_tokens: f64,
}

pub async fn get_staking_pool() -> Result<DbgStakingPool> {
    let pool = sh_cosmovisor("query staking pool", &[]).await?;
    let bonded_tokens = get_separated_val(&pool, "\n", "bonded_tokens", ":")?;
    let bonded_tokens = bonded_tokens.trim_matches('"');
    let bonded_tokens = anom_to_nom(bonded_tokens).map_add_err(|| ())?;
    let unbonded_tokens = get_separated_val(&pool, "\n", "not_bonded_tokens", ":")?;
    let unbonded_tokens = unbonded_tokens.trim_matches('"');
    let unbonded_tokens = anom_to_nom(unbonded_tokens).map_add_err(|| ())?;
    Ok(DbgStakingPool {
        bonded_tokens,
        unbonded_tokens,
    })
}

pub async fn get_validator_outstanding_rewards() -> Result<f64> {
    let valoper_addr = get_valoper_addr().await?;
    anom_to_nom(&json_inner(
        &yaml_str_to_json_value(
            &sh_cosmovisor("query distribution validator-outstanding-rewards", &[
                &valoper_addr,
            ])
            .await?,
        )?["rewards"][0]["amount"],
    ))
}

pub async fn get_validator_delegated() -> Result<f64> {
    let validator_addr = get_separated_val(
        &sh_cosmovisor("keys show validator", &[]).await?,
        "\n",
        "address",
        ":",
    )?;
    let s = sh_cosmovisor("query staking delegations", &[&validator_addr]).await?;
    anom_to_nom(&json_inner(
        &yaml_str_to_json_value(&s)?["delegation_responses"][0]["balance"]["amount"],
    ))
}

/// APR calculation is: [Amount(Rewards End) - Amount(Rewards
/// Beg)]/Amount(Delegated) * # of Blocks/Blocks_per_year
pub async fn get_apr_annual() -> Result<f64> {
    wait_for_num_blocks(1).await?;
    let delegated = get_validator_delegated().await?;
    let reward_start = get_validator_outstanding_rewards().await?;
    wait_for_num_blocks(1).await?;
    let reward_end = get_validator_outstanding_rewards().await?;
    dbg!(delegated, reward_start, reward_end);
    Ok(((reward_end - reward_start) * 365.0 * 86400.0) / (delegated * 5.0))
}
