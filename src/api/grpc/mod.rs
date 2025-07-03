#[rustfmt::skip] // tonic uses `prettyplease` to format its output
mod smoldb_p2p_grpc;

mod raft_service;
mod simple_service;

use crate::{
    api::grpc::{
        raft_service::RaftService,
        simple_service::SimpleService,
        smoldb_p2p_grpc::{raft_server::RaftServer, service_server::ServiceServer},
    },
    consensus,
};
use std::{
    net::{IpAddr, SocketAddr},
    sync::mpsc::Sender,
};
use tonic::transport::Server;

#[cfg(unix)]
async fn wait_stop_signal(for_what: &str) {
    use tokio::signal;

    let mut term = signal::unix::signal(signal::unix::SignalKind::terminate()).unwrap();
    let mut inrt = signal::unix::signal(signal::unix::SignalKind::interrupt()).unwrap();

    tokio::select! {
        _ = term.recv() => println!("Stopping {for_what} on SIGTERM"),
        _ = inrt.recv() => println!("Stopping {for_what} on SIGINT"),
    }
}

pub async fn init(
    host: String,
    grpc_port: u16,
    sender: Sender<consensus::Msg>,
) -> std::io::Result<()> {
    let mut server = Server::builder();
    let socket = SocketAddr::from((host.parse::<IpAddr>().unwrap(), grpc_port));

    let p2p_service = ServiceServer::new(SimpleService::default());
    let raft_service = RaftServer::new(RaftService::new(sender));

    server
        .add_service(p2p_service)
        .add_service(raft_service)
        .serve_with_shutdown(socket, async {
            #[cfg(unix)]
            wait_stop_signal("gRPC server").await;
        })
        .await
        .map_err(|e| std::io::Error::other(format!("Failed to start gRPC server: {e}",)))?;

    Ok(())
}
