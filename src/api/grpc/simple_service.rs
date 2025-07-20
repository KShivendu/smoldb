use tonic::{Request, Response, Status};

use crate::api::grpc::schema::{smol_server::Smol as SmolTrait, RootApiReply, RootApiRequest};

#[derive(Default)]
pub struct SmolService {}

#[tonic::async_trait]
impl SmolTrait for SmolService {
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
