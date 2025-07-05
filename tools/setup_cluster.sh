#!/bin/bash

# SmolDB Cluster Startup Script
# This script starts two SmolDB instances in separate terminals with isolated directories

SMOLDB="./target/debug/smoldb"
CLUSTER_DIR="cluster"

# Get absolute path to smoldb binary for use in subshells
SMOLDB_ABS=$(realpath "$SMOLDB")
echo "Using SmolDB binary: $SMOLDB_ABS"

# Create cluster directory structure
echo "Setting up cluster directory structure..."
mkdir -p "$CLUSTER_DIR"
cd "$CLUSTER_DIR"
mkdir -p node1 node2

# Function to detect terminal emulator and open new terminal
open_terminal() {
    local command="$1"
    local title="$2"

    # Try different terminal emulators
    if command -v gnome-terminal >/dev/null 2>&1; then
        gnome-terminal --title="$title" -- bash -c "$command; exec bash"
    elif command -v xterm >/dev/null 2>&1; then
        xterm -title "$title" -e "bash -c '$command; exec bash'" &
    elif command -v konsole >/dev/null 2>&1; then
        konsole --title "$title" -e bash -c "$command; exec bash" &
    elif command -v terminator >/dev/null 2>&1; then
        terminator --title="$title" -e "bash -c '$command; exec bash'" &
    elif command -v alacritty >/dev/null 2>&1; then
        alacritty --title "$title" -e bash -c "$command; exec bash" &
    elif command -v kitty >/dev/null 2>&1; then
        kitty --title "$title" bash -c "$command; exec bash" &
    elif [[ "$OSTYPE" == "darwin"* ]]; then
        # macOS Terminal
        osascript -e "tell application \"Terminal\" to do script \"$command\""
    else
        echo "Error: No supported terminal emulator found"
        echo "Please install one of: gnome-terminal, xterm, konsole, terminator, alacritty, kitty"
        exit 1
    fi
}

echo "Starting SmolDB cluster..."
echo "Node 1: Peer ID 101, HTTP: 0.0.0.0:9001, P2P: 0.0.0.0:5001"
echo "Node 2: Peer ID 102, HTTP: 0.0.0.0:9002, P2P: 0.0.0.0:5002"
echo ""


# Start first node (bootstrap node)
NODE1_CMD="cd node1 && $SMOLDB_ABS --url 0.0.0.0:9001 --p2p-url 0.0.0.0:5001 --peer-id 101"
echo "Starting Node 1 (Bootstrap) in ./cluster/node1..."
open_terminal "$NODE1_CMD" "SmolDB Node 1 (Bootstrap)"


echo "Waiting 5s for Node 1 to initialize..."

# Wait a moment for the first node to start
sleep 5

echo "Initializing Node 2 (Peer)..."

# Start second node (connects to bootstrap)
NODE2_CMD="cd node2 && $SMOLDB_ABS --url 0.0.0.0:9002 --p2p-url 0.0.0.0:5002 --bootstrap http://0.0.0.0:5001 --peer-id 102"
echo "Starting Node 2 (Peer) in ./cluster/node2..."
open_terminal "$NODE2_CMD" "SmolDB Node 2 (Peer)"

echo ""
echo "SmolDB cluster startup initiated!"
echo ""
echo "Node 1 (Bootstrap): http://0.0.0.0:9001"
echo "Node 2 (Peer):      http://0.0.0.0:9002"
echo ""
echo "To stop the cluster, close the terminal windows or press Ctrl+C in each terminal."
echo ""
echo "You can now interact with either node via HTTP API:"
echo "  curl http://0.0.0.0:9001/health"
echo "  curl http://0.0.0.0:9002/health"
