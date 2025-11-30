use std::sync::Arc;

use actix_web::{middleware, web::Data, App, HttpServer};
use http::Uri;
use log::info;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::{
    api::{
        cluster::get_cluster,
        collection::{
            create_collection, delete_collection, get_collection, get_collection_cluster_info,
            get_collections,
        },
        dispatcher::Dispatcher,
        points::{get_point, list_points, query_points, upsert_points},
        service::{root_api, RootApiResponse},
    },
    consensus::Persistent,
};

use cluster::__path_get_cluster;
use collection::{
    __path_create_collection, __path_delete_collection, __path_get_collection,
    __path_get_collection_cluster_info, __path_get_collections,
};
use points::{__path_get_point, __path_list_points, __path_query_points, __path_upsert_points};
use service::__path_root_api;

pub mod cluster;
pub mod collection;
pub mod dispatcher;
pub mod grpc;
pub mod helpers;
pub mod points;
pub mod service;

#[derive(OpenApi)]
#[openapi(
    paths(
        root_api,
        get_cluster,
        get_collections,
        get_collection,
        create_collection,
        delete_collection,
        get_collection_cluster_info,
        upsert_points,
        get_point,
        list_points,
        query_points,
    ),
    components(schemas(RootApiResponse, Persistent,))
)]
pub struct ApiDoc;

// // Function to start the Actix Web server
pub async fn start_http_server(url: Uri, dispatcher: Arc<Dispatcher>) -> std::io::Result<()> {
    info!("Starting Actix Web server on {url}");

    let dispatcher_app_data = Data::from(dispatcher);

    let (host, port) = (url.host().unwrap(), url.port_u16().unwrap());

    let openapi = ApiDoc::openapi();
    let swagger_ui = SwaggerUi::new("/swagger-ui/{_:.*}").url("/api-docs/openapi.json", openapi); // todo: use this

    HttpServer::new(move || {
        App::new()
            .wrap(middleware::NormalizePath::trim())
            .service(swagger_ui.clone())
            .service(root_api)
            .service(get_cluster)
            .service(get_collections)
            .service(get_collection_cluster_info)
            .service(get_collection)
            .service(delete_collection)
            .service(create_collection)
            .service(upsert_points)
            .service(get_point)
            .service(list_points)
            .service(query_points)
            .app_data(dispatcher_app_data.clone())
    })
    .bind((host, port))?
    .run()
    .await
}

// // Function to start the Tonic internal (p2p) gRPC server
pub async fn start_p2p_server(
    p2p_uri: Uri,
    dispatcher: Arc<Dispatcher>,
) -> Result<(), Box<dyn std::error::Error>> {
    let p2p_host = p2p_uri.host().unwrap().to_string();
    let p2p_port = p2p_uri.port_u16().unwrap();

    info!("Starting internal gRPC server on {p2p_host}:{p2p_port}");

    if let Err(e) = grpc::init(p2p_host, p2p_port, dispatcher).await {
        log::error!("Failed to start gRPC server: {e}");
    }

    Ok(())
}
