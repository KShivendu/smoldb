#!/bin/bash
set -eoux pipefail

cargo build # intentionally not release build to get debug logs
cargo test -p smolbench -- --nocapture
