pub mod api;
pub mod args;
pub mod consensus;
pub mod storage;

use crate::api::collection::get_collection_cluster_info;
use crate::consensus::Consensus;
use crate::consensus::ConsensusState;
use crate::{
    api::{
        cluster::{add_peer, get_cluster, ConsensusAppData},
        collection::{create_collection, get_collection, get_collections, Dispatcher},
        points::{get_point, list_points, upsert_points},
    },
    consensus::Msg,
    storage::content_manager::TableOfContent,
};
use actix_web::{
    middleware,
    web::{self, Data},
    App, HttpServer,
};
use api::service::index;
use args::parse_args;
use http::Uri;
use std::sync::{mpsc::Sender, Arc};

// Function to start the Actix Web server
async fn start_http_server(
    url: Uri,
    consensus_app_data: Data<ConsensusAppData>,
    dispatcher_app_data: Data<Dispatcher>,
) -> std::io::Result<()> {
    println!("Starting Actix Web server on {url}");

    let (host, port) = (url.host().unwrap(), url.port_u16().unwrap());

    HttpServer::new(move || {
        App::new()
            .wrap(middleware::NormalizePath::trim())
            .service(index)
            .service(get_cluster)
            .service(add_peer)
            .service(get_collections)
            .service(get_collection_cluster_info)
            .service(get_collection)
            .service(create_collection)
            .service(upsert_points)
            .service(get_point)
            .service(list_points)
            .app_data(consensus_app_data.clone())
            .app_data(dispatcher_app_data.clone())
    })
    .bind((host, port))?
    .run()
    .await
}

// Function to start the Tonic internal (p2p) gRPC server
async fn start_p2p_server(
    p2p_uri: Uri,
    msg_sender: Sender<Msg>,
    consensus_state: Option<Arc<ConsensusState>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let p2p_host = p2p_uri.host().unwrap().to_string();
    let p2p_port = p2p_uri.port_u16().unwrap();

    println!("Starting internal gRPC server on {p2p_host}:{p2p_port}");

    if let Err(e) = api::grpc::init(p2p_host, p2p_port, msg_sender, consensus_state).await {
        eprintln!("Failed to start gRPC server: {e}");
    }

    Ok(())
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let args = parse_args();

    // Create a dedicated thread for internal gRPC service while we also run Actix Web server
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(8)
        .thread_name("general")
        .build()
        .expect("Failed to create Tokio runtime");

    let consensus_async_runtime = rt.handle().clone();

    let consensus_state = Arc::new(ConsensusState::dummy(args.p2p_url.clone()));

    let sender = Consensus::start(
        args.bootstrap.clone(),
        consensus_state.clone(),
        consensus_async_runtime,
    )
    .expect("Failed to start consensus");

    let consensus_app_data = web::Data::from(Arc::new(ConsensusAppData::new(sender.clone())));

    let toc = TableOfContent::load();

    let dispatcher_app_data = web::Data::from(Arc::new(Dispatcher::from(
        toc,
        Some(consensus_state.clone()),
    )));

    let rt_http = rt.handle().clone();
    let http_handle = std::thread::spawn(move || {
        rt_http.block_on(async {
            if let Err(e) =
                start_http_server(args.url, consensus_app_data, dispatcher_app_data).await
            {
                eprintln!("HTTP Server error: {e}");
            }
        });
    });

    // Start p2p gRPC server on the same Tokio runtime
    let rt_p2p = rt.handle().clone();
    let sender_to_move = sender.clone();
    let p2p_handle = std::thread::spawn(move || {
        rt_p2p.block_on(async {
            if let Err(e) =
                start_p2p_server(args.p2p_url, sender_to_move, Some(consensus_state)).await
            {
                eprintln!("gRPC Server error: {e}");
            }
        });
    });

    http_handle.join().expect("HTTP server thread panicked");
    p2p_handle.join().expect("gRPC server thread panicked");

    Ok(())
}
