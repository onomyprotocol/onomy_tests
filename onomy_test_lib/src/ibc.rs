use log::info;
use serde_derive::{Deserialize, Serialize};
pub use super_orchestrator::stacked_errors::Result;
use super_orchestrator::{get_separated_val, stacked_errors::StackableErr};

use crate::{
    cosmovisor::{sh_cosmovisor_no_dbg, sh_cosmovisor_tx},
    hermes::{create_channel_pair, create_connection_pair, sh_hermes},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IbcSide {
    pub chain_id: String,
    pub connection: String,
    pub transfer_channel: String,
    pub ics_channel: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IbcPair {
    pub a: IbcSide,
    pub b: IbcSide,
}

impl IbcSide {
    /// This call needs to be made on the source side
    pub async fn cosmovisor_ibc_transfer_with_flags(
        &self,
        target_addr: &str,
        coins_to_send: &str,
        flags: &[&str],
    ) -> Result<()> {
        // note: tx ibc-transfer --help is wrong, it should be
        // tx ibc-transfer transfer transfer [channel to right chain]
        // [target cosmos addr] [coins to send] [gas flags] --from [source key name]

        sh_cosmovisor_tx(
            "ibc-transfer transfer transfer",
            &[&[&self.transfer_channel, target_addr, coins_to_send], flags].concat(),
        )
        .await
        .stack()?;

        Ok(())
    }

    /// Sends `denom` and uses same `denom` for gas. Uses the flags
    /// "-b block --gas auto --gas-adjustment 1.3 --gas-prices 1{denom} --from
    /// {from_key}"
    pub async fn cosmovisor_ibc_transfer(
        &self,
        from_key: &str,
        target_addr: &str,
        amount: &str,
        denom: &str,
    ) -> Result<()> {
        let coins_to_send = format!("{amount}{denom}");
        let base = format!("1{denom}");
        sh_cosmovisor_tx("ibc-transfer transfer transfer", &[
            &self.transfer_channel,
            target_addr,
            &coins_to_send,
            "-y",
            "-b",
            "block",
            "--gas",
            "auto",
            "--gas-adjustment",
            "1.3",
            "--gas-prices",
            &base,
            "--from",
            from_key,
        ])
        .await
        .stack()?;

        Ok(())
    }

    pub async fn get_ibc_denom(&self, leaf_denom: &str) -> Result<String> {
        let hash = sh_cosmovisor_no_dbg("query ibc-transfer denom-hash", &[&format!(
            "transfer/{}/{}",
            self.transfer_channel, leaf_denom
        )])
        .await
        .stack()?;
        let hash = get_separated_val(&hash, "\n", "hash", ":").stack()?;
        Ok(format!("ibc/{hash}"))
    }
}

impl IbcPair {
    /// Sets up consumer-provider and transfer-transfer IBC channels. This
    /// function assumes ICS setup has been performed, which creates a
    /// client pair automatically.
    ///
    /// TODO this currently assumes that this is the first channel creation
    /// function that happens on chain startup, it hard codes the transfer
    /// channel number. Additionally, `hermes start` should be run _after_
    /// this function is called.
    pub async fn hermes_setup_ics_pair(consumer: &str, provider: &str) -> Result<IbcPair> {
        // https://hermes.informal.systems/tutorials/local-chains/add-a-new-relay-path.html

        // Note: For ICS, there is a point where a handshake must be initiated by the
        // consumer chain, so we must make the consumer chain the "a-chain" and the
        // producer chain the "b-chain"
        let a_chain = consumer.to_owned();
        let b_chain = provider.to_owned();

        // a client is already created because of the ICS setup
        //let client_pair = create_client_pair(a_chain, b_chain).await.stack()?;
        // create one client and connection pair that will be used for IBC transfer and
        // ICS communication
        let connection_pair = create_connection_pair(&a_chain, &b_chain).await.stack()?;

        // a_chain<->b_chain consumer<->provider
        let ics_channel_pair =
            create_channel_pair(&a_chain, &connection_pair.0, "consumer", "provider", true)
                .await
                .stack()?;

        // ICS channel creation also automatically creates a transfer channel, but it
        // starts in the init state and we need to manually perform the 3 other steps.

        //let transfer_channel_pair = create_channel_pair(&a_chain, &connection_pair.0,
        // "transfer", "transfer", false).await.stack()?;

        // FIXME this is hard coded
        let transfer_channel_pair = ("channel-1".to_string(), "channel-1".to_string());
        sh_hermes(
            &format!(
                "tx chan-open-try --dst-chain {provider} --src-chain {consumer} --dst-connection \
                 connection-0 --dst-port transfer --src-port transfer --src-channel channel-1"
            ),
            &[],
        )
        .await
        .stack()?;
        sh_hermes(
            &format!(
                "tx chan-open-ack --dst-chain {consumer} --src-chain {provider} --dst-connection \
                 connection-0 --dst-port transfer --src-port transfer --dst-channel channel-1 \
                 --src-channel channel-1"
            ),
            &[],
        )
        .await
        .stack()?;
        sh_hermes(
            &format!(
                "tx chan-open-confirm --dst-chain {provider} --src-chain {consumer} \
                 --dst-connection connection-0 --dst-port transfer --src-port transfer \
                 --dst-channel channel-1 --src-channel channel-1"
            ),
            &[],
        )
        .await
        .stack()?;

        info!("{consumer} <-> {provider} consumer-provider and transfer channels have been set up");

        Ok(IbcPair {
            a: IbcSide {
                chain_id: a_chain,
                connection: connection_pair.0,
                transfer_channel: transfer_channel_pair.0,
                ics_channel: ics_channel_pair.0,
            },
            b: IbcSide {
                chain_id: b_chain,
                connection: connection_pair.1,
                transfer_channel: transfer_channel_pair.1,
                ics_channel: ics_channel_pair.1,
            },
        })
    }
}
