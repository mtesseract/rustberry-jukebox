#!/bin/sh

set -e
set -x

docker run --restart always --name rustberry-builder-armv6 -d -p 4023:22 --mount source=rustberry-builder,target=/cache rustberry-builder-armv6:latest
