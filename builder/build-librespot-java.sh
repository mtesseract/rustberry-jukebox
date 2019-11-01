#!/bin/sh

set -e
DIR=$(ssh rustberry-builder mktemp -d)

ssh rustberry-builder "\
set -x && \
cd $DIR && \
git clone https://github.com/librespot-org/librespot-java.git && \
cd librespot-java && \
git checkout new-spotify-api && \
mvn clean package
"

scp rustberry-builder:$DIR/librespot-java/core/target/librespot-core-jar-with-dependencies.jar .
