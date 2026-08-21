#!/bin/sh
set -e

cd "$(dirname "$0")/.."

if ! podman image exists localhost/wander-build; then
	podman build -t localhost/wander-build -f ci/Containerfile ci
fi

exec podman run --rm \
	-v "$(pwd)":/wander \
	-v wander-cargo-registry:/usr/local/cargo/registry \
	-v wander-cargo-git:/usr/local/cargo/git \
	-v wander-target:/wander/target \
	localhost/wander-build \
	cargo test --workspace "$@"
