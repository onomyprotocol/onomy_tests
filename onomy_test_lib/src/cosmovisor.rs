use std::time::Duration;

use log::info;
use serde_json::{json, Value};
use super_orchestrator::{
    get_separated_val, sh, sh_no_dbg,
    stacked_errors::{MapAddError, Result},
    wait_for_ok, Command, CommandRunner, FileOptions, STD_DELAY, STD_TRIES,
};
use tokio::time::sleep;

use crate::{
    anom_to_nom, json_inner, native_denom, nom, nom_denom, token18, yaml_str_to_json_value,
};

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

pub async fn fast_block_times(daemon_home: &str) -> Result<()> {
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
    Ok(())
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
    let denom_metadata = nom_denom();
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

    // decrease the governing period for fast tests
    let gov_period = "800ms";
    let gov_period: Value = gov_period.into();
    genesis["app_state"]["gov"]["voting_params"]["voting_period"] = gov_period.clone();
    genesis["app_state"]["gov"]["deposit_params"]["max_deposit_period"] = gov_period;

    // write back genesis
    let genesis_s = serde_json::to_string(&genesis)?;
    FileOptions::write_str(&genesis_file_path, &genesis_s).await?;
    FileOptions::write_str("/logs/genesis.json", &genesis_s).await?;

    fast_block_times(daemon_home).await?;

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

    genesis["app_state"]["bank"]["denom_metadata"] = native_denom();

    // min_global_self_delegation
    genesis["app_state"]["staking"]["params"]["min_global_self_delegation"] =
        global_min_self_delegation.into();

    // decrease the governing period for fast tests
    let gov_period = "800ms";
    let gov_period: Value = gov_period.into();
    genesis["app_state"]["gov"]["voting_params"]["voting_period"] = gov_period.clone();
    genesis["app_state"]["gov"]["deposit_params"]["max_deposit_period"] = gov_period;

    // write back genesis
    let genesis_s = serde_json::to_string(&genesis)?;
    FileOptions::write_str(&genesis_file_path, &genesis_s).await?;
    FileOptions::write_str("/logs/market_genesis.json", &genesis_s).await?;

    fast_block_times(daemon_home).await?;

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

/// Returns the number of proposals
pub async fn cosmovisor_get_num_proposals() -> Result<u64> {
    let comres = Command::new(
        "cosmovisor run query gov proposals --count-total --limit 1",
        &[],
    )
    .run_to_completion()
    .await?;
    if let Err(e) = comres.assert_success() {
        // work around bad zero casing design
        if comres
            .stderr
            .trim()
            .starts_with("Error: no proposals found")
        {
            return Ok(0)
        } else {
            return Err(e)
        }
    }
    let stdout = comres
        .stdout
        .split_once('\n')
        .map_add_err(|| "cosmovisor run command did not have expected info line")?
        .1;

    let v = yaml_str_to_json_value(stdout)?;
    let total = v["pagination"]["total"].as_str().map_add_err(|| ())?;
    total.parse::<u64>().map_add_err(|| ())
}

pub async fn get_persistent_peer_info(hostname: &str) -> Result<String> {
    let s = sh_cosmovisor("tendermint show-node-id", &[]).await?;
    let tendermint_id = s.trim();
    Ok(format!("{tendermint_id}@{hostname}:26656"))
}

pub async fn get_cosmovisor_subprocess_path() -> Result<String> {
    let comres = sh_no_dbg("cosmovisor run version", &[]).await?;
    let val = get_separated_val(
        comres.lines().next().map_add_err(|| ())?,
        " ",
        "\u{1b}[36mpath=\u{1b}[0m",
        "",
    )?;
    Ok(val)
}

pub struct CosmovisorOptions {
    pub halt_height: Option<u64>,
}

/// `cosmovisor run start` spawns the cosmos binary as a completely separate
/// child process, meaning that terminating the parent `Command` does not
/// actually terminate the running binary. This sends a `SIGTERM` signal to
/// properly terminate cosmovisor.
///
/// Note however that other commands like `wait_with_timeout` work as expected
/// on the internal runner
pub struct CosmovisorRunner {
    pub runner: CommandRunner,
}

impl CosmovisorRunner {
    pub async fn terminate(&mut self, timeout: Duration) -> Result<()> {
        self.runner.send_unix_sigterm()?;
        self.runner.wait_with_timeout(timeout).await
    }
}

/// This starts cosmovisor and waits for height 1
///
/// If `listen`, then `--p2p.laddr` is used on the standard"tcp://0.0.0.0:26656"
///
/// `peer` should be the `tendermint_id@host_ip:port` of the peer
pub async fn cosmovisor_start(
    log_file_name: &str,
    options: Option<CosmovisorOptions>,
) -> Result<CosmovisorRunner> {
    let cosmovisor_log = FileOptions::write2("/logs", log_file_name);

    let mut args = vec![];
    // TODO this is actually the default?
    //args.push("--p2p.laddr");
    //args.push("tcp://0.0.0.0:26656");
    args.push("--rpc.laddr");
    args.push("tcp://0.0.0.0:26657");
    /*if let Some(ref peer) = peer {
        args.push("--p2p.persistent_peers");
        args.push(peer);
    }*/
    let halt_height_s;
    if let Some(options) = options {
        if let Some(halt_height) = options.halt_height {
            args.push("--halt-height");
            halt_height_s = format!("{}", halt_height);
            args.push(&halt_height_s);
        }
    }

    let cosmovisor_runner = Command::new("cosmovisor run start --inv-check-period  1", &args)
        .stderr_log(&cosmovisor_log)
        .stdout_log(&cosmovisor_log)
        .run()
        .await?;
    // wait for status to be ok and daemon to be running
    info!("waiting for daemon to run");
    // avoid the initial debug failure
    sleep(Duration::from_millis(300)).await;
    wait_for_ok(STD_TRIES, STD_DELAY, || sh_cosmovisor("status", &[])).await?;
    // account for if we are not starting at height 0
    let current_height = get_block_height().await?;
    wait_for_height(25, Duration::from_millis(300), current_height + 1)
        .await
        .map_add_err(|| {
            format!(
                "daemon could not reach height {}, probably a genesis issue, check runner logs",
                current_height + 1
            )
        })?;
    info!("daemon has reached height {}", current_height + 1);
    // we also wait for height 2, because there are consensus failures and reward
    // propogations that only start on height 2
    wait_for_height(25, Duration::from_millis(300), current_height + 2)
        .await
        .map_add_err(|| {
            format!(
                "daemon could not reach height {}, probably a consensus failure, check runner logs",
                current_height + 2
            )
        })?;
    info!("daemon has reached height {}", current_height + 2);
    Ok(CosmovisorRunner {
        runner: cosmovisor_runner,
    })
}

pub async fn cosmovisor_get_addr(key_name: &str) -> Result<String> {
    let validator = yaml_str_to_json_value(
        &sh_cosmovisor("keys show", &[key_name])
            .await
            .map_add_err(|| ())?,
    )
    .map_add_err(|| ())?;
    Ok(json_inner(&validator[0]["address"]))
}

pub async fn get_delegations_to(valoper_addr: &str) -> Result<String> {
    sh_cosmovisor("query staking delegations-to", &[valoper_addr]).await
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

pub async fn get_outstanding_rewards(valoper_addr: &str) -> Result<f64> {
    anom_to_nom(&json_inner(
        &yaml_str_to_json_value(
            &sh_cosmovisor("query distribution validator-outstanding-rewards", &[
                valoper_addr,
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
pub async fn get_apr_annual(valoper_addr: &str) -> Result<f64> {
    wait_for_num_blocks(1).await?;
    let delegated = get_validator_delegated().await?;
    let reward_start = get_outstanding_rewards(valoper_addr).await?;
    wait_for_num_blocks(1).await?;
    let reward_end = get_outstanding_rewards(valoper_addr).await?;
    Ok(((reward_end - reward_start) * 365.0 * 86400.0) / (delegated * 5.0))
}
