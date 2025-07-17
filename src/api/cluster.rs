use crate::api::{dispatcher::Dispatcher, helpers};
use actix_web::{web, Responder};

#[actix_web::get("/cluster")]
async fn get_cluster(dispatcher: web::Data<Dispatcher>) -> impl Responder {
    helpers::time(async {
        let dispatcher = dispatcher.into_inner();
        let consensus = dispatcher.get_consensus()?;
        Ok(consensus.get_cluster_info().await)
    })
    .await
}
