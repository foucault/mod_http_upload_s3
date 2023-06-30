#!/bin/sh

img=${1:-'rust:slim'}

sudo podman run --rm \
  --user "$(id -u)":"$(id -g)" \
  -v "$PWD":/io \
  -w /io $img \
  cargo build --release

if [[ -f "${PWD}/target/release/libluas3put.so" ]]; then
  cp "${PWD}/target/release/libluas3put.so" \
    "${PWD}/target/release/luas3put.so"
fi
