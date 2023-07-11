use serde_json::{json, Value};
use super_orchestrator::{
    get_separated_val,
    stacked_errors::{MapAddError, Result},
    Command, FileOptions,
};
use tokio::time::sleep;

use crate::{
    cosmovisor::{
        cosmovisor_get_addr, cosmovisor_gov_file_proposal, fast_block_times, force_chain_id,
        set_minimum_gas_price, sh_cosmovisor, sh_cosmovisor_no_dbg, sh_cosmovisor_tx,
        wait_for_num_blocks,
    },
    json_inner, native_denom, nom, nom_denom, token18, ONOMY_IBC_NOM, TIMEOUT,
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
    sh_cosmovisor("config chain-id", &[chain_id]).await?;
    sh_cosmovisor("config keyring-backend test", &[]).await?;
    sh_cosmovisor_no_dbg("init --overwrite", &[chain_id]).await?;

    let genesis_file_path = format!("{daemon_home}/config/genesis.json");
    let genesis_s = FileOptions::read_to_string(&genesis_file_path).await?;

    // rename all "stake" to "anom"
    let genesis_s = genesis_s.replace("\"stake\"", "\"anom\"");
    let mut genesis: Value = serde_json::from_str(&genesis_s)?;

    force_chain_id(daemon_home, &mut genesis, chain_id).await?;

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

    set_minimum_gas_price(daemon_home, "1anom").await?;

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

    // unconditionally needed for some Arc tests
    sh_cosmovisor("keys add orchestrator", &[]).await?;
    sh_cosmovisor("add-genesis-account orchestrator", &[&nom(2.0e6)]).await?;

    sh_cosmovisor("gentx validator", &[
        &nom(1.0e6),
        "--chain-id",
        chain_id,
        "--min-self-delegation",
        global_min_self_delegation,
    ])
    .await?;

    sh_cosmovisor_no_dbg("collect-gentxs", &[]).await?;

    Ok(mnemonic)
}

pub async fn market_standaloned_setup(daemon_home: &str) -> Result<String> {
    let chain_id = "market_standalone";
    let global_min_self_delegation = "225000000000000000000000";
    sh_cosmovisor("config chain-id", &[chain_id]).await?;
    sh_cosmovisor("config keyring-backend test", &[]).await?;
    sh_cosmovisor_no_dbg("init --overwrite", &[chain_id]).await?;

    let genesis_file_path = format!("{daemon_home}/config/genesis.json");
    let genesis_s = FileOptions::read_to_string(&genesis_file_path).await?;

    // rename all "stake" to "native"
    let genesis_s = genesis_s.replace("\"stake\"", "\"anative\"");
    let mut genesis: Value = serde_json::from_str(&genesis_s)?;

    force_chain_id(daemon_home, &mut genesis, chain_id).await?;

    genesis["app_state"]["bank"]["denom_metadata"] = native_denom();

    // decrease the governing period for fast tests
    let gov_period = "800ms";
    let gov_period: Value = gov_period.into();
    genesis["app_state"]["gov"]["voting_params"]["voting_period"] = gov_period.clone();
    genesis["app_state"]["gov"]["deposit_params"]["max_deposit_period"] = gov_period;

    // write back genesis
    let genesis_s = serde_json::to_string(&genesis)?;
    FileOptions::write_str(&genesis_file_path, &genesis_s).await?;
    FileOptions::write_str("/logs/market_standalone_genesis.json", &genesis_s).await?;

    fast_block_times(daemon_home).await?;
    set_minimum_gas_price(daemon_home, "1anative").await?;

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

    let gen_coins = token18(2.0e6, "anative") + "," + &token18(2.0e6, "afootoken");
    let stake_coin = token18(1.0e6, "anative");
    sh_cosmovisor("add-genesis-account validator", &[&gen_coins]).await?;
    sh_cosmovisor("gentx validator", &[
        &stake_coin,
        "--chain-id",
        chain_id,
        "--min-self-delegation",
        global_min_self_delegation,
    ])
    .await?;
    sh_cosmovisor_no_dbg("collect-gentxs", &[]).await?;

    Ok(mnemonic)
}

pub async fn gravity_standalone_setup(daemon_home: &str) -> Result<String> {
    let chain_id = "gravity";
    let min_self_delegation = &token18(1.0, "");
    sh_cosmovisor("config chain-id", &[chain_id]).await?;
    sh_cosmovisor("config keyring-backend test", &[]).await?;
    sh_cosmovisor_no_dbg("init --overwrite", &[chain_id]).await?;

    let genesis_file_path = format!("{daemon_home}/config/genesis.json");
    let genesis_s = FileOptions::read_to_string(&genesis_file_path).await?;

    // rename all "stake" to "anom"
    let genesis_s = genesis_s.replace("\"stake\"", "\"anom\"");
    let mut genesis: Value = serde_json::from_str(&genesis_s)?;

    force_chain_id(daemon_home, &mut genesis, chain_id).await?;

    // put in the test `footoken` and the staking `anom`
    let denom_metadata = nom_denom();
    genesis["app_state"]["bank"]["denom_metadata"] = denom_metadata;

    // decrease the governing period for fast tests
    let gov_period = "800ms";
    let gov_period: Value = gov_period.into();
    genesis["app_state"]["gov"]["voting_params"]["voting_period"] = gov_period.clone();
    genesis["app_state"]["gov"]["deposit_params"]["max_deposit_period"] = gov_period;

    // write back genesis
    let genesis_s = serde_json::to_string(&genesis)?;
    FileOptions::write_str(&genesis_file_path, &genesis_s).await?;

    fast_block_times(daemon_home).await?;
    set_minimum_gas_price(daemon_home, "1anom").await?;

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
    // TODO for unknown reasons, add-genesis-account cannot find the keys
    let addr = cosmovisor_get_addr("validator").await?;
    sh_cosmovisor("add-genesis-account", &[&addr, &nom(2.0e6)]).await?;

    // unconditionally needed for some Arc tests
    sh_cosmovisor("keys add orchestrator", &[]).await?;
    let orch_addr = cosmovisor_get_addr("orchestrator").await?;
    sh_cosmovisor("add-genesis-account", &[&orch_addr, &nom(1.0e6)]).await?;

    let eth_keys = sh_cosmovisor("eth_keys add", &[]).await?;
    let eth_addr = &get_separated_val(&eth_keys, "\n", "address", ":")?;
    sh_cosmovisor("gentx validator", &[
        &nom(1.0e6),
        eth_addr,
        &orch_addr,
        "--chain-id",
        chain_id,
        "--min-self-delegation",
        min_self_delegation,
    ])
    .await?;
    sh_cosmovisor_no_dbg("collect-gentxs", &[]).await?;

    FileOptions::write_str(
        &format!("/logs/{chain_id}_genesis.json"),
        &FileOptions::read_to_string(&genesis_file_path).await?,
    )
    .await?;

    Ok(mnemonic)
}

/// This should be run from the provider. Returns the ccv state.
pub async fn cosmovisor_add_consumer(daemon_home: &str, consumer_id: &str) -> Result<String> {
    // `json!` doesn't like large literals beyond i32.
    // note: when changing this, check market_genesis.json
    // to see if changes are going all the way through.
    // note: the deposit is for the submission on the producer side, so we want to
    // use 2k NOM.
    let proposal_s = &format!(
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
        "consumer_redistribution_fraction": "0.0",
        "blocks_per_distribution_transmission": 1000,
        "historical_entries": 10000,
        "ccv_timeout_period": 2419200000000000,
        "transfer_timeout_period": 3600000000000,
        "unbonding_period": 1728000000000000,
        "deposit": "2000000000000000000000anom",
        "soft_opt_out_threshold": 0.0,
        "provider_reward_denoms": [],
        "reward_denoms": []
    }}"#
    );
    cosmovisor_gov_file_proposal(daemon_home, "consumer-addition", proposal_s, "1anom").await?;
    wait_for_num_blocks(1).await?;

    let tendermint_key: Value = serde_json::from_str(
        &FileOptions::read_to_string(&format!("{daemon_home}/config/priv_validator_key.json"))
            .await?,
    )?;
    let tendermint_key = json_inner(&tendermint_key["pub_key"]["value"]);
    let tendermint_key =
        format!("{{\"@type\":\"/cosmos.crypto.ed25519.PubKey\",\"key\":\"{tendermint_key}\"}}");

    // do this before getting the consumer-genesis
    sh_cosmovisor_tx("provider assign-consensus-key", &[
        consumer_id,
        &tendermint_key,
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
    .await?;

    // It appears we do not have to wait any blocks

    let ccvconsumer_state = sh_cosmovisor_no_dbg("query provider consumer-genesis", &[
        consumer_id,
        "-o",
        "json",
    ])
    .await?;

    let mut state: Value = serde_json::from_str(&ccvconsumer_state)?;
    // TODO because of the differing canonical producer and consumer versions, the
    // `consumer-genesis` currently does not handle all keys, we have to set
    // `soft_opt_out_threshold` here.
    state["params"]["soft_opt_out_threshold"] = "0.0".into();
    let ccvconsumer_state = serde_json::to_string(&state)?;

    Ok(ccvconsumer_state)
}

pub async fn marketd_setup(
    daemon_home: &str,
    chain_id: &str,
    ccvconsumer_state_s: &str,
) -> Result<()> {
    sh_cosmovisor("config chain-id", &[chain_id]).await?;
    sh_cosmovisor("config keyring-backend test", &[]).await?;
    sh_cosmovisor_no_dbg("init --overwrite", &[chain_id]).await?;
    let genesis_file_path = format!("{daemon_home}/config/genesis.json");

    // add `ccvconsumer_state` to genesis
    let genesis_s = FileOptions::read_to_string(&genesis_file_path).await?;

    let genesis_s = genesis_s.replace("\"stake\"", "\"anative\"");
    let mut genesis: Value = serde_json::from_str(&genesis_s)?;

    force_chain_id(daemon_home, &mut genesis, chain_id).await?;

    let ccvconsumer_state: Value = serde_json::from_str(ccvconsumer_state_s)?;
    genesis["app_state"]["ccvconsumer"] = ccvconsumer_state;

    // decrease the governing period for fast tests
    let gov_period = "800ms";
    let gov_period: Value = gov_period.into();
    genesis["app_state"]["gov"]["voting_params"]["voting_period"] = gov_period.clone();
    genesis["app_state"]["gov"]["deposit_params"]["max_deposit_period"] = gov_period;

    // Set governance token (for param changes and upgrades) to IBC NOM
    genesis["app_state"]["gov"]["deposit_params"]["min_deposit"][0]["amount"] =
        token18(2000.0, "").into();
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

    FileOptions::write_str(&genesis_file_path, &genesis_s).await?;
    FileOptions::write_str("/logs/market_genesis.json", &genesis_s).await?;

    let addr: &String = &cosmovisor_get_addr("validator").await?;

    // we need some native token in the bank, and don't need gentx
    sh_cosmovisor("add-genesis-account", &[addr, &token18(2.0e6, "anative")]).await?;

    fast_block_times(daemon_home).await?;
    set_minimum_gas_price(daemon_home, "1anative").await?;

    FileOptions::write_str(
        &format!("/logs/{chain_id}_genesis.json"),
        &FileOptions::read_to_string(&genesis_file_path).await?,
    )
    .await?;

    Ok(())
}

pub async fn arc_consumer_setup(
    daemon_home: &str,
    chain_id: &str,
    ccvconsumer_state_s: &str,
) -> Result<()> {
    sh_cosmovisor("config chain-id", &[chain_id]).await?;
    sh_cosmovisor("config keyring-backend test", &[]).await?;
    sh_cosmovisor_no_dbg("init --overwrite", &[chain_id]).await?;
    let genesis_file_path = format!("{daemon_home}/config/genesis.json");

    // add `ccvconsumer_state` to genesis
    let genesis_s = FileOptions::read_to_string(&genesis_file_path).await?;

    let genesis_s = genesis_s.replace("\"stake\"", "\"anative\"");
    let mut genesis: Value = serde_json::from_str(&genesis_s)?;

    force_chain_id(daemon_home, &mut genesis, chain_id).await?;

    let ccvconsumer_state: Value = serde_json::from_str(ccvconsumer_state_s)?;
    genesis["app_state"]["ccvconsumer"] = ccvconsumer_state;

    // write back genesis
    let genesis_s = serde_json::to_string(&genesis)?;
    FileOptions::write_str(&genesis_file_path, &genesis_s).await?;

    let addr: &String = &cosmovisor_get_addr("validator").await?;
    let orch_addr: &String = &cosmovisor_get_addr("orchestrator").await?;

    // we need some native token in the bank, and don't need gentx
    sh_cosmovisor("add-genesis-account", &[addr, &token18(2.0e6, "anative")]).await?;
    sh_cosmovisor("add-genesis-account", &[
        orch_addr,
        &token18(2.0e6, "anative"),
    ])
    .await?;

    let eth_keys = sh_cosmovisor("eth_keys add", &[]).await?;
    let eth_addr = &get_separated_val(&eth_keys, "\n", "address", ":")?;
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
    .await?;
    sh_cosmovisor_no_dbg("collect-gentxs", &[]).await?;

    fast_block_times(daemon_home).await?;

    FileOptions::write_str(
        &format!("/logs/{chain_id}_genesis.json"),
        &FileOptions::read_to_string(&genesis_file_path).await?,
    )
    .await?;

    Ok(())
}
