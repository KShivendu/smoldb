mod smoldb;
use std::net::{IpAddr, SocketAddr};

use smoldb::{smoldb_internal_service_server::SmoldbInternalService, RootApiReply, RootApiRequest};
use tonic::{transport::Server, Request, Response, Status};

use crate::api::grpc::smoldb::smoldb_internal_service_server::SmoldbInternalServiceServer;

// Additional health check service that follows gRPC health check protocol as described in #2614
#[derive(Default)]
pub struct SmolDbInternalGrpcService {}

#[tonic::async_trait]
impl SmoldbInternalService for SmolDbInternalGrpcService {
    async fn root_api(
        &self,
        _request: Request<RootApiRequest>,
    ) -> Result<Response<RootApiReply>, Status> {
        let response = RootApiReply {
            title: "Smoldb Internal Service".to_string(),
            commit: Some("COMMIT_HERE".to_string()),
            version: "VERSION_HERE".to_string(),
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
    let internal_service = SmolDbInternalGrpcService::default();

    let mut server = Server::builder();

    let _ = server
        .add_service(SmoldbInternalServiceServer::new(internal_service))
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
