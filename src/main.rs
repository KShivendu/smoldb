pub mod api;
pub mod args;
pub mod channel_service;
pub mod consensus;
pub mod storage;
pub mod types;

use crate::api::{
    cluster::get_cluster,
    collection::{
        create_collection, delete_collection, get_collection, get_collection_cluster_info,
        get_collections,
    },
    dispatcher::Dispatcher,
    points::{get_point, list_points, upsert_points},
};
use crate::channel_service::ChannelService;
use crate::consensus::{manager::ConsensusManager, Consensus, ConsensusState};
use crate::storage::toc::TableOfContent;
use actix_web::{middleware, web::Data, App, HttpServer};
use api::service::index;
use args::parse_args;
use http::Uri;
use std::sync::Arc;

// Function to start the Actix Web server
async fn start_http_server(url: Uri, dispatcher: Arc<Dispatcher>) -> std::io::Result<()> {
    println!("Starting Actix Web server on {url}");

    let dispatcher_app_data = Data::from(dispatcher);

    let (host, port) = (url.host().unwrap(), url.port_u16().unwrap());

    HttpServer::new(move || {
        App::new()
            .wrap(middleware::NormalizePath::trim())
            .service(index)
            .service(get_cluster)
            .service(get_collections)
            .service(get_collection_cluster_info)
            .service(get_collection)
            .service(delete_collection)
            .service(create_collection)
            .service(upsert_points)
            .service(get_point)
            .service(list_points)
            .app_data(dispatcher_app_data.clone())
    })
    .bind((host, port))?
    .run()
    .await
}

// Function to start the Tonic internal (p2p) gRPC server
async fn start_p2p_server(
    p2p_uri: Uri,
    dispatcher: Arc<Dispatcher>,
) -> Result<(), Box<dyn std::error::Error>> {
    let p2p_host = p2p_uri.host().unwrap().to_string();
    let p2p_port = p2p_uri.port_u16().unwrap();

    println!("Starting internal gRPC server on {p2p_host}:{p2p_port}");

    if let Err(e) = api::grpc::init(p2p_host, p2p_port, dispatcher).await {
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

    // Sharing the Arc<RwLock<HashMap<PeerId, Uri>>>
    let consensus_state = Arc::new(ConsensusState::new(args.p2p_url.clone(), args.peer_id));
    let channel_service = ChannelService::new(
        consensus_state.get_peer_id().await,
        consensus_state.peer_address_by_id.clone(),
    );

    let toc = TableOfContent::load(channel_service);
    let toc_arc = Arc::new(toc);

    let sender = Consensus::start(
        consensus_state.persistent.read().await.peer_id,
        args.bootstrap.clone(),
        consensus_state.clone(),
        toc_arc.clone(),
        consensus_async_runtime,
    )
    .expect("Failed to start consensus thread and loop");

    let consensus_manager = ConsensusManager::new(consensus_state.clone(), sender);

    let dispatcher = Dispatcher::from(toc_arc, Some(Arc::new(consensus_manager)));
    let dispatcher_arc = Arc::new(dispatcher);

    let rt_http = rt.handle().clone();
    let http_dispatcher_arc = dispatcher_arc.clone();
    let http_handle = std::thread::spawn(move || {
        rt_http.block_on(async {
            start_http_server(args.url, http_dispatcher_arc)
                .await
                .expect("HTTP Server stopped")
        });
    });

    // Start p2p gRPC server on the same Tokio runtime
    let rt_p2p = rt.handle().clone();
    let p2p_handle = std::thread::spawn(move || {
        rt_p2p.block_on(async {
            start_p2p_server(args.p2p_url, dispatcher_arc)
                .await
                .expect("gRPC Server stopped")
        });
    });

    http_handle.join().expect("HTTP server thread panicked");
    p2p_handle.join().expect("gRPC server thread panicked");

    Ok(())
}
