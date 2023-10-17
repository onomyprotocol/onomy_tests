use std::time::Duration;

use onomy_test_lib::{
    dockerfiles::{COSMOVISOR, ONOMY_STD},
    onomy_std_init,
    setups::market_standalone_setup,
    super_orchestrator::{
        docker::{Container, ContainerNetwork, Dockerfile},
        sh,
        stacked_errors::{Error, Result, StackableErr},
        wait_for_ok, Command, FileOptions,
    },
    Args, TIMEOUT,
};
use tokio::time::sleep;

// NOTE this will not work without the `-fh` patch, change the build to use the
// `v1.1.0-fh` market repo version of the binary

const CHAIN_ID: &str = "market";
const BINARY_NAME: &str = "marketd";
const BINARY_DIR: &str = ".market";
const VERSION: &str = "0.0.0";

const FIREHOSE_CONFIG_PATH: &str = "/root/.market/config/firehose.yml";
const FIREHOSE_CONFIG: &str = r#"start:
    args:
        - reader
        - relayer
        - merger
        - firehose
    flags:
        common-first-streamable-block: 1
        common-live-blocks-addr:
        reader-mode: node
        reader-node-path: /root/.market/cosmovisor/current/bin/marketd
        reader-node-args: start --x-crisis-skip-assert-invariants --home=/root/.market
        reader-node-logs-filter: "module=(p2p|pex|consensus|x/bank|x/market)"
        relayer-max-source-latency: 99999h
        verbose: 1"#;

const CONFIG_TOML_PATH: &str = "/root/.market/config/config.toml";
const EXTRACTOR_CONFIG: &str = r#"
[extractor]
enabled = true
output_file = "stdout"
"#;

#[rustfmt::skip]
fn standalone_dockerfile() -> String {
    let daemon_name = BINARY_NAME;
    let daemon_dir_name = BINARY_DIR;
    let version = VERSION;
    let dockerfile_resource = BINARY_NAME;
    format!(
        r#"{ONOMY_STD}
# for psql for commands to the postgres container
RUN dnf install -y postgresql
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

ENV DAEMON_NAME="{daemon_name}"
ENV DAEMON_HOME="/root/{daemon_dir_name}"
ENV DAEMON_VERSION={version}

ADD ./dockerfile_resources/{dockerfile_resource} $DAEMON_HOME/cosmovisor/genesis/$DAEMON_VERSION/bin/{daemon_name}

# for manual testing
RUN chmod +x $DAEMON_HOME/cosmovisor/genesis/$DAEMON_VERSION/bin/{daemon_name}

# set up symbolic links
RUN cosmovisor init $DAEMON_HOME/cosmovisor/genesis/$DAEMON_VERSION/bin/{daemon_name}

# some commands don't like if the data directory does not exist
RUN mkdir $DAEMON_HOME/data
"#
    )
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = onomy_std_init()?;

    if let Some(ref s) = args.entry_name {
        match s.as_str() {
            "standalone" => standalone_runner(&args).await,
            _ => Err(Error::from(format!("entry_name \"{s}\" is not recognized"))),
        }
    } else {
        let comres = Command::new(&format!("go build ./cmd/{BINARY_NAME}"), &[])
            .ci_mode(true)
            .cwd("./../market/")
            .run_to_completion()
            .await
            .stack()?;
        comres.assert_success().stack()?;
        // copy to dockerfile resources (docker cannot use files from outside cwd)
        sh(
            &format!(
                "cp ./../market/{BINARY_NAME} \
                 ./tests/dockerfiles/dockerfile_resources/{BINARY_NAME}"
            ),
            &[],
        )
        .await
        .stack()?;
        container_runner(&args).await.stack()
    }
}

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
    ])
    .await
    .stack()?;

    let entrypoint = Some(format!(
        "./target/{container_target}/release/{bin_entrypoint}"
    ));
    let entrypoint = entrypoint.as_deref();

    let mut cn = ContainerNetwork::new(
        "test",
        vec![Container::new(
            "standalone",
            Dockerfile::Contents(standalone_dockerfile()),
            entrypoint,
            &["--entry-name", "standalone"],
        )],
        Some(dockerfiles_dir),
        true,
        logs_dir,
    )
    .stack()?;
    cn.add_common_volumes(&[(logs_dir, "/logs")]);
    let uuid = cn.uuid_as_string();
    cn.add_common_entrypoint_args(&["--uuid", &uuid]);
    cn.add_container(
        Container::new(
            "postgres",
            Dockerfile::NameTag("postgres:16".to_owned()),
            None,
            &[],
        )
        .environment_vars(&[("POSTGRES_PASSWORD", "root"), ("POSTGRES_USER", "postgres")]),
    )
    .stack()?;

    cn.run_all(true).await.stack()?;
    cn.wait_with_timeout_all(true, TIMEOUT).await.stack()?;
    cn.terminate_all().await;
    Ok(())
}

async fn standalone_runner(args: &Args) -> Result<()> {
    let daemon_home = args.daemon_home.as_ref().stack()?;
    let uuid = &args.uuid;
    let firehose_log = FileOptions::write2("/logs", "firehose.log");

    market_standalone_setup(daemon_home, CHAIN_ID)
        .await
        .stack()?;

    async fn postgres_health(uuid: &str) -> Result<()> {
        let comres = Command::new(
            &format!("psql --host=postgres_{uuid} -U postgres --command=\\l"),
            &[],
        )
        .env("PGPASSWORD", "root")
        .run_to_completion()
        .await
        .stack()?;
        comres.assert_success().stack()?;
        Ok(())
    }
    wait_for_ok(10, Duration::from_secs(1), || postgres_health(uuid))
        .await
        .stack()?;

    // setup the postgres database
    let comres = Command::new(
        &format!("createdb --host=postgres_{uuid} -U postgres graph-node"),
        &[],
    )
    .env("PGPASSWORD", "root")
    .run_to_completion()
    .await
    .stack()?;
    comres.assert_success().stack()?;

    let mut config = FileOptions::read_to_string(CONFIG_TOML_PATH)
        .await
        .stack()?;
    config.push_str(EXTRACTOR_CONFIG);
    FileOptions::write_str(CONFIG_TOML_PATH, &config)
        .await
        .stack()?;

    FileOptions::write_str(FIREHOSE_CONFIG_PATH, FIREHOSE_CONFIG)
        .await
        .stack()?;

    // TODO translate into running a validator node and then the firecosmos just
    // runs a querying full node
    /*let mut cosmovisor_runner = cosmovisor_start("standalone_runner.log", None)
    .await
    .stack()?;*/
    //cosmovisor_runner.terminate(TIMEOUT).await.stack()?;

    let mut firecosmos_runner = Command::new(
        &format!(
            "firecosmos start --config {daemon_home}/config/firehose.yml --data-dir \
             {daemon_home}/fh-data"
        ),
        &[],
    )
    .stderr_log(&firehose_log)
    .stdout_log(&firehose_log)
    .run()
    .await
    .stack()?;

    // grpcurl -plaintext -max-time 2 localhost:9030 sf.firehose.v2.Stream/Blocks
    // note: we may need to pass the proto files, I don't know if reflection is not
    // working and that's why it has errors

    sleep(TIMEOUT).await;

    sleep(Duration::ZERO).await;
    firecosmos_runner.terminate().await.stack()?;

    Ok(())
}
