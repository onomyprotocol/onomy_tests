use super_orchestrator::{stacked_errors::Result, FileOptions};

#[rustfmt::skip]
const HEADER: &str = r##"# The global section has parameters that apply globally to the relayer operation.
[global]
log_level = 'info'

# Specify the mode to be used by the relayer. [Required]
[mode]

# Specify the client mode.
[mode.clients]

# Whether or not to enable the client workers. [Required]
enabled = true

# Whether or not to enable periodic refresh of clients. [Default: true]
# This feature only applies to clients that underlie an open channel.
# For Tendermint clients, the frequency at which Hermes refreshes them is 2/3 of their
# trusting period (e.g., refresh every ~9 days if the trusting period is 14 days).
# Note: Even if this is disabled, clients will be refreshed automatically if
#      there is activity on a connection or channel they are involved with.
refresh = true

# Whether or not to enable misbehaviour detection for clients. [Default: true]
misbehaviour = true

# Specify the connections mode.
[mode.connections]

# Whether or not to enable the connection workers for handshake completion. [Required]
enabled = false

# Specify the channels mode.
[mode.channels]

# Whether or not to enable the channel workers for handshake completion. [Required]
enabled = false

# Specify the packets mode.
[mode.packets]

# Whether or not to enable the packet workers. [Required]
enabled = true

# Parametrize the periodic packet clearing feature.
# Interval (in number of blocks) at which pending packets
# should be periodically cleared. A value of '0' will disable
# periodic packet clearing. [Default: 100]
clear_interval = 0

# Whether or not to clear packets on start. [Default: true]
clear_on_start = true

# Toggle the transaction confirmation mechanism.
# The tx confirmation mechanism periodically queries the `/tx_search` RPC
# endpoint to check that previously-submitted transactions
# (to any chain in this config file) have been successfully delivered.
# If they have not been, and `clear_interval = 0`, then those packets are
# queued up for re-submission.
# If set to `false`, the following telemetry metrics will be disabled:
# `acknowledgment_packets_confirmed`, `receive_packets_confirmed` and `timeout_packets_confirmed`.
# [Default: false]
tx_confirmation = false

# Auto register the counterparty payee on a destination chain to
# the relayer's address on the source chain. This can be used
# for simple configuration of the relayer to receive fees for
# relaying RecvPacket on fee-enabled channels.
# For more complex configuration, turn this off and use the CLI
# to manually register the payee addresses.
# [Default: false]
auto_register_counterparty_payee = false

# The REST section defines parameters for Hermes' built-in RESTful API.
# https://hermes.informal.systems/rest.html
[rest]

# Whether or not to enable the REST service. Default: false
enabled = true

# Specify the IPv4/6 host over which the built-in HTTP server will serve the RESTful
# API requests. Default: 127.0.0.1
host = '127.0.0.1'

# Specify the port over which the built-in HTTP server will serve the restful API
# requests. Default: 3000
port = 3000


# The telemetry section defines parameters for Hermes' built-in telemetry capabilities.
# https://hermes.informal.systems/telemetry.html
[telemetry]

# Whether or not to enable the telemetry service. Default: false
enabled = false

# Specify the IPv4/6 host over which the built-in HTTP server will serve the metrics
# gathered by the telemetry service. Default: 127.0.0.1
host = '127.0.0.1'

# Specify the port over which the built-in HTTP server will serve the metrics gathered
# by the telemetry service. Default: 3001
port = 3001

"##;

/*
[[chains]]
# Specify the chain ID. Required
id = 'onomy'

# Whether or not this is a CCV consumer chain. Default: false
# Only specifiy true for CCV consumer chain, but NOT for sovereign chains.
ccv_consumer_chain = false

# Specify the RPC address and port where the chain RPC server listens on. Required
rpc_addr = 'http://onomyd:26657'

# Specify the GRPC address and port where the chain GRPC server listens on. Required
grpc_addr = 'http://onomyd:9090'

# Specify the WebSocket address and port where the chain WebSocket server
# listens on. Required
websocket_addr = 'ws://onomyd:26657/websocket'
event_source = { mode = 'push', url = '$WS_URL', batch_delay = '200ms' }

# Specify the maximum amount of time (duration) that the RPC requests should
# take before timing out. Default: 10s (10 seconds)
# Note: Hermes uses this parameter _only_ in `start` mode; for all other CLIs,
# Hermes uses a large preconfigured timeout (on the order of minutes).
rpc_timeout = '10s'

# Specify the prefix used by the chain. Required
account_prefix = 'onomy'

# Specify the name of the private key to use for signing transactions. Required
# See the Adding Keys chapter for more information about managing signing keys:
#   https://hermes.informal.systems/commands/keys/index.html#adding-keys
key_name = 'validator'

# Specify the folder used to store the keys. Optional
# If this is not specified then the hermes home folder is used.
# key_store_folder = '$HOME/.hermes/keys'

# Specify the address type which determines:
# 1) address derivation;
# 2) how to retrieve and decode accounts and pubkeys;
# 3) the message signing method.
# The current configuration options are for Cosmos SDK and Ethermint.
#
# Example configuration for chains based on Ethermint library:
#
# address_type = { derivation = 'ethermint', proto_type
# = { pk_type = '/ethermint.crypto.v1.ethsecp256k1.PubKey' } }
#
# Default: { derivation = 'cosmos' }, i.e. address derivation as in Cosmos SDK.
# Warning: This is an advanced feature! Modify with caution.
address_type = { derivation = 'cosmos' }

# Specify the store prefix used by the on-chain IBC modules. Required
# Recommended value for Cosmos SDK: 'ibc'
store_prefix = 'ibc'

# Gas Parameters
#
# The term 'gas' is used to denote the amount of computation needed to execute
# and validate a transaction on-chain. It can be thought of as fuel that gets
# spent in order to power the on-chain execution of a transaction.
#
# Hermes attempts to simulate how much gas a transaction will expend on its
# target chain. From that, it calculates the cost of that gas by multiplying the
# amount of estimated gas by the `gas_multiplier` and the `gas_price`
# (estimated gas * `gas_multiplier` * `gas_price`) in order to compute the
# total fee to be deducted from the relayer's wallet.
#
# The `simulate_tx` operation does not always correctly estimate the appropriate
# amount of gas that a transaction requires. In those cases when the operation
# fails, Hermes will attempt to submit the transaction using the specified
# `default_gas` and `max_gas` parameters. In the case that a transaction would
# require more than `max_gas`, it doesn't get submitted and a
# `TxSimulateGasEstimateExceeded` error is returned.

# Specify the default amount of gas to be used in case the tx simulation fails,
# and Hermes cannot estimate the amount of gas needed.
# Default: 100 000
default_gas = 100000

# Specify the maximum amount of gas to be used as the gas limit for a transaction.
# If `default_gas` is unspecified, then `max_gas` will be used as `default_gas`.
# Default: 400 000
max_gas = 400000

# Specify the price per gas used of the fee to submit a transaction and
# the denomination of the fee.
#
# The specified gas price should always be greater or equal to the `min-gas-price`
# configured on the chain. This is to ensure that at least some minimal price is
# paid for each unit of gas per transaction.
#
# Required
gas_price = { price = 1, denom = 'anom' }

# Multiply this amount with the gas estimate, used to compute the fee
# and account for potential estimation error.
#
# The purpose of multiplying by `gas_multiplier` is to provide a bit of a buffer
# to catch some of the cases when the gas estimation calculation is on the low
# end.
#
# Example: With this setting set to 1.1, then if the estimated gas
# is 80_000, then gas used to compute the fee will be adjusted to
# 80_000 * 1.1 = 88_000.
#
# Default: 1.1, ie. the gas is increased by 10%
# Minimum value: 1.0
gas_multiplier = 1.1

# Specify how many IBC messages at most to include in a single transaction.
# Default: 30
max_msg_num = 30

# Specify the maximum size, in bytes, of each transaction that Hermes will submit.
# Default: 2097152 (2 MiB)
max_tx_size = 2097152

# Specify the maximum amount of time to tolerate a clock drift.
# The clock drift parameter defines how much new (untrusted) header's time
# can drift into the future. Default: 5s
clock_drift = '5s'

# Specify the maximum time per block for this chain.
# The block time together with the clock drift are added to the source drift to estimate
# the maximum clock drift when creating a client on this chain. Default: 30s
# For cosmos-SDK chains a good approximation is `timeout_propose` + `timeout_commit`
# Note: This MUST be the same as the `max_expected_time_per_block`
# genesis parameter for Tendermint chains.
max_block_time = '2s'

# Specify the amount of time to be used as the light client trusting period.
# It should be significantly less than the unbonding period
# (e.g. unbonding period = 3 weeks, trusting period = 2 weeks).
# Default: 2/3 of the `unbonding period` for Cosmos SDK chains
trusting_period = '14days'

# Specify the trust threshold for the light client, ie. the minimum fraction of validators
# which must overlap across two blocks during light client verification.
# Default: { numerator = '2', denominator = '3' }, ie. 2/3.
# Warning: This is an advanced feature! Modify with caution.
trust_threshold = { numerator = '2', denominator = '3' }

# Specify a string that Hermes will use as a memo for each transaction it submits
# to this chain. The string is limited to 50 characters. Default: '' (empty).
# Note: Hermes will append to the string defined here additional
# operational debugging information, e.g., relayer build version.
memo_prefix = ''

# This section specifies the filters for policy based relaying.
#
# Default: no policy / filters, allow all packets on all channels.
#
# Only packet filtering based on channel identifier can be specified.
# A channel filter has two fields:
# 1. `policy` - one of two types are supported:
#       - 'allow': permit relaying _only on_ the port/channel id in the list below,
#       - 'deny': permit relaying on any channel _except for_ the list below.
# 2. `list` - the list of channels specified by the port and channel identifiers.
#             Optionally, each element may also contains wildcards, for eg. 'ica*'
#             to match all identifiers starting with 'ica' or '*' to match all identifiers.
#
# Example configuration of a channel filter, only allowing packet relaying on
# channel with port ID 'transfer' and channel ID 'channel-0', as well as on
# all ICA channels.
#
# [chains.packet_filter]
# policy = 'allow'
# list = [
#   ['ica*', '*'],
#   ['transfer', 'channel-0'],
# ]

# This section specifies the filters for incentivized packet relaying.
# Default: no filters, will relay all packets even if they
# are not incentivized.
#
# It is possible to specify the channel or use wildcards for the
# channels.
# The only fee which can be parametrized is the `recv_fee`.
#
# Example configuration of a filter which will only relay incentivized
# packets, with no regards for channel and amount.
#
# [chains.packet_filter.min_fees.'*']
# recv = [ { amount = 0 } ]
#
# Example configuration of a filter which will only relay packets if they are
# from the channel 'channel-0', and they have a `recv_fee` of at least 20 stake
# or 10 uatom.
#
# [chains.packet_filter.min_fees.'channel-0']
# recv = [ { amount = 20, denom = 'stake' }, { amount = 10, denom = 'uatom' } ]

# Specify that the transaction fees should be payed from this fee granter's account.
# Optional. If unspecified (the default behavior), then no fee granter is used, and
# the account specified in `key_name` will pay the tx fees for all transactions
# submitted to this chain.
# fee_granter = ''
*/

pub struct HermesChainConfig {
    pub chain_id: String,
    pub rpc_addr: String,
    pub grpc_addr: String,
    pub event_addr: String,
    pub account_prefix: String,
    pub ccv_consumer_chain: bool,
    pub gas_denom: String,
    pub fast_block_times: bool,
}

impl HermesChainConfig {
    pub fn new(
        chain_id: &str,
        hostname: &str,
        account_prefix: &str,
        ccv_consumer_chain: bool,
        gas_denom: &str,
        fast_block_times: bool,
    ) -> Self {
        Self {
            chain_id: chain_id.to_owned(),
            rpc_addr: format!("http://{hostname}:26657"),
            grpc_addr: format!("http://{hostname}:9090"),
            event_addr: format!("ws://{hostname}:26657/websocket"),
            account_prefix: account_prefix.to_owned(),
            ccv_consumer_chain,
            gas_denom: gas_denom.to_owned(),
            fast_block_times,
        }
    }
}

impl ToString for HermesChainConfig {
    fn to_string(&self) -> String {
        let chain_id = &self.chain_id;
        let rpc_addr = &self.rpc_addr;
        let grpc_addr = &self.grpc_addr;
        let event_addr = &self.event_addr;
        let account_prefix = &self.account_prefix;
        let ccv_consumer_chain = self.ccv_consumer_chain;
        let gas_denom = &self.gas_denom;
        let max_block_time = if self.fast_block_times { "2s" } else { "30s" };
        format!(
            r##"[[chains]]
id = '{chain_id}'
ccv_consumer_chain = {ccv_consumer_chain}
rpc_addr = '{rpc_addr}'
grpc_addr = '{grpc_addr}'
event_source = {{ mode = 'push', url = '{event_addr}', batch_delay = '200ms' }}
rpc_timeout = '10s'
account_prefix = '{account_prefix}'
key_name = 'validator'
store_prefix = 'ibc'
default_gas = 100000
max_gas = 400000
# note: this is changed to IBC NOM during bootstrap
gas_price = {{ price = 1, denom = '{gas_denom}' }}
gas_multiplier = 1.1
max_msg_num = 30
max_tx_size = 2097152
clock_drift = '5s'
max_block_time = '{max_block_time}'
trusting_period = '14days'
trust_threshold = {{ numerator = '1', denominator = '3' }}
address_type = {{ derivation = 'cosmos' }}

"##
        )
    }
}

pub async fn write_hermes_config(
    chain_configs: &[HermesChainConfig],
    write_dir: &str,
) -> Result<()> {
    let mut s = HEADER.to_owned();
    for config in chain_configs {
        s += &config.to_string();
    }
    FileOptions::write_str(&format!("{write_dir}/__tmp_hermes_config.toml"), &s).await
}
