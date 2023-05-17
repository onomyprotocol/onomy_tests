FROM fedora:38
RUN dnf install -y git make cmake gcc gcc-c++ which iproute iputils procps-ng vim-minimal tmux net-tools htop tar jq npm openssl-devel perl rust cargo golang
RUN npm install -g ts-node && npm install -g typescript
RUN go install cosmossdk.io/tools/cosmovisor/cmd/cosmovisor@latest
ENV PATH=$PATH:/root/go/bin

ENV DAEMON_NAME="marketd"
ENV DAEMON_HOME="/root/.onomy_market"
ENV MARKET_CURRENT_VERSION=v0.1.0

# FIXME
ADD ./dockerfile_resources/marketd $DAEMON_HOME/cosmovisor/genesis/$MARKET_CURRENT_VERSION/bin/marketd
#ADD https://github.com/pendulum-labs/market/releases/download/$MARKET_CURRENT_VERSION/marketd $DAEMON_HOME/cosmovisor/genesis/$MARKET_CURRENT_VERSION/bin/marketd

# for manual testing
RUN chmod +x $DAEMON_HOME/cosmovisor/genesis/$MARKET_CURRENT_VERSION/bin/marketd

# set up symbolic links
RUN cosmovisor init $DAEMON_HOME/cosmovisor/genesis/$MARKET_CURRENT_VERSION/bin/marketd

# some commands don't like if the data directory does not exist
RUN mkdir $DAEMON_HOME/data
