use std::env;

use clap::Parser;
use common::{
    cosmovisor::{cosmovisor, cosmovisor_start, onomyd_setup, wait_for_height},
    ONE_SEC, TIMEOUT,
};
use lazy_static::lazy_static;
use log::info;
use serde_json::Value;
use super_orchestrator::{
    docker::{Container, ContainerNetwork},
    get_separated_val,
    net_message::NetMessenger,
    sh, std_init, FileOptions, MapAddError, Result, STD_DELAY, STD_TRIES,
};
use tokio::{fs::remove_file, time::sleep};

lazy_static! {
    static ref DAEMON_NAME: String = env::var("DAEMON_NAME").unwrap();
    static ref DAEMON_HOME: String = env::var("DAEMON_HOME").unwrap();
}

/// Runs ics_basic
#[derive(Parser, Debug)]
#[command(about)]
struct Args {
    /// If left `None`, the container runner program runs, otherwise this
    /// specifies the entrypoint to run
    #[arg(short, long)]
    entrypoint: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    std_init()?;
    let args = Args::parse();

    if let Some(ref s) = args.entrypoint {
        match s.as_str() {
            "onomyd" => onomyd_runner().await,
            "marketd" => marketd_runner().await,
            _ => format!("entrypoint \"{s}\" is not recognized").map_add_err(|| ()),
        }
    } else {
        container_runner().await
    }
}

async fn container_runner() -> Result<()> {
    let container_target = "x86_64-unknown-linux-gnu";
    let logs_dir = "./logs";
    let this_bin = "ics_basic";

    // build internal runner with `--release`
    sh("cargo build --release --bin", &[
        this_bin,
        "--target",
        container_target,
    ])
    .await?;

    // FIXME fall back to 971000347e9dce20e27b37208a0305c27ef1c458

    // build binaries
    /*
        sh("make --directory ./../onomy_workspace0/onomy/ build", &[]).await?;
        sh("make --directory ./../market/ build", &[]).await?;
        // copy to dockerfile resources (docker cannot use files from outside cwd)
        sh(
            "cp ./../onomy_workspace0/onomy/onomyd ./dockerfiles/dockerfile_resources/onomyd",
            &[],
        )
        .await?;
        sh(
            "cp ./../market/marketd ./dockerfiles/dockerfile_resources/marketd",
            &[],
        )
        .await?;
    */

    let entrypoint = &format!("./target/{container_target}/release/{this_bin}");
    let volumes = vec![
        ("./logs", "/logs"),
        ("./resources/config.toml", "/root/.hermes/config.toml"),
        ("./resources/mnemonic.txt", "/root/.hermes/mnemonic.txt"),
    ];
    // TODO is this how we should share keys?
    // remove files
    remove_file("./resources/keyring-test/validator.info").await?;
    let mut onomyd_volumes = volumes.clone();
    onomyd_volumes.push(("./resources/keyring-test", "/root/.onomy/keyring-test"));
    let mut marketd_volumes = volumes.clone();
    marketd_volumes.push((
        "./resources/keyring-test",
        "/root/.onomy_market/keyring-test",
    ));
    let mut cn = ContainerNetwork::new(
        "test",
        vec![
            Container::new(
                "onomyd",
                Some("./dockerfiles/onomyd.dockerfile"),
                None,
                &[],
                &onomyd_volumes,
                entrypoint,
                &["--entrypoint", "onomyd"],
            ),
            Container::new(
                "marketd",
                Some("./dockerfiles/marketd.dockerfile"),
                None,
                &[],
                &marketd_volumes,
                entrypoint,
                &["--entrypoint", "marketd"],
            ),
        ],
        true,
        logs_dir,
    )?;
    cn.run_all(true).await?;
    cn.wait_with_timeout_all(true, TIMEOUT).await?;
    Ok(())
}

async fn onomyd_runner() -> Result<()> {
    //let hostname = "onomyd";
    let consumer_hostname = "marketd:26000";
    let mut nm = NetMessenger::connect(STD_TRIES, STD_DELAY, consumer_hostname)
        .await
        .map_add_err(|| ())?;

    let gov_period = "4s";
    let daemon_home = DAEMON_HOME.as_str();
    onomyd_setup(daemon_home, gov_period).await?;

    let mut cosmovisor_runner = cosmovisor_start("onomyd_runner.log", true, None).await?;

    // TODO stepsStartConsumerChain

    let proposal_id = "1";

    /*
    // note: must somehow get the hashes and spawn time in, may need to change height
        {
            "title": "Propose the addition of a new chain",
            "description": "add consumer chain market",
            "chain_id": "market",
            "initial_height": {
                "revision_height": 1
            },
            "genesis_hash": "Z2VuX2hhc2g=",
            "binary_hash": "YmluX2hhc2g=",
            "spawn_time": "2023-05-18T01:15:49.83019476-05:00",
            "consumer_redistribution_fraction": "0.75",
            "blocks_per_distribution_transmission": 1000,
            "historical_entries": 10000,
            "ccv_timeout_period": 2419200000000000,
            "transfer_timeout_period": 3600000000000,
            "unbonding_period": 1728000000000000,
            "deposit": "2000000000000000000000anom"
        }
     */

    // `json!` doesn't like large literals beyond i32
    let proposal_s = r#"{
        "title": "Propose the addition of a new chain",
        "description": "add consumer chain market",
        "chain_id": "market",
        "initial_height": {
            "revision_number": 0,
            "revision_height": 1
        },
        "genesis_hash": "Z2VuX2hhc2g=",
        "binary_hash": "YmluX2hhc2g=",
        "spawn_time": "2023-05-18T01:15:49.83019476-05:00",
        "consumer_redistribution_fraction": "0.75",
        "blocks_per_distribution_transmission": 1000,
        "historical_entries": 10000,
        "ccv_timeout_period": 2419200000000000,
        "transfer_timeout_period": 3600000000000,
        "unbonding_period": 1728000000000000,
        "deposit": "2000000000000000000000anom"
    }"#;
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
    cosmovisor(
        "tx gov submit-proposal consumer-addition",
        &[&[proposal_file_path.as_str()], gas_args].concat(),
    )
    .await?;
    // the deposit is done as part of the chain addition proposal
    cosmovisor(
        "tx gov vote",
        &[[proposal_id, "yes"].as_slice(), gas_args].concat(),
    )
    .await?;

    // In the mean time get the hermes setup and consensus key assignement done

    // this uses what is set by `onomyd_setup`
    sh(
        "hermes keys add --chain onomy --mnemonic-file /root/.hermes/mnemonic.txt",
        &[],
    )
    .await?;

    // FIXME this should be from $DAEMON_HOME/config/priv_validator_key.json, not
    // some random thing from teh validator set
    let tmp_s = get_separated_val(
        &cosmovisor("query tendermint-validator-set", &[]).await?,
        "\n",
        "value",
        ":",
    )?;
    let mut consensus_pubkey = r#"{"@type":"/cosmos.crypto.ed25519.PubKey","key":""#.to_owned();
    consensus_pubkey.push_str(&tmp_s);
    consensus_pubkey.push_str("\"}}");

    println!("ccvkey: {consensus_pubkey}");

    // do this before getting the consumer-genesis
    cosmovisor(
        "tx provider assign-consensus-key market",
        &[[consensus_pubkey.as_str()].as_slice(), gas_args].concat(),
    )
    .await?;

    wait_for_height(STD_TRIES, ONE_SEC, 5).await?;

    let ccvconsumer_state =
        cosmovisor("query provider consumer-genesis market -o json", &[]).await?;

    info!("ccvconsumer_state:\n{ccvconsumer_state}\n\n");

    // send to `marketd`
    nm.send::<String>(&ccvconsumer_state).await?;

    let genesis_s =
        FileOptions::read_to_string(&format!("{daemon_home}/config/genesis.json")).await?;
    //println!("genesis: {genesis_s}");
    let genesis: Value = serde_json::from_str(&genesis_s)?;
    nm.send::<String>(&genesis["app_state"]["auth"]["accounts"].to_string())
        .await?;
    nm.send::<String>(&genesis["app_state"]["bank"].to_string())
        .await?;
    nm.send::<String>(
        &FileOptions::read_to_string(&format!("{daemon_home}/config/node_key.json")).await?,
    )
    .await?;
    nm.send::<String>(
        &FileOptions::read_to_string(&format!("{daemon_home}/config/priv_validator_key.json"))
            .await?,
    )
    .await?;

    sleep(TIMEOUT).await;
    cosmovisor_runner.terminate().await?;
    Ok(())
}

async fn marketd_runner() -> Result<()> {
    let listen = "0.0.0.0:26000";
    let mut nm = NetMessenger::listen_single_connect(listen, TIMEOUT).await?;

    let daemon_home = DAEMON_HOME.as_str();
    let chain_id = "market";
    cosmovisor("config chain-id", &[chain_id]).await?;
    cosmovisor("config keyring-backend test", &[]).await?;
    cosmovisor("init --overwrite", &[chain_id]).await?;
    let genesis_file_path = format!("{daemon_home}/config/genesis.json");

    let ccvconsumer_state_s: String = nm.recv().await?;
    let ccvconsumer_state: Value = serde_json::from_str(&ccvconsumer_state_s)?;

    let accounts_s: String = nm.recv().await?;
    let accounts: Value = serde_json::from_str(&accounts_s)?;

    let bank_s: String = nm.recv().await?;
    let bank: Value = serde_json::from_str(&bank_s)?;

    // add `ccvconsumer_state` to genesis

    let genesis_s = FileOptions::read_to_string(&genesis_file_path).await?;

    let mut genesis: Value = serde_json::from_str(&genesis_s)?;
    genesis["app_state"]["ccvconsumer"] = ccvconsumer_state;
    genesis["app_state"]["auth"]["accounts"] = accounts;
    genesis["app_state"]["bank"] = bank;
    let genesis_s = genesis.to_string();

    info!("genesis: {genesis_s}");

    FileOptions::write_str(&genesis_file_path, &genesis_s).await?;

    FileOptions::write_str(
        &format!("{daemon_home}/config/node_key.json"),
        &nm.recv::<String>().await?,
    )
    .await?;
    FileOptions::write_str(
        &format!("{daemon_home}/config/priv_validator_key.json"),
        &nm.recv::<String>().await?,
    )
    .await?;

    let mut cosmovisor_runner = cosmovisor_start("marketd_runner.log", true, None).await?;

    // need
    // $DAEMON_HOME/data/priv_validator_state.json # good
    // $DAEMON_HOME/config/node_key.json #
    // {"priv_key":{"type":"tendermint/PrivKeyEd25519","value":"
    // ZPJdDf6VM0AOpp9RA4o1TWfsJ8FlKAqRYetfz+JY6k0ocKr28vNQyxMM2XLVCl38XoSkqSjaxH4aJaXt98nKGw=="
    // }} $DAEMON_HOME/config/priv_validator_key.json
    // {
    //   "address": "B84FF2E45DC827E00316F7E521DC326D85025916",
    //   "pub_key": {
    //     "type": "tendermint/PubKeyEd25519",
    //     "value": "w0tuY2qwu+uMA6eS430yEJITssJgGAXsyCFCbNkKM7g="
    //   },
    //   "priv_key": {
    //     "type": "tendermint/PrivKeyEd25519",
    //     "value":
    // "72qperDW+FVH+uxpDCC1HeRtGDW46UdroUqLL1Eoaj7DS25jarC764wDp5LjfTIQkhOywmAYBezIIUJs2QozuA=="
    // }
    // also need keyring (/keyring-test/) to be able to do transactions

    // verified:

    // #`07-tendermint0` is created automatically to interface with the market chain
    // # this should return something after onomyd proposals have passed
    //hermes query client state --chain onomy --client 07-tendermint-0

    // # start out empty
    //hermes query channels --chain onomy
    //hermes query connections --chain onomy

    // #ClientChain {
    // #    client_id: ClientId(
    // #        "07-tendermint-0",
    // #    ),
    // #    chain_id: ChainId {
    // #        id: "market",
    // #        version: 0,
    // #    },
    // #}
    //hermes query clients --host-chain onomy

    //hermes query client connections --chain onomy --client 07-tendermint-0
    //hermes query client consensus --chain onomy --client 07-tendermint-0
    //hermes query client state --chain onomy --client 07-tendermint-0
    //hermes query client status --chain onomy --client 07-tendermint-0

    // # this is done on onomyd
    //sh("hermes keys add --chain onomy --mnemonic-file
    // /root/.hermes/mnemonic.txt", &[]).await?;

    //hermes query clients --host-chain market

    // end verified

    // # this should be it
    // hermes create connection --a-chain market --a-client 07-tendermint-0
    // --b-client 07-tendermint-0

    //hermes query client state --chain market --client 07-tendermint-0

    // hermes create client --host-chain onomy --reference-chain market
    // hermes create client --host-chain market --reference-chain onomy
    // hermes query client state --chain onomy --client 07-tendermint-0
    // hermes query client state --chain market --client 07-tendermint-0

    //  hermes create connection --a-chain onomy --a-client 07-tendermint-0
    // --b-client 07-tendermint-0

    // hermes create connection --a-chain market --a-client 07-tendermint-0
    // --b-client 07-tendermint-1 TODO check connection num
    // hermes create channel --order ordered --a-chain market --a-connection
    // connection-0 --a-port consumer --b-port provider --channel-version 1
    // hermes start

    // hermes tx chan-open-try --dst-chain onomy --src-chain market --dst-connection
    // connection-0 --dst-port provider --src-port consumer --src-channel channel-0

    sleep(TIMEOUT).await;
    cosmovisor_runner.terminate().await?;
    Ok(())
}
