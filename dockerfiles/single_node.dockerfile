FROM fedora:38
RUN dnf install -y git make cmake gcc gcc-c++ which iproute iputils procps-ng vim-minimal tmux net-tools htop tar jq npm openssl-devel perl rust cargo golang
RUN npm install -g ts-node && npm install -g typescript
RUN go install cosmossdk.io/tools/cosmovisor/cmd/cosmovisor@latest
ENV PATH=$PATH:/root/go/bin

ENV DAEMON_NAME="onomyd"
ENV DAEMON_HOME="/root/.onomy"
ENV ONOMY_CURRENT_VERSION=v1.0.3

ADD https://github.com/onomyprotocol/onomy/releases/download/$ONOMY_CURRENT_VERSION/onomyd $DAEMON_HOME/cosmovisor/genesis/$ONOMY_CURRENT_VERSION/bin/onomyd

# for manual testing
RUN chmod +x $DAEMON_HOME/cosmovisor/genesis/$ONOMY_CURRENT_VERSION/bin/onomyd

# set up symbolic links
RUN cosmovisor init $DAEMON_HOME/cosmovisor/genesis/$ONOMY_CURRENT_VERSION/bin/onomyd

# some commands don't like if the data directory does not exist
RUN mkdir $DAEMON_HOME/data

ENV GOV_PERIOD="30s"
