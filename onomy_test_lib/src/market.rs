//! Market module functions

use super_orchestrator::stacked_errors::{Error, StackableErr};
use u64_array_bigints::{u256, U256};

use crate::{
    cosmovisor::{cosmovisor_get_balances, sh_cosmovisor, sh_cosmovisor_tx},
    super_orchestrator::stacked_errors::Result,
};

pub struct CoinPair {
    coin_a: String,
    coin_b: String,
}

impl CoinPair {
    pub fn new(coin_a: &str, coin_b: &str) -> Result<Self> {
        if coin_a >= coin_b {
            Err(Error::from("coin_a >= coin_b, should be coin_a < coin_b"))
        } else {
            Ok(CoinPair {
                coin_a: coin_a.to_owned(),
                coin_b: coin_b.to_owned(),
            })
        }
    }

    pub fn coin_a(&self) -> &str {
        &self.coin_a
    }

    pub fn coin_b(&self) -> &str {
        &self.coin_b
    }

    pub fn coin_a_amount(&self, amount: U256) -> String {
        format!("{}{}", amount, self.coin_a())
    }

    pub fn coin_b_amount(&self, amount: U256) -> String {
        format!("{}{}", amount, self.coin_b())
    }

    pub fn paired_amounts(&self, amount_a: U256, amount_b: U256) -> String {
        format!(
            "{}{},{}{}",
            amount_a,
            self.coin_a(),
            amount_b,
            self.coin_b()
        )
    }

    pub fn paired(&self) -> String {
        format!("{},{}", self.coin_a(), self.coin_b())
    }

    pub async fn cosmovisor_get_balances(&self, addr: &str) -> Result<(U256, U256)> {
        let balances = cosmovisor_get_balances(addr)
            .await
            .stack_err(|| "cosmovisor_get_balances failed")?;
        let balance_a = *balances
            .get(self.coin_a())
            .stack_err(|| "did not find nonzero coin_a balance")?;
        let balance_b = *balances
            .get(self.coin_b())
            .stack_err(|| "did not find nonzero coin_b balance")?;
        Ok((balance_a, balance_b))
    }
}

// probably how this will be extended in the future, is that this is returned by
// reference from a `market()` function from some more general struct that
// handles fees and stuff
pub struct Market {
    pub account: String,
    pub fees: String,
    pub max_gas: Option<U256>,
}

impl Market {
    /// Max absolute coin amounts supported by the market module currently
    pub const MAX_COIN: U256 = u256!(5192296858534827628530496329220095);
    pub const MAX_COIN_SQUARED: U256 =
        u256!(26959946667150639794667015087019620289043427352885315420110951809025);

    pub fn new(account: &str, fees: &str) -> Self {
        Market {
            account: account.to_owned(),
            fees: fees.to_owned(),
            max_gas: None,
        }
    }

    /// Adds on "-y", "-b", "block", "--from", self.account, "--fees", self.fees
    pub async fn configured_tx<I, S>(&self, program_with_args: I) -> Result<()>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut args: Vec<String> = program_with_args
            .into_iter()
            .map(|s| s.as_ref().to_string())
            .collect();
        args.extend(
            [
                "-y",
                "-b",
                "block",
                "--from",
                &self.account,
                "--fees",
                &self.fees,
            ]
            .iter()
            .map(|s| s.to_string()),
        );
        let max_gas = self.max_gas.map(|x| format!("{x}"));
        if let Some(ref max_gas) = max_gas {
            args.push("--gas".to_string());
            args.push(max_gas.clone());
        }
        sh_cosmovisor_tx(args)
            .await
            .stack_err(|| "market module transaction error")?;
        Ok(())
    }

    /// Initiates the pool with the given amounts
    pub async fn create_pool(
        &self,
        coin_pair: &CoinPair,
        coin_a_amount: U256,
        coin_b_amount: U256,
    ) -> Result<()> {
        self.configured_tx([
            "market create-pool",
            &coin_pair.coin_a_amount(coin_a_amount),
            &coin_pair.coin_b_amount(coin_b_amount),
        ])
        .await
        .stack()?;
        Ok(())
    }

    pub async fn show_pool(&self, coin_pair: &CoinPair) -> Result<String> {
        sh_cosmovisor(["query market pool", &coin_pair.paired()])
            .await
            .stack()
    }

    pub async fn show_members(&self, coin_pair: &CoinPair) -> Result<(String, String)> {
        let member_a = sh_cosmovisor([
            "query market show-member",
            coin_pair.coin_a(),
            coin_pair.coin_b(),
        ])
        .await
        .stack()?;
        let member_b = sh_cosmovisor([
            "query market show-member",
            coin_pair.coin_b(),
            coin_pair.coin_a(),
        ])
        .await
        .stack()?;
        Ok((member_a, member_b))
    }

    pub async fn create_drop(&self, coin_pair: &CoinPair, drops: U256) -> Result<()> {
        self.configured_tx([
            "market create-drop",
            &coin_pair.paired(),
            &format!("{}", drops),
        ])
        .await
        .stack()?;
        Ok(())
    }

    pub async fn redeem_drop(&self, uid: u64) -> Result<()> {
        self.configured_tx(["market redeem-drop", &format!("{}", uid)])
            .await
            .stack()?;
        Ok(())
    }

    pub async fn market_order(
        &self,
        coin_ask: &str,
        amount_ask: U256,
        coin_bid: &str,
        amount_bid: U256,
        slippage: u16,
    ) -> Result<()> {
        self.configured_tx([
            "market market-order",
            coin_ask,
            &format!("{}", amount_ask),
            coin_bid,
            &format!("{}", amount_bid),
            &format!("{}", slippage),
        ])
        .await
        .stack()?;
        Ok(())
    }

    pub async fn create_order(
        &self,
        coin_ask: &str,
        coin_bid: &str,
        order_type: &str,
        amount: U256,
        rate: (u64, u64),
        prev_next: (u64, u64),
    ) -> Result<()> {
        self.configured_tx([
            "market create-order",
            coin_ask,
            coin_bid,
            order_type,
            &format!("{}", amount),
            &format!("{},{}", rate.0, rate.1),
            &format!("{}", prev_next.0),
            &format!("{}", prev_next.1),
        ])
        .await
        .stack()?;
        Ok(())
    }

    pub async fn cancel_order(&self, uid: u64) -> Result<()> {
        self.configured_tx(["market cancel-order", &format!("{}", uid)])
            .await
            .stack()?;
        Ok(())
    }
}
