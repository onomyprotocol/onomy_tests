use std::time::Duration;

use onomy_test_lib::{
    cosmovisor::cosmovisor_start,
    dockerfiles::{COSMOVISOR, ONOMY_STD},
    onomy_std_init,
    setups::market_standalone_setup,
    super_orchestrator::{
        docker::{Container, ContainerNetwork, Dockerfile},
        sh,
        stacked_errors::{Error, Result, StackableErr},
        Command,
    },
    Args, TIMEOUT,
};
use tokio::time::sleep;

const CHAIN_ID: &str = "market";
const BINARY_NAME: &str = "marketd";
const BINARY_DIR: &str = ".market";
const VERSION: &str = "0.0.0";

#[rustfmt::skip]
fn standalone_dockerfile() -> String {
    let daemon_name = BINARY_NAME;
    let daemon_dir_name = BINARY_DIR;
    let version = VERSION;
    let dockerfile_resource = BINARY_NAME;
    format!(
        r#"{ONOMY_STD}
{COSMOVISOR}

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

    cn.run_all(true).await.stack()?;
    cn.wait_with_timeout_all(true, TIMEOUT).await.stack()?;
    cn.terminate_all().await;
    Ok(())
}

async fn standalone_runner(args: &Args) -> Result<()> {
    //sleep(TIMEOUT).await;
    let daemon_home = args.daemon_home.as_ref().stack()?;
    market_standalone_setup(daemon_home, CHAIN_ID)
        .await
        .stack()?;
    let mut cosmovisor_runner = cosmovisor_start("standalone_runner.log", None)
        .await
        .stack()?;

    sleep(Duration::ZERO).await;
    cosmovisor_runner.terminate(TIMEOUT).await.stack()?;

    Ok(())
}
