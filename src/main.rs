pub mod args;
pub mod consensus;

use actix_web::{App, HttpServer};
use args::parse_args;
use consensus::{init_consensus, run_consensus};

#[actix_web::get("/")]
async fn index() -> &'static str {
    "Hello, world!"
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let args = parse_args();
    println!("Starting node with url: {}", args.url);

    // ToDo: Extract out mpsc::sender so we can send requests to consensus

    let (mut raft_node, slog_logger) = init_consensus(&args)
        .await
        .expect("Failed to initialize consensus");

    run_consensus(slog_logger, &mut raft_node).await;

    // Start Actix Web server on the same Tokio runtime
    HttpServer::new(|| App::new().service(index))
        .bind("127.0.0.1:9900")?
        .run()
        .await
}
