#[rustfmt::skip] // tonic uses `prettyplease` to format its output
pub mod p2p_grpc_schema;

mod points_service;
mod raft_service;
mod simple_service;

use crate::{
    api::grpc::{
        p2p_grpc_schema::{
            points_internal_server::PointsInternalServer, raft_server::RaftServer,
            service_server::ServiceServer,
        },
        points_service::PointsInternalService,
        raft_service::RaftService,
        simple_service::SimpleService,
    },
    consensus::{self, ConsensusState},
    storage::content_manager::TableOfContent,
};
use http::Uri;
use std::{
    net::{IpAddr, SocketAddr},
    sync::{mpsc::Sender, Arc},
    time::Duration,
};
use tonic::transport::{Channel, Error as TonicError, Server};

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

pub async fn make_grpc_channel(
    timeout: Duration,
    connection_timeout: Duration,
    uri: Uri,
) -> Result<Channel, TonicError> {
    let endpoint = Channel::builder(uri)
        .timeout(timeout)
        .connect_timeout(connection_timeout);
    // `connect` is using the `Reconnect` network service internally to handle dropped connections
    endpoint.connect().await
}

pub async fn make_default_grpc_channel(uri: Uri) -> Result<Channel, TonicError> {
    let bootstrap_timeout_sec = 10;
    make_grpc_channel(
        Duration::from_secs(bootstrap_timeout_sec),
        Duration::from_secs(bootstrap_timeout_sec),
        uri,
    )
    .await
}

pub async fn init(
    host: String,
    grpc_port: u16,
    toc: Arc<TableOfContent>,
    sender: Sender<consensus::Msg>,
    consensus_state: Option<Arc<ConsensusState>>,
) -> std::io::Result<()> {
    let mut server = Server::builder();
    let socket = SocketAddr::from((host.parse::<IpAddr>().unwrap(), grpc_port));

    let p2p_service = ServiceServer::new(SimpleService::default());
    let raft_service = RaftServer::new(RaftService::new(
        sender,
        toc.clone(),
        consensus_state.clone(),
    ));
    let points_service = PointsInternalServer::new(PointsInternalService::new(toc));

    server
        .add_service(p2p_service)
        .add_service(raft_service)
        .add_service(points_service)
        .serve_with_shutdown(socket, async {
            #[cfg(unix)]
            wait_stop_signal("gRPC server").await;
        })
        .await
        .map_err(|e| std::io::Error::other(format!("Failed to start gRPC server: {e}",)))?;

    Ok(())
}
