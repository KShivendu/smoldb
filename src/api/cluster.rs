use crate::{
    api::{collection::Dispatcher, helpers},
    storage::error::CollectionError,
};
use actix_web::{web, Responder};

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
