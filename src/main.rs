pub mod args;
pub mod consensus;

use actix_web::{App, HttpServer};
use args::parse_args;
use consensus::{init_consensus, run_consensus};
use tokio::task;

#[actix_web::get("/")]
async fn index() -> &'static str {
    "Hello, world!"
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let args = parse_args();
    println!("Starting node with url: {}", args.url);

    // Spawn a background async task that runs an infinite loop
    task::spawn(async move {
        let mut node = init_consensus(&args)
            .await
            .expect("Failed to initialize consensus");
        run_consensus(&mut node).await;
    });

    // Start Actix Web server on the same Tokio runtime
    HttpServer::new(|| App::new().service(index))
        .bind("127.0.0.1:9900")?
        .run()
        .await
}
