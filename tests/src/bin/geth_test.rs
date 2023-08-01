use std::{str::FromStr, time::Duration};

use clarity::Address;
use log::info;
use onomy_test_lib::{
    dockerfiles::ONOMY_STD,
    onomy_std_init,
    super_orchestrator::{
        docker::{Container, ContainerNetwork, Dockerfile},
        net_message::NetMessenger,
        sh,
        stacked_errors::{Error, Result, StackableErr},
        wait_for_ok, Command, FileOptions, STD_DELAY, STD_TRIES,
    },
    Args, TIMEOUT,
};
use web30::client::Web3;

#[tokio::main]
async fn main() -> Result<()> {
    let args = onomy_std_init()?;

    if let Some(ref s) = args.entry_name {
        match s.as_str() {
            "geth" => geth_runner().await,
            "test" => test_runner().await,
            _ => Err(Error::from(format!("entry_name \"{s}\" is not recognized"))),
        }
    } else {
        container_runner(&args).await
    }
}

#[rustfmt::skip]
const GETH: &str = r#"ADD https://gethstore.blob.core.windows.net/builds/geth-linux-amd64-1.12.0-e501b3b0.tar.gz /tmp/geth.tar.gz
RUN cd /tmp && tar -xvf * && mv /tmp/geth-linux-amd64-1.12.0-e501b3b0/geth /usr/bin/geth

RUN mkdir /resources
"#;

async fn container_runner(args: &Args) -> Result<()> {
    let logs_dir = "./tests/logs";
    let dockerfiles_dir = "./tests/dockerfiles";
    let bin_entrypoint = &args.bin_name;
    let container_target = "x86_64-unknown-linux-gnu";

    // build internal runner with `--release`
    sh("cargo build --release --bin", &[
        bin_entrypoint,
        "--target",
        container_target,
        "--features",
        "geth",
    ])
    .await?;

    let entrypoint = Some(format!(
        "./target/{container_target}/release/{bin_entrypoint}"
    ));
    let entrypoint = entrypoint.as_deref();

    let mut cn = ContainerNetwork::new(
        "test",
        vec![
            Container::new(
                "geth",
                Dockerfile::Contents(format!("{ONOMY_STD} {GETH}")),
                entrypoint,
                &["--entry-name", "geth"],
            ),
            Container::new(
                "test",
                Dockerfile::Contents(ONOMY_STD.to_owned()),
                entrypoint,
                &["--entry-name", "test"],
            ),
            /*Container::new(
                "prometheus",
                Dockerfile::NameTag("prom/prometheus:v2.44.0".to_owned()),
                None,
                &[],
            )
            .create_args(&["-p", "9090:9090"]),*/
        ],
        Some(dockerfiles_dir),
        true,
        logs_dir,
    )?
    .add_common_volumes(&[(logs_dir, "/logs")]);
    cn.run_all(true).await?;
    cn.wait_with_timeout_all(true, TIMEOUT).await?;
    Ok(())
}

async fn test_runner() -> Result<()> {
    let mut nm_geth = NetMessenger::connect(STD_TRIES, STD_DELAY, "geth:26000").await?;

    // manual HTTP request
    /*
    curl --header "content-type: application/json" --data
    '{"id":1,"jsonrpc":"2.0","method":"eth_syncing","params":[]}' http://geth:8545
    */

    // programmatic HTTP request
    /*
    sleep(Duration::from_secs(5)).await;
    let geth_client = reqwest::Client::new();
    let res = geth_client
        .post("http://geth:8545")
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/json",
        )
        .body(r#"{"method":"eth_blockNumber","params":[],"id":1,"jsonrpc":"2.0"}"#)
        .send()
        .await?
        .text()
        .await?;
    info!(res);
    */

    // requests using the `web30` crate
    let web3 = Web3::new("http://geth:8545", Duration::from_secs(30));
    // `Web3::new` only waits for initial handshakes, we need to wait for Tcp and
    // syncing
    async fn is_eth_up(web3: &Web3) -> Result<()> {
        web3.eth_syncing()
            .await
            .map(|_| ())
            .map_err(|e| Error::boxed(Box::new(e)))
    }
    wait_for_ok(STD_TRIES, STD_DELAY, || is_eth_up(&web3)).await?;
    info!("geth is running");

    dbg!(web3
        .eth_get_balance(Address::from_str("0xBf660843528035a5A4921534E156a27e64B231fE").unwrap())
        .await
        .unwrap());

    // note: check out https://crates.io/crates/prometheus
    // for running your own Prometheus metrics client

    // terminate
    nm_geth.send::<()>(&()).await?;

    Ok(())
}

#[rustfmt::skip]
const ETH_GENESIS: &str = r#"
{
    "config": {
      "chainId": 15,
      "homesteadBlock": 0,
      "eip150Block": 0,
      "eip155Block": 0,
      "eip158Block": 0,
      "byzantiumBlock": 0,
      "constantinopleBlock": 0,
      "petersburgBlock": 0,
      "istanbulBlock": 0,
      "berlinBlock": 0,
      "clique": {
        "period": 1,
        "epoch": 30000
      }
    },
    "difficulty": "1",
    "gasLimit": "8000000",
    "extradata": "0x0000000000000000000000000000000000000000000000000000000000000000Bf660843528035a5A4921534E156a27e64B231fE0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
    "alloc": {
      "0xBf660843528035a5A4921534E156a27e64B231fE": { "balance": "0x1337000000000000000000" }
    }
}
"#;

async fn geth_runner() -> Result<()> {
    let mut nm_test = NetMessenger::listen_single_connect("0.0.0.0:26000", TIMEOUT).await?;

    let genesis_file = "/resources/eth_genesis.json";
    FileOptions::write_str(genesis_file, ETH_GENESIS).await?;

    // the private key must not have the leading "0x"
    let private_key_no_0x = "b1bab011e03a9862664706fc3bbaa1b16651528e5f0e7fbfcbfdd8be302a13e7";
    let private_key_path = "/resources/test_private_key.txt";
    let test_password = "testpassword";
    let test_password_path = "/resources/test_password.txt";
    FileOptions::write_str(private_key_path, private_key_no_0x).await?;
    FileOptions::write_str(test_password_path, test_password).await?;

    sh("geth account import --password", &[
        test_password_path,
        private_key_path,
    ])
    .await?;

    sh("geth --identity \"testnet\" --networkid 15 init", &[
        genesis_file,
    ])
    .await?;

    let geth_log = FileOptions::write2("/logs", "geth_runner.log");
    let mut geth_runner = Command::new("geth", &[
        "--nodiscover",
        "--allow-insecure-unlock",
        "--unlock",
        "0xBf660843528035a5A4921534E156a27e64B231fE",
        "--password",
        test_password_path,
        "--mine",
        "--miner.etherbase",
        "0xBf660843528035a5A4921534E156a27e64B231fE",
        "--http",
        "--http.addr",
        "0.0.0.0",
        "--http.vhosts",
        "*",
        "--http.corsdomain",
        "*",
        "--nousb",
        "--verbosity",
        "4",
        // TODO --metrics.
    ])
    .stderr_log(&geth_log)
    .stdout_log(&geth_log)
    .run()
    .await?;

    // terminate
    nm_test.recv::<()>().await?;

    geth_runner.terminate().await?;
    Ok(())
}
