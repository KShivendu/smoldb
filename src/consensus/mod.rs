pub mod broker;
pub mod debuggables;
pub mod manager;
pub mod raft_storage;
pub mod ready_processor;
pub mod state;
pub mod utils;

use crate::{
    api::grpc::{
        make_grpc_channel,
        schema::{raft_client::RaftClient, AddPeerToKnownMessage},
    },
    consensus::{
        manager::ConsensusManager, raft_storage::RaftStorage, state::ConsensusState,
        utils::add_peer_to_toc_and_consensus_state,
    },
    storage::toc::{CollectionOperation, TableOfContent, STORAGE_DIR},
    types::PeerId,
};
use http::Uri;
use log::{debug, error, info, trace};
use raft::{
    prelude::{ConfChange, ConfChangeType, Entry, Message},
    Config, RawNode,
};
use slog::{o, Drain};
use std::{
    collections::HashMap,
    error::Error,
    path::Path,
    sync::{
        mpsc::{Receiver, RecvTimeoutError},
        Arc,
    },
    thread::{self},
    time::{Duration, Instant},
};
use tokio::runtime::Handle;

type ProposalId = u64;

const RAFT_TICK_INTERVAL: Duration = Duration::from_millis(100);
const RAFT_ELECTION_TICK_MS: usize = 10;
const RAFT_HEARTBEAT_TICK_MS: usize = 3;
pub const CONSENSUS_DIR: &str = "consensus";
/// Holds raft consensus state and handles bootstrapping,
/// adding peers, and running the consensus loop.
pub struct Consensus {
    pub peer_id: PeerId,
    pub raft_node: RawNode<RaftStorage>,
    pub receiver: Receiver<Msg>,
    pub runtime: Handle,
    // Probably don't keep it here since it mixes up abstraction levels
    pub toc: Arc<TableOfContent>,
    pub consensus_state: Arc<ConsensusState>,
}

impl Consensus {
    /// Bootstrap the consensus from existing node (leader)
    async fn bootstrap(
        &self,
        cluster_uri: Uri,
        consensus_state: Arc<ConsensusState>,
    ) -> Result<(), Box<dyn Error>> {
        let bootstrap_timeout_sec = 10;
        let channel = make_grpc_channel(
            Duration::from_secs(bootstrap_timeout_sec),
            Duration::from_secs(bootstrap_timeout_sec),
            cluster_uri,
        )
        .await?;

        let persistent = consensus_state.read_persistent().clone();
        let peer_id = persistent.peer_id;
        let peer_uri = persistent
            .peers
            .get(&peer_id)
            .cloned()
            .ok_or_else(|| format!("Peer with ID {peer_id} not found in persistent state"))?;

        let mut client = RaftClient::new(channel);
        let all_peers = client
            .add_peer_to_known(tonic::Request::new(AddPeerToKnownMessage {
                id: peer_id,
                uri: Some(peer_uri),
                port: Some(9920),
            }))
            .await?
            .into_inner();

        debug!("Adding peers from bootstrap: {all_peers:?}");
        for peer in all_peers.all_peers {
            // Add peer to local state
            if peer.id == peer_id {
                continue; // Skip adding self
            }

            add_peer_to_toc_and_consensus_state(
                &consensus_state,
                &self.toc,
                peer.id,
                peer.uri.parse::<Uri>()?,
                true,
            )
            .await?;
        }

        // local_state.set_first_voter(all_peeers.first_peer_id);
        // local_state.set_conf_state(ConfState::from((vec![all_peers.first_peer_id], vec![])))?;

        Ok(())
    }

    /// Initialize consensus and run in loop with a dedicated thread.
    pub fn start(
        peer_id: PeerId,
        bootstrap_uri: Option<Uri>,
        consensus_state: Arc<ConsensusState>,
        toc: Arc<TableOfContent>,
        runtime: Handle,
    ) -> Result<Arc<ConsensusManager>, Box<dyn Error>> {
        let consensus_dir = Path::new(STORAGE_DIR).join(CONSENSUS_DIR);
        let (consensus_manager, receiver) =
            ConsensusManager::init(consensus_dir.as_path(), consensus_state.clone());

        let consensus_manager = Arc::new(consensus_manager);

        let mut consensus = Self::new(
            peer_id,
            runtime,
            consensus_state.clone(),
            consensus_manager.clone(),
            receiver,
            toc,
        )?;

        // ToDo: Send initial snapshot to new followers to speed up consensus

        // Start a thread for consensus
        // Note: we don't need to preserve the thread handle,
        // as we are not going to join it later.
        // The thread will run until the program exits.
        thread::Builder::new()
            .name("consensus".to_string())
            .spawn(move || {
                // If set, running in cluster mode?
                if let Some(bootstrap_uri) = bootstrap_uri {
                    info!("Bootstrapping consensus from {bootstrap_uri}");
                    consensus
                        .runtime
                        .block_on(async {
                            consensus.bootstrap(bootstrap_uri, consensus_state).await
                        })
                        .unwrap();
                }

                info!("Starting consensus thread...");

                let rt = consensus.runtime.clone();

                // ToDo: Running loop inside async might not a good idea, figure out a way to run it in a blocking manner?
                rt.block_on(async {
                    if let Err(e) = consensus.run_loop().await {
                        error!("Consensus thread stopped with error: {e}");
                    } else {
                        error!("Consensus thread stopped");
                    }
                })
            })?;

        Ok(consensus_manager)
    }

    /// Create a new Consensus instance with a Raft node, sender, and receiver.
    fn new(
        peer_id: PeerId,
        runtime: Handle,
        consensus_state: Arc<ConsensusState>,
        consensus_manager: Arc<ConsensusManager>,
        receiver: Receiver<Msg>,
        toc: Arc<TableOfContent>,
    ) -> Result<Self, Box<dyn Error>> {
        let storage = RaftStorage::new(peer_id, consensus_manager.clone(), false);
        let logger = slog::Logger::root(slog_stdlog::StdLog.fuse(), o!());

        // let last_applied = consensus_state.read_persistent().last_applied();

        let config = Config {
            id: peer_id,
            election_tick: RAFT_ELECTION_TICK_MS,
            heartbeat_tick: RAFT_HEARTBEAT_TICK_MS,
            // applied: last_applied - 1,
            ..Default::default()
        };
        let raft = RawNode::new(&config, storage, &logger)?;

        info!("Created Raft node with ID: {peer_id}");

        let consensus = Consensus {
            peer_id,
            raft_node: raft,
            receiver,
            runtime,
            toc,
            consensus_state,
        };

        Ok(consensus)
    }

    /// Run the consensus loop at each tick.
    ///
    /// Process is as follows:
    /// 1. Wait for messages from the local receiver channel
    /// 2. If a message is received, pass it to the Raft node struct using .step() (internal raft messages) or .propose() (user defined messages)
    /// 3. If the Raft node is ready, call `on_ready()` to process the ready state (describes changes to the Raft state for other components like log, storage, and network layer).
    async fn run_loop(&mut self) -> Result<(), Box<dyn Error>> {
        // Tick the raft node per 100ms. So use an `Instant` to trace it.
        let mut t = Instant::now();

        loop {
            let Self {
                raft_node,
                receiver: local_receiver,
                runtime: _,
                peer_id: _,
                toc: _,
                consensus_state: _,
            } = self;

            let mut callbacks = HashMap::new();

            // Wait for messages from the local receiver channel
            loop {
                match local_receiver.recv_timeout(RAFT_TICK_INTERVAL) {
                    Ok(msg) => {
                        match msg {
                            Msg::Propose {
                                id,
                                operation,
                                callback,
                            } => {
                                log::info!(
                                    "Received proposal with ID: {id} and operation: {operation:?}"
                                );
                                callbacks.insert(id, callback);

                                // If adding peer, first propose a ConfChange to the Raft node
                                if let ConsensusOperation::AddPeer { peer_id, uri } = &operation {
                                    // ToDo: Use ConfChangeV2 instead
                                    let mut conf_change = ConfChange::default();
                                    conf_change.set_node_id(*peer_id);
                                    conf_change.set_change_type(ConfChangeType::AddNode);

                                    raft_node
                                        .propose_conf_change(
                                            uri.to_string().into_bytes(),
                                            conf_change,
                                        )
                                        .expect("Failed to propose conf change to add peer");
                                }

                                // Propose the operation to the Raft node log to be handled eventually by ready_processor
                                let msg_bytes = operation.into_bytes();
                                raft_node.propose(vec![], msg_bytes).unwrap();
                            }
                            Msg::Raft(message) => {
                                // Advance the state machine
                                raft_node.step(*message)?;
                            }
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        // Timeout occurred, we can tick the Raft node
                        break;
                    }
                    Err(RecvTimeoutError::Disconnected) => {
                        return Err("Receiver disconnected, exiting consensus loop".into());
                    }
                }
            }

            if t.elapsed() >= RAFT_TICK_INTERVAL {
                // Tick the raft.
                trace!("Ticking raft node");
                raft_node.tick();
                t = Instant::now();
            }

            if !raft_node.has_ready() {
                continue; // No ready state to processing ticking, continue to the next iteration
            }

            self.on_ready(&mut callbacks).await?;
        }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub enum ConsensusOperation {
    AddPeer { peer_id: PeerId, uri: String },
    RemovePeer { peer_id: PeerId },
    UpdateData(u64),
    CollectionOp(CollectionOperation),
}

impl ConsensusOperation {
    pub fn into_bytes(&self) -> Vec<u8> {
        let msg_str = serde_json::to_string(self).expect("Failed to serialize ConsensusOperation");
        msg_str.into_bytes()
    }

    pub fn from_entry(entry: &Entry) -> Result<Self, Box<dyn Error>> {
        let msg_str = String::from_utf8(entry.data.to_vec())
            .map_err(|e| format!("Failed to convert bytes to string: {e}"))?;
        let operation: ConsensusOperation = serde_json::from_str(&msg_str)
            .map_err(|e| format!("Failed to deserialize ConsensusOperation: {e}"))?;
        Ok(operation)
    }
}

pub enum Msg {
    // Custom messages for consensus operations
    Propose {
        id: ProposalId,
        operation: ConsensusOperation,
        callback: Box<dyn Fn() + Send>,
    },
    // Internal raft crate messages
    Raft(Box<Message>),
}
