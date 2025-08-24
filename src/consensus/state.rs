use crate::{
    consensus::CONSENSUS_DIR,
    error::{ConsensusError, ConsensusResult},
    storage::toc::STORAGE_DIR,
    types::PeerId,
};
use http::Uri;
use raft::{
    prelude::{ConfState, HardState},
    RaftState,
};
use rand::Rng;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::Path,
    sync::Arc,
};
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(remote = "HardState")]
struct HardStateJson {
    pub term: u64,
    pub vote: u64,
    pub commit: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(remote = "ConfState")]
struct ConfStateJson {
    pub voters: Vec<u64>,
    pub learners: Vec<u64>,
    pub learners_next: Vec<u64>,
    pub auto_leave: bool,
    pub voters_outgoing: Vec<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(remote = "RaftState")]
struct RaftStateJson {
    #[serde(with = "HardStateJson")]
    #[schema(value_type = HardStateJson)]
    pub hard_state: HardState,
    #[serde(with = "ConfStateJson")]
    #[schema(value_type = ConfStateJson)]
    pub conf_state: ConfState,
}


#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Persistent {
    pub peer_id: PeerId,
    // Using instead of HashMap to keep peers sorted (consistent) across the nodes
    pub peers: BTreeMap<PeerId, String>,
    pub raft_info: ConsensusRaftInfo,
    #[serde(with = "RaftStateJson")]
    #[schema(value_type = RaftStateJson)]
    pub raft_state: RaftState,
}

impl Persistent {
    fn load_or_create(peer_id: u64, p2p_uri: http::Uri) -> ConsensusResult<Self> {
        let consensus_dir = Path::new(STORAGE_DIR).join(CONSENSUS_DIR);
        fs::create_dir_all(&consensus_dir)?;
        let raft_state_path = consensus_dir.join("raft_state.json");

        let p = if raft_state_path.exists() {
            let data =
                std::fs::read_to_string(&raft_state_path).expect("Failed to read raft_state.json");
            serde_json::from_str(&data).expect("Failed to parse raft_state.json")
        } else {
            let default = Persistent {
                peer_id,
                peers: BTreeMap::from([(peer_id, p2p_uri.to_string())]),
                raft_info: ConsensusRaftInfo {
                    term: 0,
                    commit: 0,
                    role: "".to_string(),
                    leader: 0,
                },
                raft_state: RaftState::default(),
            };
            default.save()?;
            default
        };

        Ok(p)
    }

    // Dump to persistent storage
    pub fn save(&self) -> Result<(), ConsensusError> {
        let path = Path::new(STORAGE_DIR)
            .join(CONSENSUS_DIR)
            .join("raft_state.json");
        let data = serde_json::to_string_pretty(self)?;
        std::fs::write(path, data)?;
        Ok(())
    }
}

impl From<Persistent> for RaftState {
    fn from(val: Persistent) -> RaftState {
        // ToDo: Populate from persistent state
        RaftState {
            hard_state: Default::default(),
            conf_state: raft::eraftpb::ConfState::from((
                val.peers.keys().cloned().collect::<Vec<_>>(),
                vec![],
            )),
        }
    }
}

#[derive(Debug)]
pub struct ConsensusState {
    // Can't use async RwLock (tokio) here because `raft::Storage` trait methods are not async
    pub persistent: std::sync::RwLock<Persistent>,

    // ToDo: This is redundant with `persistent.peers`. Consider removing it
    // This is shared with `ChannelService`
    pub peer_address_by_id: Arc<RwLock<HashMap<PeerId, Uri>>>,
}

impl ConsensusState {
    pub fn get_peer_id(&self) -> PeerId {
        let persistent = self
            .persistent
            .read()
            .expect("Failed to read persistent state");
        persistent.peer_id
    }

    /// Return a read lock on the persistent state
    pub fn read_persistent(&self) -> std::sync::RwLockReadGuard<'_, Persistent> {
        self.persistent
            .read()
            .expect("Failed to read persistent state")
    }

    /// Return a write lock on the persistent state
    pub fn write_persistent(&self) -> std::sync::RwLockWriteGuard<'_, Persistent> {
        self.persistent
            .write()
            .expect("Failed to acquire persistent state write lock")
    }
}

impl ConsensusState {
    /// Create a new ConsensusState with a given p2p URI and optional default peer ID.
    pub fn new(p2p_uri: http::Uri, default_peer_id: Option<PeerId>) -> Self {
        let mut rng = rand::rng();
        // Do not generate too big peer ID, to avoid problems with serialization
        let peer_id = default_peer_id.unwrap_or_else(|| rng.random::<PeerId>() % (1 << 53));

        let p = Persistent::load_or_create(peer_id, p2p_uri.clone()).unwrap();

        // Read persistent state from disk if exists

        ConsensusState {
            persistent: std::sync::RwLock::new(p),
            peer_address_by_id: Arc::new(RwLock::new(HashMap::from([(peer_id, p2p_uri)]))),
        }
    }

    pub async fn add_peer(
        &self,
        peer_id: PeerId,
        uri: Uri,
    ) -> Result<(PeerId, Vec<(PeerId, String)>), ConsensusError> {
        // Add a new peer to the consensus state
        let mut peer_address_by_id = self.peer_address_by_id.write().await;
        peer_address_by_id.insert(peer_id, uri.clone());
        let mut persistent = self.write_persistent();
        persistent.peers.insert(peer_id, uri.to_string());

        // ToDo: Should return leader peer ID instead of current peer ID
        let this_peer_id = persistent.peer_id;
        let latest_peers = persistent.peers.clone().into_iter().collect();

        Ok((this_peer_id, latest_peers))
    }

    pub async fn get_peer_uri(&self, peer_id: PeerId) -> Result<Uri, ConsensusError> {
        let peer_address_by_id = self.peer_address_by_id.read().await;
        peer_address_by_id
            .get(&peer_id)
            .cloned()
            .ok_or_else(|| ConsensusError::ServiceError(format!("Peer ID {peer_id} not found")))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ConsensusRaftInfo {
    pub term: u64,
    pub commit: u64,
    // ToDo: Introduce pending_operations field
    pub role: String, // "leader", "follower", etc.
    pub leader: PeerId,
}
