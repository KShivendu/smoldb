#!/bin/bash
set -eoux pipefail

COVERAGE=${COVERAGE:-"0"}

if [ "$COVERAGE" == "1" ]; then
    cargo tarpaulin -o Xml --output-dir coverage/unit --lib
else
    cargo test --lib
fi
