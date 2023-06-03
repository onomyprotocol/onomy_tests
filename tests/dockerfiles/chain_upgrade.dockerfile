FROM fedora:38
RUN dnf install -y git make cmake gcc gcc-c++ which iproute iputils procps-ng vim-minimal tmux net-tools htop tar jq npm openssl-devel perl rust cargo golang
RUN go install cosmossdk.io/tools/cosmovisor/cmd/cosmovisor@latest
ENV PATH=$PATH:/root/go/bin

ENV DAEMON_NAME="onomyd"
ENV DAEMON_HOME="/root/.onomy"
ENV ONOMY_CURRENT_VERSION=v1.0.3.5
ENV ONOMY_UPGRADE_VERSION=v1.1.0
# under some circumstances such as versions with capitals, this needs to be changed (but try to
# avoid this problem in the first place)
ENV ONOMY_UPGRADE_DIR_NAME=$ONOMY_UPGRADE_VERSION

ADD https://github.com/onomyprotocol/onomy/releases/download/$ONOMY_CURRENT_VERSION/onomyd $DAEMON_HOME/cosmovisor/genesis/$ONOMY_CURRENT_VERSION/bin/onomyd
#ADD ./dockerfile_resources/onomyd $DAEMON_HOME/cosmovisor/upgrades/$ONOMY_UPGRADE_DIR_NAME/bin/onomyd
ADD https://github.com/onomyprotocol/onomy/releases/download/$ONOMY_UPGRADE_VERSION/onomyd $DAEMON_HOME/cosmovisor/upgrades/$ONOMY_UPGRADE_DIR_NAME/bin/onomyd

# for manual testing
RUN chmod +x $DAEMON_HOME/cosmovisor/genesis/$ONOMY_CURRENT_VERSION/bin/onomyd
RUN chmod +x $DAEMON_HOME/cosmovisor/upgrades/$ONOMY_UPGRADE_DIR_NAME/bin/onomyd

# set up symbolic links
RUN cosmovisor init $DAEMON_HOME/cosmovisor/genesis/$ONOMY_CURRENT_VERSION/bin/onomyd

# some commands don't like if the data directory does not exist
RUN mkdir $DAEMON_HOME/data
