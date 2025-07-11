#!/bin/bash
set -eoux pipefail

cargo build
cargo test -p smolbench
