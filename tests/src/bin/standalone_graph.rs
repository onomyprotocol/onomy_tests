use std::time::Duration;

use common::{dockerfile_standalone_onexd, DOWNLOAD_STANDALONE_ONEXD, STANDALONE_ONEX_FH_VERSION};
use log::info;
use onomy_test_lib::{
    cosmovisor::{
        cosmovisor_get_addr, cosmovisor_get_balances, cosmovisor_start, fast_block_times,
        get_self_peer_info, set_persistent_peers, sh_cosmovisor, sh_cosmovisor_no_debug,
    },
    dockerfiles::{COSMOVISOR, ONOMY_STD},
    market::{CoinPair, Market},
    onomy_std_init,
    setups::market_standalone_setup,
    super_orchestrator::{
        docker::{Container, ContainerNetwork, Dockerfile},
        net_message::NetMessenger,
        sh,
        stacked_errors::{Error, Result, StackableErr},
        wait_for_ok, Command, FileOptions,
    },
    u64_array_bigints::{self, u256},
    Args, STD_DELAY, STD_TRIES, TIMEOUT,
};
use tokio::time::sleep;

// we use a normal onexd for the validator full node, but use the `-fh` version
// for the full node that indexes for firehose

const CHAIN_ID: &str = "market";
const BINARY_NAME: &str = "marketd";
const BINARY_DIR: &str = ".market";

const FIREHOSE_CONFIG_PATH: &str = "/firehose/firehose.yml";
const FIREHOSE_CONFIG: &str = r#"start:
    args:
        - reader
        - relayer
        - merger
        - firehose
    flags:
        common-first-streamable-block: 1
        reader-mode: node
        reader-node-path: /root/.market/cosmovisor/current/bin/marketd
        reader-node-args: start --x-crisis-skip-assert-invariants --home=/firehose
        reader-node-logs-filter: "module=(p2p|pex|consensus|x/bank|x/market)"
        relayer-max-source-latency: 99999h
        verbose: 1"#;

const CONFIG_TOML_PATH: &str = "/firehose/config/config.toml";
const EXTRACTOR_CONFIG: &str = r#"
[extractor]
enabled = true
output_file = "stdout"
"#;

const GRAPH_NODE_CONFIG_PATH: &str = "/graph_node_config.toml";
const GRAPH_NODE_CONFIG: &str = r#"[deployment]
[[deployment.rule]]
shard = "primary"
indexers = [ "index_node_cosmos_1" ]

[store]
[store.primary]
connection = "postgresql://postgres:root@postgres:5432/graph-node"
pool_size = 10

[chains]
ingestor = "block_ingestor_node"

[chains.market]
shard = "primary"
protocol = "cosmos"
provider = [
  { label = "market", details = { type = "firehose", url = "http://localhost:9030/" }},
]"#;

#[rustfmt::skip]
fn standalone_dockerfile() -> String {
    // use the fh version
    let version = STANDALONE_ONEX_FH_VERSION;
    let daemon_name = BINARY_NAME;
    let daemon_dir_name = BINARY_DIR;
    format!(
        r#"{ONOMY_STD}
# postgres and protobuf dependencies
RUN dnf install -y postgresql libpq-devel protobuf protobuf-compiler protobuf-devel
# for debug
RUN go install github.com/fullstorydev/grpcurl/cmd/grpcurl@latest
# for cosmovisor
{COSMOVISOR}

# interfacing with the running graph
RUN npm install -g @graphprotocol/graph-cli

# firehose
RUN git clone --depth 1 --branch v0.6.0 https://github.com/figment-networks/firehose-cosmos
# not working for me, too flaky
#RUN cd /firehose-cosmos && make install
ADD https://github.com/graphprotocol/firehose-cosmos/releases/download/v0.6.0/firecosmos_linux_amd64 /usr/bin/firecosmos
RUN chmod +x /usr/bin/firecosmos

# graph-node
RUN git clone --depth 1 --branch v0.32.0 https://github.com/graphprotocol/graph-node
RUN cd /graph-node && cargo build --release -p graph-node

# ipfs
ADD https://dist.ipfs.tech/kubo/v0.23.0/kubo_v0.23.0_linux-amd64.tar.gz /tmp/kubo.tar.gz
RUN cd /tmp && tar -xf /tmp/kubo.tar.gz && mv /tmp/kubo/ipfs /usr/bin/ipfs
RUN ipfs init

# our subgraph
RUN git clone https://github.com/onomyprotocol/mgraph
#ADD ./dockerfile_resources/mgraph /mgraph
RUN cd /mgraph && npm install && npm run build

ENV DAEMON_NAME="{daemon_name}"
ENV DAEMON_HOME="/root/{daemon_dir_name}"
ENV DAEMON_VERSION={version}

{DOWNLOAD_STANDALONE_ONEXD}

# for manual testing
RUN chmod +x $DAEMON_HOME/cosmovisor/genesis/$DAEMON_VERSION/bin/{daemon_name}

# set up symbolic links
RUN cosmovisor init $DAEMON_HOME/cosmovisor/genesis/$DAEMON_VERSION/bin/{daemon_name}

# some commands don't like if the data directory does not exist
RUN mkdir $DAEMON_HOME/data

RUN mkdir /firehose
RUN mkdir /firehose/data
"#
    )
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = onomy_std_init()?;

    if let Some(ref s) = args.entry_name {
        match s.as_str() {
            "standalone" => standalone_runner(&args).await,
            "onex_node" => onex_node(&args).await,
            _ => Err(Error::from(format!("entry_name \"{s}\" is not recognized"))),
        }
    } else {
        container_runner(&args).await.stack()
    }
}

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
    ])
    .await
    .stack()?;

    let entrypoint = &format!("./target/{container_target}/release/{bin_entrypoint}");

    // we use a normal onexd for the validator full node, but use the `-fh` version
    // for the full node that indexes for firehose
    let mut containers =
        vec![
            Container::new("standalone", Dockerfile::contents(standalone_dockerfile()))
                .external_entrypoint(entrypoint, ["--entry-name", "standalone"])
                .await
                .stack()?,
        ];
    containers.push(
        Container::new(
            "onex_node",
            Dockerfile::contents(dockerfile_standalone_onexd()),
        )
        .external_entrypoint(entrypoint, ["--entry-name", "onex_node"])
        .await
        .stack()?,
    );

    let mut cn =
        ContainerNetwork::new("test", containers, Some(dockerfiles_dir), true, logs_dir).stack()?;
    cn.add_common_volumes([(logs_dir, "/logs")]);
    let uuid = cn.uuid_as_string();
    cn.add_common_entrypoint_args(["--uuid", &uuid]);
    cn.add_container(
        Container::new("postgres", Dockerfile::name_tag("postgres:16"))
            .environment_vars([
                ("POSTGRES_PASSWORD", "root"),
                ("POSTGRES_USER", "postgres"),
                ("POSTGRES_DB", "graph-node"),
                ("POSTGRES_INITDB_ARGS", "-E UTF8 --locale=C"),
            ])
            .no_uuid_for_host_name(),
    )
    .stack()?;

    cn.run_all(true).await.stack()?;
    cn.wait_with_timeout_all(true, Duration::from_secs(9999))
        .await
        .stack()?;
    cn.terminate_all().await;
    Ok(())
}

async fn standalone_runner(args: &Args) -> Result<()> {
    let uuid = &args.uuid;

    let mut nm_onex_node =
        NetMessenger::connect(STD_TRIES, STD_DELAY, &format!("onex_node_{uuid}:26000"))
            .await
            .stack()?;

    let firehose_err_log = FileOptions::write2("/logs", "firehose_err.log");
    let firehose_std_log = FileOptions::write2("/logs", "firehose_std.log");
    let ipfs_log = FileOptions::write2("/logs", "ipfs.log");
    let graph_log = FileOptions::write2("/logs", "graph.log");

    let mut ipfs_runner = Command::new("ipfs daemon")
        .log(Some(ipfs_log))
        .run()
        .await
        .stack()?;

    FileOptions::write_str(GRAPH_NODE_CONFIG_PATH, GRAPH_NODE_CONFIG)
        .await
        .stack()?;

    FileOptions::write_str(FIREHOSE_CONFIG_PATH, FIREHOSE_CONFIG)
        .await
        .stack()?;

    async fn postgres_health(uuid: &str) -> Result<()> {
        Command::new(format!(
            "psql --host=postgres_{uuid} -U postgres --command=\\l"
        ))
        .env("PGPASSWORD", "root")
        .run_to_completion()
        .await
        .stack()?
        .assert_success()
        .stack()?;
        Ok(())
    }
    wait_for_ok(10, Duration::from_secs(1), || postgres_health(uuid))
        .await
        .stack()?;

    // not needed if the correct envs were passed to the postgres docker instance
    /*
    // setup the postgres database
    Command::new(
        "createdb --host=postgres -U postgres graph-node",
        &[],
    )
    .env("PGPASSWORD", "root")
    .run_to_completion()
    .await
    .stack()?.assert_success().stack()?;
    */

    sh_cosmovisor(["config chain-id --home /firehose", CHAIN_ID])
        .await
        .stack()?;
    sh_cosmovisor(["config keyring-backend test --home /firehose"])
        .await
        .stack()?;
    sh_cosmovisor_no_debug(["init --overwrite --home /firehose", CHAIN_ID])
        .await
        .stack()?;
    // TODO only for validators?
    fast_block_times("/firehose").await.stack()?;

    let (genesis_s, peer_info) = nm_onex_node.recv::<(String, String)>().await.stack()?;

    FileOptions::write_str("/firehose/config/genesis.json", &genesis_s)
        .await
        .stack()?;
    set_persistent_peers("/firehose", &[peer_info])
        .await
        .stack()?;

    // for debugging sync, firehose will run the node
    /*
    let mut cosmovisor_runner = cosmovisor_start(
        "standalone_runner.log",
        Some(CosmovisorOptions {
            wait_for_status_only: true,
            home: Some("/firehose".to_owned()),
            ..Default::default()
        }),
    )
    .await
    .stack()?;
    sleep(TIMEOUT).await;
    cosmovisor_runner.terminate(TIMEOUT).await.stack()?;
    */

    let mut config = FileOptions::read_to_string(CONFIG_TOML_PATH)
        .await
        .stack()?;
    config.push_str(EXTRACTOR_CONFIG);
    FileOptions::write_str(CONFIG_TOML_PATH, &config)
        .await
        .stack()?;

    let mut firecosmos_runner = Command::new(
        "firecosmos start --config /firehose/firehose.yml --data-dir /firehose/fh-data \
         --firehose-grpc-listen-addr 0.0.0.0:9030",
    )
    .stderr_log(Some(firehose_err_log))
    .stdout_log(Some(firehose_std_log))
    .run()
    .await
    .stack()?;

    // should see stuff from
    //grpcurl -plaintext -max-time 1 localhost:9030 sf.firehose.v2.Stream/Blocks

    async fn firecosmos_health() -> Result<()> {
        Command::new("curl -sL -w 200 http://localhost:9030 -o /dev/null")
            .run_to_completion()
            .await
            .stack()?
            .assert_success()
            .stack()?;
        Ok(())
    }
    wait_for_ok(100, Duration::from_secs(1), firecosmos_health)
        .await
        .stack()?;
    info!("firehose is up");

    let mut graph_runner = Command::new(format!(
        "cargo run --release -p graph-node -- --config {GRAPH_NODE_CONFIG_PATH} --ipfs \
         127.0.0.1:5001 --node-id index_node_cosmos_1"
    ))
    .cwd("/graph-node")
    .log(Some(graph_log))
    .run()
    .await
    .stack()?;

    async fn graph_node_health() -> Result<()> {
        Command::new("curl -sL -w 200 http://localhost:8020 -o /dev/null")
            .run_to_completion()
            .await
            .stack()?
            .assert_success()
            .stack()?;
        Ok(())
    }
    wait_for_ok(100, Duration::from_secs(1), graph_node_health)
        .await
        .stack()?;
    info!("graph-node is up");

    Command::new("npm run create-local")
        .cwd("/mgraph")
        .debug(true)
        .run_to_completion()
        .await
        .stack()?
        .assert_success()
        .stack()?;
    Command::new(
        "graph deploy --version-label v0.0.0 --node http://localhost:8020/ \
        --ipfs http://localhost:5001 onomyprotocol/mgraph"
    )
    .cwd("/mgraph")
    .debug(true)
    .run_to_completion()
    .await
    .stack()?.assert_success().stack()?;

    // grpcurl -plaintext -max-time 2 localhost:9030 sf.firehose.v2.Stream/Blocks
    // note: we may need to pass the proto files, I don't know if reflection is not
    // working and that's why it has errors

    sleep(Duration::from_secs(9999)).await;

    sleep(Duration::ZERO).await;
    graph_runner.terminate().await.stack()?;
    firecosmos_runner.terminate().await.stack()?;
    ipfs_runner.terminate().await.stack()?;

    Ok(())
}

async fn onex_node(args: &Args) -> Result<()> {
    let daemon_home = args.daemon_home.as_ref().stack()?;
    let uuid = &args.uuid;
    let mut nm_test = NetMessenger::listen("0.0.0.0:26000", TIMEOUT)
        .await
        .stack()?;

    market_standalone_setup(daemon_home, CHAIN_ID)
        .await
        .stack()?;

    let genesis_s = FileOptions::read_to_string(&format!("{daemon_home}/config/genesis.json"))
        .await
        .stack()?;
    let peer_info = get_self_peer_info(&format!("onex_node_{uuid}"), "26656")
        .await
        .stack()?;

    let mut cosmovisor_runner = cosmovisor_start(&format!("{CHAIN_ID}d_runner.log"), None).await?;

    // send genesis file, self peer info, and indicate that we are started
    nm_test
        .send::<(String, String)>(&(genesis_s, peer_info))
        .await
        .stack()?;

    let mut market = Market::new("validator", "1000000anative");
    market.max_gas = Some(u256!(1000000));

    let addr = &cosmovisor_get_addr("validator").await.stack()?;
    info!("{:?}", cosmovisor_get_balances(addr).await.stack()?);
    let coin_pair = CoinPair::new("afootoken", "anative").stack()?;

    // test numerical limits
    market
        .create_pool(&coin_pair, Market::MAX_COIN, Market::MAX_COIN)
        .await
        .stack()?;
    market
        .create_drop(&coin_pair, Market::MAX_COIN_SQUARED)
        .await
        .stack()?;
    market.show_pool(&coin_pair).await.stack()?;
    market.show_members(&coin_pair).await.stack()?;
    market
        .market_order(
            coin_pair.coin_a(),
            Market::MAX_COIN,
            coin_pair.coin_b(),
            Market::MAX_COIN,
            5000,
        )
        .await
        .stack()?;
    market.redeem_drop(1).await.stack()?;
    market
        .create_order(
            coin_pair.coin_a(),
            coin_pair.coin_b(),
            "stop",
            Market::MAX_COIN,
            (1100, 900),
            (0, 0),
        )
        .await
        .stack()?;
    market
        .create_order(
            coin_pair.coin_a(),
            coin_pair.coin_b(),
            "limit",
            Market::MAX_COIN,
            (1100, 900),
            (0, 0),
        )
        .await
        .stack()?;
    market.cancel_order(6).await.stack()?;

    sleep(Duration::from_secs(9999)).await;

    //http://192.168.208.3:8000/subgraphs/name/onomyprotocol/mgraph

    cosmovisor_runner.terminate(TIMEOUT).await.stack()?;

    Ok(())
}
