#!/bin/bash

set -e

if [ "$1" = "smoldb" ] || [ "${1#-}" != "$1" ]; then
  # If the first argument is 'smoldb' or an option (starts with -), run smoldb with all args
  exec /smoldb "$@"
else
  # Otherwise, run the command as-is (e.g., bash)
  exec "$@"
fi
