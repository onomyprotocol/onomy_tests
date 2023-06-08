FROM fedora:38
RUN dnf install -y git make cmake gcc gcc-c++ which iproute iputils procps-ng vim-minimal tmux net-tools htop tar jq npm openssl-devel perl rust cargo golang

ADD https://gethstore.blob.core.windows.net/builds/geth-linux-amd64-1.12.0-e501b3b0.tar.gz /tmp/geth.tar.gz
RUN cd /tmp && tar -xvf * && mv /tmp/geth-linux-amd64-1.12.0-e501b3b0/geth /usr/bin/geth

RUN mkdir /resources
