use serde_json::Value;
use super_orchestrator::{
    stacked_errors::{MapAddError, Result},
    FileOptions,
};

use crate::{
    cosmovisor::{
        cosmovisor_get_addr, cosmovisor_get_num_proposals, fast_block_times, sh_cosmovisor,
    },
    json_inner, token18,
};

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

    // we will just place the file under the config folder
    let proposal_file_path = format!("{daemon_home}/config/consumer_add_proposal.json");
    FileOptions::write_str(&proposal_file_path, proposal_s)
        .await
        .map_add_err(|| ())?;

    let gas_args = [
        "--gas",
        "auto",
        "--gas-adjustment",
        "1.3",
        "-y",
        "-b",
        "block",
        "--from",
        "validator",
    ]
    .as_slice();
    sh_cosmovisor(
        "tx gov submit-proposal consumer-addition",
        &[&[proposal_file_path.as_str()], gas_args].concat(),
    )
    .await?;
    let proposal_id = format!("{}", cosmovisor_get_num_proposals().await?);
    // the deposit is done as part of the chain addition proposal
    sh_cosmovisor(
        "tx gov vote",
        &[[&proposal_id, "yes"].as_slice(), gas_args].concat(),
    )
    .await?;

    // In the mean time get consensus key assignment done

    let tendermint_key: Value = serde_json::from_str(
        &FileOptions::read_to_string(&format!("{daemon_home}/config/priv_validator_key.json"))
            .await?,
    )?;
    let tendermint_key = json_inner(&tendermint_key["pub_key"]["value"]);
    let tendermint_key =
        format!("{{\"@type\":\"/cosmos.crypto.ed25519.PubKey\",\"key\":\"{tendermint_key}\"}}");

    // do this before getting the consumer-genesis
    sh_cosmovisor(
        "tx provider assign-consensus-key",
        &[[consumer_id, tendermint_key.as_str()].as_slice(), gas_args].concat(),
    )
    .await?;

    // It appears we do not have to wait any blocks

    let ccvconsumer_state = sh_cosmovisor("query provider consumer-genesis", &[
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
    sh_cosmovisor("init --overwrite", &[chain_id]).await?;
    let genesis_file_path = format!("{daemon_home}/config/genesis.json");

    // add `ccvconsumer_state` to genesis
    let genesis_s = FileOptions::read_to_string(&genesis_file_path).await?;
    let mut genesis: Value = serde_json::from_str(&genesis_s)?;
    let ccvconsumer_state: Value = serde_json::from_str(ccvconsumer_state_s)?;
    genesis["app_state"]["ccvconsumer"] = ccvconsumer_state;
    let genesis_s = genesis.to_string();

    // I will name the token "native" because it won't be staked in the normal sense
    let genesis_s = genesis_s.replace("\"stake\"", "\"native\"");

    FileOptions::write_str(&genesis_file_path, &genesis_s).await?;

    let addr: &String = &cosmovisor_get_addr("validator").await?;

    // we need some native token in the bank, and don't need gentx
    sh_cosmovisor("add-genesis-account", &[addr, &token18(2.0e6, "native")]).await?;

    FileOptions::write_str(
        &format!("/logs/{chain_id}_genesis.json"),
        &FileOptions::read_to_string(&genesis_file_path).await?,
    )
    .await?;

    fast_block_times(daemon_home).await?;

    Ok(())
}
