#[rustfmt::skip] // tonic uses `prettyplease` to format its output
mod smoldb_p2p_grpc;

use smoldb_p2p_grpc::{
    service_server::Service, service_server::ServiceServer, RootApiReply, RootApiRequest,
};
use std::net::{IpAddr, SocketAddr};
use tonic::{transport::Server, Request, Response, Status};

#[derive(Default)]
pub struct P2PService {}

#[tonic::async_trait]
impl Service for P2PService {
    async fn root_api(
        &self,
        _request: Request<RootApiRequest>,
    ) -> Result<Response<RootApiReply>, Status> {
        let response = RootApiReply {
            title: "Smoldb Internal Service".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        };

        Ok(Response::new(response))
    }
}

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

pub async fn init(host: String, grpc_port: u16) -> std::io::Result<()> {
    let socket = SocketAddr::from((host.parse::<IpAddr>().unwrap(), grpc_port));
    let p2p_service = P2PService::default();

    let mut server = Server::builder();

    let _ = server
        .add_service(ServiceServer::new(p2p_service))
        .serve_with_shutdown(socket, async {
            #[cfg(unix)]
            wait_stop_signal("gRPC server").await;
        })
        .await
        .or_else(|e| {
            Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to start gRPC server: {e}",),
            ))
        })?;

    Ok(())
}
