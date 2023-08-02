use std::{collections::BTreeMap, time::Duration};

use log::info;
use serde_json::Value;
use super_orchestrator::{
    get_separated_val, sh, sh_no_dbg,
    stacked_errors::{Error, Result, StackableErr},
    wait_for_ok, Command, CommandRunner, FileOptions, STD_DELAY, STD_TRIES,
};
use tokio::time::sleep;
use u64_array_bigints::U256;

use crate::{anom_to_nom, json_inner, yaml_str_to_json_value};

/// A wrapper around `super_orchestrator::sh` that prefixes "cosmovisor run"
/// onto `cmd_with_args` and removes the first line of output (in order to
/// remove the INF line that always shows with cosmovisor runs)
pub async fn sh_cosmovisor(cmd_with_args: &str, args: &[&str]) -> Result<String> {
    let stdout = sh(&format!("cosmovisor run {cmd_with_args}"), args).await?;
    Ok(stdout
        .split_once('\n')
        .stack_err(|| "cosmovisor run command did not have expected info line")?
        .1
        .to_owned())
}

pub async fn sh_cosmovisor_no_dbg(cmd_with_args: &str, args: &[&str]) -> Result<String> {
    let stdout = sh_no_dbg(&format!("cosmovisor run {cmd_with_args}"), args).await?;
    Ok(stdout
        .split_once('\n')
        .stack_err(|| "cosmovisor run command did not have expected info line")?
        .1
        .to_owned())
}

/// This adds on a "tx" command arg and adds extra handling to propogate if the
/// chain level transaction failed (cosmovisor will not return a successful
/// status if the transaction was at least successfully transmitted, ignoring if
/// the transaction result was unsuccessful)
///
/// NOTE: You need to pass the argument `-y` to confirm without needing piped
/// input, and the arguments `-b block` for the error handling to work properly
pub async fn sh_cosmovisor_tx(cmd_with_args: &str, args: &[&str]) -> Result<serde_json::Value> {
    let res = sh_cosmovisor_no_dbg(&format!("tx {cmd_with_args}"), args)
        .await
        .stack_err(|| "sh_cosmovisor_tx() initial command failed")?;

    let res = yaml_str_to_json_value(&res).stack_err(|| ())?;
    if res["code"].as_u64().stack()? == 0 {
        Ok(res)
    } else {
        Err(Error::from(format!("raw_log: {}", res["raw_log"]))).stack_err(|| {
            format!("sh_cosmovisor_tx(cmd_with_args: {cmd_with_args}, args: {args:?})")
        })
    }
}

/// Cosmos-SDK configuration gets messed up by different Git commit and tag
/// states, this overwrites the in the given genesis and client.toml
pub async fn force_chain_id(daemon_home: &str, genesis: &mut Value, chain_id: &str) -> Result<()> {
    // genesis
    genesis["chain_id"] = chain_id.into();
    // client.toml
    let client_file_path = format!("{daemon_home}/config/client.toml");
    let client_s = FileOptions::read_to_string(&client_file_path)
        .await
        .stack()?;
    let mut client: toml::Value = toml::from_str(&client_s).stack()?;
    client["chain-id"] = chain_id.into();
    let client_s = toml::to_string_pretty(&client).stack()?;
    FileOptions::write_str(&client_file_path, &client_s)
        .await
        .stack()?;
    Ok(())
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
    let config_s = FileOptions::read_to_string(&config_file_path)
        .await
        .stack()?;
    let mut config: toml::Value = toml::from_str(&config_s).stack()?;
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
    let config_s = toml::to_string_pretty(&config).stack()?;
    FileOptions::write_str(&config_file_path, &config_s)
        .await
        .stack()?;
    Ok(())
}

pub async fn set_minimum_gas_price(daemon_home: &str, min_gas_price: &str) -> Result<()> {
    let app_toml_path = format!("{daemon_home}/config/app.toml");
    let app_toml_s = FileOptions::read_to_string(&app_toml_path).await.stack()?;
    let mut app_toml: toml::Value = toml::from_str(&app_toml_s).stack()?;
    app_toml["minimum-gas-prices"] = min_gas_price.into();
    let app_toml_s = toml::to_string_pretty(&app_toml).stack()?;
    FileOptions::write_str(&app_toml_path, &app_toml_s)
        .await
        .stack()?;
    Ok(())
}

/// Note that this interprets "null" height as 0
pub async fn get_block_height() -> Result<u64> {
    let block_s = sh_cosmovisor_no_dbg("query block", &[]).await.stack()?;
    let block: Value = serde_json::from_str(&block_s).stack()?;
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
            Err(Error::empty())
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
    let height = get_block_height().await.stack()?;
    wait_for_height(STD_TRIES, STD_DELAY, height + num_blocks)
        .await
        .stack()
}

/// Returns the number of proposals
pub async fn cosmovisor_get_num_proposals() -> Result<u64> {
    let comres = Command::new(
        "cosmovisor run query gov proposals --count-total --limit 1",
        &[],
    )
    .run_to_completion()
    .await
    .stack()?;
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
        .stack_err(|| "cosmovisor run command did not have expected info line")?
        .1;

    let v = yaml_str_to_json_value(stdout).stack()?;
    let total = v["pagination"]["total"].as_str().stack()?;
    total.parse::<u64>().stack()
}

/*
{
  "title": "Parameter Change",
  "description": "Making a parameter change",
  "changes": [
    {
      "subspace": "staking",
      "key": "BondDenom",
      "value": "ibc/hello"
    }
  ],
  "deposit": "2000000000000000000000anom"
}
 */

/// Writes the proposal at `{daemon_home}/config/proposal.json` and runs `tx gov
/// submit-proposal [proposal_type]`.
///
/// Gov proposals have the annoying property that error statuses (e.x. bad fees
/// will not result in an error at the `Command` level) are not propogated, this
/// will detect if an error happens.
pub async fn cosmovisor_submit_gov_file_proposal(
    daemon_home: &str,
    proposal_type: &str,
    proposal_s: &str,
    base_fee: &str,
) -> Result<()> {
    let proposal_file_path = format!("{daemon_home}/config/proposal.json");
    FileOptions::write_str(&proposal_file_path, proposal_s)
        .await
        .stack()?;
    sh_cosmovisor_tx("gov submit-proposal", &[
        proposal_type,
        &proposal_file_path,
        "--gas",
        "auto",
        "--gas-adjustment",
        "1.3",
        "--gas-prices",
        base_fee,
        "-y",
        "-b",
        "block",
        "--from",
        "validator",
    ])
    .await
    .stack_err(|| {
        format!(
            "cosmovisor_submit_gov_file_proposal(proposal_type: {proposal_type}, proposal_s: \
             {proposal_s})"
        )
    })?;
    Ok(())
}

pub async fn cosmovisor_gov_file_proposal(
    daemon_home: &str,
    proposal_type: &str,
    proposal_s: &str,
    base_fee: &str,
) -> Result<()> {
    cosmovisor_submit_gov_file_proposal(daemon_home, proposal_type, proposal_s, base_fee)
        .await
        .stack()?;
    let proposal_id = format!("{}", cosmovisor_get_num_proposals().await.stack()?);
    // the deposit is done as part of the chain addition proposal
    sh_cosmovisor_tx("gov vote", &[
        &proposal_id,
        "yes",
        "--gas",
        "auto",
        "--gas-adjustment",
        // market ICS needs this for some reason
        "2.3",
        "--gas-prices",
        base_fee,
        "-y",
        "-b",
        "block",
        "--from",
        "validator",
    ])
    .await
    .stack()?;
    Ok(())
}

pub async fn cosmovisor_submit_gov_proposal(
    proposal_type: &str,
    proposal_args: &[&str],
    base_fee: &str,
) -> Result<()> {
    let mut args = vec![];
    args.push(proposal_type);
    args.extend(proposal_args);
    args.extend([
        "--gas",
        "auto",
        "--gas-adjustment",
        "1.3",
        "--gas-prices",
        base_fee,
        "-y",
        "-b",
        "block",
        "--from",
        "validator",
    ]);
    sh_cosmovisor_tx("gov submit-proposal", &args)
        .await
        .stack()?;
    Ok(())
}

pub async fn cosmovisor_gov_proposal(
    proposal_type: &str,
    proposal_args: &[&str],
    deposit: &str,
    base_fee: &str,
) -> Result<()> {
    cosmovisor_submit_gov_proposal(proposal_type, proposal_args, base_fee)
        .await
        .stack()?;
    let proposal_id = format!("{}", cosmovisor_get_num_proposals().await.stack()?);
    sh_cosmovisor_tx("gov deposit", &[
        &proposal_id,
        deposit,
        "--gas",
        "auto",
        "--gas-adjustment",
        "1.3",
        "--gas-prices",
        base_fee,
        "-y",
        "-b",
        "block",
        "--from",
        "validator",
    ])
    .await
    .stack()?;
    // the deposit is done as part of the chain addition proposal
    sh_cosmovisor_tx("gov vote", &[
        &proposal_id,
        "yes",
        "--gas",
        "auto",
        "--gas-adjustment",
        "1.3",
        "--gas-prices",
        base_fee,
        "-y",
        "-b",
        "block",
        "--from",
        "validator",
    ])
    .await
    .stack()?;
    Ok(())
}

pub async fn get_persistent_peer_info(hostname: &str) -> Result<String> {
    let s = sh_cosmovisor("tendermint show-node-id", &[])
        .await
        .stack()?;
    let tendermint_id = s.trim();
    Ok(format!("{tendermint_id}@{hostname}:26656"))
}

pub async fn get_cosmovisor_subprocess_path() -> Result<String> {
    let comres = sh_no_dbg("cosmovisor run version", &[]).await.stack()?;
    let val = get_separated_val(
        comres.lines().next().stack()?,
        " ",
        "\u{1b}[36mpath=\u{1b}[0m",
        "",
    )
    .stack()?;
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
        self.runner.send_unix_sigterm().stack()?;
        self.runner.wait_with_timeout(timeout).await.stack()
    }
}

/// This starts cosmovisor and waits for height 1
///
/// `--rpc.laddr` with 0.0.0.0:26657 instead of 127.0.0.1 is used
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

    // this is required for our Hermes setups
    args.push("--rpc.laddr");
    args.push("tcp://0.0.0.0:26657");

    //args.push("--p2p.laddr");
    //args.push("tcp://0.0.0.0:26656");
    //args.push("--grpc.address");
    //args.push("0.0.0.0:9090");
    //args.push("--grpc.enable");
    //args.push("true");
    /*if let Some(ref peer) = peer {
        args.push("--p2p.persistent_peers");
        args.push(peer);
    }*/
    let halt_height_s;
    let mut quick_halt = false;
    if let Some(options) = options {
        if let Some(halt_height) = options.halt_height {
            if halt_height <= 2 {
                quick_halt = true;
            }
            args.push("--halt-height");
            halt_height_s = format!("{}", halt_height);
            args.push(&halt_height_s);
        }
    }

    let cosmovisor_runner = Command::new("cosmovisor run start --inv-check-period  1", &args)
        .stderr_log(&cosmovisor_log)
        .stdout_log(&cosmovisor_log)
        .run()
        .await
        .stack()?;

    if quick_halt {
        info!("skipping waiting because halt_height <= 2");
    } else {
        // wait for status to be ok and daemon to be running
        info!("waiting for daemon to run");
        // avoid the initial debug failure
        sleep(Duration::from_millis(300)).await;
        wait_for_ok(STD_TRIES, STD_DELAY, || sh_cosmovisor("status", &[]))
            .await
            .stack()?;
        // account for if we are not starting at height 0
        let current_height = get_block_height().await?;
        wait_for_height(25, Duration::from_millis(300), current_height + 1)
            .await
            .stack_err(|| {
                format!(
                    "daemon {} could not reach height {}, probably a genesis issue, check runner \
                     logs",
                    log_file_name,
                    current_height + 1
                )
            })?;
        info!(
            "daemon {} has reached height {}",
            log_file_name,
            current_height + 1
        );
        // we also wait for height 2, because there are consensus failures and reward
        // propogations that only start on height 2
        wait_for_height(25, Duration::from_millis(300), current_height + 2)
            .await
            .stack_err(|| {
                format!(
                    "daemon could not reach height {}, probably a consensus failure, check runner \
                     logs",
                    current_height + 2
                )
            })?;
        info!(
            "daemon {} has reached height {}",
            log_file_name,
            current_height + 2
        );
    }
    Ok(CosmovisorRunner {
        runner: cosmovisor_runner,
    })
}

pub async fn cosmovisor_get_addr(key_name: &str) -> Result<String> {
    let validator =
        yaml_str_to_json_value(&sh_cosmovisor("keys show", &[key_name]).await.stack()?).stack()?;
    Ok(json_inner(&validator[0]["address"]))
}

/// Returns a mapping of denoms to amounts
pub async fn cosmovisor_get_balances(addr: &str) -> Result<BTreeMap<String, U256>> {
    let balances = sh_cosmovisor_no_dbg("query bank balances", &[addr])
        .await
        .stack()?;
    let balances = yaml_str_to_json_value(&balances).stack()?;
    let mut res = BTreeMap::new();
    for balance in balances["balances"].as_array().stack()? {
        res.insert(
            json_inner(&balance["denom"]),
            U256::from_dec_or_hex_str(&json_inner(&balance["amount"])).stack()?,
        );
    }
    Ok(res)
}

/// This uses flags "-b block --gas auto --gas-adjustment 1.3 --gas-prices
/// 1{denom}"
pub async fn cosmovisor_bank_send(
    src_addr: &str,
    dst_addr: &str,
    amount: &str,
    denom: &str,
) -> Result<()> {
    sh_cosmovisor_tx(
        &format!(
            "bank send {src_addr} {dst_addr} {amount}{denom} -y -b block --gas auto \
             --gas-adjustment 1.3 --gas-prices 1{denom}"
        ),
        &[],
    )
    .await
    .stack_err(|| "cosmovisor_bank_send")?;
    Ok(())
}

pub async fn get_delegations_to(valoper_addr: &str) -> Result<String> {
    sh_cosmovisor("query staking delegations-to", &[valoper_addr]).await
}

pub async fn get_treasury() -> Result<f64> {
    let inner = json_inner(
        &yaml_str_to_json_value(
            &sh_cosmovisor("query dao show-treasury", &[])
                .await
                .stack()?,
        )
        .stack()?["treasury_balance"][0]["amount"],
    );
    anom_to_nom(&inner).stack_err(|| format!("inner was: {inner}"))
}

pub async fn get_treasury_inflation_annual() -> Result<f64> {
    wait_for_num_blocks(1).await.stack()?;
    let start = get_treasury().await.stack()?;
    wait_for_num_blocks(1).await.stack()?;
    let end = get_treasury().await.stack()?;
    // we assume 5 second blocks
    Ok(((end - start) / (start * 5.0)) * (86400.0 * 365.0))
}

#[derive(Debug)]
pub struct DbgStakingPool {
    pub bonded_tokens: f64,
    pub unbonded_tokens: f64,
}

pub async fn get_staking_pool() -> Result<DbgStakingPool> {
    let pool = sh_cosmovisor("query staking pool", &[]).await.stack()?;
    let bonded_tokens = get_separated_val(&pool, "\n", "bonded_tokens", ":").stack()?;
    let bonded_tokens = bonded_tokens.trim_matches('"');
    let bonded_tokens = anom_to_nom(bonded_tokens).stack()?;
    let unbonded_tokens = get_separated_val(&pool, "\n", "not_bonded_tokens", ":").stack()?;
    let unbonded_tokens = unbonded_tokens.trim_matches('"');
    let unbonded_tokens = anom_to_nom(unbonded_tokens).stack()?;
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
            .await
            .stack()?,
        )
        .stack()?["rewards"][0]["amount"],
    ))
    .stack()
}

pub async fn get_validator_delegated() -> Result<f64> {
    let validator_addr = get_separated_val(
        &sh_cosmovisor("keys show validator", &[]).await.stack()?,
        "\n",
        "address",
        ":",
    )?;
    let s = sh_cosmovisor("query staking delegations", &[&validator_addr])
        .await
        .stack()?;
    anom_to_nom(&json_inner(
        &yaml_str_to_json_value(&s).stack()?["delegation_responses"][0]["balance"]["amount"],
    ))
    .stack()
}

/// APR calculation is: [Amount(Rewards End) - Amount(Rewards
/// Beg)]/Amount(Delegated) * # of Blocks/Blocks_per_year
pub async fn get_apr_annual(valoper_addr: &str) -> Result<f64> {
    wait_for_num_blocks(1).await.stack()?;
    let delegated = get_validator_delegated().await.stack()?;
    let reward_start = get_outstanding_rewards(valoper_addr).await.stack()?;
    wait_for_num_blocks(1).await.stack()?;
    let reward_end = get_outstanding_rewards(valoper_addr).await.stack()?;
    Ok(((reward_end - reward_start) * 365.0 * 86400.0) / (delegated * 5.0))
}
