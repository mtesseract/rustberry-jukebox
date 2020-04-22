FROM ubuntu:latest

RUN apt-get update && apt-get dist-upgrade --yes \
    && apt-get install --yes vi curl git pkg-config libssl-dev openssh-server \
    && useradd -ms /bin/bash builder

ARG RASPBERRY_PI_TOOLS_COMMIT_ID=b0c869bc929587a7e1d20a98e2dc828a24ca396a
RUN curl -sL https://github.com/raspberrypi/tools/archive/$RASPBERRY_PI_TOOLS_COMMIT_ID.tar.gz \
    | tar xzf - -C /usr/local --strip-components=1 tools-${RASPBERRY_PI_TOOLS_COMMIT_ID}/arm-bcm2708

ENV PATH=/usr/local/arm-bcm2708/arm-linux-gnueabihf/bin:$PATH
ENV PATH=/usr/local/arm-bcm2708/arm-linux-gnueabihf/libexec/gcc/arm-linux-gnueabihf/4.9.3:$PATH

RUN cd /usr/local && \
    VERS="1.1.1g" && \
    curl -O "https://www.openssl.org/source/openssl-$VERS.tar.gz" && \
    tar xzf "openssl-$VERS.tar.gz" && \
    mv openssl-$VERS openssl && \
    export MACHINE=armv6 && \
    export ARCH=arm && \
    export CC=arm-linux-gnueabihf-gcc && \
    cd openssl && \
    ./config shared && \
    make && \
    cd /usr/local && \
    curl -O https://www.alsa-project.org/files/pub/lib/alsa-lib-1.2.2.tar.bz2 && \
    tar xjf alsa-lib-1.2.2.tar.bz2 && \
    cd alsa-lib-1.2.2 && \
    export CC=arm-linux-gnueabihf-gcc && \
    ./configure --host=arm-linux-gnueabihf && \
    make && \
    make install

RUN mkdir /cache && chown -R builder /cache

USER builder 

RUN curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y \
    && . ~/.cargo/env \
    && echo "source ~/.cargo/env > ~builder/.bash_profile" \
    && rustup target add arm-unknown-linux-gnueabihf


RUN echo "[target.arm-unknown-linux-gnueabihf]" > /home/builder/.cargo/config && \
    echo "linker = \"arm-linux-gnueabihf-gcc\"" >> /home/builder/.cargo/config && \
    chown builder /home/builder/.cargo/config

ADD rustberry-builder.pub /home/builder/.ssh/authorized_keys

VOLUME /cache

USER root

CMD /etc/init.d/ssh start && while true; do sleep 60; done
