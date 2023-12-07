pub const ONOMY_BASE: &str = "fedora:39";

#[rustfmt::skip]
pub const ONOMY_STD: &str = r#"FROM fedora:39
RUN dnf install -y git rust cargo make cmake gcc gcc-c++ golang perl npm which iproute iputils net-tools procps-ng vim-minimal tmux htop tar jq openssl-devel pass
"#;

pub const COSMOVISOR: &str = r#"RUN go install cosmossdk.io/tools/cosmovisor/cmd/cosmovisor@latest
ENV PATH=$PATH:/root/go/bin
"#;

#[rustfmt::skip]
pub const HERMES: &str = r#"ADD https://github.com/informalsystems/hermes/releases/download/v1.7.1/hermes-v1.7.1-x86_64-unknown-linux-gnu.tar.gz /root/.hermes/bin/
RUN cd /root/.hermes/bin/ && tar -vxf *
ENV PATH=$PATH:/root/.hermes/bin
ENV HERMES_HOME="/root/.hermes"
"#;

pub fn dockerfile_hermes(config_resource: &str) -> String {
    format!(
        r#"{ONOMY_STD}

{HERMES}

ADD ./dockerfile_resources/{config_resource} $HERMES_HOME/config.toml
"#
    )
}

//ADD https://github.com/onomyprotocol/onomy/releases/download/$DAEMON_VERSION/{daemon_name}
//$DAEMON_HOME/cosmovisor/genesis/$DAEMON_VERSION/bin/{daemon_name}

pub fn onomy_std_cosmos_daemon_with_arbitrary(
    daemon_name: &str,
    daemon_dir_name: &str,
    version: &str,
    arbitrary: &str,
) -> String {
    format!(
        r#"{ONOMY_STD}
{COSMOVISOR}

ENV DAEMON_NAME="{daemon_name}"
ENV DAEMON_HOME="/root/{daemon_dir_name}"
ENV DAEMON_VERSION={version}

{arbitrary}

# for manual testing
RUN chmod +x $DAEMON_HOME/cosmovisor/genesis/$DAEMON_VERSION/bin/{daemon_name}

# set up symbolic links
RUN cosmovisor init $DAEMON_HOME/cosmovisor/genesis/$DAEMON_VERSION/bin/{daemon_name}

# some commands don't like if the data directory does not exist
RUN mkdir $DAEMON_HOME/data
"#
    )
}

#[rustfmt::skip]
pub fn onomy_std_cosmos_daemon(
    daemon_name: &str,
    daemon_dir_name: &str,
    version: &str,
    dockerfile_resource: &str,
) -> String {
    let arbitrary = format!(
        r#"ADD ./dockerfile_resources/{dockerfile_resource} $DAEMON_HOME/cosmovisor/genesis/$DAEMON_VERSION/bin/{daemon_name}"#
    );
    onomy_std_cosmos_daemon_with_arbitrary(daemon_name, daemon_dir_name, version, &arbitrary)
}

pub const ONOMYD_VERSION: &str = "v1.1.4";
pub const ONEXD_VERSION: &str = "v1.0.3-onex";
pub const ONEXD_FH_VERSION: &str = "v1.0.3-onex-fh";
pub const STANDALONE_ONEX_VERSION: &str = "v1.1.0";
pub const STANDALONE_ONEX_FH_VERSION: &str = "v1.1.0-fh";

#[rustfmt::skip]
pub const DOWNLOAD_ONOMYD: &str = r#"ADD https://github.com/onomyprotocol/onomy/releases/download/$DAEMON_VERSION/onomyd $DAEMON_HOME/cosmovisor/genesis/$DAEMON_VERSION/bin/onomyd"#;

pub fn dockerfile_onomyd() -> String {
    onomy_std_cosmos_daemon_with_arbitrary("onomyd", ".onomy", ONOMYD_VERSION, DOWNLOAD_ONOMYD)
}

#[rustfmt::skip]
pub const DOWNLOAD_ONEXD: &str = r#"ADD https://github.com/onomyprotocol/multiverse/releases/download/$DAEMON_VERSION/onexd $DAEMON_HOME/cosmovisor/genesis/$DAEMON_VERSION/bin/onexd"#;

pub fn dockerfile_onexd() -> String {
    onomy_std_cosmos_daemon_with_arbitrary("onexd", ".onomy_onex", ONEXD_VERSION, DOWNLOAD_ONEXD)
}

#[rustfmt::skip]
pub const DOWNLOAD_STANDALONE_ONEXD: &str = r#"ADD https://github.com/onomyprotocol/market/releases/download/$DAEMON_VERSION/marketd $DAEMON_HOME/cosmovisor/genesis/$DAEMON_VERSION/bin/marketd"#;

pub fn dockerfile_standalone_onexd() -> String {
    onomy_std_cosmos_daemon_with_arbitrary(
        "marketd",
        ".market",
        STANDALONE_ONEX_VERSION,
        DOWNLOAD_STANDALONE_ONEXD,
    )
}
