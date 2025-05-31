pub mod args;
pub mod consensus;

use actix_web::{App, HttpServer, web};
use args::parse_args;
use consensus::{Msg, init_consensus, run_consensus_receiver_loop, send_propose};
use std::sync::{Arc, mpsc::Sender};

#[actix_web::get("/")]
async fn index() -> &'static str {
    "Hello, world!"
}

#[actix_web::get("/cluster")]
async fn get_cluster() -> &'static str {
    "Cluster information"
}

#[actix_web::get("/cluster/peer/add")]
async fn add_peer(consensus: web::Data<ConsensusAppData>) -> &'static str {
    consensus
        .sender
        .send(Msg::Propose {
            id: 100, // Example ID, should be replaced with actual logic
            callback: Box::new(|| println!("Callback executed for adding peer")),
        })
        .expect("Failed to send message to consensus");
    "Added peer"
}

struct ConsensusAppData {
    sender: Sender<Msg>,
}

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

    println!("Moving FURTHER");

    tokio::spawn(async move {
        println!("Running consensus receiver loop");
        run_consensus_receiver_loop(&mut raft_node, receiver).await;
    });

    let consensus_app_data = web::Data::from(Arc::new(ConsensusAppData { sender }));

    println!("Running Actix Web server on {}", args.url);

    // Start Actix Web server on the same Tokio runtime
    HttpServer::new(move || {
        App::new()
            .service(index)
            .service(get_cluster)
            .service(add_peer)
            .app_data(consensus_app_data.clone())
    })
    .bind(args.url)?
    .run()
    .await
}
