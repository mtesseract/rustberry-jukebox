#!/bin/sh

set -e
set -x

docker run --restart always --name rustberry-builder -d -p 4022:22 --mount source=rustberry-builder,target=/cache rustberry-builder:latest
