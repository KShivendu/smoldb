use tonic::{Request, Response, Status};

use crate::api::grpc::p2p_grpc_schema::{service_server::Service, RootApiReply, RootApiRequest};

#[derive(Default)]
pub struct SimpleService {}

#[tonic::async_trait]
impl Service for SimpleService {
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
