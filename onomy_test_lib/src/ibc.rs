use std::time::Duration;

use log::info;
pub use super_orchestrator::stacked_errors::Result;
use super_orchestrator::{get_separated_val, stacked_errors::MapAddError};
use tokio::time::sleep;

pub use crate::types::{IbcPair, IbcSide};
use crate::{
    cosmovisor::{sh_cosmovisor_no_dbg, sh_cosmovisor_tx},
    hermes::{create_channel_pair, create_connection_pair},
};

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
        .await?;

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
        .await?;

        Ok(())
    }

    pub async fn get_ibc_denom(&self, leaf_denom: &str) -> Result<String> {
        let hash = sh_cosmovisor_no_dbg("query ibc-transfer denom-hash", &[&format!(
            "transfer/{}/{}",
            self.transfer_channel, leaf_denom
        )])
        .await
        .map_add_err(|| ())?;
        let hash = get_separated_val(&hash, "\n", "hash", ":").map_add_err(|| ())?;
        Ok(format!("ibc/{hash}"))
    }
}

impl IbcPair {
    /// Sets up transfer and consumer-provider IBC channels. This function
    /// assumes ICS setup has been performed, which creates a client pair
    /// automatically.
    pub async fn hermes_setup_pair(consumer: &str, provider: &str) -> Result<IbcPair> {
        // https://hermes.informal.systems/tutorials/local-chains/add-a-new-relay-path.html

        // Note: For ICS, there is a point where a handshake must be initiated by the
        // consumer chain, so we must make the consumer chain the "a-chain" and the
        // producer chain the "b-chain"
        let a_chain = consumer.to_owned();
        let b_chain = provider.to_owned();

        // a client is already created because of the ICS setup
        //let client_pair = create_client_pair(a_chain, b_chain).await?;
        // create one client and connection pair that will be used for IBC transfer and
        // ICS communication
        let connection_pair = create_connection_pair(&a_chain, &b_chain).await?;

        // this results in some mismatch errors but we use it for now for speeding up
        // things
        let tmp = (a_chain.clone(), connection_pair.clone());
        let transfer_task = tokio::task::spawn(async move {
            let (a_chain, connection_pair) = tmp;
            // a_chain<->b_chain transfer<->transfer
            create_channel_pair(
                &a_chain.clone(),
                &connection_pair.0.clone(),
                "transfer",
                "transfer",
                false,
            )
            .await
            .unwrap()
        });

        // make sure the transfer task gets the first connection TODO make this more
        // rigorous
        sleep(Duration::from_secs(1)).await;

        // a_chain<->b_chain consumer<->provider
        let ics_channel_pair =
            create_channel_pair(&a_chain, &connection_pair.0, "consumer", "provider", true).await?;

        let transfer_channel_pair = transfer_task.await?;

        info!("{consumer} <-> {provider} transfer and consumer-provider channels have been set up");

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
