use crate::{
    api::collection::Dispatcher,
    consensus::{ConsensusOperation, Msg},
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
    let dispatcher = dispatcher.into_inner();

    if let Some(cluster_info) = dispatcher.get_cluster_info().await {
        return HttpResponse::Ok().json(cluster_info);
    } else {
        return HttpResponse::Ok().json(json!({
            "enabled": false,
        }));
    }
}

#[actix_web::get("/cluster/peer/add")]
async fn add_peer(consensus: web::Data<ConsensusAppData>) -> &'static str {
    consensus.submit_consensus_op(ConsensusOperation::AddPeer {
        peer_id: 123, // Example peer ID, should be replaced with actual logic
        uri: "http://example.com".to_string(), // Example URI, should be replaced with actual logic
    });
    "Added peer"
}
