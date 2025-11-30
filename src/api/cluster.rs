use crate::api::{dispatcher::Dispatcher, helpers};
use actix_web::{get, web, Responder};

#[utoipa::path(
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
