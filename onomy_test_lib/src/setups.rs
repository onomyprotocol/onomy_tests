use serde_json::{json, Value};
use super_orchestrator::{
    get_separated_val,
    stacked_errors::{Result, StackableErr},
    Command, FileOptions,
};
use tokio::time::sleep;

use crate::{
    cosmovisor::{
        cosmovisor_get_addr, cosmovisor_gov_file_proposal, fast_block_times, force_chain_id,
        set_minimum_gas_price, sh_cosmovisor, sh_cosmovisor_no_dbg, sh_cosmovisor_tx,
        wait_for_num_blocks,
    },
    native_denom, nom, nom_denom, token18, ONOMY_IBC_NOM, TEST_AMOUNT, TIMEOUT,
};

// make sure some things are imported so we don't have to wrangle with this for
// manual debugging
fn _unused() {
    drop(sleep(TIMEOUT));
}

/// NOTE: this is stuff you would not want to run in production.
/// NOTE: this is intended to be run inside containers only
///
/// This additionally returns the single validator mnemonic
pub async fn onomyd_setup(daemon_home: &str) -> Result<String> {
    let chain_id = "onomy";
    let global_min_self_delegation = &token18(225.0e3, "");
    sh_cosmovisor("config chain-id", &[chain_id])
        .await
        .stack()?;
    sh_cosmovisor("config keyring-backend test", &[])
        .await
        .stack()?;
    sh_cosmovisor_no_dbg("init --overwrite", &[chain_id])
        .await
        .stack()?;

    let genesis_file_path = format!("{daemon_home}/config/genesis.json");
    let genesis_s = FileOptions::read_to_string(&genesis_file_path)
        .await
        .stack()?;

    // rename all "stake" to "anom"
    let genesis_s = genesis_s.replace("\"stake\"", "\"anom\"");
    let mut genesis: Value = serde_json::from_str(&genesis_s).stack()?;

    force_chain_id(daemon_home, &mut genesis, chain_id)
        .await
        .stack()?;

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
    let genesis_s = serde_json::to_string(&genesis).stack()?;
    FileOptions::write_str(&genesis_file_path, &genesis_s)
        .await
        .stack()?;

    fast_block_times(daemon_home).await.stack()?;

    set_minimum_gas_price(daemon_home, "1anom").await.stack()?;

    // we need the stderr to get the mnemonic
    let comres = Command::new("cosmovisor run keys add validator", &[])
        .run_to_completion()
        .await
        .stack()?;
    comres.assert_success()?;
    let mnemonic = comres
        .stderr
        .trim()
        .lines()
        .last()
        .stack_err(|| "no last line")?
        .trim()
        .to_owned();
    sh_cosmovisor("add-genesis-account validator", &[&nom(2.0e6)])
        .await
        .stack()?;

    // unconditionally needed for some Arc tests
    sh_cosmovisor("keys add orchestrator", &[]).await.stack()?;
    sh_cosmovisor("add-genesis-account orchestrator", &[&nom(2.0e6)])
        .await
        .stack()?;

    sh_cosmovisor("gentx validator", &[
        &nom(1.0e6),
        "--chain-id",
        chain_id,
        "--min-self-delegation",
        global_min_self_delegation,
    ])
    .await
    .stack()?;

    sh_cosmovisor_no_dbg("collect-gentxs", &[]).await.stack()?;

    FileOptions::write_str(
        "/logs/genesis.json",
        &FileOptions::read_to_string(&genesis_file_path)
            .await
            .stack()?,
    )
    .await
    .stack()?;

    Ok(mnemonic)
}

pub async fn market_standalone_setup(daemon_home: &str, chain_id: &str) -> Result<String> {
    sh_cosmovisor("config chain-id", &[chain_id])
        .await
        .stack()?;
    sh_cosmovisor("config keyring-backend test", &[])
        .await
        .stack()?;
    sh_cosmovisor_no_dbg("init --overwrite", &[chain_id])
        .await
        .stack()?;

    let genesis_file_path = format!("{daemon_home}/config/genesis.json");
    let genesis_s = FileOptions::read_to_string(&genesis_file_path)
        .await
        .stack()?;

    // rename all "stake" to "native"
    let genesis_s = genesis_s.replace("\"stake\"", "\"anative\"");
    let mut genesis: Value = serde_json::from_str(&genesis_s).stack()?;

    force_chain_id(daemon_home, &mut genesis, chain_id)
        .await
        .stack()?;

    genesis["app_state"]["bank"]["denom_metadata"] = native_denom();

    // decrease the governing period for fast tests
    let gov_period = "800ms";
    let gov_period: Value = gov_period.into();
    genesis["app_state"]["gov"]["voting_params"]["voting_period"] = gov_period.clone();
    genesis["app_state"]["gov"]["deposit_params"]["max_deposit_period"] = gov_period;

    // write back genesis
    let genesis_s = serde_json::to_string(&genesis).stack()?;
    FileOptions::write_str(&genesis_file_path, &genesis_s)
        .await
        .stack()?;
    FileOptions::write_str("/logs/market_standalone_genesis.json", &genesis_s)
        .await
        .stack()?;

    fast_block_times(daemon_home).await.stack()?;
    set_minimum_gas_price(daemon_home, "1anative")
        .await
        .stack()?;

    // we need the stderr to get the mnemonic
    let comres = Command::new("cosmovisor run keys add validator", &[])
        .run_to_completion()
        .await
        .stack()?;
    comres.assert_success()?;
    let mnemonic = comres
        .stderr
        .trim()
        .lines()
        .last()
        .stack_err(|| "no last line")?
        .trim()
        .to_owned();

    //let gen_coins = token18(2.0e6, "anative") + "," + &token18(2.0e6,
    // "afootoken");
    let gen_coins = format!("{TEST_AMOUNT}anative,{TEST_AMOUNT}afootoken");
    let stake_coin = token18(1.0e6, "anative");
    sh_cosmovisor("add-genesis-account validator", &[&gen_coins])
        .await
        .stack()?;
    sh_cosmovisor("gentx validator", &[
        &stake_coin,
        "--chain-id",
        chain_id,
        "--min-self-delegation",
        "1",
    ])
    .await
    .stack()?;
    sh_cosmovisor_no_dbg("collect-gentxs", &[]).await.stack()?;

    Ok(mnemonic)
}

// NOTE: this uses the local tendermint consAddr for the bridge power
pub async fn gravity_standalone_setup(daemon_home: &str, use_old_gentx: bool) -> Result<String> {
    let chain_id = "gravity";
    let min_self_delegation = &token18(1.0, "");
    sh_cosmovisor("config chain-id", &[chain_id])
        .await
        .stack()?;
    sh_cosmovisor("config keyring-backend test", &[])
        .await
        .stack()?;
    sh_cosmovisor_no_dbg("init --overwrite", &[chain_id])
        .await
        .stack()?;

    let genesis_file_path = format!("{daemon_home}/config/genesis.json");
    let genesis_s = FileOptions::read_to_string(&genesis_file_path)
        .await
        .stack()?;

    // rename all "stake" to "anom"
    let genesis_s = genesis_s.replace("\"stake\"", "\"anom\"");
    let mut genesis: Value = serde_json::from_str(&genesis_s).stack()?;

    force_chain_id(daemon_home, &mut genesis, chain_id)
        .await
        .stack()?;

    // put in the test `footoken` and the staking `anom`
    let denom_metadata = nom_denom();
    genesis["app_state"]["bank"]["denom_metadata"] = denom_metadata;

    // decrease the governing period for fast tests
    let gov_period = "800ms";
    let gov_period: Value = gov_period.into();
    genesis["app_state"]["gov"]["voting_params"]["voting_period"] = gov_period.clone();
    genesis["app_state"]["gov"]["deposit_params"]["max_deposit_period"] = gov_period;

    // write back genesis
    let genesis_s = serde_json::to_string(&genesis).stack()?;
    FileOptions::write_str(&genesis_file_path, &genesis_s)
        .await
        .stack()?;

    fast_block_times(daemon_home).await.stack()?;
    set_minimum_gas_price(daemon_home, "1anom").await.stack()?;

    // we need the stderr to get the mnemonic
    let comres = Command::new("cosmovisor run keys add validator", &[])
        .run_to_completion()
        .await
        .stack()?;
    comres.assert_success()?;
    let mnemonic = comres
        .stderr
        .trim()
        .lines()
        .last()
        .stack_err(|| "no last line")?
        .trim()
        .to_owned();
    // TODO for unknown reasons, add-genesis-account cannot find the keys
    let addr = cosmovisor_get_addr("validator").await.stack()?;
    sh_cosmovisor("add-genesis-account", &[&addr, &nom(2.0e6)])
        .await
        .stack()?;

    // unconditionally needed for some Arc tests
    sh_cosmovisor("keys add orchestrator", &[]).await.stack()?;
    let orch_addr = cosmovisor_get_addr("orchestrator").await.stack()?;
    sh_cosmovisor("add-genesis-account", &[&orch_addr, &nom(1.0e6)])
        .await
        .stack()?;

    let eth_keys = sh_cosmovisor("eth_keys add", &[]).await.stack()?;
    let eth_addr = &get_separated_val(&eth_keys, "\n", "address", ":").stack()?;

    let consaddr = sh_cosmovisor("tendermint show-address", &[]).await?;
    let consaddr = consaddr.trim();

    if use_old_gentx {
        sh_cosmovisor("gentx", &[
            "validator",
            &nom(1.0e6),
            eth_addr,
            &orch_addr,
            "--chain-id",
            chain_id,
            "--min-self-delegation",
            min_self_delegation,
        ])
        .await
        .stack()?;
    } else {
        sh_cosmovisor("gentx", &[
            &nom(1.0e6),
            consaddr,
            eth_addr,
            "orchestrator",
            "--chain-id",
            chain_id,
            "--min-self-delegation",
            min_self_delegation,
        ])
        .await
        .stack()?;
    }
    sh_cosmovisor_no_dbg("collect-gentxs", &[]).await.stack()?;

    FileOptions::write_str(
        &format!("/logs/{chain_id}_genesis.json"),
        &FileOptions::read_to_string(&genesis_file_path)
            .await
            .stack()?,
    )
    .await
    .stack()?;

    Ok(mnemonic)
}

pub fn test_proposal(consumer_id: &str, reward_denom: &str) -> String {
    format!(
        r#"{{
        "title": "Propose the addition of a new chain",
        "description": "add consumer chain",
        "chain_id": "{consumer_id}",
        "initial_height": {{
            "revision_number": 0,
            "revision_height": 1
        }},
        "genesis_hash": "Z2VuX2hhc2g=",
        "binary_hash": "YmluX2hhc2g=",
        "spawn_time": "2023-05-18T01:15:49.83019476-05:00",
        "unbonding_period": 1728000000000000,
        "provider_reward_denoms": ["anom"],
        "reward_denoms": ["{reward_denom}"],
        "consumer_redistribution_fraction": "0.75",
        "blocks_per_distribution_transmission": 5,
        "soft_opt_out_threshold": "0.0",
        "historical_entries": 10000,
        "ccv_timeout_period": 2419200000000000,
        "transfer_timeout_period": 3600000000000,
        "deposit": "500000000000000000000anom"
    }}"#
    )
}

/// This should be run from the provider. Returns the ccv state.
///
/// Note: `sh_cosmovisor_tx("provider register-consumer-reward-denom
/// [IBC-denom]` may need to be run afterwards if it is intended to receive
/// consumer rewards
pub async fn cosmovisor_add_consumer(
    daemon_home: &str,
    consumer_id: &str,
    proposal_s: &str,
) -> Result<String> {
    let proposal: Value = serde_json::from_str(proposal_s).stack()?;

    let tendermint_key = sh_cosmovisor("tendermint show-validator", &[])
        .await
        .stack()?;
    let tendermint_key = tendermint_key.trim();

    cosmovisor_gov_file_proposal(daemon_home, "consumer-addition", proposal_s, "1anom")
        .await
        .stack()?;
    wait_for_num_blocks(1).await.stack()?;

    // do this before getting the consumer-genesis
    sh_cosmovisor_tx("provider assign-consensus-key", &[
        consumer_id,
        tendermint_key,
        // TODO for unknown reasons, `onomyd` with nonzero gas fee breaks non `--fees` usage
        //"--gas",
        //"auto",
        //"--gas-adjustment",
        //"1.3",
        "--fees",
        "1000000anom",
        "-y",
        "-b",
        "block",
        "--from",
        "validator",
    ])
    .await
    .stack()?;

    // It appears we do not have to wait any blocks

    let ccvconsumer_state = sh_cosmovisor_no_dbg("query provider consumer-genesis", &[
        consumer_id,
        "-o",
        "json",
    ])
    .await
    .stack()?;

    let mut state: Value = serde_json::from_str(&ccvconsumer_state).stack()?;

    // fix missing fields TODO when we update canonical versions we should be able
    // to remove this
    state["params"]["soft_opt_out_threshold"] = "0.0".into();
    state["params"]["provider_reward_denoms"] = proposal["provider_reward_denoms"].clone();
    state["params"]["reward_denoms"] = proposal["reward_denoms"].clone();

    let ccvconsumer_state = serde_json::to_string(&state).stack()?;

    Ok(ccvconsumer_state)
}

pub async fn marketd_setup(
    daemon_home: &str,
    chain_id: &str,
    ccvconsumer_state_s: &str,
) -> Result<()> {
    sh_cosmovisor("config chain-id", &[chain_id])
        .await
        .stack()?;
    sh_cosmovisor("config keyring-backend test", &[])
        .await
        .stack()?;
    sh_cosmovisor_no_dbg("init --overwrite", &[chain_id])
        .await
        .stack()?;
    let genesis_file_path = format!("{daemon_home}/config/genesis.json");

    // add `ccvconsumer_state` to genesis
    let genesis_s = FileOptions::read_to_string(&genesis_file_path)
        .await
        .stack()?;

    let genesis_s = genesis_s.replace("\"stake\"", "\"anative\"");
    let mut genesis: Value = serde_json::from_str(&genesis_s).stack()?;

    force_chain_id(daemon_home, &mut genesis, chain_id)
        .await
        .stack()?;

    let ccvconsumer_state: Value = serde_json::from_str(ccvconsumer_state_s).stack()?;
    genesis["app_state"]["ccvconsumer"] = ccvconsumer_state;

    // decrease the governing period for fast tests
    let gov_period = "800ms";
    let gov_period: Value = gov_period.into();
    genesis["app_state"]["gov"]["voting_params"]["voting_period"] = gov_period.clone();
    genesis["app_state"]["gov"]["deposit_params"]["max_deposit_period"] = gov_period;

    // Set governance token (for param changes and upgrades) to IBC NOM
    genesis["app_state"]["gov"]["deposit_params"]["min_deposit"][0]["amount"] =
        token18(500.0, "").into();
    genesis["app_state"]["gov"]["deposit_params"]["min_deposit"][0]["denom"] = ONOMY_IBC_NOM.into();
    genesis["app_state"]["staking"]["params"]["bond_denom"] = ONOMY_IBC_NOM.into();

    // Set market burn token to IBC NOM
    genesis["app_state"]["market"]["params"]["burn_coin"] = ONOMY_IBC_NOM.into();

    // NOTE: do not under any circumstance make a mint denom an IBC token.
    // We will zero and reset inflation to anative just to make sure.
    genesis["app_state"]["mint"]["minter"]["inflation"] = "0.0".into();
    genesis["app_state"]["mint"]["params"]["mint_denom"] = "anative".into();
    genesis["app_state"]["mint"]["params"]["inflation_min"] = "0.0".into();
    genesis["app_state"]["mint"]["params"]["inflation_max"] = "0.0".into();
    genesis["app_state"]["mint"]["params"]["inflation_rate_change"] = "0.0".into();

    let genesis_s = genesis.to_string();

    FileOptions::write_str(&genesis_file_path, &genesis_s)
        .await
        .stack()?;
    FileOptions::write_str(&format!("/logs/{chain_id}_genesis.json"), &genesis_s)
        .await
        .stack()?;

    let addr: &String = &cosmovisor_get_addr("validator").await.stack()?;

    // we need some native token in the bank, and don't need gentx
    sh_cosmovisor("add-genesis-account", &[addr, &token18(2.0e6, "anative")])
        .await
        .stack()?;

    fast_block_times(daemon_home).await.stack()?;
    set_minimum_gas_price(daemon_home, "1anative")
        .await
        .stack()?;

    FileOptions::write_str(
        &format!("/logs/{chain_id}_genesis.json"),
        &FileOptions::read_to_string(&genesis_file_path)
            .await
            .stack()?,
    )
    .await
    .stack()?;

    Ok(())
}

pub async fn arc_consumer_setup(
    daemon_home: &str,
    chain_id: &str,
    ccvconsumer_state_s: &str,
) -> Result<()> {
    sh_cosmovisor("config chain-id", &[chain_id])
        .await
        .stack()?;
    sh_cosmovisor("config keyring-backend test", &[])
        .await
        .stack()?;
    sh_cosmovisor_no_dbg("init --overwrite", &[chain_id])
        .await
        .stack()?;
    let genesis_file_path = format!("{daemon_home}/config/genesis.json");

    // add `ccvconsumer_state` to genesis
    let genesis_s = FileOptions::read_to_string(&genesis_file_path)
        .await
        .stack()?;

    let genesis_s = genesis_s.replace("\"stake\"", "\"anative\"");
    let mut genesis: Value = serde_json::from_str(&genesis_s).stack()?;

    force_chain_id(daemon_home, &mut genesis, chain_id)
        .await
        .stack()?;

    let ccvconsumer_state: Value = serde_json::from_str(ccvconsumer_state_s).stack()?;
    genesis["app_state"]["ccvconsumer"] = ccvconsumer_state;

    // write back genesis
    let genesis_s = serde_json::to_string(&genesis).stack()?;
    FileOptions::write_str(&genesis_file_path, &genesis_s)
        .await
        .stack()?;

    let addr: &String = &cosmovisor_get_addr("validator").await.stack()?;
    let orch_addr: &String = &cosmovisor_get_addr("orchestrator").await.stack()?;

    // we need some native token in the bank, and don't need gentx
    sh_cosmovisor("add-genesis-account", &[addr, &token18(2.0e6, "anative")])
        .await
        .stack()?;
    sh_cosmovisor("add-genesis-account", &[
        orch_addr,
        &token18(2.0e6, "anative"),
    ])
    .await
    .stack()?;

    let eth_keys = sh_cosmovisor("eth_keys add", &[]).await.stack()?;
    let eth_addr = &get_separated_val(&eth_keys, "\n", "address", ":").stack()?;
    let min_self_delegation = &token18(1.0, "");
    sh_cosmovisor("gentx validator", &[
        &token18(1.0e6, "anative"),
        eth_addr,
        orch_addr,
        "--chain-id",
        chain_id,
        "--min-self-delegation",
        min_self_delegation,
    ])
    .await
    .stack()?;
    sh_cosmovisor_no_dbg("collect-gentxs", &[]).await.stack()?;

    fast_block_times(daemon_home).await.stack()?;

    FileOptions::write_str(
        &format!("/logs/{chain_id}_genesis.json"),
        &FileOptions::read_to_string(&genesis_file_path)
            .await
            .stack()?,
    )
    .await
    .stack()?;

    Ok(())
}
