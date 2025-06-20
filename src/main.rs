pub mod api;
pub mod args;
pub mod consensus;
pub mod storage;

use crate::api::{
    cluster::{ConsensusAppData, add_peer, get_cluster},
    collection::{Dispatcher, get_collection, get_collections},
};
use actix_web::{App, HttpServer, web};
use api::service::index;
use args::parse_args;
use consensus::{init_consensus, run_consensus_receiver_loop, send_propose};
use std::sync::Arc;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let args = parse_args();
    println!("Starting node with url: {}", args.url);

    // ToDo: Extract out mpsc::sender so we can send requests to consensus

    let (mut raft_node, _slog_logger, sender_receiver) = init_consensus(&args)
        .await
        .expect("Failed to initialize consensus");

    let (sender, receiver) = sender_receiver;

    // If you don't clone sender, you get Disconnected error if function is finished (however, it's not cause now we have infinite loop)
    send_propose(sender.clone());

    tokio::spawn(async move {
        println!("Running consensus receiver loop");
        run_consensus_receiver_loop(&mut raft_node, receiver).await;
    });

    let consensus_app_data = web::Data::from(Arc::new(ConsensusAppData::new(sender)));

    let dispatcher_app_data = web::Data::from(Arc::new(Dispatcher::dummy()));

    println!("Running Actix Web server on {}", args.url);

    // Start Actix Web server on the same Tokio runtime
    HttpServer::new(move || {
        App::new()
            .service(index)
            .service(get_cluster)
            .service(add_peer)
            .service(get_collections)
            .service(get_collection)
            .app_data(consensus_app_data.clone())
            .app_data(dispatcher_app_data.clone())
    })
    .bind(args.url)?
    .run()
    .await
}
