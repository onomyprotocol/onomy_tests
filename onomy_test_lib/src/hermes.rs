use std::time::Duration;

use log::info;
use serde_json::Value;
use super_orchestrator::{
    sh, sh_no_dbg,
    stacked_errors::{Error, Result, StackableErr},
    Command, CommandRunner, FileOptions,
};

use crate::json_inner;
pub use crate::{hermes_config::*, ibc::IbcPair};

/// A wrapper around `super_orchestrator::sh` that prefixes "hermes --json". The
/// last line is parsed as a `Value` and the inner "result" is returned.
pub async fn sh_hermes(cmd_with_args: &str, args: &[&str]) -> Result<Value> {
    info!("running hermes({cmd_with_args}, {args:?})");
    let stdout = sh(&format!("hermes --json {cmd_with_args}"), args)
        .await
        .stack()?;
    let res = stdout.lines().last().stack()?;
    let res: Value = serde_json::from_str(res).stack()?;
    let res = res.get("result").stack()?.to_owned();
    Ok(res)
}

pub async fn sh_hermes_no_dbg(cmd_with_args: &str, args: &[&str]) -> Result<Value> {
    let stdout = sh_no_dbg(&format!("hermes --json {cmd_with_args}"), args)
        .await
        .stack()?;
    let res = stdout.lines().last().stack()?;
    let res: Value = serde_json::from_str(res).stack()?;
    let res = res.get("result").stack()?.to_owned();
    Ok(res)
}

/// Returns a single client if it exists. Returns an error if two redundant
/// clients were found.
pub async fn get_client(host_chain: &str, reference_chain: &str) -> Result<String> {
    let clients = sh_hermes_no_dbg("query clients --host-chain", &[host_chain])
        .await
        .stack_err(|| "failed to query for host chain")?;
    let clients = clients.as_array().stack()?;
    let mut client_id = None;
    for client in clients {
        if json_inner(&client["chain_id"]) == reference_chain {
            if client_id.is_some() {
                // we have already seen this, we don't want to need to handle ambiguity
                return Err(Error::from(format!(
                    "found two clients associated with host_chain {host_chain} and \
                     reference_chain {reference_chain}"
                )))
            }
            client_id = Some(json_inner(&client["client_id"]));
        }
    }
    client_id.stack_err(|| {
        format!(
            "could not find client associated with host_chain {host_chain} and reference_chain \
             {reference_chain}"
        )
    })
}

/// Returns the 07-tendermint-x of `a_chain` tracking the state of `b_chain` and
/// vice versa.
///
/// Returns an error if a client already exists for either side.
///
/// Note: for ICS pairs a client is created automatically by the process of
/// setting up ICS.
pub async fn create_client_pair(a_chain: &str, b_chain: &str) -> Result<(String, String)> {
    // avoid creating redundant clients, for which there shouldn't be any use, and
    // because it is almost certainly a cause for bugs
    if get_client(a_chain, b_chain).await.is_ok() {
        return Err(Error::from(format!(
            "a client already exists between {a_chain} and {b_chain}"
        )))
    }
    if get_client(b_chain, a_chain).await.is_ok() {
        return Err(Error::from(format!(
            "a client already exists between {b_chain} and {a_chain}"
        )))
    }
    let client0 = json_inner(
        &sh_hermes("create client --host-chain", &[
            a_chain,
            "--reference-chain",
            b_chain,
        ])
        .await
        .stack()?["CreateClient"]["client_id"],
    );
    let client1 = json_inner(
        &sh_hermes("create client --host-chain", &[
            b_chain,
            "--reference-chain",
            a_chain,
        ])
        .await
        .stack()?["CreateClient"]["client_id"],
    );
    Ok((client0, client1))
}

/// Returns the connection-x of the new connection on the side of `a_chain` and
/// `b_chain`.
pub async fn create_connection_pair(a_chain: &str, b_chain: &str) -> Result<(String, String)> {
    let a_client = get_client(a_chain, b_chain).await.stack_err(|| {
        format!("client hosted by {a_chain} not created before `create_connection_pair` was called")
    })?;
    let b_client = get_client(b_chain, a_chain).await.stack_err(|| {
        format!("client hosted by {b_chain} not created before `create_connection_pair` was called")
    })?;

    let res = &sh_hermes("create connection --a-chain", &[
        a_chain,
        "--a-client",
        &a_client,
        "--b-client",
        &b_client,
    ])
    .await
    .stack()?;
    Ok((
        json_inner(&res["a_side"]["connection_id"]),
        json_inner(&res["b_side"]["connection_id"]),
    ))
}

/// Returns the channel-x identifiers of a new channel over `a_connection`
/// between `a_port` and `b_port`.
///
/// Note: For ICS, there is a point where a handshake must be initiated by the
/// consumer chain, so we must make the consumer chain the "a-chain" and the
/// producer chain the "b-chain"
pub async fn create_channel_pair(
    a_chain: &str,
    a_connection: &str,
    a_port: &str,
    b_port: &str,
    ordered: bool,
) -> Result<(String, String)> {
    let order: &[&str] = if ordered {
        &["--order", "ordered"]
    } else {
        &[]
    };
    let res = &sh_hermes(
        "create channel --a-chain",
        &[
            &[
                a_chain,
                "--a-connection",
                a_connection,
                "--a-port",
                a_port,
                "--b-port",
                b_port,
            ],
            order,
        ]
        .concat(),
    )
    .await
    .stack()?;
    Ok((
        json_inner(&res["a_side"]["channel_id"]),
        json_inner(&res["b_side"]["channel_id"]),
    ))
}

impl IbcPair {
    pub async fn hermes_check_acks(&self) -> Result<()> {
        // check all channels on both sides
        sh_hermes_no_dbg("query packet acks --chain", &[
            &self.b.chain_id,
            "--port",
            "transfer",
            "--channel",
            &self.a.transfer_channel,
        ])
        .await
        .stack()?;
        sh_hermes_no_dbg("query packet acks --chain", &[
            &self.a.chain_id,
            "--port",
            "transfer",
            "--channel",
            &self.b.transfer_channel,
        ])
        .await
        .stack()?;
        sh_hermes_no_dbg("query packet acks --chain", &[
            &self.b.chain_id,
            "--port",
            "provider",
            "--channel",
            &self.a.ics_channel,
        ])
        .await
        .stack()?;
        sh_hermes_no_dbg("query packet acks --chain", &[
            &self.a.chain_id,
            "--port",
            "consumer",
            "--channel",
            &self.b.ics_channel,
        ])
        .await
        .stack()?;
        Ok(())
    }
}

pub struct HermesRunner {
    pub runner: CommandRunner,
}

impl HermesRunner {
    pub async fn terminate(&mut self, timeout: Duration) -> Result<()> {
        self.runner.send_unix_sigterm().stack()?;
        self.runner.wait_with_timeout(timeout).await.stack()
    }
}

pub async fn hermes_start(log_file: &str) -> Result<HermesRunner> {
    let hermes_log = FileOptions::write(log_file);
    let hermes_runner = Command::new("hermes start", &[])
        .stderr_log(&hermes_log)
        .stdout_log(&hermes_log)
        .run()
        .await
        .stack()?;
    Ok(HermesRunner {
        runner: hermes_runner,
    })
}

/// Note: uses "price = 1.0"
pub async fn hermes_set_gas_price_denom(
    hermes_home: &str,
    chain_id: &str,
    gas_price_denom: &str,
) -> Result<()> {
    // can't get simpler than this without importing a lot of stuff
    let outer_table: toml::Value = toml::from_str(&format!(
        "gas-price = {{ price = 1.0, denom = '{gas_price_denom}' }}"
    ))
    .unwrap();
    let inner_table = outer_table["gas-price"].clone();

    let config_path = format!("{hermes_home}/config.toml");
    let config_s = FileOptions::read_to_string(&config_path).await.stack()?;
    let mut config: toml::Value = toml::from_str(&config_s).stack()?;
    for chain in config["chains"].as_array_mut().stack()? {
        if chain["id"].as_str().stack()? == chain_id {
            chain["gas_price"] = inner_table;
            break
        }
    }
    let config_s = toml::to_string_pretty(&config).stack()?;
    FileOptions::write_str(&config_path, &config_s)
        .await
        .stack()?;
    Ok(())
}
