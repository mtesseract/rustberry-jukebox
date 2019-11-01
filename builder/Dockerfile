FROM ubuntu:latest

RUN apt-get update && apt-get dist-upgrade --yes \
    && apt-get install --yes curl git gcc gcc-multilib-arm-linux-gnueabihf pkg-config libssl-dev \
    && useradd -ms /bin/bash builder

RUN cd /usr/local && \
    VERS="1.1.1d" && \
    curl -O "https://www.openssl.org/source/openssl-$VERS.tar.gz" && \
    tar xzf "openssl-$VERS.tar.gz" && \
    mv openssl-$VERS openssl && \
    export MACHINE=armv7 && \
    export ARCH=arm && \
    export CC=arm-linux-gnueabihf-gcc && \
    cd openssl && \
    ./config shared && \
    make

RUN apt-get clean
RUN apt-get update && apt-get install openssh-server --yes

RUN su builder bash -c '\
    curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y \
    && . ~/.cargo/env \
    && echo "source ~/.cargo/env > ~builder/.bash_profile" \
    && rustup target add armv7-unknown-linux-gnueabihf \
    '

RUN su builder bash -c '\
    echo -e "[target.armv7-unknown-linux-gnueabihf]\nlinker = \"arm-linux-gnueabihf-gcc-7\"\n" > ~/.cargo/config \
    '

RUN apt-get update && apt-get install --yes maven

ADD rustberry-builder.pub /home/builder/.ssh/authorized_keys

RUN mkdir /cache && chown -R builder /cache
VOLUME /cache

CMD /etc/init.d/ssh start && while true; do sleep 60; done
