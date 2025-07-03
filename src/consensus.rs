use crate::api::grpc::{
    make_grpc_channel,
    smoldb_p2p_grpc::{raft_client::RaftClient, AddPeerToKnownMessage},
};
use http::Uri;
use raft::{
    prelude::{Entry, EntryType, Message},
    storage::MemStorage,
    Config, RawNode,
};
use rand::Rng;
use serde::Serialize;
use serde_json::Value;
use slog::{o, Drain};
use std::{
    collections::{BTreeMap, HashMap},
    error::Error,
    sync::{
        mpsc::{self, channel, Receiver, RecvTimeoutError, Sender},
        Arc,
    },
    thread::{self},
    time::Duration,
};
use tokio::{runtime::Handle, sync::RwLock, time::Instant};

const RAFT_TICK_INTERVAL: Duration = Duration::from_millis(100);

pub type PeerId = u64;

#[derive(Debug, Clone, Serialize)]
pub struct Persistent {
    pub peer_id: PeerId,
    // Using instead of HashMap to keep peers sorted (consistent) across the nodes
    pub peers: BTreeMap<PeerId, String>,
    pub raft_info: Value,
}

#[derive(Debug)]
pub struct ConsensusState {
    // ToDo: Replace with parking_lot::RwLock?
    pub persistent: RwLock<Persistent>,
}

impl ConsensusState {
    pub fn dummy(p2p_uri: http::Uri) -> Self {
        let mut rng = rand::rng();
        // Do not generate too big peer ID, to avoid problems with serialization
        let random_peer_id = rng.random::<PeerId>() % (1 << 53);

        let p = Persistent {
            peer_id: random_peer_id,
            peers: BTreeMap::from([(random_peer_id, p2p_uri.to_string())]),
            raft_info: serde_json::json!({
                "term": 1,
                "commit_index": 1,
                "last_applied": 1,
                "role": "leader",
                "leader": 1
            }),
        };
        ConsensusState {
            persistent: RwLock::new(p),
        }
    }

    pub async fn add_peer(&self, peer_id: PeerId, uri: Uri) -> Result<(), Box<dyn Error>> {
        // Add a new peer to the consensus state
        let mut persistent = self.persistent.write().await;
        // if persistent.peers.contains_key(&peer_id) {
        //     return Err(Box::from(format!("Peer with ID {peer_id} already exists.")));
        // }
        persistent.peers.insert(peer_id, uri.to_string());
        Ok(())
    }
}

pub struct Consensus {
    raft_node: RawNode<MemStorage>,
    receiver: Receiver<Msg>,
    runtime: Handle,
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
            consensus_state
                .add_peer(peer.id, peer.uri.parse::<Uri>()?)
                .await?;
        }

        // local_state.set_first_voter(all_peeers.first_peer_id);
        // local_state.set_conf_state(ConfState::from((vec![all_peers.first_peer_id], vec![])))?;

        Ok(())
    }

    /// Initialize consensus and run in loop with a dedicated thread.
    pub fn start(
        bootstrap_uri: Option<Uri>,
        consensus_state: Arc<ConsensusState>,
        runtime: Handle,
    ) -> Result<Sender<Msg>, Box<dyn Error>> {
        let (mut consensus, sender) = Self::new(runtime)?;

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
                if let Err(e) = consensus.run_loop() {
                    eprintln!("Consensus thread stopped with error: {e}");
                } else {
                    println!("Consensus thread stopped");
                }
            })?;

        Ok(sender)
    }

    /// Create a new Consensus instance with a Raft node, sender, and receiver.
    fn new(runtime: Handle) -> Result<(Self, Sender<Msg>), Box<dyn Error>> {
        let storage = MemStorage::new_with_conf_state((vec![1], vec![]));
        let logger = slog::Logger::root(slog_stdlog::StdLog.fuse(), o!());

        let config = Config {
            id: 1, // The unique ID for the Raft node
            ..Default::default()
        };
        let raft = RawNode::new(&config, storage, &logger)?;

        let (sender, receiver) = channel::<Msg>();

        let consensus = Consensus {
            raft_node: raft,
            receiver,
            runtime,
        };

        Ok((consensus, sender))
    }

    /// Run the consensus loop at each tick.
    fn run_loop(&mut self) -> Result<(), Box<dyn Error>> {
        loop {
            let Self {
                raft_node,
                receiver,
                runtime: _,
            } = self;

            // Wait for messages from the receiver
            match receiver.recv_timeout(RAFT_TICK_INTERVAL) {
                Ok(msg) => {
                    match msg {
                        Msg::Propose {
                            id,
                            operation,
                            callback: _,
                        } => {
                            println!(
                                "Received proposal with ID: {id} and operation: {operation:?}"
                            );
                        }
                        Msg::Raft(message) => {
                            println!("Received Raft message: {message:?}");
                            // Process the Raft message
                            raft_node.step(*message)?;
                        }
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    // Timeout occurred, we can tick the Raft node
                    raft_node.tick();
                }
                Err(RecvTimeoutError::Disconnected) => {
                    println!("Receiver disconnected, exiting loop.");
                    return Ok(());
                }
            }
        }
    }
}

#[derive(Debug)]
pub enum ConsensusOperation {
    AddPeer { peer_id: PeerId, uri: String },
    UpdateData(u64),
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

/// ToDo: Integrate into  [`Consensus::run_loop`] to handle incoming messages
pub async fn run_consensus_receiver_loop(
    raft_node: &mut RawNode<MemStorage>,
    receiver: mpsc::Receiver<Msg>,
) {
    println!("Starting Raft consensus receiver loop...");
    let mut t = Instant::now();
    let mut timeout = RAFT_TICK_INTERVAL;

    let mut cbs = HashMap::new();

    loop {
        // Wait for a message or timeout (whichever happens first) to proceed with Raft tick
        match receiver.recv_timeout(timeout) {
            Ok(Msg::Propose {
                id,
                callback,
                operation,
            }) => {
                // ToDo: Handle different consensus operations
                println!("Received proposal with ID: {id} and operation {operation:?}");
                cbs.insert(id, callback);
                // ToDo: Data needs to be converted to CBOR format

                // Note: this returns ProposalDropped when the ID is repeated.
                raft_node
                    .propose(vec![], vec![id])
                    .unwrap_or_else(|_| panic!("failed to propose entry with {id}"));
            }
            Ok(Msg::Raft(m)) => {
                raft_node.step(*m).unwrap();
            }
            Err(RecvTimeoutError::Timeout) => {
                // Timeout occurred, checking Raft node...
            }
            Err(RecvTimeoutError::Disconnected) => {
                println!("ERROR: Receiver disconnected, exiting loop.");
                return;
            }
        }

        let d = t.elapsed();
        t = Instant::now();

        if d >= timeout {
            timeout = RAFT_TICK_INTERVAL;
            // We drive Raft every 100ms.
            raft_node.tick();
        } else {
            timeout -= d;
        }

        on_ready(raft_node, &mut cbs);
    }
}

fn on_ready(raft_node: &mut RawNode<MemStorage>, _cbs: &mut HashMap<u8, Box<dyn Fn() + Send>>) {
    loop {
        if !raft_node.has_ready() {
            return;
        }

        println!("Raft node is ready, processing message...");

        let store = raft_node.raft.raft_log.store.clone();

        // The Raft is ready, we can do something now.
        let mut ready = raft_node.ready();

        // ToDo: Consensus snapshots

        if !ready.messages().is_empty() {
            send_messages(ready.take_messages());
        }

        let mut last_apply_index = 0;
        handle_committed_entries(ready.take_committed_entries(), &mut last_apply_index);

        if !ready.entries().is_empty() {
            // Append entries to the Raft log.
            store.wl().append(ready.entries()).unwrap();
        }

        if let Some(hs) = ready.hs() {
            // Raft HardState changed, and we need to persist it.
            store.wl().set_hardstate(hs.clone());
        }

        if !ready.persisted_messages().is_empty() {
            // Send out the persisted messages come from the node.
            send_messages(ready.take_persisted_messages());
        }

        // Advance the Raft.
        let mut light_ready = raft_node.advance(ready);
        // Update commit index.
        if let Some(commit) = light_ready.commit_index() {
            store.wl().mut_hard_state().set_commit(commit);
        }
        // Send out the messages to other peers.
        send_messages(light_ready.take_messages());
        // Apply all committed entries.
        handle_committed_entries(light_ready.take_committed_entries(), &mut last_apply_index);
        // Advance the apply index.
        raft_node.advance_apply();

        println!("Raft node processed a ready state.");
    }
}

/// Send out the messages to other peers
fn send_messages(messages: Vec<Message>) {
    for msg in messages {
        println!("Sending message: {msg:?}");
    }
}

/// Handle committed entries
fn handle_committed_entries(entries: Vec<Entry>, last_apply_index: &mut u64) {
    for entry in entries {
        // Mostly, you need to save the last apply index to resume applying
        // after restart. Here we just ignore this because we use a Memory storage.
        *last_apply_index = entry.index;

        if entry.data.is_empty() {
            // Empty entry, when the peer becomes Leader it will send an empty entry.
            continue;
        }

        match entry.get_entry_type() {
            EntryType::EntryNormal => handle_normal(entry),
            // It's recommended to always use `EntryType::EntryConfChangeV2.
            EntryType::EntryConfChange => handle_conf_change(entry),
            EntryType::EntryConfChangeV2 => handle_conf_change_v2(entry),
        }
    }
}

fn handle_normal(entry: Entry) {
    println!("Handle normal entry: {entry:?}");
}

fn handle_conf_change(entry: Entry) {
    println!("Handle conf change entry: {entry:?}");
}

fn handle_conf_change_v2(entry: Entry) {
    println!("Handle conf change v2 entry: {entry:?}");
}
