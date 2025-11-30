use crate::{
    api::{dispatcher::Dispatcher, helpers},
    consensus::{debuggables::DebuggableEntry, state::Persistent},
};
use actix_web::{get, web, Responder};

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

#[derive(serde::Serialize)]
struct PeerkConsensusResponse {
    entries: Vec<DebuggableEntry>,
}

#[actix_web::get("/cluster/inspect")]
async fn get_cluster_consensus(dispatcher: web::Data<Dispatcher>) -> impl Responder {
    helpers::time(async {
        let dispatcher = dispatcher.into_inner();
        let consensus = dispatcher.get_consensus()?;
        let consensus_top_10 = consensus.peek_consensus_wal(10);
        Ok(PeerkConsensusResponse {
            entries: consensus_top_10,
        })
    })
    .await
}
