use serde_json::{json, Value};
use super_orchestrator::{
    stacked_errors::{Result, StackableErr},
    stacked_get, stacked_get_mut, Command, FileOptions,
};
use tokio::time::sleep;

use crate::{
    cosmovisor::{
        cosmovisor_gov_file_proposal, fast_block_times, force_chain_id, set_minimum_gas_price,
        sh_cosmovisor, sh_cosmovisor_no_debug, sh_cosmovisor_tx, wait_for_num_blocks,
    },
    nom_denom, token18, TEST_AMOUNT, TIMEOUT,
};

// make sure some things are imported so we don't have to wrangle with this for
// manual debugging
fn _unused() {
    drop(sleep(TIMEOUT));
}

#[derive(Default, Clone)]
pub struct CosmosSetupOptions {
    pub chain_id: String,

    pub daemon_home: String,

    pub gov_token: String,

    pub gas_token: String,

    pub ccvconsumer_state: Option<String>,

    // mnemonic for the validator to use instead of randomly generating
    pub validator_mnemonic: Option<String>,

    pub hermes_mnemonic: Option<String>,

    pub onex_testnet_amounts: bool,

    // used for APR tests, as normally there is a lot of undelegated tokens that would mess up
    // calculations
    pub high_staking_level: bool,

    // used for checking the numerical limits of the market
    pub large_test_amount: bool,

    // special Onomy main provider chain only modules
    pub onomy_special: bool,
}

impl CosmosSetupOptions {
    pub fn new(
        daemon_home: &str,
        chain_id: &str,
        gov_token: &str,
        gas_token: &str,
        ccvconsumer_state: Option<&str>,
    ) -> Self {
        CosmosSetupOptions {
            chain_id: chain_id.to_owned(),
            daemon_home: daemon_home.to_owned(),
            gov_token: gov_token.to_owned(),
            gas_token: gas_token.to_owned(),
            ccvconsumer_state: ccvconsumer_state.map(|s| s.to_string()),
            ..Default::default()
        }
    }

    pub fn onomy(daemon_home: &str) -> Self {
        CosmosSetupOptions {
            chain_id: "onomy".to_owned(),
            daemon_home: daemon_home.to_owned(),
            gov_token: "anom".to_owned(),
            onomy_special: true,
            gas_token: "anom".to_owned(),
            ..Default::default()
        }
    }
}

#[derive(Default, Clone)]
pub struct CosmosSetupResults {
    pub validator_mnemonic: Option<String>,
    // a mnemonic to a separate account intended for the hermes relayer, for avoiding an annoying
    // issue with account sequence mismatches
    pub hermes_mnemonic: Option<String>,
}

/// NOTE: this is stuff you would not want to run in production.
/// NOTE: this is intended to be run inside containers only
///
/// This additionally returns the single validator mnemonic
pub async fn cosmovisor_setup(options: CosmosSetupOptions) -> Result<CosmosSetupResults> {
    let daemon_home = &options.daemon_home;
    let chain_id = &options.chain_id;
    let gov_token = &options.gov_token;
    let gas_token = &options.gas_token;
    sh_cosmovisor(["config chain-id", chain_id]).await.stack()?;
    sh_cosmovisor(["config keyring-backend test"])
        .await
        .stack()?;
    sh_cosmovisor_no_debug(["init --overwrite", chain_id])
        .await
        .stack()?;

    let genesis_file_path = format!("{daemon_home}/config/genesis.json");
    let genesis_s = FileOptions::read_to_string(&genesis_file_path)
        .await
        .stack()?;

    // rename all "stake" to "anom"
    let genesis_s = genesis_s.replace("\"stake\"", &format!("\"{gov_token}\""));
    let mut genesis: Value = serde_json::from_str(&genesis_s).stack()?;

    force_chain_id(daemon_home, &mut genesis, chain_id)
        .await
        .stack()?;

    // for consumer chains
    if let Some(ref ccvconsumer_state_s) = options.ccvconsumer_state {
        let ccvconsumer_state: Value = serde_json::from_str(ccvconsumer_state_s).stack()?;
        *stacked_get_mut!(genesis["app_state"]["ccvconsumer"]) = ccvconsumer_state;
    }

    // put in the test `footoken` and the staking `anom`
    let denom_metadata = nom_denom();
    *stacked_get_mut!(genesis["app_state"]["bank"]["denom_metadata"]) = denom_metadata;

    if options.onomy_special {
        // init DAO balance
        let amount = token18(100.0e6, "");
        let treasury_balance = json!([{"denom": "anom", "amount": amount}]);
        *stacked_get_mut!(genesis["app_state"]["dao"]["treasury_balance"]) = treasury_balance;
    }

    // disable community_tax
    *stacked_get_mut!(genesis["app_state"]["distribution"]["params"]["community_tax"]) = json!("0");

    let global_min_self_delegation = &token18(225.0e3, "");
    if options.onomy_special {
        // min_global_self_delegation
        *stacked_get_mut!(
            genesis["app_state"]["staking"]["params"]["min_global_self_delegation"]
        ) = global_min_self_delegation.to_owned().into();
    }

    // decrease the governing period for fast tests
    let gov_period = "800ms";
    let gov_period: Value = gov_period.into();
    *stacked_get_mut!(genesis["app_state"]["gov"]["voting_params"]["voting_period"]) =
        gov_period.clone();
    *stacked_get_mut!(genesis["app_state"]["gov"]["deposit_params"]["max_deposit_period"]) =
        gov_period;

    // Set governance token
    *stacked_get_mut!(genesis["app_state"]["gov"]["deposit_params"]["min_deposit"][0]["amount"]) =
        token18(500.0, "").into();
    *stacked_get_mut!(genesis["app_state"]["gov"]["deposit_params"]["min_deposit"][0]["denom"]) =
        options.gov_token.as_str().into();
    *stacked_get_mut!(genesis["app_state"]["staking"]["params"]["bond_denom"]) =
        options.gov_token.as_str().into();

    // write back genesis
    let genesis_s = serde_json::to_string(&genesis).stack()?;
    FileOptions::write_str(&genesis_file_path, &genesis_s)
        .await
        .stack()?;

    fast_block_times(daemon_home).await.stack()?;

    set_minimum_gas_price(daemon_home, &format!("1{gas_token}"))
        .await
        .stack()?;

    let validator_mnemonic = if let Some(ref mnemonic) = options.validator_mnemonic {
        Command::new(format!(
            "{daemon_home}/cosmovisor/current/bin/onomyd keys add validator --recover"
        ))
        .run_with_input_to_completion(mnemonic.as_bytes())
        .await
        .stack()?
        .assert_success()
        .stack()?;
        Some(mnemonic.to_owned())
    } else if options.ccvconsumer_state.as_deref().is_none() {
        // we need the stderr to get the mnemonic
        let comres = Command::new("cosmovisor run keys add validator")
            .run_to_completion()
            .await
            .stack()?;
        comres.assert_success().stack()?;
        let mnemonic = comres
            .stderr_as_utf8()
            .stack()?
            .trim()
            .lines()
            .last()
            .stack_err(|| "no last line")?
            .trim()
            .to_owned();
        Some(mnemonic)
    } else {
        None
    };

    let hermes_mnemonic = if let Some(ref mnemonic) = options.hermes_mnemonic {
        Command::new(format!(
            "{daemon_home}/cosmovisor/current/bin/onomyd keys add hermes --recover"
        ))
        .run_with_input_to_completion(mnemonic.as_bytes())
        .await
        .stack()?
        .assert_success()
        .stack()?;
        Some(mnemonic.to_owned())
    } else if options.ccvconsumer_state.as_deref().is_none() {
        // we need the stderr to get the mnemonic
        let comres = Command::new("cosmovisor run keys add hermes")
            .run_to_completion()
            .await
            .stack()?;
        comres.assert_success().stack()?;
        let mnemonic = comres
            .stderr_as_utf8()
            .stack()?
            .trim()
            .lines()
            .last()
            .stack_err(|| "no last line")?
            .trim()
            .to_owned();
        Some(mnemonic)
    } else {
        None
    };

    let amount = if options.onex_testnet_amounts {
        "15000000000000000000000000abtc,100000000000000000000000000anom,\
         20000000000000000000000000000ausdc,20000000000000000000000000000ausdt,\
         20000000000000000000000000wei"
            .to_owned()
    } else if options.large_test_amount {
        format!("{TEST_AMOUNT}{gov_token},{TEST_AMOUNT}afootoken")
    } else {
        format!("2000000000000000000000000{gov_token},2000000000000000000000000afootoken")
    };

    sh_cosmovisor(["add-genesis-account validator", &amount])
        .await
        .stack()?;
    sh_cosmovisor([
        "add-genesis-account hermes",
        &format!("100000000000000000000{gas_token}"),
    ])
    .await
    .stack()?;

    let self_delegate = if options.high_staking_level {
        /*sh_cosmovisor("keys add orchestrator", &[]).await.stack()?;
        sh_cosmovisor("add-genesis-account orchestrator", &[&nom(2.0e6)])
            .await
            .stack()?;*/
        token18(1.99e6, gov_token)
    } else {
        token18(1.0e6, gov_token)
    };

    if options.ccvconsumer_state.is_none() {
        sh_cosmovisor([
            "gentx validator",
            &self_delegate,
            "--chain-id",
            chain_id,
            "--min-self-delegation",
            global_min_self_delegation,
        ])
        .await
        .stack()?;
        sh_cosmovisor_no_debug(["collect-gentxs"]).await.stack()?;
    }

    FileOptions::write_str(
        format!("/logs/genesis_{chain_id}.json"),
        &FileOptions::read_to_string(&genesis_file_path)
            .await
            .stack()?,
    )
    .await
    .stack()?;

    Ok(CosmosSetupResults {
        validator_mnemonic,
        hermes_mnemonic,
    })
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

    let tendermint_key = sh_cosmovisor(["tendermint show-validator"]).await.stack()?;
    let tendermint_key = tendermint_key.trim();

    cosmovisor_gov_file_proposal(daemon_home, Some("consumer-addition"), proposal_s, "1anom")
        .await
        .stack()?;
    wait_for_num_blocks(1).await.stack()?;

    // do this before getting the consumer-genesis
    sh_cosmovisor_tx([
        "provider assign-consensus-key",
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

    let ccvconsumer_state =
        sh_cosmovisor_no_debug(["query provider consumer-genesis", consumer_id, "-o", "json"])
            .await
            .stack()?;

    let mut state: Value = serde_json::from_str(&ccvconsumer_state).stack()?;

    // fix missing fields TODO when we update canonical versions we should be able
    // to remove this
    stacked_get_mut!(state["params"])["soft_opt_out_threshold"] = "0.0".into();
    stacked_get_mut!(state["params"])["provider_reward_denoms"] =
        stacked_get!(proposal["provider_reward_denoms"]).clone();
    stacked_get_mut!(state["params"])["reward_denoms"] =
        stacked_get!(proposal["reward_denoms"]).clone();

    let ccvconsumer_state = serde_json::to_string(&state).stack()?;

    Ok(ccvconsumer_state)
}
