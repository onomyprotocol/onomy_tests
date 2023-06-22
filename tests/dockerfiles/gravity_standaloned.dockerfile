FROM fedora:38
RUN dnf install -y git make cmake gcc gcc-c++ which iproute iputils procps-ng vim-minimal tmux net-tools htop tar jq npm openssl-devel perl rust cargo golang
RUN go install cosmossdk.io/tools/cosmovisor/cmd/cosmovisor@latest
ENV PATH=$PATH:/root/go/bin

ENV DAEMON_NAME="gravity"
ENV DAEMON_HOME="/root/.gravity"
ENV GRAVITY_CURRENT_VERSION=v0.1.0

ADD ./dockerfile_resources/gravity $DAEMON_HOME/cosmovisor/genesis/$GRAVITY_CURRENT_VERSION/bin/gravity

# for manual testing
RUN chmod +x $DAEMON_HOME/cosmovisor/genesis/$GRAVITY_CURRENT_VERSION/bin/gravity

# set up symbolic links
RUN cosmovisor init $DAEMON_HOME/cosmovisor/genesis/$GRAVITY_CURRENT_VERSION/bin/gravity

# some commands don't like if the data directory does not exist
RUN mkdir $DAEMON_HOME/data
