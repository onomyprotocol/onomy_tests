//! Used to verify that an ONEX genesis proposal and genesis file do not have
//! problems.
//!
//! needs --proposal-path to the consumer addition proposal, --genesis-path to
//! the partial genesis, and --mnemonic-path to some account that has funds for
//! the hermes thing to work
//!
//! note: this uses the same validator mnemonic for hermes, so there is a chance
//! of a spurious account mismatch error
//!
//! NOTE: for the final final genesis you should check disabling the line that
//! overwrites "ccvconsumer", disabling the "genesis_time" overwrite, and check
//! that the bootstrap runner has OK logs, says "this node is not a validator",
//! and sleeps until genesis time

#[rustfmt::skip]
/*
e.x.

cargo r --bin onex_genesis -- --proposal-path ./../environments/testnet/onex-testnet-4/genesis-proposal.json --genesis-path ./../environments/testnet/onex-testnet-4/partial-genesis.json --mnemonic-path ./../testnet_dealer_mnemonic.txt

*/

use std::time::Duration;

use log::info;
use onomy_test_lib::{
    cosmovisor::{
        cosmovisor_bank_send, cosmovisor_get_addr, cosmovisor_get_balances,
        cosmovisor_gov_file_proposal, cosmovisor_start, fast_block_times, set_minimum_gas_price,
        sh_cosmovisor, sh_cosmovisor_no_debug, sh_cosmovisor_tx, wait_for_num_blocks,
    },
    dockerfiles::{dockerfile_hermes, dockerfile_onexd, dockerfile_onomyd},
    hermes::{
        hermes_set_gas_price_denom, hermes_start, sh_hermes, write_hermes_config,
        HermesChainConfig, IbcPair,
    },
    market::{CoinPair, Market},
    onomy_std_init, reprefix_bech32,
    setups::{cosmovisor_add_consumer, cosmovisor_setup, CosmosSetupOptions},
    super_orchestrator::{
        docker::{Container, ContainerNetwork, Dockerfile},
        net_message::NetMessenger,
        remove_files_in_dir, sh,
        stacked_errors::{ensure, ensure_eq, Error, Result, StackableErr},
        stacked_get, stacked_get_mut, FileOptions,
    },
    token18,
    u64_array_bigints::{
        u256, {self},
    },
    yaml_str_to_json_value, Args, ONOMY_IBC_NOM, STD_DELAY, STD_TRIES, TIMEOUT,
};
use serde_json::{json, Value};
use tokio::time::sleep;

const PROVIDER_ACCOUNT_PREFIX: &str = "onomy";
const CONSUMER_ACCOUNT_PREFIX: &str = "onomy";

const HERMES_MNEMONIC: &str = "suspect glove east just retreat relax south garment ketchup salmon \
                               chicken toilet nasty coach stairs logic churn solve super seminar \
                               dune midnight monitor peace";

pub async fn onexd_setup(
    daemon_home: &str,
    chain_id: &str,
    ccvconsumer_state_s: &str,
) -> Result<()> {
    sh_cosmovisor(["config chain-id", chain_id]).await.stack()?;
    sh_cosmovisor(["config keyring-backend test"])
        .await
        .stack()?;
    sh_cosmovisor_no_debug(["init --overwrite", chain_id])
        .await
        .stack()?;
    let genesis_file_path = format!("{daemon_home}/config/genesis.json");

    // add `ccvconsumer_state` to genesis
    let genesis_s = FileOptions::read_to_string("/resources/tmp/genesis.json")
        .await
        .stack()?;

    let mut genesis: Value = serde_json::from_str(&genesis_s).stack()?;

    let time = chrono::offset::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    *stacked_get_mut!(genesis["genesis_time"]) = time.into();

    // put some aONEX balance on our account so it can be bonded
    let array = genesis["app_state"]["bank"]["balances"]
        .as_array_mut()
        .stack()?;
    for balance in array {
        if balance["address"].as_str().unwrap() == "onomy1yks83spz6lvrrys8kh0untt22399tskk6jafcv" {
            balance["coins"].as_array_mut().unwrap().insert(
                1,
                json!({"denom": "aonex", "amount": "20000000000000000000000000000"}),
            );
            break
        }
    }

    // need to add the hermes account manually
    stacked_get_mut!(genesis["app_state"]["auth"]["accounts"])
        .as_array_mut()
        .stack()?
        .push(json!(
            {
                "@type": "/cosmos.auth.v1beta1.BaseAccount",
                "address": "onomy1p8zprjj83p7elv0dpjeefexrdjpqhj29tw7gre",
                "pub_key": null,
                "account_number": "0",
                "sequence": "0"
            }
        ));
    stacked_get_mut!(genesis["app_state"]["bank"]["balances"])
        .as_array_mut()
        .stack()?
        .push(json!(
            {
            "address": "onomy1p8zprjj83p7elv0dpjeefexrdjpqhj29tw7gre",
            "coins": [
                {
                    "denom": "aonex",
                    "amount": "100000000000000000000"
                }
            ]
            }
        ));

    let ccvconsumer_state: Value = serde_json::from_str(ccvconsumer_state_s).stack()?;
    *stacked_get_mut!(genesis["app_state"]["ccvconsumer"]) = ccvconsumer_state;

    // decrease the governing period for fast tests
    let gov_period = "800ms";
    let gov_period: Value = gov_period.into();
    *stacked_get_mut!(genesis["app_state"]["gov"]["voting_params"]["voting_period"]) =
        gov_period.clone();
    *stacked_get_mut!(genesis["app_state"]["gov"]["deposit_params"]["max_deposit_period"]) =
        gov_period;

    let genesis_s = genesis.to_string();

    FileOptions::write_str(&genesis_file_path, &genesis_s)
        .await
        .stack()?;
    FileOptions::write_str(&format!("/logs/{chain_id}_genesis.json"), &genesis_s)
        .await
        .stack()?;

    fast_block_times(daemon_home).await.stack()?;
    set_minimum_gas_price(daemon_home, "1aonex").await.stack()?;

    FileOptions::write_str(
        &format!("/logs/{chain_id}_genesis.json"),
        &FileOptions::read_to_string(&genesis_file_path)
            .await
            .stack()?,
    )
    .await
    .stack()?;

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = onomy_std_init()?;

    if let Some(ref s) = args.entry_name {
        match s.as_str() {
            "onomyd" => onomyd_runner(&args).await,
            "consumer" => consumer(&args).await,
            "hermes" => hermes_runner(&args).await,
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
        "--features",
        "onex_genesis",
    ])
    .await
    .stack()?;

    // prepare volumed resources
    remove_files_in_dir("./tests/resources/keyring-test/", &[".address", ".info"])
        .await
        .stack()?;

    // put in the genesis files
    FileOptions::copy(
        args.genesis_path
            .as_deref()
            .stack_err(|| "need to pass --genesis-path")?,
        "./tests/resources/tmp/genesis.json",
    )
    .await
    .stack()?;
    FileOptions::copy(
        args.proposal_path
            .as_deref()
            .stack_err(|| "need to pass --proposal-path")?,
        "./tests/resources/tmp/proposal.json",
    )
    .await
    .stack()?;

    let proposal = FileOptions::read_to_string(args.genesis_path.as_deref().stack()?)
        .await
        .stack()?;
    let proposal: Value = serde_json::from_str(&proposal).stack()?;
    let consumer_id = stacked_get!(proposal["chain_id"])
        .as_str()
        .stack()?
        .to_owned();

    let mut onomyd_args = vec!["--entry-name", "onomyd", "--consumer-id", &consumer_id];
    if let Some(mnemonic_path) = args.mnemonic_path.as_deref() {
        FileOptions::copy(mnemonic_path, "./tests/resources/tmp/mnemonic.txt")
            .await
            .stack()?;
        onomyd_args.extend(["--mnemonic-path", "/resources/tmp/mnemonic.txt"])
    }

    let entrypoint = &format!("./target/{container_target}/release/{bin_entrypoint}");

    let mut cn = ContainerNetwork::new(
        "test",
        vec![
            Container::new(
                "hermes",
                Dockerfile::contents(dockerfile_hermes("__tmp_hermes_config.toml")),
            )
            .external_entrypoint(entrypoint, [
                "--entry-name",
                "hermes",
                "--consumer-id",
                &consumer_id,
            ])
            .await
            .stack()?,
            Container::new("onomyd", Dockerfile::contents(dockerfile_onomyd()))
                .external_entrypoint(entrypoint, onomyd_args)
                .await
                .stack()?
                .volumes([
                    (
                        "./tests/resources/keyring-test",
                        "/root/.onomy/keyring-test",
                    ),
                    ("./tests/resources/tmp", "/resources/tmp"),
                ]),
            Container::new("consumer", Dockerfile::Contents(dockerfile_onexd()))
                .external_entrypoint(entrypoint, [
                    "--entry-name",
                    "consumer",
                    "--consumer-id",
                    &consumer_id,
                ])
                .await
                .stack()?
                .volumes([
                    (
                        "./tests/resources/keyring-test",
                        "/root/.onomy_onex/keyring-test",
                    ),
                    ("./tests/resources/tmp", "/resources/tmp"),
                ]),
        ],
        Some(dockerfiles_dir),
        true,
        logs_dir,
    )
    .stack()?;
    cn.add_common_volumes([(logs_dir, "/logs")]);
    let uuid = cn.uuid_as_string();
    cn.add_common_entrypoint_args(["--uuid", &uuid]);

    // prepare hermes config
    write_hermes_config(
        &[
            HermesChainConfig::new(
                "onomy",
                &format!("onomyd_{uuid}"),
                "onomy",
                false,
                "anom",
                true,
            ),
            HermesChainConfig::new(
                &consumer_id,
                &format!("consumer_{uuid}"),
                CONSUMER_ACCOUNT_PREFIX,
                true,
                "aonex",
                true,
            ),
        ],
        &format!("{dockerfiles_dir}/dockerfile_resources"),
    )
    .await
    .stack()?;

    cn.run_all(true).await.stack()?;
    cn.wait_with_timeout_all(true, TIMEOUT).await.stack()?;
    cn.terminate_all().await;
    Ok(())
}

async fn hermes_runner(args: &Args) -> Result<()> {
    let hermes_home = args.hermes_home.as_ref().stack()?;
    let consumer_id = args.consumer_id.as_deref().stack()?;
    let mut nm_onomyd = NetMessenger::listen("0.0.0.0:26000", TIMEOUT)
        .await
        .stack()?;

    // get mnemonic from onomyd
    let mnemonic: String = nm_onomyd.recv().await.stack()?;
    // set keys for our chains
    FileOptions::write_str("/root/.hermes/mnemonic.txt", &mnemonic)
        .await
        .stack()?;
    sh_hermes(["keys add --chain onomy --mnemonic-file /root/.hermes/mnemonic.txt"])
        .await
        .stack()?;
    FileOptions::write_str("/mnemonic.txt", HERMES_MNEMONIC)
        .await
        .stack()?;
    sh_hermes([format!(
        "keys add --chain {consumer_id} --mnemonic-file /mnemonic.txt"
    )])
    .await
    .stack()?;

    // wait for setup
    nm_onomyd.recv::<()>().await.stack()?;

    let ibc_pair =
        IbcPair::hermes_setup_ics_pair(consumer_id, "07-tendermint-0", "onomy", "07-tendermint-0")
            .await
            .stack()?;
    let mut hermes_runner = hermes_start("/logs/hermes_bootstrap_runner.log")
        .await
        .stack()?;
    ibc_pair.hermes_check_acks().await.stack()?;

    // tell that chains have been connected
    nm_onomyd.send::<IbcPair>(&ibc_pair).await.stack()?;

    // signal to update gas denom
    let ibc_nom = nm_onomyd.recv::<String>().await.stack()?;
    hermes_runner.terminate(TIMEOUT).await.stack()?;
    hermes_set_gas_price_denom(hermes_home, consumer_id, &ibc_nom)
        .await
        .stack()?;

    // restart
    let mut hermes_runner = hermes_start("/logs/hermes_runner.log").await.stack()?;
    nm_onomyd.send::<()>(&()).await.stack()?;

    // termination signal
    nm_onomyd.recv::<()>().await.stack()?;
    hermes_runner.terminate(TIMEOUT).await.stack()?;
    Ok(())
}

async fn onomyd_runner(args: &Args) -> Result<()> {
    let uuid = &args.uuid;
    let consumer_id = args.consumer_id.as_deref().stack()?;
    let daemon_home = args.daemon_home.as_ref().stack()?;
    let mut nm_hermes =
        NetMessenger::connect(STD_TRIES, STD_DELAY, &format!("hermes_{uuid}:26000"))
            .await
            .stack()?;
    let mut nm_consumer =
        NetMessenger::connect(STD_TRIES, STD_DELAY, &format!("consumer_{uuid}:26001"))
            .await
            .stack()?;

    let mut options = CosmosSetupOptions::onomy(daemon_home);
    if let Some(ref mnemonic_path) = args.mnemonic_path {
        let mnemonic = FileOptions::read_to_string(mnemonic_path).await.stack()?;
        options.validator_mnemonic = Some(mnemonic.clone());
        options.hermes_mnemonic = Some(HERMES_MNEMONIC.to_owned());
    }
    let cosmores = cosmovisor_setup(options).await.stack()?;
    // send mnemonic to hermes
    nm_hermes
        .send::<String>(&cosmores.validator_mnemonic.stack()?)
        .await
        .stack()?;

    // keep these here for local testing purposes
    let addr = &cosmovisor_get_addr("validator").await.stack()?;
    sleep(Duration::ZERO).await;

    let mut cosmovisor_runner = cosmovisor_start("onomyd_runner.log", None).await.stack()?;

    //let proposal = onomy_test_lib::setups::test_proposal(consumer_id, "anom");
    let proposal = FileOptions::read_to_string("/resources/tmp/proposal.json")
        .await
        .stack()?;
    let mut proposal: Value = serde_json::from_str(&proposal).stack()?;

    let time = chrono::offset::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    *stacked_get_mut!(proposal["spawn_time"]) = time.into();
    let proposal = &proposal.to_string();
    info!("PROPOSAL: {proposal}");
    let ccvconsumer_state = cosmovisor_add_consumer(daemon_home, consumer_id, proposal)
        .await
        .stack()?;

    // send to consumer
    nm_consumer
        .send::<String>(&ccvconsumer_state)
        .await
        .stack()?;

    // send keys
    nm_consumer
        .send::<String>(
            &FileOptions::read_to_string(&format!("{daemon_home}/config/node_key.json"))
                .await
                .stack()?,
        )
        .await
        .stack()?;
    nm_consumer
        .send::<String>(
            &FileOptions::read_to_string(&format!("{daemon_home}/config/priv_validator_key.json"))
                .await
                .stack()?,
        )
        .await
        .stack()?;

    // wait for consumer to be online
    nm_consumer.recv::<()>().await.stack()?;
    // notify hermes to connect the chains
    nm_hermes.send::<()>(&()).await.stack()?;
    // when hermes is done
    let ibc_pair = nm_hermes.recv::<IbcPair>().await.stack()?;
    info!("IbcPair: {ibc_pair:?}");

    // send anom to consumer
    ibc_pair
        .b
        .cosmovisor_ibc_transfer(
            "validator",
            &reprefix_bech32(addr, CONSUMER_ACCOUNT_PREFIX).stack()?,
            &token18(2.0e3, ""),
            "anom",
        )
        .await
        .stack()?;
    // it takes time for the relayer to complete relaying
    wait_for_num_blocks(4).await.stack()?;
    // notify consumer that we have sent NOM
    nm_consumer.send::<IbcPair>(&ibc_pair).await.stack()?;

    // tell hermes to restart with updated gas denom on its side
    let ibc_nom = nm_consumer.recv::<String>().await.stack()?;
    nm_hermes.send::<String>(&ibc_nom).await.stack()?;
    nm_hermes.recv::<()>().await.stack()?;
    nm_consumer.send::<()>(&()).await.stack()?;

    // recieve round trip signal
    nm_consumer.recv::<()>().await.stack()?;
    // check that the IBC NOM converted back to regular NOM
    ensure_eq!(
        cosmovisor_get_balances("onomy1gk7lg5kd73mcr8xuyw727ys22t7mtz9gh07ul3")
            .await
            .stack()?["anom"],
        u256!(5000)
    );

    // signal to collectively terminate
    nm_hermes.send::<()>(&()).await.stack()?;
    nm_consumer.send::<()>(&()).await.stack()?;
    cosmovisor_runner.terminate(TIMEOUT).await.stack()?;

    FileOptions::write_str(
        "/logs/onomyd_export.json",
        &sh_cosmovisor_no_debug(["export"]).await.stack()?,
    )
    .await
    .stack()?;

    Ok(())
}

async fn consumer(args: &Args) -> Result<()> {
    let daemon_home = args.daemon_home.as_ref().stack()?;
    let consumer_id = args.consumer_id.as_deref().stack()?;
    let chain_id = consumer_id;
    let mut nm_onomyd = NetMessenger::listen("0.0.0.0:26001", TIMEOUT)
        .await
        .stack()?;
    // we need the initial consumer state
    let ccvconsumer_state_s: String = nm_onomyd.recv().await.stack()?;

    onexd_setup(daemon_home, chain_id, &ccvconsumer_state_s)
        .await
        .stack()?;

    // get keys
    let node_key = nm_onomyd.recv::<String>().await.stack()?;
    // we used same keys for consumer as provider, need to copy them over or else
    // the node will not be a working validator for itself
    FileOptions::write_str(&format!("{daemon_home}/config/node_key.json"), &node_key)
        .await
        .stack()?;

    let priv_validator_key = nm_onomyd.recv::<String>().await.stack()?;
    FileOptions::write_str(
        &format!("{daemon_home}/config/priv_validator_key.json"),
        &priv_validator_key,
    )
    .await
    .stack()?;

    let mut cosmovisor_runner =
        cosmovisor_start(&format!("{chain_id}d_bootstrap_runner.log"), None)
            .await
            .stack()?;

    let addr = &cosmovisor_get_addr("validator").await.stack()?;

    // signal that we have started
    nm_onomyd.send::<()>(&()).await.stack()?;

    // wait for provider to send us stuff
    let ibc_pair = nm_onomyd.recv::<IbcPair>().await.stack()?;
    // get the name of the IBC NOM. Note that we can't do this on the onomyd side,
    // it has to be with respect to the consumer side
    let ibc_nom = &ibc_pair.a.get_ibc_denom("anom").await.stack()?;
    ensure_eq!(ibc_nom, ONOMY_IBC_NOM);
    let balances = cosmovisor_get_balances(addr).await.stack()?;
    ensure!(balances.contains_key(ibc_nom));

    // we have IBC NOM, shut down, change gas in app.toml, restart
    cosmovisor_runner.terminate(TIMEOUT).await.stack()?;
    set_minimum_gas_price(daemon_home, &format!("1{ibc_nom}"))
        .await
        .stack()?;
    let mut cosmovisor_runner = cosmovisor_start(&format!("{chain_id}d_runner.log"), None)
        .await
        .stack()?;
    // tell hermes to restart with updated gas denom on its side
    nm_onomyd.send::<String>(ibc_nom).await.stack()?;
    nm_onomyd.recv::<()>().await.stack()?;
    info!("restarted with new gas denom");

    // test normal transfer
    let dst_addr = &reprefix_bech32(
        "onomy1gk7lg5kd73mcr8xuyw727ys22t7mtz9gh07ul3",
        CONSUMER_ACCOUNT_PREFIX,
    )
    .stack()?;
    cosmovisor_bank_send(addr, dst_addr, "5000", ibc_nom)
        .await
        .stack()?;
    ensure_eq!(
        cosmovisor_get_balances(dst_addr).await.stack()?[ibc_nom],
        u256!(5000)
    );

    let test_addr = &reprefix_bech32(
        "onomy1gk7lg5kd73mcr8xuyw727ys22t7mtz9gh07ul3",
        PROVIDER_ACCOUNT_PREFIX,
    )
    .stack()?;
    info!("sending back to {}", test_addr);

    // send some IBC NOM back to origin chain using it as gas
    ibc_pair
        .a
        .cosmovisor_ibc_transfer("validator", test_addr, "5000", ibc_nom)
        .await
        .stack()?;
    wait_for_num_blocks(4).await.stack()?;

    // market module specific sanity checks (need to check all tx commands
    // specifically to make sure permissions are correct)

    let amount = u256!(100000000000000000);
    let amount_sqr = amount.checked_mul(amount).unwrap();
    let coin_pair = CoinPair::new("aonex", ibc_nom).stack()?;
    let mut market = Market::new("validator", &format!("1000000{ibc_nom}"));
    market.max_gas = Some(u256!(1000000));
    market
        .create_pool(&coin_pair, amount, amount)
        .await
        .stack()?;
    market.create_drop(&coin_pair, amount_sqr).await.stack()?;
    market.show_pool(&coin_pair).await.stack()?;
    market.show_members(&coin_pair).await.stack()?;
    market
        .market_order(coin_pair.coin_a(), amount, coin_pair.coin_b(), amount, 5000)
        .await
        .stack()?;
    market.redeem_drop(1).await.stack()?;
    market
        .create_order(
            coin_pair.coin_a(),
            coin_pair.coin_b(),
            "stop",
            amount,
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
            amount,
            (1100, 900),
            (0, 0),
        )
        .await
        .stack()?;
    //market.cancel_order(6).await.stack()?;

    let pubkey = sh_cosmovisor(["tendermint show-validator"]).await.stack()?;
    let pubkey = pubkey.trim();
    sh_cosmovisor_tx([
        "staking",
        "create-validator",
        "--commission-max-change-rate",
        "0.01",
        "--commission-max-rate",
        "0.10",
        "--commission-rate",
        "0.05",
        "--from",
        "validator",
        "--min-self-delegation",
        "1",
        "--amount",
        &token18(500.0, "aonex"),
        "--fees",
        &format!("1000000{ONOMY_IBC_NOM}"),
        "--pubkey",
        pubkey,
        "-y",
        "-b",
        "block",
    ])
    .await
    .stack()?;

    // round trip signal
    nm_onomyd.send::<()>(&()).await.stack()?;

    // termination signal
    nm_onomyd.recv::<()>().await.stack()?;

    wait_for_num_blocks(1).await.stack()?;

    // test a simple text proposal
    let test_deposit = token18(500.0, "aonex");
    let proposal = json!({
        "title": "Text Proposal",
        "description": "a text proposal",
        "type": "Text",
        "deposit": test_deposit
    });
    cosmovisor_gov_file_proposal(
        daemon_home,
        None,
        &proposal.to_string(),
        &format!("1{ibc_nom}"),
    )
    .await
    .stack()?;
    let proposals = sh_cosmovisor(["query gov proposals"]).await.stack()?;
    assert!(proposals.contains("PROPOSAL_STATUS_PASSED"));

    // but first, test governance with IBC NOM as the token
    let test_crisis_denom = ibc_nom.as_str();
    let test_deposit = token18(500.0, "aonex");
    wait_for_num_blocks(1).await.stack()?;
    cosmovisor_gov_file_proposal(
        daemon_home,
        Some("param-change"),
        &format!(
            r#"
    {{
        "title": "Parameter Change",
        "description": "Making a parameter change",
        "changes": [
          {{
            "subspace": "crisis",
            "key": "ConstantFee",
            "value": {{"denom":"{test_crisis_denom}","amount":"1337"}}
          }}
        ],
        "deposit": "{test_deposit}"
    }}
    "#
        ),
        &format!("1{ibc_nom}"),
    )
    .await
    .stack()?;
    wait_for_num_blocks(5).await.stack()?;
    // just running this for debug, param querying is weird because it is json
    // inside of yaml, so we will instead test the exported genesis
    sh_cosmovisor(["query params subspace crisis ConstantFee"])
        .await
        .stack()?;

    cosmovisor_runner.terminate(TIMEOUT).await.stack()?;

    let exported = sh_cosmovisor_no_debug(["export"]).await.stack()?;
    FileOptions::write_str(&format!("/logs/{chain_id}_export.json"), &exported)
        .await
        .stack()?;
    let exported = yaml_str_to_json_value(&exported).stack()?;
    ensure_eq!(
        stacked_get!(exported["app_state"]["crisis"]["constant_fee"]["denom"]),
        test_crisis_denom
    );
    ensure_eq!(
        stacked_get!(exported["app_state"]["crisis"]["constant_fee"]["amount"]),
        "1337"
    );

    Ok(())
}
