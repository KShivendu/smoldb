use crate::{
    api::{collection::Dispatcher, helpers},
    consensus::{ConsensusOperation, Msg},
    storage::error::CollectionError,
};
use actix_web::{web, HttpResponse, Responder};
use serde_json::json;
use std::sync::mpsc::Sender;

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

#[actix_web::get("/cluster")]
async fn get_cluster(dispatcher: web::Data<Dispatcher>) -> impl Responder {
    helpers::time(async {
        let dispatcher = dispatcher.into_inner();

        if let Some(cluster_info) = dispatcher.get_cluster_info().await {
            Ok(cluster_info)
        } else {
            Err(CollectionError::ServiceError(
                "Cluster info is not available".to_string(),
            ))
        }
    })
    .await
}

// ToDo: Drop this API?
#[actix_web::get("/cluster/peer/add")]
async fn add_peer(consensus: web::Data<ConsensusAppData>) -> HttpResponse {
    helpers::time(async {
        consensus.submit_consensus_op(ConsensusOperation::AddPeer {
            peer_id: 123, // Example peer ID, should be replaced with actual logic
            uri: "http://example.com".to_string(), // Example URI, should be replaced with actual logic
        });

        Ok(json!({
            "status": "success",
            "message": "Peer addition operation submitted"
        }))
    })
    .await
}
