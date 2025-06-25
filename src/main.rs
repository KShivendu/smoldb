pub mod api;
pub mod args;
pub mod consensus;
pub mod storage;

use crate::{
    api::{
        cluster::{ConsensusAppData, add_peer, get_cluster},
        collection::{Dispatcher, create_collection, get_collection, get_collections},
        points::{get_point, list_points, upsert_points},
    },
    args::Args,
    consensus::Msg,
    storage::content_manager::TableOfContent,
};
use actix_web::{App, HttpServer, middleware, web};
use api::service::index;
use args::parse_args;
use consensus::{init_consensus, run_consensus_receiver_loop, send_propose};
use std::sync::{Arc, mpsc};

async fn setup_consensus(args: &Args) -> std::io::Result<mpsc::Sender<Msg>> {
    // ToDo: Extract out mpsc::sender so we can send requests to consensus

    let (mut raft_node, _slog_logger, sender_receiver) = init_consensus(args)
        .await
        .expect("Failed to initialize consensus");

    let (sender, receiver) = sender_receiver;

    // If you don't clone sender, you get Disconnected error if function is finished (however, it's not cause now we have infinite loop)
    send_propose(sender.clone());

    tokio::spawn(async move {
        println!("Running consensus receiver loop");
        run_consensus_receiver_loop(&mut raft_node, receiver).await;
    });

    Ok(sender)
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let args = parse_args();
    println!("Starting node with url: {}", args.url);

    let sender = setup_consensus(&args)
        .await
        .expect("Failed to setup consensus");

    let consensus_app_data = web::Data::from(Arc::new(ConsensusAppData::new(sender)));

    let toc = TableOfContent::load();

    let dispatcher_app_data = web::Data::from(Arc::new(Dispatcher::from(toc)));

    println!("Running Actix Web server on {}", args.url);

    // Start Actix Web server on the same Tokio runtime
    HttpServer::new(move || {
        App::new()
            .wrap(middleware::NormalizePath::trim())
            .service(index)
            .service(get_cluster)
            .service(add_peer)
            .service(get_collections)
            .service(get_collection)
            .service(create_collection)
            .service(upsert_points)
            .service(get_point)
            .service(list_points)
            .app_data(consensus_app_data.clone())
            .app_data(dispatcher_app_data.clone())
    })
    .bind(args.url)?
    .run()
    .await
}
