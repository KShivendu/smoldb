use crate::{
    api::{dispatcher::Dispatcher, helpers},
    consensus::{debuggables::DebuggableEntry, state::Persistent},
};
use actix_web::{get, web, Responder};
use serde::Serialize;
use utoipa::ToSchema;

#[utoipa::path(
    tag = "Cluster",
    responses(
        (status = 200, description = "Get info about cluster consensus", body = Persistent),
    ),
)]
#[get("/cluster")]
async fn get_cluster(dispatcher: web::Data<Dispatcher>) -> impl Responder {
    helpers::time(async {
        let dispatcher = dispatcher.into_inner();
        let consensus = dispatcher.get_consensus()?;
        Ok(consensus.get_cluster_info().await)
    })
    .await
}

#[derive(Serialize, ToSchema)]
struct PeekConsensusResponse {
    entries: Vec<DebuggableEntry>,
}

#[utoipa::path(
    tag = "Cluster",
    responses(
        (status = 200, description = "Get top 10 consensus log entries", body = PeekConsensusResponse),
    ),
)]
#[get("/cluster/inspect")]
async fn get_cluster_consensus(dispatcher: web::Data<Dispatcher>) -> impl Responder {
    helpers::time(async {
        let dispatcher = dispatcher.into_inner();
        let consensus = dispatcher.get_consensus()?;
        let consensus_top_10 = consensus.peek_consensus_wal(10);
        Ok(PeekConsensusResponse {
            entries: consensus_top_10,
        })
    })
    .await
}
