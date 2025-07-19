#!/bin/bash

COMMAND="$1"
shift  # Remove first argument, rest are passed to the command

# Check if there are unstaged changes
STASHED=0
if ! git diff --quiet; then
    echo "🗃️  Stashing unstaged changes..."
    git stash push -m "rusty-hook temporary stash" --keep-index --include-untracked
    STASHED=1
fi

# Function to restore stash on exit
restore_stash() {
    if [ "$STASHED" = "1" ]; then
        echo "📦 Restoring stashed changes..."
        if git stash list | grep -q "rusty-hook temporary stash"; then
            git stash pop
        else
            echo "⚠️  Warning: Could not find stash to restore"
        fi
    fi
}

# Set trap to restore stash on exit (success or failure)
trap restore_stash EXIT

# Run the actual command and capture its exit status
set -e  # Exit on error
echo "🚀 Running: $COMMAND $*"
eval "$COMMAND $*"
