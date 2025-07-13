use crate::{
    api::{collection::Dispatcher, helpers},
    consensus::ConsensusOperation,
    storage::error::CollectionError,
};
use actix_web::{web, HttpResponse, Responder};
use serde_json::json;

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
async fn add_peer(dispatcher: web::Data<Dispatcher>) -> HttpResponse {
    helpers::time(async {
        dispatcher
            .send_operation(ConsensusOperation::AddPeer {
                peer_id: 123, // Example peer ID, should be replaced with actual logic
                uri: "http://example.com".to_string(), // Example URI, should be replaced with actual logic
            })
            .await?;

        Ok(json!({
            "status": "success",
            "message": "Peer addition operation submitted"
        }))
    })
    .await
}
