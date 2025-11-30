#!/bin/bash
set -eoux pipefail

cargo build -r
cargo test -p smolbench -- --nocapture
