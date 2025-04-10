use crate::proto::HubEvent;
use tokio::sync::mpsc;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Connection failed: {0}")]
    ConnectionError(String),
    #[error("Transport error: {0}")]
    TransportError(#[from] tonic::transport::Error),
    #[error("gRPC status error: {0}")]
    StatusError(#[from] tonic::Status),
    #[error("Internal Redis error: {0}")]
    InternalRedisError(#[from] crate::redis::error::Error),
    #[error("Processing error: {0}")]
    ProcessingError(String),
    #[error("Channel send error: {0}")]
    ChannelError(#[from] Box<mpsc::error::SendError<HubEvent>>),
}
