#!/bin/bash
set -eoux pipefail

cargo test -p smolbench
