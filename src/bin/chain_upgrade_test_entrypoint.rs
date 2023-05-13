use std::env;

use common::nom;
use lazy_static::lazy_static;
use serde_json::{json, Value};
use super_orchestrator::{
    acquire_file_path, close_file, get_separated_val, sh, Command, MapAddError, Result,
};
use tokio::{
    fs::OpenOptions,
    io::{AsyncReadExt, AsyncWriteExt},
};

lazy_static! {
    static ref DAEMON_NAME: String = env::var("DAEMON_NAME").unwrap();
    static ref DAEMON_HOME: String = env::var("DAEMON_HOME").unwrap();
    static ref ONOMY_CURRENT_VERSION: String = env::var("ONOMY_CURRENT_VERSION").unwrap();
    static ref ONOMY_UPGRADE_VERSION: String = env::var("ONOMY_UPGRADE_VERSION").unwrap();
    static ref GOV_PERIOD: String = env::var("GOV_PERIOD").unwrap();
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::new()
        .filter_level(log::LevelFilter::Info)
        .init();

    // NOTE: this is stuff you would not want to run in production

    let chain_id = "onomy";
    sh("cosmovisor run config chain-id", &[chain_id]).await?;
    sh("cosmovisor run config keyring-backend test", &[]).await?;
    sh("cosmovisor run init --overwrite", &[chain_id]).await?;

    let genesis_file_path =
        acquire_file_path(&format!("{}/config/genesis.json", *DAEMON_HOME)).await?;
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

    let denom_metadata = json!(
        [{"name": "Foo Token", "symbol": "FOO", "base": "footoken", "display": "mfootoken",
        "description": "A non-staking test token", "denom_units": [{"denom": "footoken",
        "exponent": 0}, {"denom": "mfootoken", "exponent": 6}]},
        {"name": "NOM", "symbol": "NOM", "base": "anom", "display": "nom","description":
        "Nom token", "denom_units": [{"denom": "anom", "exponent": 0}, {"denom": "nom",
        "exponent": 18}]}]
    );

    genesis["app_state"]["bank"]["denom_metadata"] = denom_metadata;
    let gov_period: Value = GOV_PERIOD.as_str().into();
    genesis["app_state"]["gov"]["voting_params"]["voting_period"] = gov_period.clone();
    genesis["app_state"]["gov"]["deposit_params"]["max_deposit_period"] = gov_period;

    let genesis_s = serde_json::to_string(&genesis)?;
    // write back, just reopen
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
    ])
    .await?;
    sh("cosmovisor run gravity collect-gentxs", &[]).await?;
    sh("cosmovisor run collect-gentxs", &[]).await?;

    // done preparing
    /*
    //let mut cosmovisor = Command::new("cosmovisor run start --inv-check-period
    // 1", &[]).run().await?;

    let upgrade_height = "10";
    let proposal_id = "1";
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

    let upgrade_version = ONOMY_UPGRADE_VERSION.as_str();
    let description = &format!("\"upgrade {upgrade_version}\"");
    sh(
        "cosmovisor run tx gov submit-proposal software-upgrade",
        &[
            [
                upgrade_version,
                "--title",
                description,
                "--description",
                description,
                "--upgrade-height",
                upgrade_height,
            ]
            .as_slice(),
            gas_args,
        ]
        .concat(),
    )
    .await?;
    sh(
        "cosmovisor run tx gov deposit",
        &[[proposal_id, &nom(2000.0)].as_slice(), gas_args].concat(),
    )
    .await?;
    sh(
        "cosmovisor run tx gov vote",
        &[[proposal_id, "yes"].as_slice(), gas_args].concat(),
    )
    .await?;*/

    //cosmovisor.terminate().await?;

    tokio::time::sleep(common::TIMEOUT).await;

    Ok(())
}
