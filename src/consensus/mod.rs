pub mod broker;
pub mod debuggables;
pub mod manager;
pub mod raft_storage;
pub mod ready_processor;

use crate::{
    api::grpc::{
        make_grpc_channel,
        p2p_grpc_schema::{raft_client::RaftClient, AddPeerToKnownMessage},
    },
    consensus::raft_storage::RaftStorage,
    storage::toc::{CollectionOperation, TableOfContent},
    types::PeerId,
};
use http::Uri;
use raft::{
    prelude::{ConfChange, ConfChangeType, Message, MessageType},
    Config, RawNode,
};
use rand::Rng;
use serde::Serialize;
use slog::{o, Drain};
use std::{
    collections::{BTreeMap, HashMap},
    error::Error,
    sync::{
        mpsc::{channel, Receiver, RecvTimeoutError, Sender},
        Arc,
    },
    thread::{self},
    time::{Duration, Instant},
};
use tokio::{runtime::Handle, sync::RwLock};

const RAFT_TICK_INTERVAL: Duration = Duration::from_millis(100);
const RAFT_ELECTION_TICK_MS: usize = 10;
const RAFT_HEARTBEAT_TICK_MS: usize = 3;

#[derive(Debug, Clone, Serialize)]
pub struct Persistent {
    pub peer_id: PeerId,
    // Using instead of HashMap to keep peers sorted (consistent) across the nodes
    pub peers: BTreeMap<PeerId, String>,
    pub raft_info: ConsensusRaftInfo,
}

#[derive(Debug)]
pub struct ConsensusState {
    // ToDo: Replace with parking_lot::RwLock?
    pub persistent: RwLock<Persistent>,
    pub peer_address_by_id: Arc<RwLock<HashMap<PeerId, Uri>>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConsensusRaftInfo {
    pub term: u64,
    pub commit_index: u64,
    pub last_applied: u64,
    pub role: String, // "leader", "follower", etc.
    pub leader: PeerId,
}

impl ConsensusState {
    pub fn dummy(p2p_uri: http::Uri, default_peer_id: Option<PeerId>) -> Self {
        let mut rng = rand::rng();
        // Do not generate too big peer ID, to avoid problems with serialization
        let peer_id = default_peer_id.unwrap_or_else(|| rng.random::<PeerId>() % (1 << 53));

        let p = Persistent {
            peer_id,
            peers: BTreeMap::from([(peer_id, p2p_uri.to_string())]),
            raft_info: ConsensusRaftInfo {
                term: 1,
                commit_index: 1,
                last_applied: 1,
                role: "leader".to_string(),
                leader: 1,
            },
        };
        ConsensusState {
            persistent: RwLock::new(p),
            peer_address_by_id: Arc::new(RwLock::new(HashMap::from([(peer_id, p2p_uri)]))),
        }
    }

    pub async fn add_peer(&self, peer_id: PeerId, uri: Uri) -> Result<(), Box<dyn Error>> {
        // Add a new peer to the consensus state
        let mut persistent = self.persistent.write().await;
        persistent.peers.insert(peer_id, uri.to_string());
        Ok(())
    }
}

/// Holds raft consensus state and handles bootstrapping,
/// adding peers, and running the consensus loop.
pub struct Consensus {
    pub peer_id: PeerId,
    pub raft_node: RawNode<RaftStorage>,
    pub receiver: Receiver<Msg>,
    pub runtime: Handle,
    // Probably don't keep it here since it mixes up abstraction levels
    pub toc: Arc<TableOfContent>,
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

        let persistent = consensus_state.persistent.read().await.clone();
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

        println!("Adding all received peers: {all_peers:?}");
        for peer in all_peers.all_peers {
            // Add peer to local state
            if peer.id == peer_id {
                continue; // Skip adding self
            }

            consensus_state
                .add_peer(peer.id, peer.uri.parse::<Uri>()?)
                .await?;

            let collections = self.toc.collections.read().await;
            for (collection_name, collection) in collections.iter() {
                let mut replica_holder = collection.replica_holder.write().await;
                replica_holder
                    .add_remote_shards(peer.id, collection_name.clone())
                    .await?;
            }
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
    ) -> Result<Sender<Msg>, Box<dyn Error>> {
        let (mut consensus, sender) = Self::new(peer_id, runtime, toc)?;

        // if bootstrap_uri.is_none() {
        //     // For now, assuming that this is the leader and creating an intial before we even start the consensus loop:
        //     let mut s = Snapshot::default();
        //     s.mut_metadata().index = 1;
        //     s.mut_metadata().term = 1;
        //     s.mut_metadata().mut_conf_state().voters = vec![1];
        //     consensus.raft_node.store().wl().apply_snapshot(s).expect("Should be able to apply initial snapshot");
        // }

        // Start a thread for consensus
        // Note: we don't need to preserve the thread handle,
        // as we are not going to join it later.
        // The thread will run until the program exits.
        thread::Builder::new()
            .name("consensus".to_string())
            .spawn(move || {
                // If set, running in cluster mode?
                if let Some(bootstrap_uri) = bootstrap_uri {
                    println!("Bootstrapping consensus from {bootstrap_uri}");
                    consensus
                        .runtime
                        .block_on(async {
                            consensus.bootstrap(bootstrap_uri, consensus_state).await
                        })
                        .unwrap();
                }

                println!("Starting consensus thread...");

                let rt = consensus.runtime.clone();

                // ToDo: Running loop inside async might not a good idea, figure out a way to run it in a blocking manner?
                rt.block_on(async {
                    if let Err(e) = consensus.run_loop().await {
                        eprintln!("Consensus thread stopped with error: {e}");
                    } else {
                        println!("Consensus thread stopped");
                    }
                })
            })?;

        Ok(sender)
    }

    /// Create a new Consensus instance with a Raft node, sender, and receiver.
    fn new(
        peer_id: PeerId,
        runtime: Handle,
        toc: Arc<TableOfContent>,
    ) -> Result<(Self, Sender<Msg>), Box<dyn Error>> {
        // let storage = MemStorage::default();

        let storage = RaftStorage::new(peer_id);
        let logger = slog::Logger::root(slog_stdlog::StdLog.fuse(), o!());

        let config = Config {
            id: peer_id,
            election_tick: RAFT_ELECTION_TICK_MS,
            heartbeat_tick: RAFT_HEARTBEAT_TICK_MS,
            ..Default::default()
        };
        let raft = RawNode::new(&config, storage, &logger)?;

        if peer_id == 101 {
            // raft.store().wl().storag
            // let mut s = Snapshot::default();
            // s.mut_metadata().index = 2;
            // s.mut_metadata().term = 1;
            // s.mut_metadata().mut_conf_state().voters = vec![peer_id];
            // raft.store()
            //     .wl()
            //     .apply_snapshot(s)
            //     .expect("Should be able to apply initial snapshot");
        } else {
            // For followers, they'll apply this snapshot when they receive it from the leader
        }

        println!("Created Raft node with ID: {peer_id}");

        let (sender, receiver) = channel::<Msg>();

        let consensus = Consensus {
            peer_id,
            raft_node: raft,
            receiver,
            runtime,
            toc,
        };

        Ok((consensus, sender))
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
            } = self;

            let mut callbacks = HashMap::new();

            let mut is_heartbeat = false;
            // let _last_index_before = raft_node.raft.raft_log.last_index() + 1;
            // println!("Last index before processing: {_last_index_before}");

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
                                println!(
                                    "Received proposal with ID: {id} and operation: {operation:?}"
                                );
                                callbacks.insert(id, callback);

                                if let ConsensusOperation::AddPeer { peer_id, uri } = operation {
                                    let mut conf_change = ConfChange::default();
                                    conf_change.set_node_id(peer_id);
                                    conf_change.set_change_type(ConfChangeType::AddNode);

                                    raft_node
                                        .propose_conf_change(
                                            uri.to_string().into_bytes(),
                                            conf_change,
                                        )
                                        .expect("Failed to propose conf change to add peer");

                                    // let mut change = ConfChangeV2::default();

                                    // change.set_changes(vec![raft_proto::new_conf_change_single(
                                    //     peer_id,
                                    //     ConfChangeType::AddLearnerNode,
                                    // )]);
                                    // raft_node
                                    //     .propose_conf_change(uri.to_string().into_bytes(), change)
                                    //     .unwrap();
                                } else {
                                    // Propose the operation to the Raft node log
                                    let msg_str = format!("{operation:?}");
                                    let msg_bytes = msg_str.into_bytes();
                                    raft_node.propose(vec![], msg_bytes).unwrap();
                                }
                            }
                            Msg::Raft(message) => {
                                // println!("Received internal raft message: {message:?}");
                                // Advance the state machine

                                let msg_type = message.get_msg_type();

                                if msg_type == MessageType::MsgHeartbeat {
                                    is_heartbeat = true;
                                }

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
                raft_node.tick();
                t = Instant::now();
            }

            if !raft_node.has_ready() {
                continue; // No ready state to processing ticking, continue to the next iteration
            }

            self.on_ready(&mut callbacks, !is_heartbeat).await;
        }
    }
}

#[derive(Debug)]
pub enum ConsensusOperation {
    AddPeer { peer_id: PeerId, uri: String },
    UpdateData(u64),
    CollectionOp(CollectionOperation),
}

pub enum Msg {
    // Custom messages for consensus operations
    Propose {
        id: u8,
        operation: ConsensusOperation,
        callback: Box<dyn Fn() + Send>,
    },
    // Internal raft crate messages
    Raft(Box<Message>),
}
