use std::env;

use clap::Parser;
use common::{
    cosmovisor::{cosmovisor, cosmovisor_start, onomyd_setup, wait_for_height},
    ONE_SEC, TIMEOUT,
};
use lazy_static::lazy_static;
use log::info;
use super_orchestrator::{
    acquire_dir_path, acquire_file_path, close_file,
    docker::{Container, ContainerNetwork},
    get_separated_val,
    net_message::{wait_for_ok_lookup_host, NetMessenger},
    sh, std_init, Error, MapAddError, Result, STD_DELAY, STD_TRIES,
};
use tokio::{fs::OpenOptions, io::AsyncWriteExt, time::sleep};

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
    let volumes = &[("./logs", "/logs")];
    let mut cn = ContainerNetwork::new(
        "test",
        vec![
            Container::new(
                "onomyd",
                Some("./dockerfiles/onomyd.dockerfile"),
                None,
                &[],
                volumes,
                entrypoint,
                &["--entrypoint", "onomyd"],
            ),
            Container::new(
                "marketd",
                Some("./dockerfiles/marketd.dockerfile"),
                None,
                &[],
                volumes,
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
    let gov_period = "4s";
    let daemon_home = DAEMON_HOME.as_str();
    onomyd_setup(daemon_home, gov_period).await?;
    let mut cosmovisor_runner = cosmovisor_start("onomyd_runner.log").await?;

    let config_dir_path = acquire_dir_path(&format!("{daemon_home}/config")).await?;

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
            "revision_height": 5
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
    let mut proposal_file_path = config_dir_path.clone();
    proposal_file_path.push("consumer_add_proposal.json");
    let mut proposal_file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&proposal_file_path)
        .await?;
    proposal_file.write_all(proposal_s.as_bytes()).await?;
    close_file(proposal_file).await?;

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
        &[&[proposal_file_path.to_str().unwrap()], gas_args].concat(),
    )
    .await?;
    // the deposit is done as part of the chain addition proposal
    cosmovisor(
        "tx gov vote",
        &[[proposal_id, "yes"].as_slice(), gas_args].concat(),
    )
    .await?;

    // In the mean time get the consensus key assignement done

    // we can go ahead and get the consensus key assignment done (can this be done
    // later?)

    // we need to get the ed25519 consensus key
    // it seems the consensus key set is entirely separate TODO for now we just grab
    // any key

    // this is wrong on many levels, the design is not making this easy
    let tmp_s = get_separated_val(
        &cosmovisor("query tendermint-validator-set", &[]).await?,
        "\n",
        "value",
        ":",
    )?;
    let mut consensus_pubkey = r#"{"@type":"/cosmos.crypto.ed25519.PubKey","key":""#.to_owned();
    consensus_pubkey.push_str(&tmp_s);
    consensus_pubkey.push_str("\"}}");

    cosmovisor(
        "tx provider assign-consensus-key market",
        &[[consensus_pubkey.as_str()].as_slice(), gas_args].concat(),
    )
    .await?;

    wait_for_height(STD_TRIES, ONE_SEC, 5).await?;

    let consumer_genesis = cosmovisor("query provider consumer-genesis market", &[]).await?;
    // get us away from this worse-of-two-evils format
    let deserializer = serde_yaml::Deserializer::from_str(&consumer_genesis);
    let mut tmp = vec![];
    let mut serializer = serde_json::Serializer::new(&mut tmp);
    serde_transcode::transcode(deserializer, &mut serializer).unwrap();
    let consumer_genesis = String::from_utf8(tmp)
        .map_err(|e| Error::boxed(Box::new(e) as Box<dyn std::error::Error>))
        .map_add_err(|| ())?;

    info!("consumer_genesis:\n{consumer_genesis}\n\n");

    let host = "marketd:26000";
    wait_for_ok_lookup_host(STD_TRIES, STD_DELAY, host).await?;
    let mut nm = NetMessenger::connect(host, TIMEOUT).await?;
    nm.send::<String>(&consumer_genesis).await?;

    sleep(TIMEOUT).await;
    cosmovisor_runner.terminate().await?;
    Ok(())
}

async fn marketd_runner() -> Result<()> {
    let daemon_home = DAEMON_HOME.as_str();
    let chain_id = "market";
    cosmovisor("config chain-id", &[chain_id]).await?;
    cosmovisor("config keyring-backend test", &[]).await?;
    cosmovisor("init --overwrite", &[chain_id]).await?;

    let host = "0.0.0.0:26000";
    let mut nm = NetMessenger::listen_single_connect(host, TIMEOUT).await?;
    let genesis_s: String = nm.recv().await?;

    // overwrite genesis
    let genesis_file_path =
        acquire_file_path(&format!("{daemon_home}/config/genesis.json")).await?;
    let mut genesis_file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&genesis_file_path)
        .await?;
    genesis_file.write_all(genesis_s.as_bytes()).await?;
    close_file(genesis_file).await?;

    let mut cosmovisor_runner = cosmovisor_start("marketd_runner.log").await?;

    sleep(TIMEOUT).await;
    cosmovisor_runner.terminate().await?;
    Ok(())
}
