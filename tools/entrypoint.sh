#!/bin/bash

set -e

if [ "$1" = "smoldb" ] || [ -z "$1" ] || [[ "$1" == -* ]]; then
  # If the first argument is 'smoldb' or empty or an option (starts with -), run smoldb with all args
  exec /smoldb "$@"
else
  # Otherwise, run the command as-is (e.g., /bin/bash)
  exec "$@"
fi
