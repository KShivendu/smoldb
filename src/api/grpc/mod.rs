#[rustfmt::skip] // tonic uses `prettyplease` to format its output
pub mod p2p_grpc_schema;

mod points_service;
mod raft_service;
mod simple_service;

use crate::api::{
    collection::Dispatcher,
    grpc::{
        p2p_grpc_schema::{
            points_internal_server::PointsInternalServer, raft_server::RaftServer,
            service_server::ServiceServer,
        },
        points_service::PointsInternalService,
        raft_service::RaftService,
        simple_service::SimpleService,
    },
};
use http::Uri;
use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
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
    dispatcher: Arc<Dispatcher>,
) -> std::io::Result<()> {
    let mut server = Server::builder();
    let socket = SocketAddr::from((host.parse::<IpAddr>().unwrap(), grpc_port));

    let p2p_service = ServiceServer::new(SimpleService::default());

    let (sender, toc, consensus_manager) = (
        dispatcher
            .consensus_manager
            .as_ref()
            .map(|c| c.get_sender())
            .unwrap()
            .unwrap()
            .clone(),
        dispatcher.toc.clone(),
        dispatcher.consensus_manager.clone().unwrap(),
    );

    let raft_service = RaftServer::new(RaftService::new(sender, toc, consensus_manager));
    let points_service =
        PointsInternalServer::new(PointsInternalService::new(dispatcher.toc.clone()));

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
