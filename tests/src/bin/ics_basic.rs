use onomy_test_lib::{
    cosmovisor::{cosmovisor_start, onomyd_setup, sh_cosmovisor_no_dbg},
    cosmovisor_ics::{cosmovisor_add_consumer, marketd_setup},
    hermes::{hermes_start, sh_hermes},
    ibc::IbcPair,
    onomy_std_init,
    super_orchestrator::{
        docker::{Container, ContainerNetwork},
        net_message::NetMessenger,
        remove_files_in_dir, sh,
        stacked_errors::{MapAddError, Result},
        FileOptions, STD_DELAY, STD_TRIES,
    },
    Args, TIMEOUT,
};

#[tokio::main]
async fn main() -> Result<()> {
    let args = onomy_std_init()?;

    if let Some(ref s) = args.entry_name {
        match s.as_str() {
            "onomyd" => onomyd_runner(&args).await,
            "marketd" => marketd_runner(&args).await,
            "hermes" => hermes_runner().await,
            _ => format!("entry_name \"{s}\" is not recognized").map_add_err(|| ()),
        }
    } else {
        sh("make --directory ./../onomy/ build", &[]).await?;
        sh("make --directory ./../market/ build", &[]).await?;
        // copy to dockerfile resources (docker cannot use files from outside cwd)
        sh(
            "cp ./../onomy/onomyd ./tests/dockerfiles/dockerfile_resources/onomyd",
            &[],
        )
        .await?;
        sh(
            "cp ./../market/marketd ./tests/dockerfiles/dockerfile_resources/marketd",
            &[],
        )
        .await?;
        container_runner(&args).await
    }
}

async fn container_runner(args: &Args) -> Result<()> {
    let bin_entrypoint = &args.bin_name;
    let container_target = "x86_64-unknown-linux-gnu";
    let logs_dir = "./tests/logs";

    // build internal runner with `--release`
    sh("cargo build --release --bin", &[
        bin_entrypoint,
        "--target",
        container_target,
    ])
    .await?;

    // prepare volumed resources
    remove_files_in_dir("./tests/resources/keyring-test/", &["address", "info"]).await?;

    let entrypoint = Some(format!(
        "./target/{container_target}/release/{bin_entrypoint}"
    ));
    let entrypoint = entrypoint.as_deref();
    let volumes = vec![(logs_dir, "/logs")];
    let mut onomyd_volumes = volumes.clone();
    let mut consumer_volumes = volumes.clone();
    onomyd_volumes.push((
        "./tests/resources/keyring-test",
        "/root/.onomy/keyring-test",
    ));
    consumer_volumes.push((
        "./tests/resources/keyring-test",
        "/root/.onomy_market/keyring-test",
    ));

    let mut cn = ContainerNetwork::new(
        "test",
        vec![
            Container::new(
                "hermes",
                Some("./tests/dockerfiles/hermes.dockerfile"),
                None,
                &volumes,
                entrypoint,
                &["--entry-name", "hermes"],
            ),
            Container::new(
                "onomyd",
                Some("./tests/dockerfiles/onomyd.dockerfile"),
                None,
                &onomyd_volumes,
                entrypoint,
                &["--entry-name", "onomyd"],
            ),
            Container::new(
                "marketd",
                Some("./tests/dockerfiles/marketd.dockerfile"),
                None,
                &consumer_volumes,
                entrypoint,
                &["--entry-name", "marketd"],
            ),
        ],
        true,
        logs_dir,
    )?;
    cn.run_all(true).await?;
    cn.wait_with_timeout_all(true, TIMEOUT).await?;
    Ok(())
}

async fn hermes_runner() -> Result<()> {
    let mut nm_onomyd = NetMessenger::listen_single_connect("0.0.0.0:26000", TIMEOUT).await?;

    // get mnemonic from onomyd
    let mnemonic: String = nm_onomyd.recv().await?;
    // set keys for our chains
    FileOptions::write_str("/root/.hermes/mnemonic.txt", &mnemonic).await?;
    sh_hermes(
        "keys add --chain onomy --mnemonic-file /root/.hermes/mnemonic.txt",
        &[],
    )
    .await?;
    sh_hermes(
        "keys add --chain market --mnemonic-file /root/.hermes/mnemonic.txt",
        &[],
    )
    .await?;

    // wait for setup
    nm_onomyd.recv::<()>().await?;

    let ibc_pair = IbcPair::hermes_setup_pair("market", "onomy").await?;
    let mut hermes_runner = hermes_start().await?;
    ibc_pair.hermes_check_acks().await?;

    // tell that chains have been connected
    nm_onomyd.send::<()>(&()).await?;

    hermes_runner.terminate(TIMEOUT).await?;
    Ok(())
}

async fn onomyd_runner(args: &Args) -> Result<()> {
    let consumer_id = "market";
    let daemon_home = args.daemon_home.as_ref().map_add_err(|| ())?;
    let mut nm_hermes = NetMessenger::connect(STD_TRIES, STD_DELAY, "hermes:26000")
        .await
        .map_add_err(|| ())?;
    let mut nm_consumer = NetMessenger::connect(STD_TRIES, STD_DELAY, "marketd:26001")
        .await
        .map_add_err(|| ())?;

    let mnemonic = onomyd_setup(daemon_home, false).await?;
    // send mnemonic to hermes
    nm_hermes.send::<String>(&mnemonic).await?;

    let mut cosmovisor_runner = cosmovisor_start("onomyd_runner.log", None).await?;

    let ccvconsumer_state = cosmovisor_add_consumer(daemon_home, consumer_id).await?;

    // send to consumer
    nm_consumer.send::<String>(&ccvconsumer_state).await?;

    // send keys
    nm_consumer
        .send::<String>(
            &FileOptions::read_to_string(&format!("{daemon_home}/config/node_key.json")).await?,
        )
        .await?;
    nm_consumer
        .send::<String>(
            &FileOptions::read_to_string(&format!("{daemon_home}/config/priv_validator_key.json"))
                .await?,
        )
        .await?;

    // wait for consumer to be online
    nm_consumer.recv::<()>().await?;
    // notify hermes to connect the chains
    nm_hermes.send::<()>(&()).await?;
    // when hermes is done
    nm_hermes.recv::<()>().await?;
    // finish
    nm_consumer.send::<()>(&()).await?;

    cosmovisor_runner.terminate(TIMEOUT).await?;
    Ok(())
}

async fn marketd_runner(args: &Args) -> Result<()> {
    let daemon_home = args.daemon_home.as_ref().map_add_err(|| ())?;
    let chain_id = "market";
    let mut nm_onomyd = NetMessenger::listen_single_connect("0.0.0.0:26001", TIMEOUT).await?;
    // we need the initial consumer state
    let ccvconsumer_state_s: String = nm_onomyd.recv().await?;

    marketd_setup(daemon_home, chain_id, &ccvconsumer_state_s).await?;

    // get keys
    let node_key = nm_onomyd.recv::<String>().await?;
    // we used same keys for consumer as producer, need to copy them over or else
    // the node will not be a working validator for itself
    FileOptions::write_str(&format!("{daemon_home}/config/node_key.json"), &node_key).await?;

    let priv_validator_key = nm_onomyd.recv::<String>().await?;
    FileOptions::write_str(
        &format!("{daemon_home}/config/priv_validator_key.json"),
        &priv_validator_key,
    )
    .await?;

    let mut cosmovisor_runner = cosmovisor_start(&format!("{chain_id}d_runner.log"), None).await?;

    // signal that we have started
    nm_onomyd.send::<()>(&()).await?;

    // wait for finish
    nm_onomyd.recv::<()>().await?;

    cosmovisor_runner.terminate(TIMEOUT).await?;
    FileOptions::write_str(
        "/logs/exported_market_genesis.json",
        &sh_cosmovisor_no_dbg("export", &[]).await?,
    )
    .await?;
    Ok(())
}
