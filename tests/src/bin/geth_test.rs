//! example that we keep around for bridges

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
        wait_for_ok, Command, FileOptions,
    },
    Args, STD_DELAY, STD_TRIES, TIMEOUT,
};
use web30::client::Web3;

#[tokio::main]
async fn main() -> Result<()> {
    let args = onomy_std_init()?;

    if let Some(ref s) = args.entry_name {
        match s.as_str() {
            "geth" => geth_runner().await,
            "test" => test_runner(&args).await,
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
    sh([
        "cargo build --release --bin",
        bin_entrypoint,
        "--target",
        container_target,
        "--features",
        "geth",
    ])
    .await
    .stack()?;

    let entrypoint = &format!("./target/{container_target}/release/{bin_entrypoint}");

    let mut cn = ContainerNetwork::new(
        "test",
        vec![
            Container::new("geth", Dockerfile::contents(format!("{ONOMY_STD} {GETH}")))
                .external_entrypoint(entrypoint, ["--entry-name", "geth"])
                .await
                .stack()?,
            Container::new("test", Dockerfile::contents(ONOMY_STD.to_owned()))
                .external_entrypoint(entrypoint, ["--entry-name", "test"])
                .await
                .stack()?,
            /*Container::new(
                "prometheus",
                Dockerfile::NameTag("prom/prometheus:v2.44.0".to_owned()),
            )
            .create_args(&["-p", "9090:9090"]),*/
        ],
        Some(dockerfiles_dir),
        true,
        logs_dir,
    )
    .stack()?;
    cn.add_common_volumes([(logs_dir, "/logs")]);
    let uuid = cn.uuid_as_string();
    cn.add_common_entrypoint_args(["--uuid", &uuid]);
    cn.run_all(true).await.stack()?;
    cn.wait_with_timeout_all(true, TIMEOUT).await.stack()?;
    cn.terminate_all().await;
    Ok(())
}

async fn test_runner(args: &Args) -> Result<()> {
    let mut nm_geth =
        NetMessenger::connect(STD_TRIES, STD_DELAY, &format!("geth_{}:26000", args.uuid))
            .await
            .stack()?;

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
        .await.stack()?
        .text()
        .await.stack()?;
    info!(res);
    */

    let geth_url = &format!("http://geth_{}:8545", args.uuid);

    // requests using the `web30` crate
    let web3 = Web3::new(geth_url, Duration::from_secs(30));
    // `Web3::new` only waits for initial handshakes, we need to wait for Tcp and
    // syncing
    async fn is_eth_up(web3: &Web3) -> Result<()> {
        web3.eth_syncing().await.map(|_| ()).stack()
    }
    wait_for_ok(STD_TRIES, STD_DELAY, || is_eth_up(&web3))
        .await
        .stack()?;
    info!("geth is running");

    dbg!(web3
        .eth_get_balance(Address::from_str("0xBf660843528035a5A4921534E156a27e64B231fE").unwrap())
        .await
        .unwrap());

    // note: check out https://crates.io/crates/prometheus
    // for running your own Prometheus metrics client

    // terminate
    nm_geth.send::<()>(&()).await.stack()?;

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
    let mut nm_test = NetMessenger::listen("0.0.0.0:26000", TIMEOUT)
        .await
        .stack()?;

    let genesis_file = "/resources/eth_genesis.json";
    FileOptions::write_str(genesis_file, ETH_GENESIS)
        .await
        .stack()?;

    // the private key must not have the leading "0x"
    let private_key_no_0x = "b1bab011e03a9862664706fc3bbaa1b16651528e5f0e7fbfcbfdd8be302a13e7";
    let private_key_path = "/resources/test_private_key.txt";
    let test_password = "testpassword";
    let test_password_path = "/resources/test_password.txt";
    FileOptions::write_str(private_key_path, private_key_no_0x)
        .await
        .stack()?;
    FileOptions::write_str(test_password_path, test_password)
        .await
        .stack()?;

    sh([
        "geth account import --password",
        test_password_path,
        private_key_path,
    ])
    .await
    .stack()?;

    sh([
        "geth --identity \"testnet\" --networkid 15 init",
        genesis_file,
    ])
    .await
    .stack()?;

    let geth_log = FileOptions::write2("/logs", "geth_runner.log");
    let mut geth_runner = Command::new("geth")
        .args([
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
        .log(Some(geth_log))
        .run()
        .await
        .stack()?;

    // terminate
    nm_test.recv::<()>().await.stack()?;

    geth_runner.terminate().await.stack()?;
    Ok(())
}
