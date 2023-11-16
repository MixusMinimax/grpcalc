use std::error::Error;

use async_trait::async_trait;
use tonic::transport::Server;
use tonic::{Request, Response, Status};

use proto::calculator::calculator_server::{Calculator, CalculatorServer};

use crate::proto::calculator::{AddRequest, AddResponse};

pub mod proto {
    pub mod calculator {
        tonic::include_proto!("calculator");
    }

    pub(crate) const FILE_DESCRIPTOR_SET: &[u8] =
        tonic::include_file_descriptor_set!("proto_descriptor");
}

#[derive(Default)]
pub struct CalculatorService {}

#[async_trait]
impl Calculator for CalculatorService {
    async fn add(&self, request: Request<AddRequest>) -> Result<Response<AddResponse>, Status> {
        let input = request.into_inner();
        println!("Received request: {:?}", input);
        let output = AddResponse {
            result: input.a + input.b.unwrap_or(0),
            message: None,
            b: input.b,
        };
        Ok(Response::new(output))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let addr = "0.0.0.0:50051".parse().unwrap();
    let calculator_service = CalculatorService::default();

    println!("Calculator server listening on {}", addr);

    Server::builder()
        .add_service(
            tonic_reflection::server::Builder::configure()
                .register_encoded_file_descriptor_set(proto::FILE_DESCRIPTOR_SET)
                .build()
                .unwrap(),
        )
        .add_service(CalculatorServer::new(calculator_service))
        .serve_with_shutdown(addr, async {
            tokio::signal::ctrl_c()
                .await
                .expect("failed to install CTRL+C signal handler");
        })
        .await?;

    Ok(())
}
