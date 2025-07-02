use crate::consensus::{ConsensusOperation, Msg};
use actix_web::{web, HttpResponse, Responder};
use serde::Serialize;
use serde_json::Value;
use std::{collections::HashMap, sync::mpsc::Sender};

type PeerId = u64;

pub struct ConsensusAppData {
    sender: Sender<Msg>,
}

impl ConsensusAppData {
    pub fn new(sender: Sender<Msg>) -> Self {
        ConsensusAppData { sender }
    }

    pub fn submit_consensus_op(&self, operation: ConsensusOperation) {
        self.sender
            .send(Msg::Propose {
                id: 100, // Example ID, should be replaced with actual logic
                operation,
                callback: Box::new(|| println!("Callback executed for adding peer")),
            })
            .expect("Failed to send message to consensus");
    }
}

#[derive(Serialize)]
struct ClusterResponse {
    peer_id: PeerId,
    peers: HashMap<PeerId, String>,
    raft_info: Value,
}

#[actix_web::get("/cluster")]
async fn get_cluster() -> impl Responder {
    HttpResponse::Ok().json(ClusterResponse {
        peer_id: 1,
        peers: HashMap::from([(1, "http://n0.cluster:9901".to_string())]),
        raft_info: serde_json::json!({
            "term": 1,
            "commit_index": 1,
            "last_applied": 1,
            "role": "leader",
            "leader": 1
        }),
    })
}

#[actix_web::get("/cluster/peer/add")]
async fn add_peer(consensus: web::Data<ConsensusAppData>) -> &'static str {
    consensus.submit_consensus_op(ConsensusOperation::AddPeer {
        peer_id: 123, // Example peer ID, should be replaced with actual logic
        uri: "http://example.com".to_string(), // Example URI, should be replaced with actual logic
    });
    "Added peer"
}
